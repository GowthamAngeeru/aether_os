use aether_os::core::bloom::BloomFilter;
use aether_os::core::vector::VectorEngine;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

fn bench_bloom_filter(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_filter");
    let filter = BloomFilter::new(100_000, 0.01);

    for i in 0..50_000usize {
        filter.insert(&format!("query-{}", i));
    }

    group.bench_function("contains_miss", |b| {
        b.iter(|| filter.contains(black_box("this-query-was-never-inserted")))
    });

    group.bench_function("contains_hit", |b| {
        b.iter(|| filter.contains(black_box("query-25000")))
    });

    group.finish();
}

fn bench_cosine_similarity(c: &mut Criterion) {
    let mut group = c.benchmark_group("cosine_similarity");

    let vec_a: Vec<f32> = (0..384).map(|i| (i as f32).sin()).collect();
    let vec_b: Vec<f32> = (0..384).map(|i| (i as f32).cos()).collect();

    let norm_a: f32 = vec_a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = vec_b.iter().map(|x| x * x).sum::<f32>().sqrt();
    let vec_a_norm: Vec<f32> = vec_a.iter().map(|x| x / norm_a).collect();
    let vec_b_norm: Vec<f32> = vec_b.iter().map(|x| x / norm_b).collect();

    group.bench_function("unit_vector_dot_product_384dim", |b| {
        b.iter(|| VectorEngine::cosine_similarity(black_box(&vec_a_norm), black_box(&vec_b_norm)))
    });

    group.finish();
}

fn bench_cache_scan(c: &mut Criterion) {
    use aether_os::core::cache::SemanticCache;
    use std::time::Duration;

    let mut group = c.benchmark_group("cache_scan");
    let _rt = tokio::runtime::Runtime::new().unwrap();

    for cache_size in [100usize, 1_000, 10_000] {
        let cache = SemanticCache::new_in_memory(50_000, Duration::from_secs(86400));

        for i in 0..cache_size {
            let fake_vector: Vec<f32> = (0..384).map(|j| ((i + j) as f32).sin()).collect();
            let norm: f32 = fake_vector.iter().map(|x| x * x).sum::<f32>().sqrt();
            let normalized: Vec<f32> = fake_vector.iter().map(|x| x / norm).collect();
            cache.insert(
                format!("query-{}", i),
                normalized,
                format!("response-{}", i),
            );
        }

        let zero_vector = vec![0.0f32; 384];
        group.bench_with_input(
            BenchmarkId::new("search_miss", cache_size),
            &cache_size,
            |b, _| b.iter(|| cache.search(black_box(&zero_vector))),
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_bloom_filter,
    bench_cosine_similarity,
    bench_cache_scan
);
criterion_main!(benches);
