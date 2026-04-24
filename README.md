# ⚡ AetherOS | Enterprise AI Edge Gateway

AetherOS is an ultra-low latency, distributed Retrieval-Augmented Generation (RAG) architecture. It bridges a high-concurrency Rust edge server with a Python LangChain intelligence layer, utilizing semantic caching and consistent vector retrieval to deliver real-time AI responses.

![Architecture Status](https://img.shields.io/badge/Architecture-Distributed_Microservices-blue)
![Rust](https://img.shields.io/badge/Edge-Rust_Axum-orange)
![Python](https://img.shields.io/badge/Brain-Python_FastAPI-yellow)
![Frontend](https://img.shields.io/badge/UI-Next.js_16-black)

## 🧠 System Architecture

AetherOS strictly separates network orchestration from AI processing, utilizing a high-throughput HTTP/SSE bridge to connect the edge to the intelligence layer. 

1. **The UI (Next.js 16):** React Server Components frontend consuming Server-Sent Events (SSE) for real-time token streaming.
2. **The Edge (Rust/Axum):** Highly concurrent API gateway handling rate limiting (Token Bucket), Bloom filtering, and request validation.
3. **The Semantic Cache (Redis + ONNX):** CPU-native ONNX runtime embedded in Rust generates 384-dimensional vectors of incoming prompts, checking Redis for semantic similarity. 
4. **Deep Retrieval (Qdrant):** Rust queries a high-performance vector database to inject architectural context into the prompt using Cosine Similarity.
5. **The Brain (Python/FastAPI):** A dedicated microservice receiving augmented prompts via REST, orchestrating LLM generation, and streaming tokens back to the edge.

> 🛡️ **Enterprise gRPC Branch:** A strict, fail-fast binary implementation of the Brain-to-Edge transport layer utilizing `tonic` and Protocol Buffers is permanently preserved on the `enterprise-grpc` branch for internal, zero-proxy environments.

## 📊 Performance & Benchmarks

AetherOS is stress-tested using `k6` to guarantee ultra-low latency under concurrent load. The custom Semantic Cache architecture intercepts redundant queries, completely bypassing the LLM generation phase to save API costs and drastically reduce TTFT (Time To First Token).

**Local k6 Load Test Results (10 Concurrent VUs):**
* **Cache Hit Rate:** `95.8%` (Flawless Thundering Herd mitigation)
* **Average Latency:** `275ms` (Includes local ONNX vector embedding math + Redis network lookup)
* **Error Rate:** `0.00%` (Perfect stability under sustained load)

## 🚀 Core Features

- **Sub-300ms Semantic Caching:** Prevents redundant LLM API calls by serving mathematically similar questions directly from memory.
- **Dual Transport Architecture:** Default HTTP/SSE streaming for maximum proxy compatibility (`main`), with a preserved `enterprise-grpc` branch enforcing strict Protocol Buffer contracts.
- **Zero-Copy Streaming:** Tokens are streamed asynchronously from the OpenAI API $\rightarrow$ Python $\rightarrow$ Rust $\rightarrow$ Next.js UI with zero blocking operations.
- **Hardware-Accelerated Math:** Local embeddings generated using the `fastembed` Rust crate and ONNX runtime, eliminating the latency and cost of network-based embedding APIs.

## 💻 Tech Stack Overview

| Layer              | Technology                           | Purpose                                  |
| :----------------- | :----------------------------------- | :--------------------------------------- |
| **Frontend** | Next.js 16, React Markdown, Tailwind | Real-time SSE streaming interface        |
| **Gateway** | Rust, Axum, Tokio                    | High-throughput async routing            |
| **Math Engine** | ONNX Runtime, FastEmbed              | Local vector embedding generation        |
| **Caching** | Redis                                | Ephemeral semantic state storage         |
| **Knowledge Base** | Qdrant                               | Persistent document vectors              |
| **Intelligence** | Python, FastAPI, LangChain           | LLM orchestration and logic              |
| **Transport** | HTTP/SSE & gRPC (Branch Dependent)   | Inter-service microservice communication |

## ⚙️ Quick Start (Local Development)

### Prerequisites

- Rust toolchain
- Python 3.10+
- Node.js 20+
- Docker (for Redis and Qdrant)

### Boot Sequence

**1. Start Infrastructure**
```bash
docker run -p 6379:6379 -d redis
docker run -p 6334:6334 -p 6333:6333 -d qdrant/qdrant

2. Boot the Python Brain (Port 8000)

cd brain
# Activate virtual environment
# Windows: .\venv\Scripts\activate | Mac/Linux: source venv/bin/activate
pip install -r requirements.txt
uvicorn src.main:app --host 0.0.0.0 --port 8000

3. Boot the Rust Edge (Port 3000)

# Ensure ONNX DLLs are in your project root or PATH
cargo run
4. Boot the Next.js UI (Port 3001)

cd web_ui
npm install
npm run dev -- -p 3001