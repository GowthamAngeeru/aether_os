# AetherOS: System Architecture & Design Decisions

This document outlines the architectural decisions, data flow, and trade-offs made during the engineering of AetherOS. The primary objective of this system is to deliver a highly concurrent, fault-tolerant AI Gateway with sub-50ms latency for cached queries, effectively minimizing expensive LLM API calls.

## 1. System Topology

AetherOS utilizes a microservices architecture, strictly separating the high-throughput network edge from the CPU-intensive AI orchestration layer.

- **Edge Gateway (Rust / Axum):** The entry point for all client requests. Responsible for connection pooling, SSE streaming, rate limiting, and embedding generation.
- **Intelligence Node (Python / LangChain):** An isolated gRPC server responsible for prompt orchestration, LLM communication, and contextual synthesis.
- **State Layer (Redis):** Ephemeral storage for the Semantic Cache.
- **Knowledge Layer (Qdrant):** Persistent vector storage for Retrieval-Augmented Generation (RAG).
- **Presentation Layer (Next.js 16):** React Server Components UI utilizing custom buffering to parse SSE streams.

## 2. The Semantic RAG Data Flow

When a user submits a prompt, AetherOS executes the following pipeline:

### Phase A: Edge Intercept & Vectorization

1. The Next.js client initiates an HTTP POST request to the Rust Gateway.
2. The Rust Gateway immediately generates a 384-dimensional vector embedding of the raw text string locally using the `fastembed` crate and the ONNX CPU runtime. **Decision:** Running embeddings at the edge via Rust eliminates a network hop to an external API (like OpenAI's embedding model), saving ~150ms per request.

### Phase B: The Semantic Cache (50ms SLA)

1. The generated vector is used to perform a Cosine Similarity search against Redis.
2. If a semantic match (confidence > 0.95) is found, the Gateway retrieves the cached completion and streams it back to the Next.js client via SSE. **Decision:** Bypassing the Python Brain and LLM entirely on cache hits reduces API costs to $0.00 and latency to <50ms.

### Phase C: Contextual Retrieval

1. On a cache miss, the Rust Gateway queries Qdrant using the same vector to retrieve the top $K=5$ most relevant architectural document chunks.
2. The Rust Gateway constructs an `augmented_prompt` containing the retrieved context and the user's original query.

### Phase D: Intelligence Orchestration

1. The `augmented_prompt` is serialized into a Protocol Buffer and transmitted to the Python Brain over a high-speed gRPC channel. **Decision:** gRPC/Protobuf was chosen over REST/JSON for inter-service communication due to its binary serialization speed and strict typing contracts.
2. The Python Brain utilizes LangChain to query the foundation LLM, streaming the generated tokens back to the Rust Gateway, which passes them directly to the Next.js client (Zero-Copy Streaming).
3. Upon completion, the Rust Gateway asynchronously writes the original prompt vector and the final response back to Redis to satisfy future cache hits.

## 3. Failure Modes & Mitigation

| Component Failure     | Mitigation Strategy                                                                                        | System Impact                                                     |
| :-------------------- | :--------------------------------------------------------------------------------------------------------- | :---------------------------------------------------------------- |
| **Redis Goes Down**   | Rust catches the connection error and automatically bypasses the cache, routing directly to Qdrant/Python. | Increased latency/cost, but no downtime.                          |
| **Qdrant Goes Down**  | Rust catches the error, skips context retrieval, and sends a "blind" prompt to Python.                     | Lower response accuracy, but system remains available.            |
| **Python Brain Dies** | Rust gRPC client catches the transport error and instantly streams a graceful error to the Next.js client. | AI generation halted; Gateway remains up to handle other traffic. |

## 4. Future Scaling Considerations

- **Distributed Tracing:** Implementing OpenTelemetry headers to trace requests across the Rust and Python boundaries.
- **Consistent Hashing:** Upgrading the gRPC bridge to support a swarm of Python Brains, routing specific topic vectors to specific nodes.
