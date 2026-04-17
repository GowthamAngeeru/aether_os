use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, SearchParamsBuilder, SearchPointsBuilder,
    UpsertPointsBuilder, Value as QdrantValue, VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

use crate::core::vector::EMBEDDING_DIMS;

// ─── Constants ────────────────────────────────────────────────────────────────
pub const DEFAULT_TOP_K: u64 = 5;
const STARTUP_RETRY_COUNT: u32 = 5;
const STARTUP_RETRY_DELAY: Duration = Duration::from_secs(2);

// ─── VectorDB ─────────────────────────────────────────────────────────────────
pub struct VectorDB {
    client: Qdrant,
    collection_name: String,
}

impl VectorDB {
    pub async fn new(url: &str, collection_name: &str) -> Result<Self> {
        info!(url = %url, collection = %collection_name, "qdrant_connecting");

        let client = Qdrant::from_url(url)
            .build()
            .context("Failed to build Qdrant client")?;

        let mut last_error = None;
        for attempt in 1..=STARTUP_RETRY_COUNT {
            match client.health_check().await {
                Ok(_) => {
                    info!(url = %url, attempt = attempt, "qdrant_connected");
                    last_error = None;
                    break;
                }
                Err(e) => {
                    warn!(attempt = attempt, error = %e, "qdrant_health_check_failed");
                    last_error = Some(e);
                    tokio::time::sleep(STARTUP_RETRY_DELAY).await;
                }
            }
        }

        if let Some(e) = last_error {
            return Err(anyhow::anyhow!("Qdrant unreachable: {}", e));
        }

        let db = Self {
            client,
            collection_name: collection_name.to_string(),
        };

        db.init_collection().await?;
        Ok(db)
    }

    // ── Collection Lifecycle ──────────────────────────────────────────────
    async fn init_collection(&self) -> Result<()> {
        let exists = self
            .client
            .collection_exists(&self.collection_name)
            .await
            .context("Failed to check collection")?;

        if exists {
            info!("qdrant_collection_exists");
            return Ok(());
        }

        info!("qdrant_creating_collection");

        self.client
            .create_collection(
                CreateCollectionBuilder::new(&self.collection_name).vectors_config(
                    VectorParamsBuilder::new(EMBEDDING_DIMS as u64, Distance::Cosine),
                ),
            )
            .await
            .context("Failed to create collection")?;

        Ok(())
    }

    // ── Ingestion: Write Vectors ──────────────────────────────────────────
    pub async fn upsert_vectors(&self, chunks: Vec<DocumentChunk>) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let count = chunks.len();

        let points: Vec<PointStruct> = chunks
            .into_iter()
            .map(|chunk| {
                let point_id = Uuid::new_v4();
                let mut payload = std::collections::HashMap::new();

                payload.insert("text".to_string(), QdrantValue::from(chunk.text));
                payload.insert("source".to_string(), QdrantValue::from(chunk.source));
                payload.insert(
                    "chunk_index".to_string(),
                    QdrantValue::from(chunk.chunk_index as i64),
                );

                PointStruct::new(point_id.to_string(), chunk.vector, payload)
            })
            .collect();

        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection_name, points).wait(true))
            .await?;

        info!(chunks_stored = count, "qdrant_upsert_complete");
        Ok(())
    }

    // ── RAG Retrieval: Search ─────────────────────────────────────────────
    pub async fn search(&self, query_vector: &[f32], top_k: u64) -> Result<Vec<RetrievedChunk>> {
        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection_name, query_vector.to_vec(), top_k)
                    .with_payload(true)
                    .params(SearchParamsBuilder::default().exact(false)),
            )
            .await?;

        let chunks: Vec<RetrievedChunk> = results
            .result
            .into_iter()
            .filter_map(|point| {
                let text = point.payload.get("text")?.as_str().map(|s| s.to_string())?;
                let source = point
                    .payload
                    .get("source")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown".to_string());

                Some(RetrievedChunk {
                    text,
                    source,
                    score: point.score,
                })
            })
            .collect();

        Ok(chunks)
    }

    pub async fn search_default(&self, query_vector: &[f32]) -> Result<Vec<RetrievedChunk>> {
        self.search(query_vector, DEFAULT_TOP_K).await
    }

    pub async fn vector_count(&self) -> Result<u64> {
        let info = self.client.collection_info(&self.collection_name).await?;
        // API 1.17.0 uses points_count instead of vectors_count
        Ok(info
            .result
            .map(|r| r.points_count.unwrap_or(0))
            .unwrap_or(0))
    }
}

// ─── Domain Types ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct DocumentChunk {
    pub vector: Vec<f32>,
    pub text: String,
    pub source: String,
    pub chunk_index: usize,
}

#[derive(Debug, Clone)]
pub struct RetrievedChunk {
    pub text: String,
    pub source: String,
    pub score: f32,
}
