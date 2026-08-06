//! OSC packet encoding: converts `OscPacket` to a byte buffer.
//!
//! All multi-byte integers are big-endian per the OSC specification.

use alloc::{string::String, vec::Vec};

use crate::{OscArg, OscBundle, OscMessage, OscPacket, OscTimeTag};

/// Encodes an OSC packet (message or bundle) into a byte vector.
///
/// # Precondition: no embedded NUL in strings
///
/// Every string this packet carries — [`OscMessage::address`](crate::OscMessage::address) and
/// any [`OscArg::String`] — must not contain an embedded NUL (`'\0'`) byte. `encode` is
/// infallible and does not validate this: an embedded NUL does not produce an error, it
/// silently produces a packet that decodes back to a truncated string (the decoder treats the
/// first NUL as the end-of-string marker) with the remaining bytes reinterpreted as unrelated
/// packet data. Callers that accept string content from outside the program are responsible
/// for rejecting or escaping embedded NULs before constructing an [`OscMessage`]/[`OscArg`].
pub fn encode(packet: &OscPacket) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_packet(packet, &mut buf);
    buf
}

fn encode_packet(packet: &OscPacket, buf: &mut Vec<u8>) {
    match packet {
        OscPacket::Message(msg) => encode_message(msg, buf),
        OscPacket::Bundle(bundle) => encode_bundle(bundle, buf),
    }
}

fn encode_message(msg: &OscMessage, buf: &mut Vec<u8>) {
    write_str(&msg.address, buf);

    // Build the type tag string: starts with ',', then one tag per argument.
    let mut type_tags = String::from(",");
    collect_type_tags(&msg.args, &mut type_tags);
    write_str(&type_tags, buf);

    // Write argument data (flat, in order; array brackets only appear in type tags).
    write_args(&msg.args, buf);
}

/// Recursively appends type tags for `args`, including `[` / `]` delimiters for arrays.
fn collect_type_tags(args: &[OscArg], tags: &mut String) {
    for arg in args {
        match arg {
            OscArg::Array(inner) => {
                tags.push('[');
                collect_type_tags(inner, tags);
                tags.push(']');
            }
            other => tags.push(other.type_tag()),
        }
    }
}

/// Writes the raw bytes for each argument (no type tags, no brackets — flat sequence).
fn write_args(args: &[OscArg], buf: &mut Vec<u8>) {
    for arg in args {
        write_arg(arg, buf);
    }
}

fn write_arg(arg: &OscArg, buf: &mut Vec<u8>) {
    match arg {
        OscArg::Int(v) => buf.extend_from_slice(&v.to_be_bytes()),
        OscArg::Float(v) => buf.extend_from_slice(&v.to_be_bytes()),
        OscArg::String(s) => write_str(s, buf),
        OscArg::Blob(b) => write_blob(b, buf),
        OscArg::Long(v) => buf.extend_from_slice(&v.to_be_bytes()),
        OscArg::Double(v) => buf.extend_from_slice(&v.to_be_bytes()),
        OscArg::TimeTag(t) => write_timetag(t, buf),
        OscArg::Char(c) => {
            // OSC 'c' is a 4-byte big-endian char (lower 8 bits = ASCII).
            buf.extend_from_slice(&(*c as u32).to_be_bytes());
        }
        OscArg::Color(r, g, b, a) => buf.extend_from_slice(&[*r, *g, *b, *a]),
        OscArg::Midi(m) => buf.extend_from_slice(m),
        // T, F, N, I carry no argument bytes — only the type tag.
        OscArg::Bool(_) | OscArg::Nil | OscArg::Impulse => {}
        OscArg::Array(inner) => {
            // Argument bytes from array elements are emitted flat, same as top level.
            write_args(inner, buf);
        }
    }
}

fn encode_bundle(bundle: &OscBundle, buf: &mut Vec<u8>) {
    // "#bundle\0" is exactly 8 bytes, already 4-byte aligned.
    buf.extend_from_slice(b"#bundle\0");
    write_timetag(&bundle.time, buf);

    for element in &bundle.elements {
        // Each element is prefixed by its 4-byte byte count.
        let start = buf.len();
        buf.extend_from_slice(&[0u8; 4]); // placeholder for size
        encode_packet(element, buf);
        let element_len = (buf.len() - start - 4) as u32;
        buf[start..start + 4].copy_from_slice(&element_len.to_be_bytes());
    }
}

