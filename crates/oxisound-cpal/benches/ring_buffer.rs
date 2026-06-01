//! Criterion benchmark: SPSC ring buffer throughput.
//!
//! Measures how many f32 samples per second can be pushed/popped between a
//! producer and consumer in a tight loop, simulating the audio callback
//! pattern used in oxisound-cpal.
//!
//! Run: `cargo bench -p oxisound-cpal --bench ring_buffer`

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};

fn bench_ring_buffer_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_throughput");

    for &buffer_frames in &[128usize, 256, 512, 1024] {
        let sample_count = buffer_frames * 2; // stereo
        group.throughput(Throughput::Elements(sample_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(buffer_frames),
            &sample_count,
            |b, &n| {
                let rb = HeapRb::<f32>::new(n * 4); // 4x capacity — never stalls
                let (mut prod, mut cons) = rb.split();
                // Pre-fill half capacity so the consumer always has data on first iter.
                for i in 0..n * 2 {
                    let _ = prod.try_push(i as f32 * 0.001);
                }
                b.iter(|| {
                    // Simulate one audio callback: drain N samples, then refill N.
                    let mut sum = 0.0_f32;
                    for _ in 0..n {
                        sum += cons.try_pop().unwrap_or(0.0);
                    }
                    for i in 0..n {
                        let _ = prod.try_push(i as f32 * 0.001);
                    }
                    std::hint::black_box(sum)
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_ring_buffer_throughput);
criterion_main!(benches);
