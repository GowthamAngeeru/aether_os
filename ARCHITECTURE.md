# AetherOS — System Architecture & Design Decisions

## 1. Problem Statement

Standard RAG applications have two expensive failure modes at scale:

**Redundant LLM calls:** 40-60% of user queries in domain-specific 
applications are semantically equivalent to previously answered questions.
Each call costs ~$0.002 and takes 2-3 seconds. Standard caches cannot
detect semantic equivalence — only exact string matches.

**Hallucination on domain knowledge:** Foundation LLMs generate 
plausible-sounding but incorrect answers when asked about proprietary 
systems, internal documentation, or recent events outside training data.

AetherOS eliminates both with a semantic cache + vector retrieval pipeline.

---

## 2. System Topology
┌──────────┐    HTTPS/SSE    ┌──────────────────────┐
│ Next.js  │◄───────────────►│   Rust Gateway       │
│ (Vercel) │                 │   (axum + tokio)     │
└──────────┘                 └────────┬──────────────┘
                                      │
                    ┌─────────────────┼──────────────────┐
                    │                 │                  │
                    ▼                 ▼                  ▼
                    ┌──────────┐    ┌──────────────┐   ┌──────────────┐
                    │  Upstash │    │ Qdrant Cloud │   │ Python Brain │
                    │  Redis   │    │ (Vector DB)  │   │ (FastAPI)    │
                    │ (Cache)  │    │              │   │ LangChain    │
                    └──────────┘    └──────────────┘   └──────────────┘

---

## 3. Request Lifecycle

### Phase 1 — Shield Layer (Layer 4, Network)
Every request passes through two defenses before any application logic runs:

**Token Bucket Rate Limiter**
- Implementation: `parking_lot::Mutex` + `DashMap<IpAddr, TokenBucket>`
- Per-IP isolation: one client cannot starve others
- Refill rate: configurable via `RATE_LIMIT_RPS` env var
- Response on limit: 429 with `Retry-After` header (RFC 6585)

**Bloom Filter — Cache Penetration Shield**
- Implementation: `Vec<AtomicU64>` (true bit packing, not `Vec<bool>`)
- Hash function: FNV-1a with Kirsch-Mitzenmacher double hashing
- Guarantee: Definitive NO — keys not in cache are rejected at O(1)
- False positive rate: 1% at 100,000 expected elements (~117KB RAM)

### Phase 2 — Embedding Engine
Input:  Raw prompt string
Engine: fastembed-rs + AllMiniLML6V2 (ONNX runtime, local CPU)
Output: 384-dimensional L2-normalized unit vector
Cost:   $0.000 (no API call)
Time:   ~200-250ms (warm ONNX runtime)

**Why local embedding over OpenAI Embeddings API?**
- Eliminates ~150ms network round-trip per request
- Eliminates ~$0.0001 per embedding API call
- No dependency on external availability
- AllMiniLML6V2 matches OpenAI ada-002 quality for semantic similarity

### Phase 3 — Semantic Cache (The Core Innovation)
Query:  Cosine similarity between new vector and all cached vectors
Match:  score ≥ 0.92 → cache HIT → return response, skip LLM
No match: cache MISS → proceed to Qdrant

**Threshold selection (0.92):**

| Threshold | Effect |
|---|---|
| < 0.90 | False positives — unrelated topics incorrectly match |
| 0.90-0.92 | Optimal — catches paraphrases, rejects topic shifts |
| > 0.95 | False negatives — clear paraphrases miss the cache |

**Unit vector optimization:**
`AllMiniLML6V2` produces L2-normalized vectors where `||v|| = 1.0`.
For unit vectors: `cosine_similarity(A, B) = dot_product(A, B)`.
This eliminates two `sqrt()` calls per comparison — significant savings
at scale with O(N) scan across 50,000 cached vectors.

**Two-layer storage:**
DashMap (RAM):

Sub-millisecond O(N) cosine scan
Capacity: 50,000 vectors (~110MB RAM)
LRU eviction at 80% capacity warning

Redis (Persistent):

Write-through: every insert fires background Tokio task
Zero user latency cost for Redis write
TTL stored as Unix timestamp — preserved across restarts
Restore on boot: SCAN cursor (never KEYS * which blocks Redis)

