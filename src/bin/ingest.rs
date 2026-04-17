use anyhow::{Context, Result};
use std::fs;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

use aether_os::core::qdrant::{DocumentChunk, VectorDB};
use aether_os::core::vector::VectorEngine;

const CHUNK_SIZE_CHARS: usize = 800;
const CHUNK_OVERLAP_CHARS: usize = 150;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    info!("ingestion_pipeline_starting");
    let pipeline_start = Instant::now();

    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
    let collection =
        std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "aetheros_knowledge".to_string());
    let file_path =
        std::env::var("INGEST_FILE").unwrap_or_else(|_| "data/architecture.txt".to_string());

    info!(
        qdrant_url   = %qdrant_url,
        collection   = %collection,
        file_path    = %file_path,
        "ingestion_config"
    );

    info!("loading_vector_engine — this takes a few seconds on first run");
    let vector_engine =
        Arc::new(VectorEngine::new().context("Failed to load VectorEngine — check ONNX runtime")?);

    let vector_db = VectorDB::new(&qdrant_url, &collection)
        .await
        .context("Failed to connect to Qdrant — is docker-compose up?")?;

    let content = fs::read_to_string(&file_path)
        .with_context(|| format!("Cannot read '{}'. Did you create the file?", file_path))?;

    info!(
        file        = %file_path,
        char_count  = content.len(),
        "document_loaded"
    );

    let chunks = chunk_document(&content, &file_path);

    info!(
        chunk_count = chunks.len(),
        chunk_size = CHUNK_SIZE_CHARS,
        overlap = CHUNK_OVERLAP_CHARS,
        "document_chunked"
    );

    if chunks.is_empty() {
        error!("No chunks produced — document may be empty or too short");
        std::process::exit(1);
    }

    info!("embedding_all_chunks_in_batch");
    let embed_start = Instant::now();
    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();

    let vectors = vector_engine
        .embed_batch(texts)
        .context("Batch embedding failed")?;

    info!(
        chunks_embedded = vectors.len(),
        elapsed_ms = embed_start.elapsed().as_millis(),
        "batch_embedding_complete"
    );

    let document_chunks: Vec<DocumentChunk> = chunks
        .into_iter()
        .zip(vectors.into_iter())
        .map(|(chunk, vector)| DocumentChunk {
            vector,
            text: chunk.text,
            source: chunk.source,
            chunk_index: chunk.index,
        })
        .collect();

    info!(
        chunks_to_upsert = document_chunks.len(),
        "upserting_to_qdrant"
    );

    let upsert_start = Instant::now();
    vector_db
        .upsert_vectors(document_chunks)
        .await
        .context("Qdrant upsert failed")?;

    info!(
        elapsed_ms = upsert_start.elapsed().as_millis(),
        "upsert_complete"
    );

    let total = vector_db
        .vector_count()
        .await
        .context("Failed to verify vector count")?;

    info!(
        total_vectors = total,
        pipeline_ms = pipeline_start.elapsed().as_millis(),
        "ingestion_pipeline_complete"
    );

    println!("\n✅ Ingestion Complete");
    println!("   Vectors in '{}': {}", collection, total);
    println!("   Total time: {}ms", pipeline_start.elapsed().as_millis());
    println!("\n   Your RAG system is now ready to answer questions.");

    Ok(())
}

// ─── Chunking Logic ───────────────────────────────────────────────────────────
struct RawChunk {
    text: String,
    source: String,
    index: usize,
}

fn chunk_document(content: &str, source: &str) -> Vec<RawChunk> {
    let sentences: Vec<&str> = content
        .split_inclusive(|c| c == '.' || c == '?' || c == '!')
        .map(|s| s.trim())
        .filter(|s| s.len() > 10)
        .collect();

    let mut chunks: Vec<RawChunk> = Vec::new();
    let mut current_chunk = String::new();
    let mut overlap_buffer = String::new();
    let mut chunk_index = 0;

    for sentence in &sentences {
        if !current_chunk.is_empty() && current_chunk.len() + sentence.len() > CHUNK_SIZE_CHARS {
            let chunk_text = current_chunk.trim().to_string();
            if !chunk_text.is_empty() {
                chunks.push(RawChunk {
                    text: chunk_text.clone(),
                    source: source.to_string(),
                    index: chunk_index,
                });
                chunk_index += 1;

                overlap_buffer = chunk_text
                    .chars()
                    .rev()
                    .take(CHUNK_OVERLAP_CHARS)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
            }
            current_chunk = overlap_buffer.clone();
            current_chunk.push(' ');
        }
        current_chunk.push_str(sentence);
        current_chunk.push(' ');
    }

    let final_chunk = current_chunk.trim().to_string();
    if !final_chunk.is_empty() {
        chunks.push(RawChunk {
            text: final_chunk,
            source: source.to_string(),
            index: chunk_index,
        });
    }

    chunks
}
