//! OSC packet decoding: converts a byte slice into an `OscPacket`.
//!
//! The decoder is zero-copy for blobs and strings during intermediate parsing;
//! final values are allocated into owned `OscArg` variants.

use alloc::{string::ToString, vec, vec::Vec};

use crate::{OscArg, OscBundle, OscError, OscMessage, OscPacket, OscTimeTag};

/// Maximum nesting depth accepted for OSC bundle-within-bundle recursion
/// (`decode_bundle`) and OSC array-within-message nesting (`[`/`]` type
/// tags in `read_typed_args`).
///
/// OSC packets are untrusted, attacker-controlled bytes — e.g. an arbitrary
/// UDP datagram handed to `decode` via `OscReceiver::recv`. Without a cap, a
/// single ~64 KB datagram can encode tens of thousands of nesting levels
/// (each bracket or bundle-size-prefix costs only a handful of bytes). The
/// resulting `OscPacket`/`OscArg` tree overflows the stack the moment
/// anything walks it recursively — `Drop` glue, `Debug`, `PartialEq`, or
/// `encode` — even though *building* the tree is otherwise cheap. The cap
/// is therefore enforced at construction time, before such a tree can ever
/// exist, rather than deferred to a later traversal.
const MAX_NESTING_DEPTH: usize = 32;

/// Decodes an OSC packet from a byte slice.
///
/// Returns an error if the data is truncated, misaligned, or has an invalid structure.
pub fn decode(data: &[u8]) -> Result<OscPacket, OscError> {
    decode_with_depth(data, 0)
}

/// Inner implementation of [`decode`] threading a bundle-nesting depth counter.
///
/// `depth` counts the number of `#bundle`-within-`#bundle` levels already
/// entered; decoding is rejected once it would exceed [`MAX_NESTING_DEPTH`].
fn decode_with_depth(data: &[u8], depth: usize) -> Result<OscPacket, OscError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(OscError(alloc::format!(
            "OSC bundle nesting exceeds maximum depth of {MAX_NESTING_DEPTH}"
        )));
    }
    if data.starts_with(b"#bundle\0") {
        decode_bundle(data, depth).map(OscPacket::Bundle)
    } else if data.first().copied() == Some(b'/') {
        decode_message(data).map(OscPacket::Message)
    } else {
        Err(OscError(
            "packet must start with '/' (message) or '#bundle\\0' (bundle)".to_string(),
        ))
    }
}

// ─── bundle ───────────────────────────────────────────────────────────────────

fn decode_bundle(data: &[u8], depth: usize) -> Result<OscBundle, OscError> {
    // "#bundle\0" is 8 bytes; time tag is the next 8 bytes.
    let mut pos = 8usize;
    let time = read_timetag(data, &mut pos)?;

    let mut elements = Vec::new();
    while pos < data.len() {
        let size = read_u32(data, &mut pos)? as usize;
        // See `element_end` docs: `size` is an attacker-controlled 4-byte length prefix and
        // can be as large as `u32::MAX`, which equals `usize::MAX` on a 32-bit target.
        let end = element_end(pos, size)?;
        if end > data.len() {
            return Err(OscError(
                "bundle element size exceeds remaining data".to_string(),
            ));
        }
        let element_data = &data[pos..end];
        elements.push(decode_with_depth(element_data, depth + 1)?);
        pos = end;
    }

    Ok(OscBundle { time, elements })
}

/// Computes the end offset (`pos + size`) of a bundle element, guarding against overflow.
///
/// `size` comes straight from an attacker-controlled 4-byte big-endian length prefix, so it
/// can be as large as `u32::MAX`. On a 32-bit target `usize` is also 32 bits wide, so
/// `u32::MAX` *is* `usize::MAX` — a plain `pos + size` can wrap around past `usize::MAX` back
/// down to a small value. That wrapped value could slip past a naive `> data.len()` bounds
/// check (which is exactly the align4-padding-style bypass the `read_str`/`read_blob` guards
/// elsewhere in this file already close for their own arithmetic) and then panic when slicing
/// `data[pos..end]` with a start greater than its end. `checked_add` turns the overflow itself
/// into a typed [`OscError`] instead of a silently-wrong bounds check or a panic.
fn element_end(pos: usize, size: usize) -> Result<usize, OscError> {
    pos.checked_add(size).ok_or_else(|| {
        OscError("bundle element size overflows offset (corrupt or hostile input)".to_string())
    })
}