fn write_timetag(t: &OscTimeTag, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&t.seconds.to_be_bytes());
    buf.extend_from_slice(&t.fractional.to_be_bytes());
}

/// Writes `s` followed by a NUL terminator, then pads to the next 4-byte boundary.
///
/// # Precondition
///
/// `s` must not contain an embedded NUL (`'\0'`) byte. OSC strings are NUL-terminated on the
/// wire, so `read_str` (the decoder counterpart) stops at the *first* NUL it finds. Passing a
/// string with an embedded NUL therefore does not error here — it silently produces a packet
/// that decodes back to a *truncated* string, with the bytes after the embedded NUL
/// reinterpreted as whatever comes next (the type-tag string or the following argument). This
/// function is infallible by design (matching [`encode`]'s infallible signature), so the
/// precondition is enforced by the caller, not by this function; construct [`OscArg::String`]
/// and [`OscMessage::address`](crate::OscMessage::address) values accordingly. OSC 1.0 also
/// specifies printable-ASCII string content — this implementation is more permissive and
/// accepts any NUL-free valid UTF-8, which round-trips correctly but is technically off-spec
/// for non-ASCII text.
fn write_str(s: &str, buf: &mut Vec<u8>) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0); // NUL terminator
    pad4(buf);
}

/// Writes a 4-byte big-endian length, then the blob bytes, then pads to 4-byte boundary.
///
/// # Precondition
///
/// `b.len()` must fit in a `u32` (OSC blobs are length-prefixed with a 4-byte big-endian
/// count). A longer blob is not rejected — `b.len() as u32` silently truncates the stored
/// length, which corrupts the packet (the writer still emits all of `b`'s bytes, but the
/// decoder will only read back the truncated count). Not reachable in practice on a 64-bit
/// target without first allocating a multi-gigabyte buffer.
fn write_blob(b: &[u8], buf: &mut Vec<u8>) {
    buf.extend_from_slice(&(b.len() as u32).to_be_bytes());
    buf.extend_from_slice(b);
    pad4(buf);
}

