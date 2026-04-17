use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Rate limit exceeded. Bucket empty.")]
    RateLimitExceeded,

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Security violation: Request blocked by Semantic Firewall.")]
    SecurityViolation,

    #[error("Internal gateway error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            AppError::ValidationError(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::SecurityViolation => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::Internal(_) => {
                tracing::error!("Internal system crash: {:?}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        let body = Json(json!({
            "error": error_message,
            "status": status.as_u16()
        }));

        (status, body).into_response()
    }
}