// ─── message ──────────────────────────────────────────────────────────────────

fn decode_message(data: &[u8]) -> Result<OscMessage, OscError> {
    let mut pos = 0usize;
    let address = read_str(data, &mut pos)?.to_string();

    // Type tag string must start with ','; absence means zero arguments.
    let type_tags: Vec<char> = if pos < data.len() && data[pos] == b',' {
        let raw = read_str(data, &mut pos)?;
        // Skip the leading ','.
        raw.chars().skip(1).collect()
    } else {
        Vec::new()
    };

    let args = read_typed_args(data, &mut pos, &type_tags)?;
    Ok(OscMessage { address, args })
}

// ─── argument decoding ────────────────────────────────────────────────────────

/// Reads arguments from `data[pos..]` according to `type_tags`.
///
/// Handles nested arrays via an explicit stack: `[` pushes a new accumulator,
/// `]` pops and appends the collected `OscArg::Array` to the parent.
fn read_typed_args(
    data: &[u8],
    pos: &mut usize,
    type_tags: &[char],
) -> Result<Vec<OscArg>, OscError> {
    // Stack of Vec<OscArg>: index 0 is the outermost (top-level) argument list.
    let mut stack: Vec<Vec<OscArg>> = vec![Vec::new()];

    for &tag in type_tags {
        match tag {
            '[' => {
                // Reject *before* opening a new level once already at the cap.
                // `stack.len()` is 1 (the top-level accumulator) plus the
                // number of currently-open `[` brackets, so this bounds the
                // deepest `OscArg::Array` nesting an attacker's type-tag
                // string can produce to `MAX_NESTING_DEPTH` levels.
                if stack.len() >= MAX_NESTING_DEPTH {
                    return Err(OscError(alloc::format!(
                        "OSC array nesting exceeds maximum depth of {MAX_NESTING_DEPTH}"
                    )));
                }
                stack.push(Vec::new());
            }
            ']' => {
                let inner = stack
                    .pop()
                    .ok_or_else(|| OscError("unexpected ']' in type tags".to_string()))?;
                // Guard against an empty stack after pop (malformed input).
                let top = stack
                    .last_mut()
                    .ok_or_else(|| OscError("unmatched ']' in type tags".to_string()))?;
                top.push(OscArg::Array(inner));
            }
            _ => {
                let arg = read_single_arg(data, pos, tag)?;
                let top = stack.last_mut().ok_or_else(|| {
                    OscError("argument outside of valid type-tag scope".to_string())
                })?;
                top.push(arg);
            }
        }
    }

    if stack.len() != 1 {
        return Err(OscError("unclosed '[' in OSC type tags".to_string()));
    }

    Ok(stack.remove(0))
}

