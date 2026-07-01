## Rust component benchmarks (Criterion)

Run: `cargo bench`
Hardware: [run `wmic cpu get name`]
Configuration: 100 samples, 5-second measurement window

### Bloom filter

| Operation                  | P50 latency |
| -------------------------- | ----------- |
| `contains` — definite miss | 27 ns       |
| `contains` — definite hit  | 24 ns       |

O(1) probabilistic early rejection. At 27ns per check, the Bloom
filter adds negligible overhead even at 100,000 RPS — 2.7ms of
total CPU time per second across all requests.

Implementation: `Vec<AtomicU64>` true bit packing with
Kirsch-Mitzenmacher double hashing. `Vec<bool>` would use 8×
the memory; bit packing keeps the filter in L1 cache.

### Cosine similarity — 384-dimensional unit vectors

| Operation                          | P50 latency |
| ---------------------------------- | ----------- |
| Unit vector dot product (384 dims) | 57 ns       |

AllMiniLML6V2 produces L2-normalized unit vectors.
cosine_similarity(A, B) = dot_product(A, B) for unit vectors,
eliminating two sqrt() calls per comparison.

### Semantic cache scan — O(N) linear

| Cache size     | P50 scan latency | Per-entry cost |
| -------------- | ---------------- | -------------- |
| 100 entries    | 17.5 μs          | 175 ns         |
| 1,000 entries  | 175 μs           | 175 ns         |
| 10,000 entries | 3.04 ms          | 304 ns         |

Per-entry cost exceeds raw cosine similarity (57ns) by ~247ns
due to DashMap iterator overhead, expiry check, and branch
prediction cost. The scan is memory-bandwidth-bound, not
compute-bound, at large cache sizes.

**Scale boundary:** At 10,000 entries, scan is 3.04ms vs
2,000ms LLM latency — 660x faster. Extrapolating, HNSW
becomes advantageous at approximately 500,000–650,000 cached
entries where O(N) scan approaches 200ms. Upgrade path:
RedisVL with HNSW approximate nearest neighbor index.
