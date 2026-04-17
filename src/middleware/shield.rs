use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use uuid::Uuid;

use crate::core::rate_limit::RateLimiter;
use crate::error::AppError;

pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Priority order (industry standard):
/// 1. `X-Forwarded-For` header (set by Nginx, AWS ALB, Cloudflare)
/// 2. `X-Real-IP` header (set by some Nginx configurations)
/// 3. `ConnectInfo` socket address (direct connection, no proxy)
fn extract_client_ip(headers: &HeaderMap, socket_addr: SocketAddr) -> IpAddr {
    if let Some(forwarded_for) = headers.get("x-forwarded-for") {
        if let Ok(value) = forwarded_for.to_str() {
            if let Some(client_ip) = value.split(',').next() {
                if let Ok(ip) = client_ip.parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }

    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(value) = real_ip.to_str() {
            if let Ok(ip) = value.parse::<IpAddr>() {
                return ip;
            }
        }
    }

    socket_addr.ip()
}

/// Global Layer 4 defense middleware.
///
/// Responsibilities (in order):
/// 1. Stamp a unique request ID for distributed tracing
/// 2. Extract the true client IP (proxy-aware)
/// 3. Enforce per-IP rate limiting
/// 4. Inject request ID into response headers for client correlation

pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let request_id = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    req.extensions_mut().insert(RequestId(request_id.clone()));

    let client_ip = extract_client_ip(req.headers(), socket_addr);

    let span = tracing::info_span!(
        "rate_limit_middleware",
        request_id=%request_id,
        client_ip=%client_ip,
    );

    let _enter = span.enter();

    if !limiter.is_allowed(client_ip) {
        tracing::warn!(
            request_id=%request_id,
            client_ip=%client_ip,
            "Rate limit exceeded"
        );

        return Ok(rate_limit_response(&request_id));
    }

    tracing::debug!(
        request_id=%request_id,
        client_ip=%client_ip,
        "request_allowed"
    );

    let mut response = next.run(req).await;

    if let Ok(header_value) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(REQUEST_ID_HEADER, header_value);
    }

    Ok(response)
}

fn rate_limit_response(request_id: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", HeaderValue::from_static("1"));
    headers.insert("x-ratelimit-limit", HeaderValue::from_static("10"));

    if let Ok(v) = HeaderValue::from_str(&request_id) {
        headers.insert(REQUEST_ID_HEADER, v);
    }

    (
        StatusCode::TOO_MANY_REQUESTS,
        headers,
        "Rate limit exceeded. Please back off.",
    )
        .into_response()
}

#[derive(Debug, Clone)]
pub struct RequestId(pub String);