fn read_single_arg(data: &[u8], pos: &mut usize, tag: char) -> Result<OscArg, OscError> {
    match tag {
        'i' => Ok(OscArg::Int(read_i32(data, pos)?)),
        'f' => Ok(OscArg::Float(f32::from_be_bytes(read_exact::<4>(
            data, pos,
        )?))),
        's' => Ok(OscArg::String(read_str(data, pos)?.to_string())),
        'b' => {
            let blob = read_blob(data, pos)?;
            Ok(OscArg::Blob(blob.to_vec()))
        }
        'h' => Ok(OscArg::Long(i64::from_be_bytes(read_exact::<8>(
            data, pos,
        )?))),
        'd' => Ok(OscArg::Double(f64::from_be_bytes(read_exact::<8>(
            data, pos,
        )?))),
        't' => Ok(OscArg::TimeTag(read_timetag(data, pos)?)),
        'c' => {
            let code = u32::from_be_bytes(read_exact::<4>(data, pos)?);
            let c = char::from_u32(code)
                .ok_or_else(|| OscError(alloc::format!("invalid char code: {code}")))?;
            Ok(OscArg::Char(c))
        }
        'r' => {
            let bytes = read_exact::<4>(data, pos)?;
            Ok(OscArg::Color(bytes[0], bytes[1], bytes[2], bytes[3]))
        }
        'm' => Ok(OscArg::Midi(read_exact::<4>(data, pos)?)),
        'T' => Ok(OscArg::Bool(true)),
        'F' => Ok(OscArg::Bool(false)),
        'N' => Ok(OscArg::Nil),
        'I' => Ok(OscArg::Impulse),
        other => Err(OscError(alloc::format!("unknown OSC type tag: '{other}'"))),
    }
}

// ─── low-level readers ────────────────────────────────────────────────────────

fn read_str<'a>(data: &'a [u8], pos: &mut usize) -> Result<&'a str, OscError> {
    let start = *pos;
    // `start` may already be past the end of `data`: a previous read_str/read_blob
    // call's 4-byte alignment padding can push the cursor beyond the buffer even
    // though the unaligned content ended exactly at the buffer boundary.
    if start > data.len() {
        return Err(OscError(alloc::format!(
            "truncated data: OSC string starts at offset {start}, have {}",
            data.len()
        )));
    }
    // Find the NUL terminator.
    let nul = data[start..]
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| OscError("unterminated OSC string".to_string()))?;
    let s = core::str::from_utf8(&data[start..start + nul])
        .map_err(|_| OscError("OSC string is not valid UTF-8".to_string()))?;
    // Advance past the string + NUL, then pad to 4-byte boundary.
    *pos = align4(start + nul + 1);
    Ok(s)
}

fn read_blob<'a>(data: &'a [u8], pos: &mut usize) -> Result<&'a [u8], OscError> {
    let len = read_u32(data, pos)? as usize;
    let end = pos
        .checked_add(len)
        .ok_or_else(|| OscError("blob length overflow".to_string()))?;
    if end > data.len() {
        return Err(OscError(alloc::format!(
            "blob length {len} exceeds remaining data"
        )));
    }
    let blob = &data[*pos..end];
    *pos = align4(end);
    Ok(blob)
}

fn read_timetag(data: &[u8], pos: &mut usize) -> Result<OscTimeTag, OscError> {
    let seconds = u32::from_be_bytes(read_exact::<4>(data, pos)?);
    let fractional = u32::from_be_bytes(read_exact::<4>(data, pos)?);
    Ok(OscTimeTag {
        seconds,
        fractional,
    })
}

fn read_u32(data: &[u8], pos: &mut usize) -> Result<u32, OscError> {
    Ok(u32::from_be_bytes(read_exact::<4>(data, pos)?))
}

fn read_i32(data: &[u8], pos: &mut usize) -> Result<i32, OscError> {
    Ok(i32::from_be_bytes(read_exact::<4>(data, pos)?))
}

fn read_exact<const N: usize>(data: &[u8], pos: &mut usize) -> Result<[u8; N], OscError> {
    // Same overflow class as `element_end`: guard `*pos + N` with `checked_add` rather than a
    // plain `+`. `N` is a small compile-time constant (4 or 8 in this crate), so this branch is
    // unreachable via the current call graph once `decode_bundle`'s own arithmetic is guarded
    // (nothing else can drive `*pos` anywhere near `usize::MAX`) — kept for defense in depth so
    // the same defect class can never resurface here if a future caller changes that.
    let end = pos.checked_add(N).ok_or_else(|| {
        OscError(alloc::format!(
            "truncated data: offset {} overflows while requesting {N} bytes",
            *pos
        ))
    })?;
    if end > data.len() {
        return Err(OscError(alloc::format!(
            "truncated data: need {N} bytes at offset {}, have {}",
            *pos,
            data.len()
        )));
    }
    let mut buf = [0u8; N];
    buf.copy_from_slice(&data[*pos..end]);
    *pos = end;
    Ok(buf)
}

