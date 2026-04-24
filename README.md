# ⚡ AetherOS — Enterprise AI Edge Gateway

> A production-grade, distributed RAG gateway built in Rust.  
> Semantic caching eliminates redundant LLM API calls — **95.8% cache 
> hit rate** under load, reducing OpenAI spend by up to **95.8%** 
> on repeated semantic queries.

[![Demo Video](https://img.shields.io/badge/▶_Demo-YouTube-red?style=for-the-badge)](YOUR_YOUTUBE_LINK)
[![Live App](https://img.shields.io/badge/Live-Vercel-black?style=for-the-badge)](YOUR_VERCEL_LINK)
[![Architecture](https://img.shields.io/badge/Docs-Architecture-blue?style=for-the-badge)](./ARCHITECTURE.md)

---

## The Problem It Solves

RAG applications have two expensive failure modes:Problem 1 — Redundant LLM calls
User A asks: "What is AetherOS?"       → LLM call → $0.002
User B asks: "Can you explain AetherOS?" → LLM call → $0.002 (same answer)AetherOS solution: Cosine similarity (threshold 0.92) detects semantic
equivalence → serves cached answer → $0.000, no LLM callProblem 2 — LLM hallucination on domain knowledge
LLM asked about YOUR system: makes up plausible-sounding wrong answersAetherOS solution: Qdrant vector retrieval injects real document
context into the prompt before LLM generation

---

## Benchmark Results

Measured locally with k6 · 10 concurrent VUs · 261 requests

| Metric | With AetherOS Cache | Without Cache |
|---|---|---|
| Cache Hit Latency P95 | **358ms** | N/A |
| LLM Generation Latency | ~2000-3000ms | ~2000-3000ms |
| Cache Hit Rate | **95.8%** | 0% |
| Error Rate | **0.00%** | — |
| API Cost (261 requests) | **~$0.022** | **~$0.522** |
| **Cost Reduction** | **95.8%** | baseline |

> **Note on latency:** The 275ms pipeline includes ~200-250ms for local
> ONNX embedding inference via `fastembed-rs` (no API call). The cache
> lookup itself is sub-1ms. The primary value of the cache is
> **cost elimination** — a cache hit at 275ms saves a $0.002 
> OpenAI API call and 2-3 seconds of LLM generation time.

---

## System Architecture
┌─────────────────────────────────────────────────────────────────┐
│                         USER (Browser)                          │
└──────────────────────────────┬──────────────────────────────────┘
                               │ HTTPS POST /generate
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                    RUST GATEWAY (axum + tokio)                  │
│                                                                 │
│  ┌─────────────────┐   ┌──────────────┐   ┌─────────────────┐   │
│  │  Token Bucket   │   │ Bloom Filter │   │  Request ID     │   │
│  │  Rate Limiter   │   │ (Cache Guard)│   │  (UUID Trace)   │   │
│  │  (per-IP)       │   │  FNV-1a hash │   │                 │   │
│  └────────┬────────┘   └──────┬───────┘   └─────────────────┘   │
│           │  Layer 4          │  Layer 7                        │
│           └──────────┬────────┘                                 │
│                      ▼                                          │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │         fastembed-rs + ONNX Runtime (local CPU)           │  │
│  │    prompt → 384-dim unit vector  (~200ms, $0.000)         │  │
│  └──────────────────────────┬────────────────────────────────┘  │
│                             │                                   │
│             ┌───────────────▼───────────────┐                    │
│             │     SEMANTIC CACHE LOOKUP     │                   │
│             │   cosine_similarity ≥ 0.92?   │                    │
│             └──────────┬────────────┬───────┘                   │
│                        │ HIT        │ MISS                      │
│                        ▼            ▼                           │
│             ┌──────────────┐ ┌──────────────────────────────┐   │
│             │ Redis Cache  │ │   Qdrant Vector Search       │   │
│             │ ~1ms lookup  │ │   top-5 chunks, HNSW index   │   │
│             │ $0.000 cost  │ └──────────────┬───────────────┘   │
│             └──────┬───────┘                │                   │
│                    │                        ▼                   │
│                    │         ┌──────────────────────────────┐   │
│                    │         │    PYTHON BRAIN (FastAPI)    │   │
│                    │         │  LangChain + gpt-4o-mini     │   │
│                    │         │  Context-grounded generation │   │
│                    │         │  ~2000ms, ~$0.002/call       │   │
│                    │         └──────────────┬───────────────┘   │
│             ┌──────▼─────────────────────────▼───────────────┐  │
│             │         SSE TOKEN STREAM → BROWSER             │  │
│             └────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘

---

## Core Engineering Decisions

### Why Rust for the Gateway?
Rust has **zero garbage collection pauses**. On a 275ms cache hit path, 
a Go/JVM GC pause of 10-50ms is a meaningful latency spike. Rust also 
runs `fastembed-rs` natively — embedding locally means no OpenAI 
Embeddings API call, saving ~150ms and ~$0.0001 per request.

### Why Cosine Similarity at 0.92 Threshold?
Chosen empirically: below 0.90 produces false positives (unrelated 
topics match), above 0.95 misses clear paraphrases. `AllMiniLML6V2` 
produces L2-normalized unit vectors, so cosine similarity reduces to 
a dot product — eliminating two `sqrt()` calls per comparison.

### Why DashMap + Redis (Two Cache Layers)?DashMap (RAM):  Sub-millisecond O(N) cosine scan, 50K vector limit
Redis (Disk):   Persistence across restarts, TTL preserved via Unix
timestamps (not reset on reboot)
Upgrade path:   RedisVL + HNSW index when N > 50,000 vectors

### Dual Transport Architecture
The inter-service bridge was designed with **gRPC + Protocol Buffers** 
for binary efficiency and strict typing. The deployed version uses 
**HTTP/SSE** for Cloudflare proxy compatibility on free-tier infrastructure.

| | gRPC Branch (`enterprise-grpc`) | HTTP/SSE Branch (`main`) |
|---|---|---|
| Transport | HTTP/2 + Protobufs | HTTP/1.1 + SSE |
| Payload | Binary (~60% smaller) | JSON/text |
| Streaming | Native bidirectional | Server-sent events |
| Use case | Private network deployment | Public cloud (Cloudflare) |
| Status | Preserved, local demo | Deployed on Render |

---

## Tech Stack

| Layer | Technology | Why |
|---|---|---|
| **Gateway** | Rust, Axum, Tokio | Zero GC, fearless concurrency |
| **Embeddings** | fastembed-rs, ONNX | Local inference, no API cost |
| **Semantic Cache** | DashMap + Redis | RAM speed + restart persistence |
| **Vector DB** | Qdrant | HNSW index, written in Rust |
| **Rate Limiting** | Token Bucket (parking_lot) | Lock-free per-IP throttling |
| **Cache Guard** | Bloom Filter (AtomicU64) | O(1) cache penetration prevention |
| **AI Pipeline** | Python, FastAPI, LangChain | Unmatched AI ecosystem |
| **LLM** | OpenAI gpt-4o-mini | Cost-efficient, streaming API |
| **Frontend** | Next.js 16, Tailwind | SSE streaming, React Server Components |
| **Deployment** | Render + Upstash + Qdrant Cloud + Vercel | Full cloud, free tier |

---

## Quick Start

### Prerequisites
- Rust toolchain (stable)
- Python 3.10+
- Node.js 20+
- Docker Desktop

### Boot Sequence

```bash1. Start infrastructure
docker-compose up -d2. Ingest knowledge base into Qdrant
cargo run --bin ingest3. Start Python Brain (Port 8000)
cd brain
source venv/bin/activate  # Windows: .\venv\Scripts\activate
uvicorn src.main:app --host 0.0.0.0 --port 80004. Start Rust Gateway (Port 3000)
cd ..
cargo run5. Start Next.js UI (Port 3001)
cd web_ui
npm install && npm run dev -- -p 30016. Run benchmarks
k6 run benchmarks/load_test.js

### Environment Variables

```bashRust Gateway
REDIS_URL=redis://127.0.0.1:6379
QDRANT_URL=http://localhost:6334
BRAIN_URL=http://localhost:8000
FRONTEND_URL=http://localhost:3001
QDRANT_COLLECTION=aetheros_knowledgePython Brain
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-4o-mini

---

## Repository Structureaether_os/
├── src/
│   ├── core/
│   │   ├── bloom.rs         # FNV-1a Bloom Filter (AtomicU64 bit array)
│   │   ├── cache.rs         # Write-Through semantic cache (DashMap + Redis)
│   │   ├── rate_limit.rs    # Token Bucket (parking_lot, per-IP DashMap)
│   │   ├── vector.rs        # fastembed-rs cosine similarity engine
│   │   └── qdrant.rs        # Qdrant HNSW vector search client
│   ├── brain/
│   │   └── client.rs        # HTTP Brain client (deployed transport)
│   ├── middleware/
│   │   └── shield.rs        # Rate limit + UUID request tracing
│   ├── handlers.rs          # SSE streaming generate handler
│   └── bin/
│       └── ingest.rs        # Document chunking + Qdrant ingestion pipeline
├── brain/
│   ├── proto/
│   │   └── aether.proto     # gRPC contract (enterprise-grpc branch)
│   └── src/
│       └── main.py          # FastAPI SSE service
├── web_ui/                  # Next.js 16 frontend
├── benchmarks/
│   └── load_test.js         # k6 semantic cache load test
├── data/
│   └── architecture.txt     # Ingested knowledge document
├── docker-compose.yml
└── ARCHITECTURE.md          # Detailed design decisions

---

## Live Deployment

| Service | Platform | URL |
|---|---|---|
| Frontend | Vercel | [aether-os-zeta.vercel.app](YOUR_LINK) |
| Rust Gateway | Render | [aetheros-gateway.onrender.com](YOUR_LINK) |
| Python Brain | Render | [aetheros-brain.onrender.com](YOUR_LINK) |
| Redis Cache | Upstash | Managed |
| Vector DB | Qdrant Cloud | Managed |