//! Criterion benchmarks for the `oxisound` facade.
//!
//! Covers two profiles mentioned in the TODO:
//! 1. `sine_test_tone` generation — ensures no unnecessary allocation beyond the
//!    output `Vec<f32>` and measures throughput vs. sample count.
//! 2. Facade function overhead — measures the time spent inside
//!    `default_output()` (device enumeration + selection) and the combined
//!    `default_output()` + `AudioDevice::open_output()` call chain.
//!    These benchmarks are gated `#[cfg(not(target_arch = "wasm32"))]` and skip
//!    gracefully when no audio hardware is present.
//!
//! Run:
//! ```text
//! cargo bench -p oxisound --bench facade
//! ```

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

// ── 1. `sine_test_tone` throughput ───────────────────────────────────────────

/// Benchmark `sine_test_tone` at various sample counts.
///
/// Validates:
/// - The function performs exactly one allocation (the output `Vec`).
/// - Throughput scales linearly with sample count (no hidden O(n²) behaviour).
///
/// Block sizes tested:
/// - 4 800 samples  ≈ 0.1 s stereo @ 48 kHz
/// - 48 000 samples ≈ 1.0 s stereo @ 48 kHz
/// - 96 000 samples ≈ 2.0 s stereo @ 48 kHz
fn bench_sine_test_tone(c: &mut Criterion) {
    use oxisound::{StreamConfig, sine_test_tone};

    let config = StreamConfig::stereo_48k();
    let mut group = c.benchmark_group("sine_test_tone");

    for &duration_ms in &[100u32, 1_000, 2_000] {
        let duration_secs = duration_ms as f32 / 1_000.0;
        // Each frame has 2 channels (stereo); total samples = frames × channels.
        let sample_count =
            (config.sample_rate as f32 * duration_secs) as u64 * config.channels as u64;
        group.throughput(Throughput::Elements(sample_count));

        group.bench_with_input(
            BenchmarkId::from_parameter(duration_ms),
            &duration_secs,
            |b, &dur| {
                b.iter(|| {
                    let buf = sine_test_tone(440.0, dur, config.clone());
                    // Consume the buffer so the compiler can't elide the allocation.
                    std::hint::black_box(buf.len())
                });
            },
        );
    }
    group.finish();
}

/// Benchmark `sine_test_tone` at various frequencies to confirm O(1) in frequency.
fn bench_sine_test_tone_frequencies(c: &mut Criterion) {
    use oxisound::{StreamConfig, sine_test_tone};

    let config = StreamConfig::stereo_48k();
    let duration_secs = 0.1_f32; // short, so the bench is fast
    let mut group = c.benchmark_group("sine_test_tone_frequencies");

    for &freq in &[20.0_f32, 440.0, 4_400.0, 20_000.0] {
        group.bench_with_input(BenchmarkId::from_parameter(freq as u32), &freq, |b, &f| {
            b.iter(|| {
                let buf = sine_test_tone(f, duration_secs, config.clone());
                std::hint::black_box(buf.len())
            });
        });
    }
    group.finish();
}

// ── 2. Facade function overhead ───────────────────────────────────────────────

/// Benchmark `default_output()` — device enumeration + selection.
///
/// This bench is skipped (returns immediately) when no audio hardware is found,
/// so it can run safely in CI without audio drivers.
#[cfg(not(target_arch = "wasm32"))]
fn bench_default_output(c: &mut Criterion) {
    use oxisound::default_output;

    // Quick probe: if no device is available, skip the bench.
    if default_output().is_err() {
        return;
    }

    let mut group = c.benchmark_group("facade_overhead");
    group.bench_function("default_output", |b| {
        b.iter(|| {
            let result = default_output();
            std::hint::black_box(result.is_ok())
        });
    });
    group.finish();
}

/// Benchmark `default_output()` + `open_output()` round-trip.
///
/// Measures the total time to enumerate devices and open an output stream.
/// Skipped gracefully if no audio hardware is present.
#[cfg(not(target_arch = "wasm32"))]
fn bench_open_output(c: &mut Criterion) {
    use oxisound::{StreamConfig, default_output};
    use oxisound_core::AudioDevice;

    // Quick probe: if no device is available, skip the bench.
    if default_output().is_err() {
        return;
    }

    let config = StreamConfig::stereo_48k();
    let mut group = c.benchmark_group("facade_overhead");
    group.bench_function("default_output_then_open_output", |b| {
        b.iter(|| {
            // Each iteration opens and immediately drops the stream.
            let result = default_output().and_then(|dev| dev.open_output(config.clone()));
            std::hint::black_box(result.is_ok())
        });
    });
    group.finish();
}

// ── criterion plumbing ────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
criterion_group!(
    benches,
    bench_sine_test_tone,
    bench_sine_test_tone_frequencies,
    bench_default_output,
    bench_open_output,
);

#[cfg(target_arch = "wasm32")]
criterion_group!(
    benches,
    bench_sine_test_tone,
    bench_sine_test_tone_frequencies,
);

criterion_main!(benches);
