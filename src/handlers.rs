use std::convert::Infallible;
use std::sync::Arc;

use async_stream::stream;
use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use serde::Deserialize;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::error::AppError;
use crate::grpc::GenerateRequest;
use crate::AppState;

const MIN_PROMPT_LEN: usize = 3;
const MAX_PROMPT_LEN: usize = 1024;

#[derive(Debug, Deserialize)]
pub struct GeneratePayload {
    pub prompt: String,
}

const EVT_TOKEN: &str = "token";
const EVT_CACHE_HIT: &str = "cache_hit";
const EVT_DONE: &str = "done";
const EVT_ERROR: &str = "error";

#[instrument(skip(state), fields(request_id))]
pub async fn generate_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GeneratePayload>,
) -> Response {
    let prompt = payload.prompt.trim().to_string();

    if prompt.len() < MIN_PROMPT_LEN {
        return AppError::ValidationError(format!(
            "Prompt too short: minimum {} characters",
            MIN_PROMPT_LEN
        ))
        .into_response();
    }
    if prompt.len() > MAX_PROMPT_LEN {
        return AppError::ValidationError(format!(
            "Prompt too long: maximum {} characters",
            MAX_PROMPT_LEN
        ))
        .into_response();
    }

    let request_id = Uuid::new_v4().to_string();
    tracing::Span::current().record("request_id", &request_id);
    info!(request_id = %request_id, prompt_len = prompt.len(), "generate_request");

    let engine = Arc::clone(&state.vector_engine);
    let prompt_clone = prompt.clone();

    // 1. Convert human text to math
    let query_vector = match engine.embed_async(prompt_clone).await {
        Ok(v) => v,
        Err(e) => {
            error!(request_id = %request_id, error = %e, "embedding_failed");
            return error_sse_stream("Embedding failed — internal error");
        }
    };

    // 2. Check 50ms Semantic Cache
    if let Some(cached_response) = state.semantic_cache.search(&query_vector) {
        info!(request_id = %request_id, "cache_hit — serving from memory");

        let stream = stream! {
            yield Result::<Event, Infallible>::Ok(
                Event::default()
                    .event(EVT_CACHE_HIT)
                    .data(cached_response)
            );
            yield Result::<Event, Infallible>::Ok(Event::default().event(EVT_DONE).data(""));
        };

        return Sse::new(stream)
            .keep_alive(KeepAlive::default())
            .into_response();
    }

    info!(request_id = %request_id, "cache_miss — querying vector database");

    // 3. ─── RAG INTERCEPT START ───
    // Query Qdrant for the closest matching architectural documents
    let retrieved_chunks = match state.vector_db.search_default(&query_vector).await {
        Ok(chunks) => chunks,
        Err(e) => {
            warn!(request_id = %request_id, error = %e, "qdrant_search_failed — proceeding without context");
            Vec::new() // If Qdrant fails, just answer from general knowledge
        }
    };

    // Construct the super-prompt with injected context
    let augmented_prompt = if retrieved_chunks.is_empty() {
        prompt.clone()
    } else {
        info!(request_id = %request_id, chunks_found = retrieved_chunks.len(), "context_retrieved");
        let mut assembled = String::from(
            "Use the following system context to answer the user's prompt accurately. If the answer cannot be deduced from the context, answer based on your general knowledge.\n\n--- System Context ---\n"
        );
        for (i, chunk) in retrieved_chunks.iter().enumerate() {
            assembled.push_str(&format!("[Source {}]: {}\n", i + 1, chunk.text));
        }
        assembled.push_str("\n--- User Prompt ---\n");
        assembled.push_str(&prompt);
        assembled
    };
    // ─── RAG INTERCEPT END ───

    info!(request_id = %request_id, "routing_to_python_brain");

    let mut grpc_client = state.rag_client.clone();
    let cache = Arc::clone(&state.semantic_cache);

    // We cache using the ORIGINAL prompt/vector, not the augmented one!
    let prompt_for_cache = prompt.clone();
    let vector_for_cache = query_vector.clone();
    let rid = request_id.clone();

    // 4. Stream to Python via gRPC
    let stream = stream! {
        let request = tonic::Request::new(GenerateRequest {
            prompt: augmented_prompt, // <-- Injecting the super-prompt here!
            request_id: rid.clone(),
            max_tokens: 1024,
        });

        let mut grpc_stream = match grpc_client.generate(request).await {
            Ok(resp) => resp.into_inner(),
            Err(e) => {
                error!(request_id = %rid, error = %e, "grpc_call_failed");
                yield Result::<Event, Infallible>::Ok(
                    Event::default()
                        .event(EVT_ERROR)
                        .data("Python Brain unavailable")
                );
                return;
            }
        };

        let mut full_response = String::new();
        let mut token_count = 0u32;

        loop {
            match grpc_stream.message().await {
                Err(e) => {
                    error!(request_id = %rid, tokens_streamed = token_count, error = %e, "grpc_stream_error");
                    yield Result::<Event, Infallible>::Ok(
                        Event::default()
                            .event(EVT_ERROR)
                            .data("Stream interrupted")
                    );
                    break;
                }
                Ok(None) => {
                    warn!(request_id = %rid, tokens_streamed = token_count, "grpc_stream_ended_without_final_signal");
                    break;
                }
                Ok(Some(msg)) => {
                    if msg.is_final {
                        let cache_clone  = Arc::clone(&cache);
                        let p = prompt_for_cache.clone();
                        let v = vector_for_cache.clone();
                        let r = msg.full_text.clone();

                        tokio::spawn(async move {
                            cache_clone.insert(p, v, r);
                        });

                        info!(request_id = %rid, tokens_streamed = token_count, "generation_complete — response cached");
                        yield Result::<Event, Infallible>::Ok(Event::default().event(EVT_DONE).data(""));
                        break;
                    } else {
                        full_response.push_str(&msg.token);
                        token_count += 1;
                        yield Result::<Event, Infallible>::Ok(
                            Event::default()
                                .event(EVT_TOKEN)
                                .data(msg.token)
                        );
                    }
                }
            }
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

fn error_sse_stream(message: &'static str) -> Response {
    let stream = stream! {
        yield Result::<Event, Infallible>::Ok(
            Event::default()
                .event(EVT_ERROR)
                .data(message)
        );
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