/// Rounds `n` up to the next 4-byte boundary.
#[inline]
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OscArg, OscBundle, OscMessage, OscPacket, OscTimeTag, encode};

    fn msg(address: &str, args: Vec<OscArg>) -> OscPacket {
        OscPacket::Message(OscMessage {
            address: address.to_string(),
            args,
        })
    }

    #[test]
    fn decode_invalid_address_no_slash() {
        // Does not start with '/' or '#bundle\0'.
        let bad = b"invalid_packet\0\0";
        assert!(decode(bad).is_err());
    }

    #[test]
    fn decode_alignment_padding_past_end_returns_error() {
        // Hand-crafted (unpadded) packet:
        //   address:  "/a\0\0"   (4 bytes, already 4-byte aligned)
        //   type tags: ",s\0"    (3 bytes, deliberately missing the pad byte
        //                          that a well-formed encoder would add)
        // Reading the type-tag string advances `pos` via `align4(7) == 8`,
        // which lands one byte past the 7-byte buffer. The subsequent
        // `read_str` call for the 's' argument must then detect that its
        // start position is out of bounds and return an error instead of
        // panicking on an out-of-range slice.
        let bad: &[u8] = b"/a\0\0,s\0";
        assert_eq!(bad.len(), 7);
        assert!(decode(bad).is_err());
    }

    #[test]
    fn decode_truncated_data() {
        // Encode a message with an Int arg, then cut off the arg bytes.
        let bytes = encode(&msg("/x", vec![OscArg::Int(99)]));
        // Drop the last 2 bytes — the Int arg is only partially present.
        let truncated = &bytes[..bytes.len() - 2];
        assert!(decode(truncated).is_err());
    }

    #[test]
    fn decode_bundle_immediate_time_tag() {
        let bundle = OscPacket::Bundle(OscBundle {
            time: OscTimeTag::IMMEDIATE,
            elements: vec![msg("/a", vec![OscArg::Int(1)])],
        });
        let bytes = encode(&bundle);
        let decoded = decode(&bytes).expect("decode failed");
        if let OscPacket::Bundle(b) = decoded {
            assert_eq!(b.time, OscTimeTag::IMMEDIATE);
            assert_eq!(b.elements.len(), 1);
        } else {
            panic!("expected Bundle");
        }
    }

    #[test]
    fn encode_bundle_two_messages() {
        let bundle = OscPacket::Bundle(OscBundle {
            time: OscTimeTag {
                seconds: 42,
                fractional: 0,
            },
            elements: vec![
                msg("/a", vec![OscArg::Int(1)]),
                msg("/b", vec![OscArg::Float(2.0)]),
            ],
        });
        let bytes = encode(&bundle);
        let decoded = decode(&bytes).expect("decode failed");
        if let OscPacket::Bundle(b) = decoded {
            assert_eq!(b.elements.len(), 2);
        } else {
            panic!("expected Bundle");
        }
    }

    // ─── bundle element-size arithmetic guard (checked_add overflow regression) ───────────────
    //
    // `decode_bundle` computes an element's end offset from `pos + size`, where `size` is a
    // 4-byte big-endian length prefix taken directly from untrusted bytes. `element_end`'s
    // `checked_add` (rather than a plain `+`) is what keeps a wire-format-maximal `size`
    // (`u32::MAX` — which equals `usize::MAX` on a 32-bit target) from wrapping the addition
    // around to a small value that would slip past the `end > data.len()` bounds check and then
    // panic when slicing `data[pos..end]` with a reversed range. `read_exact`'s `*pos + N` gets
    // the same `checked_add` treatment for the identical reason (defense in depth).

    #[test]
    fn element_end_overflow_returns_typed_error_not_panic() {
        // Direct, width-independent proof that `element_end` catches the overflow: adding 1 to
        // `usize::MAX` overflows regardless of whether `usize` is 32 or 64 bits wide, so this
        // exercises the exact branch a 32-bit target would hit via `decode_bundle` fed a
        // wire-format-maximal size — without needing to actually build for a 32-bit target.
        let err = element_end(usize::MAX, 1).expect_err("pos + size overflow must be an Err");
        assert!(
            err.0.contains("overflows offset"),
            "expected an overflow error, got: {}",
            err.0
        );

        // Symmetric case: a huge `size` (as `read_u32(..) as usize` could produce) added to a
        // small but nonzero `pos`.
        let err2 = element_end(20, usize::MAX).expect_err("pos + size overflow must be an Err");
        assert!(
            err2.0.contains("overflows offset"),
            "expected an overflow error, got: {}",
            err2.0
        );
    }

    #[test]
    fn element_end_in_range_returns_sum() {
        assert_eq!(element_end(20, 100).expect("no overflow"), 120);
        assert_eq!(element_end(0, 0).expect("no overflow"), 0);
    }

    #[test]
    fn read_exact_pos_overflow_returns_typed_error_not_panic() {
        // Same width-independent overflow proof as `element_end`, but for `read_exact`'s own
        // `*pos + N` guard.
        let data = [0u8; 4];
        let mut pos = usize::MAX;
        let err = read_exact::<4>(&data, &mut pos).expect_err("pos + N overflow must be an Err");
        assert!(
            err.0.contains("overflows"),
            "expected an overflow-flavored truncation error, got: {}",
            err.0
        );
    }

    #[test]
    fn decode_bundle_element_size_near_usize_max_returns_error_not_panic() {
        // End-to-end regression test through the public `decode` entry point, built from raw
        // wire bytes (not via `OscArg`/`encode`, since `encode` always computes a truthful size
        // prefix from the actual encoded element — it cannot be asked to lie about the size the
        // way a hostile packet can).
        //
        // `size` is set to `u32::MAX`: the largest value the OSC wire format's 4-byte size
        // prefix can express, which is also exactly `usize::MAX` on a 32-bit target. This test
        // is written to be meaningful — and pass — on both 32-bit and 64-bit hosts: the branch
        // that produces the `Err` differs (`element_end`'s `checked_add` overflow on 32-bit vs.
        // the ordinary "exceeds remaining data" bounds check on 64-bit, since `pos + size` does
        // not overflow a 64-bit `usize`), but either way `decode` must return a typed error,
        // never panic.
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(b"#bundle\0");
        data.extend_from_slice(&0u32.to_be_bytes()); // time tag: seconds
        data.extend_from_slice(&1u32.to_be_bytes()); // time tag: fractional (IMMEDIATE)
        data.extend_from_slice(&u32::MAX.to_be_bytes()); // element size prefix: u32::MAX
        // Deliberately no element bytes follow — a well-formed bundle would need `u32::MAX`
        // more bytes here, which `decode` must reject long before trying to read them.

        let result = decode(&data);
        assert!(
            result.is_err(),
            "bundle element size near usize::MAX must be rejected with a typed error, not \
             panic (and must not be silently accepted), got: {result:?}"
        );
    }

    #[test]
    fn roundtrip_all_types() {
        let packet = msg(
            "/all",
            vec![
                OscArg::Int(-1),
                OscArg::Float(1.5),
                OscArg::String("osc".to_string()),
                OscArg::Blob(vec![0x01, 0x02]),
                OscArg::Long(i64::MAX),
                OscArg::Double(core::f64::consts::PI),
                OscArg::TimeTag(OscTimeTag {
                    seconds: 10,
                    fractional: 500,
                }),
                OscArg::Char('Z'),
                OscArg::Color(255, 128, 0, 255),
                OscArg::Midi([0x90, 0x3C, 0x40, 0x00]),
                OscArg::Bool(true),
                OscArg::Bool(false),
                OscArg::Nil,
                OscArg::Impulse,
            ],
        );
        let bytes = encode(&packet);
        let decoded = decode(&bytes).expect("roundtrip failed");
        // Re-encode for stable float comparison.
        assert_eq!(encode(&decoded), bytes);
    }

    // ─── nesting-depth guard (stack-overflow DoS regression) ──────────────────
    //
    // `OscReceiver::recv` (server.rs) hands `decode` up to 65536 raw bytes
    // from an untrusted UDP socket. Before the `MAX_NESTING_DEPTH` guard, a
    // single crafted datagram could produce tens of thousands of nested
    // `OscArg::Array` levels (via `[`/`]` type tags) or `#bundle`-within-
    // `#bundle` levels, overflowing the stack the moment the resulting tree
    // was dropped, debug-printed, compared, or re-encoded. These tests use
    // small (but past-the-cap) depths — enough to prove the guard fires —
    // without themselves risking a stack overflow while constructing the
    // fixture.

    #[test]
    fn decode_array_nesting_exceeds_max_depth_returns_error() {
        // Build MAX_NESTING_DEPTH + 8 levels of nested `OscArg::Array` and
        // confirm `decode` rejects the encoded bytes with a typed error
        // instead of reconstructing a tree deep enough to blow the stack
        // on drop/encode/Debug. Assert on the error text (not just
        // `is_err()`) so this can't silently start passing for the wrong
        // reason (e.g. an unrelated truncation bug) if the depth guard
        // itself ever regresses.
        let mut arg = OscArg::Int(1);
        for _ in 0..(MAX_NESTING_DEPTH + 8) {
            arg = OscArg::Array(vec![arg]);
        }
        let bytes = encode(&msg("/deep", vec![arg]));
        let err =
            decode(&bytes).expect_err("array nesting beyond MAX_NESTING_DEPTH must be rejected");
        assert!(
            err.0.contains("array nesting exceeds maximum depth"),
            "expected an array-nesting-depth error, got: {}",
            err.0
        );
    }

    #[test]
    fn decode_16384_open_brackets_raw_udp_payload_rejected_fast() {
        // The exact attack payload named in the audit: "a single datagram
        // of 16384 '[' followed by 16384 ']'" fed straight to `decode` —
        // precisely what `OscReceiver::recv` does with untrusted UDP bytes.
        // Built as raw wire bytes (not via `OscArg`/`encode`) so this is
        // the actual attacker-controlled payload, not a stand-in for it.
        // Brackets consume zero argument bytes, so ~32 KB of type-tag
        // string alone requests a 16384-level-deep `Vec<OscArg>` nesting.
        // Before the depth guard, construction itself would not recurse
        // (the loop in `read_typed_args` is iterative) and would succeed;
        // only the *next* recursive traversal of the resulting tree (Drop,
        // Debug, PartialEq, or `encode`) would overflow the stack. The
        // guard must instead reject during construction, within the first
        // `MAX_NESTING_DEPTH` loop iterations — i.e. cheaply and fast.
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(b"/a\0\0"); // 4-byte-aligned address, zero args
        let mut type_tags: Vec<u8> = vec![b','];
        type_tags.extend(core::iter::repeat_n(b'[', 16384));
        type_tags.extend(core::iter::repeat_n(b']', 16384));
        type_tags.push(0); // NUL terminator
        while !type_tags.len().is_multiple_of(4) {
            type_tags.push(0);
        }
        data.extend_from_slice(&type_tags);

        let err =
            decode(&data).expect_err("16384-deep raw array nesting must be rejected, not decoded");
        assert!(
            err.0.contains("array nesting exceeds maximum depth"),
            "expected an array-nesting-depth error, got: {}",
            err.0
        );
    }

    #[test]
    fn decode_array_nesting_within_max_depth_succeeds() {
        // One level below the cap must still round-trip: the guard must not
        // reject legitimate (if unusually deep) OSC arrays.
        let mut arg = OscArg::Int(7);
        for _ in 0..(MAX_NESTING_DEPTH - 1) {
            arg = OscArg::Array(vec![arg]);
        }
        let bytes = encode(&msg("/ok", vec![arg]));
        assert!(
            decode(&bytes).is_ok(),
            "expected in-limit array nesting to decode successfully"
        );
    }

    #[test]
    fn decode_bundle_nesting_exceeds_max_depth_returns_error() {
        // Build MAX_NESTING_DEPTH + 8 levels of bundle-within-bundle nesting
        // and confirm `decode` rejects it with a typed error instead of
        // recursing (`decode_bundle` -> `decode`) deep enough to blow the
        // stack. Assert on the error text so this can't silently pass for
        // the wrong reason if the depth guard itself regresses.
        let mut packet = msg("/leaf", vec![OscArg::Int(1)]);
        for _ in 0..(MAX_NESTING_DEPTH + 8) {
            packet = OscPacket::Bundle(OscBundle {
                time: OscTimeTag::IMMEDIATE,
                elements: vec![packet],
            });
        }
        let bytes = encode(&packet);
        let err =
            decode(&bytes).expect_err("bundle nesting beyond MAX_NESTING_DEPTH must be rejected");
        assert!(
            err.0.contains("bundle nesting exceeds maximum depth"),
            "expected a bundle-nesting-depth error, got: {}",
            err.0
        );
    }

    #[test]
    fn decode_2000_nested_bundles_raw_payload_rejected_fast() {
        // Raw-bytes bundle-within-bundle nesting at single-UDP-datagram
        // scale: 2000 levels x 20 bytes/level (`#bundle\0` + 8-byte time
        // tag + 4-byte size prefix) is ~40 KB, comfortably inside the
        // 65536-byte buffer `OscReceiver::recv` reads from an untrusted
        // socket. Built iteratively from the innermost message outward
        // (each wrap is flat byte concatenation, no recursion), so
        // constructing this fixture can never itself overflow the stack —
        // only the *old*, unguarded `decode_bundle` -> `decode` recursion
        // that would have walked back down through all 2000 levels was at
        // risk.
        fn wrap_bundle(inner: &[u8]) -> Vec<u8> {
            let mut buf = Vec::with_capacity(20 + inner.len());
            buf.extend_from_slice(b"#bundle\0");
            buf.extend_from_slice(&0u32.to_be_bytes()); // time tag: seconds
            buf.extend_from_slice(&1u32.to_be_bytes()); // time tag: fractional (IMMEDIATE)
            buf.extend_from_slice(&(inner.len() as u32).to_be_bytes());
            buf.extend_from_slice(inner);
            buf
        }

        let mut data = encode(&msg("/leaf", vec![OscArg::Int(1)]));
        for _ in 0..2000 {
            data = wrap_bundle(&data);
        }
        assert!(
            data.len() < 65536,
            "fixture must fit in a single UDP datagram like OscReceiver::recv reads, got {} bytes",
            data.len()
        );

        let err =
            decode(&data).expect_err("2000-deep raw bundle nesting must be rejected, not decoded");
        assert!(
            err.0.contains("bundle nesting exceeds maximum depth"),
            "expected a bundle-nesting-depth error, got: {}",
            err.0
        );
    }

    #[test]
    fn decode_bundle_nesting_within_max_depth_succeeds() {
        // One level below the cap must still round-trip.
        let mut packet = msg("/leaf", vec![OscArg::Int(7)]);
        for _ in 0..(MAX_NESTING_DEPTH - 1) {
            packet = OscPacket::Bundle(OscBundle {
                time: OscTimeTag::IMMEDIATE,
                elements: vec![packet],
            });
        }
        let bytes = encode(&packet);
        assert!(
            decode(&bytes).is_ok(),
            "expected in-limit bundle nesting to decode successfully"
        );
    }

    // ─── proptest: encode ∘ decode round-trip stability ───────────────────────
    //
    // Property: for any well-formed `OscMessage` this crate can construct, encoding it and
    // then decoding those bytes back must succeed, and re-encoding the decoded value must
    // reproduce the exact same bytes. This is the generalized form of `roundtrip_all_types`
    // above — proptest supplies arbitrary (address, args) combinations instead of one
    // hand-picked case, which is exactly the kind of coverage that would have caught both the
    // `read_str` alignment-padding OOB and the unbounded-nesting stack-overflow defects this
    // crate previously shipped.
    //
    // Byte-level comparison (`encode(&decode(&bytes)?) == bytes`) is used instead of
    // structural equality (`decoded == original`) so float args (which may be generated as
    // NaN) never cause a spurious failure: `roundtrip_all_types` documents the same rationale
    // ("Re-encode for stable float comparison").
    //
    // Array nesting is generated well under `MAX_NESTING_DEPTH` (32) so this test exercises
    // ordinary encode/decode behavior, not the depth guard (which has its own dedicated tests
    // above).
    mod proptest_roundtrip {
        use super::*;
        use alloc::string::String;
        use proptest::prelude::*;

        /// Printable-ASCII, NUL-free string strategy — satisfies `write_str`'s documented
        /// precondition (see `encode.rs`) so this test is never exercising the *known*,
        /// separately-documented embedded-NUL truncation hazard.
        fn arb_string(max_len: usize) -> impl Strategy<Value = String> {
            proptest::collection::vec(0x20u8..=0x7E, 0..max_len)
                .prop_map(|bytes| String::from_utf8(bytes).expect("ASCII range is valid UTF-8"))
        }

        /// OSC address: always starts with `/` so top-level `decode` dispatches to
        /// `decode_message` rather than erroring on the "must start with '/'" check.
        fn arb_address() -> impl Strategy<Value = String> {
            arb_string(16).prop_map(|s| alloc::format!("/{s}"))
        }

        fn arb_leaf_arg() -> impl Strategy<Value = OscArg> {
            prop_oneof![
                any::<i32>().prop_map(OscArg::Int),
                any::<f32>().prop_map(OscArg::Float),
                arb_string(24).prop_map(OscArg::String),
                proptest::collection::vec(any::<u8>(), 0..24).prop_map(OscArg::Blob),
                any::<i64>().prop_map(OscArg::Long),
                any::<f64>().prop_map(OscArg::Double),
                (any::<u32>(), any::<u32>()).prop_map(|(seconds, fractional)| {
                    OscArg::TimeTag(OscTimeTag {
                        seconds,
                        fractional,
                    })
                }),
                any::<char>().prop_map(OscArg::Char),
                (any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>())
                    .prop_map(|(r, g, b, a)| OscArg::Color(r, g, b, a)),
                any::<[u8; 4]>().prop_map(OscArg::Midi),
                any::<bool>().prop_map(OscArg::Bool),
                Just(OscArg::Nil),
                Just(OscArg::Impulse),
            ]
        }

        /// Recursive strategy for `OscArg`, including `Array`, bounded to a depth (4) and
        /// collection size (0..4 per level) that stay far below `MAX_NESTING_DEPTH` (32).
        fn arb_arg() -> impl Strategy<Value = OscArg> {
            arb_leaf_arg().prop_recursive(4, 32, 4, |inner| {
                proptest::collection::vec(inner, 0..4).prop_map(OscArg::Array)
            })
        }

        fn arb_message() -> impl Strategy<Value = OscMessage> {
            (arb_address(), proptest::collection::vec(arb_arg(), 0..6))
                .prop_map(|(address, args)| OscMessage { address, args })
        }

        proptest! {
            #[test]
            fn encode_decode_roundtrip_is_byte_stable(msg in arb_message()) {
                let bytes = encode(&OscPacket::Message(msg));
                let decoded = decode(&bytes).expect("encode() output must always decode successfully");
                let re_encoded = encode(&decoded);
                prop_assert_eq!(bytes, re_encoded);
            }
        }
    }
}