Scale boundary:
N > 50,000 → upgrade to RedisVL with HNSW approximate nearest
neighbor index. O(log N) search, scales to millions of vectors.

### Phase 4 — Vector Retrieval (Cache Miss Path)
Query:  Same 384-dim vector against Qdrant collection
Index:  HNSW (Hierarchical Navigable Small World)
Return: Top-5 most similar document chunks with metadata
Distance metric: Cosine (optimal for unit-normalized vectors)

**Why Qdrant over Pinecone/Weaviate?**
- Written in Rust — consistent with gateway technology
- HNSW index: O(log N) approximate nearest neighbor
- Self-hosted: no per-query cost, data sovereignty
- Free cloud tier for demonstration deployment

### Phase 5 — Intelligence Layer
Retrieved chunks are injected into the prompt as context:

```python
system_prompt = """You are AetherOS, an expert AI assistant.
Use the following retrieved context to answer accurately.
Never hallucinate facts not present in the context.

Retrieved Context:
{top_5_qdrant_chunks}"""
```

LangChain streams tokens via `llm.astream()`. Each token is forwarded
immediately as an SSE frame — zero buffering, true streaming.

---

## 4. Transport Architecture

### Primary (Deployed): HTTP/SSE
Rust Gateway ──HTTP POST──► Python Brain
◄──SSE stream── Python Brain

**Why not gRPC in production?**
Cloudflare's reverse proxy (used by Render free tier) strips HTTP/2
trailers required for gRPC server-side streaming. HTTP/SSE works
correctly through all proxy layers.

### Enterprise Branch (`enterprise-grpc`): gRPC + Protobufs
Rust Gateway ──Protobuf──► Python Brain (grpcio)
◄──stream────  (tonic RagServiceClient)

**Proto contract:** `brain/proto/aether.proto`

| Property | gRPC | HTTP/SSE |
|---|---|---|
| Payload encoding | Binary Protobuf | JSON + text |
| Payload size | ~60% smaller | Baseline |
| Streaming | Native HTTP/2 | SSE |
| Proxy compatibility | Requires private network | Works everywhere |
| Type safety | Compile-time (proto) | Runtime validation |

**Restoring gRPC:** Requires private networking (Railway, Fly.io, or
paid Render). Code change: 2 files (`src/brain/client.rs` → 
`src/grpc/mod.rs`, `brain/src/main.py` → grpcio server).

---

## 5. Failure Modes & Mitigations

| Failure | Detection | Mitigation | User Impact |
|---|---|---|---|
| Redis unavailable | Startup health check | RAM-only mode, no persistence | Higher cost on restart |
| Qdrant unavailable | 5× retry with 2s backoff | Skip context retrieval, blind prompt | Lower response accuracy |
| Python Brain cold start | 90s timeout + DeadlineExceeded | User-facing "warming up" message | 30-60s first request |
| Cache capacity exceeded | 80% fill warning log | LRU eviction of 500 least-used entries | Slightly lower hit rate |
| Embedding dimension mismatch | Startup warm-up validation | Process exit with clear error | Startup failure only |
| Rate limit exceeded | Token bucket empty | 429 + Retry-After header | Per-IP throttling |

---

## 6. Scale Boundaries & Upgrade Paths
Current design is optimized for:
Concurrent users:    < 1,000
Cached vectors:      < 50,000
Documents ingested:  < 10,000 chunks
Upgrade path at scale:
50K+ vectors    → RedisVL + HNSW (replace DashMap O(N) scan)
1K+ RPS         → Horizontal Rust gateway scaling (stateless)
10K+ RPS        → Kafka/NATS between gateway and Brain
Multi-region     → Qdrant cluster + Redis Sentinel

---

## 7. Key Metrics

| Metric | Value | Measurement |
|---|---|---|
| Cache hit rate | 95.8% | k6, 261 requests, similar query pool |
| Pipeline P95 latency | 358ms | Includes 200-250ms ONNX embedding |
| Cache lookup time | <1ms | DashMap cosine similarity |
| LLM generation time | ~2000-3000ms | gpt-4o-mini, 1024 token budget |
| Error rate under load | 0.00% | k6, 10 concurrent VUs |
| Cost reduction | 95.8% | On semantically similar query traffic |
| Bloom Filter RAM | ~117KB | 100K elements, 1% FP rate |
| Embedding model | 0MB API | AllMiniLML6V2, local ONNX |