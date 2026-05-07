⚡ AetherOS | Enterprise AI Edge Gateway

AetherOS is an ultra-low latency, distributed Retrieval-Augmented Generation (RAG) architecture. It bridges a high-concurrency Rust edge server with a Python LangChain intelligence layer, utilizing semantic caching and consistent vector retrieval to deliver sub-50ms AI responses.
   
## 🧠 System Architecture

AetherOS strictly separates network orchestration from AI processing, utilizing a high-speed gRPC bridge to connect the edge to the intelligence layer.

* **The UI (Next.js 16):** React Server Components frontend consuming Server-Sent Events (SSE) for real-time token streaming.
* **The Edge (Rust/Axum):** Highly concurrent API gateway handling rate limiting, Bloom filtering, and request validation.
* **The Semantic Cache (Redis + ONNX):** CPU-native ONNX runtime embedded in Rust generates 384-dimensional vectors of incoming prompts, checking Redis for semantic similarity. Cache hits return in <50ms, completely bypassing the LLM.
* **Deep Retrieval (Qdrant):** Rust queries a high-performance vector database to inject architectural context into the prompt using Cosine Similarity.
* **The Brain (Python/gRPC):** A dedicated microservice receiving augmented prompts over Protocol Buffers, orchestrating LLM generation, and streaming tokens back to the edge.

## 🚀 Core Features

* **Sub-50ms Semantic Caching:** Prevents redundant LLM API calls by serving mathematically similar questions directly from memory.
* **Fail-Fast gRPC Bridge:** Strict contract enforcement between the Rust Gateway and Python Brain using `tonic` and Protocol Buffers.
* **Zero-Copy Streaming:** Tokens are streamed from the OpenAI API → Python → Rust → Next.js UI with zero blocking operations.
* **Hardware-Accelerated Math:** Local embeddings generated using the `fastembed` Rust crate and ONNX runtime, eliminating the latency and cost of network-based embedding APIs.

## 💻 Tech Stack Overview

| Layer | Technology | Purpose |
| :--- | :--- | :--- |
| **Frontend** | Next.js 16, Tailwind | Real-time SSE streaming interface |
| **Gateway** | Rust, Axum, Tokio | High-throughput async routing |
| **Math Engine** | ONNX Runtime, FastEmbed | Local vector embedding generation |
| **Caching** | Redis | Ephemeral semantic state storage |
| **Knowledge Base** | Qdrant | Persistent document vectors |
| **Intelligence** | Python, LangChain | LLM orchestration and logic |
| **Transport** | gRPC, Protocol Buffers | Inter-service microservice communication |

## ⚙️ Quick Start (Local Development)

### Prerequisites
* Rust toolchain
* Python 3.10+
* Node.js 20+
* Docker (for Redis and Qdrant)

### Boot Sequence

**1. Start Infrastructure**

  bash
  docker run -p 6379:6379 -d redis
  docker run -p 6334:6334 -p 6333:6333 -d qdrant/qdrant
**2. Boot the Python Brain (Port 50051)

  Bash
  cd brain 
  python -m venv venv 
  source venv/bin/activate  # Windows: .\venv\Scripts\activate 
  pip install -r requirements.txt 
  python src/main.py
**3. Boot the Rust Edge (Port 3000)
  (Ensure ONNX DLLs are in your path if on Windows)
  
  Bash
  cargo run
**4. Boot the Next.js UI (Port 3001)

  Bash
  cd web_ui 
  npm install 
  npm run dev
