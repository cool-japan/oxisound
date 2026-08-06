//! Fuzz target for `oxisound_osc::decode`, the OSC packet decoder.
//!
//! This is the parser that `OscReceiver::recv` (crates/oxisound-osc/src/server.rs) feeds
//! directly with up to 65536 raw bytes read from an untrusted UDP socket — the receiving
//! application never gets a chance to validate the datagram before it reaches `decode`. It
//! previously shipped two confirmed defects in this exact class:
//!
//! - An out-of-bounds slice panic in `read_str` when 4-byte alignment padding pushed the read
//!   cursor past the end of the buffer (fixed; see the `decode_alignment_padding_past_end_returns_error`
//!   regression test in `crates/oxisound-osc/src/decode.rs`).
//! - Unbounded `[`/`]` array nesting and `#bundle`-within-`#bundle` nesting, which builds a
//!   tree deep enough that dropping/encoding/debug-printing it overflows the stack (fixed via
//!   `MAX_NESTING_DEPTH`; see the nesting-depth guard tests in the same file).
//!
//! `decode` must never panic on any input — only return `Ok` or `Err`. This target is the
//! general form of both regression tests above: it exercises arbitrary malformed bytes rather
//! than one specific crafted case, so it can catch the *next* boundary bug in this class before
//! it reaches an untrusted network listener.
//!
//! Run with (requires the nightly toolchain `cargo-fuzz` uses internally):
//! ```text
//! cargo +nightly fuzz run osc_decode
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use oxisound_osc::decode;

fuzz_target!(|data: &[u8]| {
    // The return value is deliberately discarded: the only property under test is "does not
    // panic" (no OOB slice index, no arithmetic overflow panic in a debug build, no
    // unwrap/expect on this path, and no stack overflow from unbounded recursion/nesting).
    let _ = decode(data);
});
