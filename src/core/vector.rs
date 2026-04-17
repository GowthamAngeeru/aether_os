use anyhow::{bail, Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use ndarray::ArrayView1;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, info, instrument, warn};

pub const EMBEDDING_DIMS: usize = 384;
const MAX_PROMPT_CHARS: usize = 1024;
pub const SIMILARITY_THRESHOLD: f32 = 0.85;

pub struct VectorEngine {
    model: Mutex<TextEmbedding>,
}

unsafe impl Send for VectorEngine {}
unsafe impl Sync for VectorEngine {}

impl VectorEngine {
    pub fn new() -> Result<Self> {
        let start = Instant::now();
        info!("vector_engine_loading");

        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
        )
        .context("Failed to initialize FastEmbed AllMiniLML6V2 model")?;

        let engine = Self {
            model: Mutex::new(model),
        };

        info!(
            elapsed_ms = start.elapsed().as_millis(),
            "vector_engine_loaded"
        );

        let warmup_start = Instant::now();
        engine
            .embed("warm up the ONNX runtime cache")
            .context("Warm-up inference failed — model may be corrupted")?;

        info!(
            elapsed_ms = warmup_start.elapsed().as_millis(),
            "vector_engine_warmed_up"
        );

        Ok(engine)
    }

    #[instrument(skip(self), fields(prompt_len = text.len()))]
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        if text.trim().is_empty() {
            bail!("embed: prompt cannot be empty or whitespace-only");
        }

        if text.len() > MAX_PROMPT_CHARS {
            warn!(
                prompt_len = text.len(),
                max_len = MAX_PROMPT_CHARS,
                "embed: prompt exceeds max length, truncating"
            );
        }

        let start = Instant::now();

        let mut model_guard = self.model.lock().unwrap();

        let mut embeddings = model_guard
            .embed(vec![text.to_string()], None)
            .context("FastEmbed inference failed")?;

        let vector = embeddings.remove(0);

        if vector.len() != EMBEDDING_DIMS {
            bail!(
                "embed: expected {} dimensions, got {} — model mismatch",
                EMBEDDING_DIMS,
                vector.len()
            );
        }

        debug!(
            elapsed_us = start.elapsed().as_micros(),
            dims = vector.len(),
            "embedding_generated"
        );

        Ok(vector)
    }

    pub async fn embed_async(self: Arc<Self>, text: String) -> Result<Vec<f32>> {
        tokio::task::spawn_blocking(move || self.embed(&text))
            .await
            .context("spawn_blocking panicked during embed")?
    }

    #[instrument(skip(self, texts), fields(batch_size = texts.len()))]
    pub fn embed_batch(&self, texts: Vec<&str>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let start = Instant::now();
        let string_texts: Vec<String> = texts.into_iter().map(|s| s.to_string()).collect();

        let mut model_guard = self.model.lock().unwrap();
        let embeddings = model_guard
            .embed(string_texts, None)
            .context("FastEmbed batch inference failed")?;

        Ok(embeddings)
    }

    pub fn cosine_similarity(vec_a: &[f32], vec_b: &[f32]) -> f32 {
        let a = ArrayView1::from(vec_a);
        let b = ArrayView1::from(vec_b);
        a.dot(&b).clamp(-1.0, 1.0)
    }

    pub fn is_semantic_match(vec_a: &[f32], vec_b: &[f32]) -> bool {
        Self::cosine_similarity(vec_a, vec_b) >= SIMILARITY_THRESHOLD
    }
}
