use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, instrument, warn};

#[derive(Debug, Serialize)]
pub struct BrainRequest {
    pub prompt: String,
    pub request_id: String,
    pub max_tokens: u32,
    pub context_chunks: Vec<String>,
}

#[derive(Debug)]
pub enum BrainEvent {
    Token(String),
    Done { full_text: String },
    Error(String),
}

// --- The Client ---
#[derive(Clone)]
pub struct BrainClient {
    http: Client,
    base_url: String,
}

impl BrainClient {
    pub fn new(base_url: String) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(90)) // Long timeout for LLM generation
            .pool_max_idle_per_host(10)
            .build()
            .context("Failed to build HTTP client")?;
        Ok(Self { http, base_url })
    }

    pub async fn health_check(&self) -> Result<bool> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .context("Brain health check request failed")?;
        Ok(resp.status().is_success())
    }

    #[instrument(skip(self), fields(request_id = %request.request_id))]
    pub async fn generate(
        &self,
        request: BrainRequest,
    ) -> Result<impl futures_util::Stream<Item = Result<BrainEvent>>> {
        let response = self
            .http
            .post(format!("{}/generate", self.base_url))
            .json(&request)
            .send()
            .await
            .context("Brain generate request failed")?;

        if !response.status().is_success() {
            anyhow::bail!("Brain returned error status: {}", response.status());
        }

        // Convert byte stream -> parsed BrainEvent stream
        let byte_stream = response.bytes_stream();
        let event_stream = async_stream::stream! {
            let mut buffer = String::new();
            futures_util::pin_mut!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Err(e) => {
                        error!(error = %e, "brain_stream_read_error");
                        yield Err(anyhow::anyhow!("Stream read error: {}", e));
                        return;
                    }
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));

                        // Parse complete SSE frames separated by \n\n
                        while let Some(frame_end) = buffer.find("\n\n") {
                            let frame = buffer[..frame_end].to_string();
                            buffer = buffer[frame_end + 2..].to_string();

                            let mut event_type = "message";
                            let mut data = "";

                            for line in frame.lines() {
                                if let Some(e) = line.strip_prefix("event: ") {
                                    event_type = e;
                                } else if let Some(d) = line.strip_prefix("data: ") {
                                    data = d;
                                }
                            }

                            let brain_event = match event_type {
                                "token" => {
                                    debug!(token_len = data.len(), "brain_token_received");
                                    BrainEvent::Token(data.to_string())
                                }
                                "done" => {
                                    let full_text = serde_json::from_str::<serde_json::Value>(data)
                                        .ok()
                                        .and_then(|v| v["full_text"].as_str().map(String::from))
                                        .unwrap_or_default();
                                    BrainEvent::Done { full_text }
                                }
                                "error" => {
                                    let msg = serde_json::from_str::<serde_json::Value>(data)
                                        .ok()
                                        .and_then(|v| v["error"].as_str().map(String::from))
                                        .unwrap_or_else(|| data.to_string());
                                    warn!(error = %msg, "brain_returned_error");
                                    BrainEvent::Error(msg)
                                }
                                _ => continue,
                            };
                            yield Ok(brain_event);
                        }
                    }
                }
            }
        };
        Ok(event_stream)
    }
}
