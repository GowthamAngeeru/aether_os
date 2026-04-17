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

            rate_limit_rps: 5.0,
            rate_limit_capacity: 10.0,
            similarity_threshold: 0.92,

            bloom_capacity: 100_000,
            bloom_fp_rate: 0.01,
        }
    }
}