fn pad4(buf: &mut Vec<u8>) {
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    // Explicit `alloc` imports: under `--no-default-features` (no_std), `vec!` and
    // `.to_string()` are not in scope via any prelude the way they are under `std` — unlike
    // this crate's other test modules (e.g. `decode.rs`), this file's top-level `use` only
    // pulls in `String`/`Vec`, not the `vec!` macro or `ToString`.
    use crate::{OscArg, OscMessage, OscPacket, OscTimeTag, decode};
    use alloc::{string::ToString, vec};

    fn msg(address: &str, args: Vec<OscArg>) -> OscPacket {
        OscPacket::Message(OscMessage {
            address: address.to_string(),
            args,
        })
    }

    #[test]
    fn encode_simple_message() {
        let packet = msg(
            "/test",
            vec![
                OscArg::Int(42),
                OscArg::Float(core::f32::consts::PI),
                OscArg::String("hello".to_string()),
            ],
        );
        let bytes = encode(&packet);
        let decoded = decode(&bytes).expect("round-trip decode failed");
        // Float comparison needs tolerance; check via re-encode instead.
        let bytes2 = encode(&decoded);
        assert_eq!(bytes, bytes2);
    }

    #[test]
    fn encode_empty_message() {
        let packet = msg("/empty", vec![]);
        let bytes = encode(&packet);
        let decoded = decode(&bytes).expect("decode failed");
        assert_eq!(decoded, packet);
    }

    #[test]
    fn encode_bool_nil_impulse() {
        let packet = msg(
            "/flags",
            vec![
                OscArg::Bool(true),
                OscArg::Bool(false),
                OscArg::Nil,
                OscArg::Impulse,
            ],
        );
        let bytes = encode(&packet);
        // T, F, N, I carry no bytes beyond the type tags.
        let decoded = decode(&bytes).expect("decode failed");
        assert_eq!(decoded, packet);
    }

    #[test]
    fn encode_blob_non_multiple_of_4() {
        // A 5-byte blob: 4-byte len + 5 bytes + 3 bytes pad = 12 bytes of blob section.
        let blob_data = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
        let packet = msg("/blob", vec![OscArg::Blob(blob_data.clone())]);
        let bytes = encode(&packet);
        let decoded = decode(&bytes).expect("decode failed");
        assert_eq!(decoded, packet);
    }

    #[test]
    fn embedded_nul_in_string_arg_silently_truncates_on_roundtrip() {
        // Documents (rather than fixes) the precondition on `write_str`/`encode`: an
        // embedded NUL in an `OscArg::String` is not rejected — the decoder's `read_str`
        // stops at the *first* NUL, so the value that comes back out is silently truncated
        // instead of matching what went in. This test exists so the hazard has a permanent,
        // executable demonstration instead of only living in a doc comment; it intentionally
        // asserts the *broken* (truncated) round-trip, not equality with the original.
        let packet = msg("/nul", vec![OscArg::String("abc\0def".to_string())]);
        let bytes = encode(&packet);
        let decoded = decode(&bytes)
            .expect("decode of a well-formed (if semantically broken) packet must still succeed");
        match decoded {
            OscPacket::Message(m) => match &m.args[0] {
                OscArg::String(s) => assert_eq!(
                    s, "abc",
                    "embedded NUL must truncate the string at the first NUL byte, matching the \
                     documented precondition on OscArg::String / OscMessage::address"
                ),
                other => panic!("expected OscArg::String, got {other:?}"),
            },
            other => panic!("expected OscPacket::Message, got {other:?}"),
        }
    }

    #[test]
    fn encode_array_nested() {
        let packet = msg(
            "/array",
            vec![OscArg::Array(vec![
                OscArg::Int(1),
                OscArg::Int(2),
                OscArg::Array(vec![OscArg::Float(3.0)]),
            ])],
        );
        let bytes = encode(&packet);
        let decoded = decode(&bytes).expect("decode failed");
        assert_eq!(decoded, packet);
    }

    #[test]
    fn vlq_padding_string_lengths() {
        // Strings of length 1, 2, 3, 4 must all pad to a 4-byte boundary after NUL.
        for len in 1usize..=4 {
            let s: alloc::string::String = "x".repeat(len);
            let mut buf = Vec::new();
            write_str(&s, &mut buf);
            assert_eq!(
                buf.len() % 4,
                0,
                "string of len {} produced unaligned buffer (len {})",
                len,
                buf.len()
            );
            // Length 1 -> "x\0\0\0" = 4, length 4 -> "xxxx\0\0\0\0" = 8, etc.
            let expected = (len + 1).div_ceil(4) * 4;
            assert_eq!(buf.len(), expected);
        }
    }

    #[test]
    fn type_tag_chars() {
        assert_eq!(OscArg::Int(0).type_tag(), 'i');
        assert_eq!(OscArg::Float(0.0).type_tag(), 'f');
        assert_eq!(OscArg::String(alloc::string::String::new()).type_tag(), 's');
        assert_eq!(OscArg::Blob(alloc::vec![]).type_tag(), 'b');
        assert_eq!(OscArg::Long(0).type_tag(), 'h');
        assert_eq!(OscArg::Double(0.0).type_tag(), 'd');
        assert_eq!(OscArg::TimeTag(OscTimeTag::IMMEDIATE).type_tag(), 't');
        assert_eq!(OscArg::Char('A').type_tag(), 'c');
        assert_eq!(OscArg::Color(0, 0, 0, 0).type_tag(), 'r');
        assert_eq!(OscArg::Midi([0; 4]).type_tag(), 'm');
        assert_eq!(OscArg::Bool(true).type_tag(), 'T');
        assert_eq!(OscArg::Bool(false).type_tag(), 'F');
        assert_eq!(OscArg::Nil.type_tag(), 'N');
        assert_eq!(OscArg::Impulse.type_tag(), 'I');
        assert_eq!(OscArg::Array(alloc::vec![]).type_tag(), '[');
    }
}
