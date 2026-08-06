//! Fuzz target for `oxisound_smf::parse`, the Standard MIDI File (SMF) byte-stream parser.
//!
//! This parser consumes arbitrary `.mid` file bytes — content an application reads from disk
//! or accepts as an upload, entirely attacker-controlled. On manual review it consistently uses
//! bounds-checked readers (`require_bytes`/`read_vlq`/`get()` in
//! `crates/oxisound-smf/src/parser.rs`), but it has no dedicated fuzz coverage, and the OSC
//! decoder in this same workspace shipped a boundary-arithmetic bug of exactly this kind (see
//! `osc_decode.rs` in this directory) despite looking bounds-clean on inspection too.
//!
//! `parse` must never panic on any input — only return `Ok` or `Err`.
//!
//! Run with (requires the nightly toolchain `cargo-fuzz` uses internally):
//! ```text
//! cargo +nightly fuzz run smf_parse
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use oxisound_smf::parse;

fuzz_target!(|data: &[u8]| {
    // The return value is deliberately discarded: the only property under test is "does not
    // panic" (no OOB slice index, no arithmetic overflow panic in a debug build, no
    // unwrap/expect on this path).
    let _ = parse(data);
});
