//! OSC packet decoding: converts a byte slice into an `OscPacket`.
//!
//! The decoder is zero-copy for blobs and strings during intermediate parsing;
//! final values are allocated into owned `OscArg` variants.

use alloc::{string::ToString, vec, vec::Vec};

use crate::{OscArg, OscBundle, OscError, OscMessage, OscPacket, OscTimeTag};

/// Decodes an OSC packet from a byte slice.
///
/// Returns an error if the data is truncated, misaligned, or has an invalid structure.
pub fn decode(data: &[u8]) -> Result<OscPacket, OscError> {
    if data.starts_with(b"#bundle\0") {
        decode_bundle(data).map(OscPacket::Bundle)
    } else if data.first().copied() == Some(b'/') {
        decode_message(data).map(OscPacket::Message)
    } else {
        Err(OscError(
            "packet must start with '/' (message) or '#bundle\\0' (bundle)".to_string(),
        ))
    }
}

// ─── bundle ───────────────────────────────────────────────────────────────────

fn decode_bundle(data: &[u8]) -> Result<OscBundle, OscError> {
    // "#bundle\0" is 8 bytes; time tag is the next 8 bytes.
    let mut pos = 8usize;
    let time = read_timetag(data, &mut pos)?;

    let mut elements = Vec::new();
    while pos < data.len() {
        let size = read_u32(data, &mut pos)? as usize;
        if pos + size > data.len() {
            return Err(OscError(
                "bundle element size exceeds remaining data".to_string(),
            ));
        }
        let element_data = &data[pos..pos + size];
        elements.push(decode(element_data)?);
        pos += size;
    }

    Ok(OscBundle { time, elements })
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
            '[' => stack.push(Vec::new()),
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
    if *pos + N > data.len() {
        return Err(OscError(alloc::format!(
            "truncated data: need {N} bytes at offset {}, have {}",
            *pos,
            data.len()
        )));
    }
    let mut buf = [0u8; N];
    buf.copy_from_slice(&data[*pos..*pos + N]);
    *pos += N;
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
}
