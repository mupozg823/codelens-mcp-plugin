pub fn configured_rerank_blend() -> f64 {
    std::env::var("CODELENS_RERANK_BLEND")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| (0.0..=1.0).contains(v))
        .unwrap_or(0.75)
}
