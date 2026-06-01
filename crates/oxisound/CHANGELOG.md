# Changelog

All notable changes to the `oxisound` crate. Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

> **Note:** 1.0.0 requires an explicit release decision from the COOLJAPAN team.

## [0.1.0] - 2026-05-25

### Added
- **M0 — Skeleton**: Workspace setup, `Cargo.toml` with `default = ["pure"]` feature, `#![forbid(unsafe_code)]`.
- **M1 — Public facade**: `pub use` of all `oxisound-core` types; `default_output()`, `default_input()`, `enumerate_devices()` convenience functions; `CpalDevice` re-export under `pure` feature.
- **M2 — Convenience helpers**: `open_output()`, `open_input()`, `latency_ms()`, `format_devices()`.
- **M3 — Device selection + duplex**: `select_device()`, `duplex_stream()`; `jack_output()` behind `jack` feature; `asio_output()` behind `asio` feature.
- **M4 — Async API + sine tone**: `sine_test_tone()` pure-Rust sine wave generator; `async_output()` and `capture_stream()` behind `tokio` feature; re-exports `AsyncOutputStream` and `AsyncInputStream`.
- **M5 — Examples + docs**: Four worked examples (`device_info`, `sine_tone`, `playback`, `capture`); full rustdoc with `# Examples` on all public items; this CHANGELOG.
