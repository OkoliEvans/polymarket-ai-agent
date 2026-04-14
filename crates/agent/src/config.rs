// crates/agent/src/config.rs
//! Agent configuration — loaded from environment / .env file.

use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct Config {
    // ── Polymarket ────────────────────────────────────────────────────────────
    /// Gamma API base URL.
    pub polymarket_api_url: String,
    /// CLOB API base URL (for order placement).
    pub polymarket_clob_url: String,
    /// Private key for signing trades (hex, with or without 0x prefix).
    pub polymarket_private_key: String,
    /// CTF Exchange proxy address.
    pub polymarket_proxy: String,

    // ── Veil ──────────────────────────────────────────────────────────────────
    /// Gateway URL.
    pub veil_gateway_url: String,
    /// Model ID registered in the gateway.
    pub veil_model_id: String,

    // ── Agent behaviour ───────────────────────────────────────────────────────
    /// Skip trades where confidence is below this threshold. [0.5, 1.0]
    pub min_confidence: f32,
    /// Maximum bet size per trade in USDC.
    pub max_bet_usdc: f64,
    /// How often to scan for new markets (seconds).
    pub poll_interval_secs: u64,
    /// Skip markets with 24h volume below this (USDC).
    pub min_volume_24h: f64,
    /// Path to the weights file.
    pub weights_path: String,
    /// Paper trading mode — decisions are logged but no real bets are placed.
    pub paper_trading: bool,
    pub database_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            polymarket_api_url: std::env::var("POLYMARKET_API_URL")
                .unwrap_or_else(|_| "https://gamma-api.polymarket.com".into()),

            polymarket_clob_url: std::env::var("POLYMARKET_CLOB_URL")
                .unwrap_or_else(|_| "https://clob.polymarket.com".into()),

            polymarket_private_key: std::env::var("POLYMARKET_PRIVATE_KEY")
                .map_err(|_| anyhow!("POLYMARKET_PRIVATE_KEY must be set"))?,

            polymarket_proxy: std::env::var("POLYMARKET_PROXY_ADDRESS")
                .map_err(|_| anyhow!("POLYMARKET_PROXY_ADDRESS must be set"))?,

            veil_gateway_url: std::env::var("VEIL_GATEWAY_URL")
                .unwrap_or_else(|_| "http://localhost:8080".into()),

            veil_model_id: std::env::var("VEIL_MODEL_ID")
                .unwrap_or_else(|_| "polymarket_mlp_v1".into()),

            min_confidence: std::env::var("MIN_CONFIDENCE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.65),

            max_bet_usdc: std::env::var("MAX_BET_SIZE_USDC")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5.0),

            poll_interval_secs: std::env::var("POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),

            min_volume_24h: std::env::var("MIN_VOLUME_24H")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5_000.0),

            weights_path: std::env::var("WEIGHTS_PATH")
                .unwrap_or_else(|_| "weights/polymarket_mlp_v1.bin".into()),

            paper_trading: std::env::var("PAPER_TRADING")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true), // default ON — must explicitly opt out

            database_url: std::env::var("DATABASE_URL").ok(),
        })
    }
}
