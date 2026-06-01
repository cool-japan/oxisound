//! Criterion benchmark: sample format conversion overhead.
//!
//! Measures the `FromSample`/`ToSample` dispatch overhead for different sample
//! formats used in oxisound-cpal stream builders.  The dasp_sample conversions
//! are compile-time-dispatched, so the numbers here reflect LLVM's ability to
//! vectorise/inline the conversion arithmetic at the given block size.
//!
//! Run: `cargo bench -p oxisound-cpal --bench format_conv`

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
// `Sample` brings `from_sample` and `to_sample` into scope (dasp_sample re-export).
use cpal::Sample;

fn bench_format_conversion(c: &mut Criterion) {
    let n = 512_usize; // one typical audio block
    let input_f32: Vec<f32> = (0..n).map(|i| (i as f32 / n as f32) * 2.0 - 1.0).collect();
    let input_i16: Vec<i16> = (0..n).map(|i| (i as i16).wrapping_mul(32)).collect();

    let mut group = c.benchmark_group("format_conversion");
    group.throughput(Throughput::Elements(n as u64));

    // F32 → F32: identity, establishes the measurement baseline.
    group.bench_function(BenchmarkId::new("f32_to_f32", n), |b| {
        b.iter(|| {
            let out: Vec<f32> = input_f32.iter().map(|&s| f32::from_sample(s)).collect();
            std::hint::black_box(out)
        });
    });

    // I16 → F32: integer-range normalisation (divide by i16::MAX).
    group.bench_function(BenchmarkId::new("i16_to_f32", n), |b| {
        b.iter(|| {
            let out: Vec<f32> = input_i16.iter().map(|&s| s.to_sample::<f32>()).collect();
            std::hint::black_box(out)
        });
    });

    // F32 → I16: float-to-integer with saturation.
    group.bench_function(BenchmarkId::new("f32_to_i16", n), |b| {
        b.iter(|| {
            let out: Vec<i16> = input_f32.iter().map(|&s| i16::from_sample(s)).collect();
            std::hint::black_box(out)
        });
    });

    // F32 → I32: widening integer conversion.
    group.bench_function(BenchmarkId::new("f32_to_i32", n), |b| {
        b.iter(|| {
            let out: Vec<i32> = input_f32.iter().map(|&s| i32::from_sample(s)).collect();
            std::hint::black_box(out)
        });
    });

    // F32 → F64: widening float conversion.
    group.bench_function(BenchmarkId::new("f32_to_f64", n), |b| {
        b.iter(|| {
            let out: Vec<f64> = input_f32.iter().map(|&s| f64::from_sample(s)).collect();
            std::hint::black_box(out)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_format_conversion);
criterion_main!(benches);
