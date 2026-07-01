#[derive(Debug, Clone)]
pub struct AppConfig {
    pub port: u16,
    pub rate_limit_rps: f64,
    pub rate_limit_capacity: f64,
    pub similarity_threshold: f32,

    pub bloom_capacity: usize,
    pub bloom_fp_rate: f64,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("CRITICAL: PORT must be a valid number"),

            rate_limit_rps: std::env::var("RATE_LIMIT_RPS")
                .unwrap_or_else(|_| "5.0".to_string())
                .parse::<f64>()
                .unwrap_or(5.0),
            rate_limit_capacity: std::env::var("RATE_LIMIT_CAPACITY")
                .unwrap_or_else(|_| "10.0".to_string())
                .parse::<f64>()
                .unwrap_or(10.0),
            similarity_threshold: std::env::var("SIMILARITY_THRESHOLD")
                .unwrap_or_else(|_| "0.92".to_string())
                .parse::<f32>()
                .unwrap_or(0.92),
            bloom_capacity: std::env::var("BLOOM_CAPACITY")
                .unwrap_or_else(|_| "100000".to_string())
                .parse::<usize>()
                .unwrap_or(100_000),
            bloom_fp_rate: 0.01,
        }
    }
}
