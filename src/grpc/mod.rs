pub mod aether {
    tonic::include_proto!("aether");
}

pub use aether::rag_service_client::RagServiceClient;
pub use aether::{GenerateRequest, GenerateResponse, HealthRequest, HealthResponse};
