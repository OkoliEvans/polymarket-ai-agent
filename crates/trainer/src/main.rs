// crates/trainer/src/main.rs
//! Polymarket MLP trainer
//!
//! Fetches resolved Polymarket markets, trains a 4→8→2 MLP, and exports
//! the weights to `weights/polymarket_mlp_v1.bin`.
//!
//! Usage:
//!   cargo run -p trainer
//!
//! Env overrides:
//!   MAX_MARKETS=1000         — total resolved markets to fetch (default: 1000)
//!   TRAIN_RATIO=0.8          — fraction used for training; rest is validation (default: 0.8)
//!   EPOCHS=300               — training epochs (default: 300)
//!   LEARNING_RATE=0.001      — Adam LR (default: 0.001)
//!   WEIGHTS_OUT=weights/...  — output path (default: weights/polymarket_mlp_v1.bin)
//!
//! ## Why these defaults changed from the original
//!
//! MAX_MARKETS raised 500→1000: the Gamma API date filter (2024-01-01+) reduces
//! usable markets, so we fetch more to compensate.
//!
//! EPOCHS raised 200→300: with a proper train/val split the model can no longer
//! trivially memorise resolution prices, so it needs more epochs to converge.
//!
//! The val split is now time-based (oldest 80% train, newest 20% val) rather
//! than random. See `data::fetch_and_split` for details.

mod data;
mod export;
mod train;

use anyhow::Result;
use reqwest::Client;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let max_markets: usize = std::env::var("MAX_MARKETS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);

    let train_ratio: f64 = std::env::var("TRAIN_RATIO")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|r| r.clamp(0.5, 0.95))
        .unwrap_or(0.8);

    let epochs: usize = std::env::var("EPOCHS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);

    let learning_rate: f64 = std::env::var("LEARNING_RATE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1e-3);

    let weights_out =
        std::env::var("WEIGHTS_OUT").unwrap_or_else(|_| "weights/polymarket_mlp_v1.bin".into());

    info!(
        max_markets,
        train_ratio,
        epochs,
        learning_rate,
        %weights_out,
        "trainer starting"
    );

    // ── Fetch and split ───────────────────────────────────────────────────────
    // fetch_and_split:
    //   - Fetches only 2024-01-01+ markets (no stale 2020-2021 data)
    //   - Sorts by end_date ascending
    //   - Splits chronologically: oldest train_ratio → train, rest → val
    //   - Uses lastTradePrice (not outcomePrices) as price feature
    let client = Client::builder()
        .user_agent("polymarket-veil-agent/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let split = data::fetch_and_split(&client, max_markets, train_ratio).await?;

    info!(
        train = split.train.len(),
        val = split.val.len(),
        "data split ready"
    );

    if split.train.len() < 50 {
        anyhow::bail!(
            "only {} training samples — too few to train reliably. \
             Lower MAX_MARKETS floor or check Gamma API connectivity.",
            split.train.len()
        );
    }

    if split.val.len() < 10 {
        anyhow::bail!(
            "only {} validation samples — increase MAX_MARKETS or \
             lower TRAIN_RATIO.",
            split.val.len()
        );
    }

    // ── Train ─────────────────────────────────────────────────────────────────
    // train::train now receives separate train and val slices so it never
    // performs its own internal split. This replaces the previous single-slice
    // call: train::train(&markets, &cfg).
    let cfg = train::TrainConfig {
        epochs,
        learning_rate,
        ..Default::default()
    };

    let result = train::train(&split.train, &split.val, &cfg)?;

    info!(
        train_accuracy = format!("{:.1}%", result.train_accuracy * 100.0),
        val_accuracy = format!("{:.1}%", result.val_accuracy * 100.0),
        "training complete"
    );

    // ── Sanity checks ─────────────────────────────────────────────────────────

    // A model memorising resolution prices scores 100% — something is wrong.
    if result.val_accuracy >= 1.0 {
        anyhow::bail!(
            "val_accuracy {:.1}% — model is likely memorising labels (data leakage). \
             Check that outcomePrices is not being used as a price feature.",
            result.val_accuracy * 100.0
        );
    }

    // Worse than random means the model inverted its predictions.
    if result.val_accuracy < 0.48 {
        tracing::warn!(
            "val_accuracy {:.1}% is below random — consider more data, \
             longer training, or adjusted hyperparameters before deploying.",
            result.val_accuracy * 100.0
        );
    }

    // A large gap between train and val accuracy signals overfitting.
    let gap = result.train_accuracy - result.val_accuracy;
    if gap > 0.15 {
        tracing::warn!(
            "train/val accuracy gap {:.1}% — model may be overfitting. \
             Consider increasing MAX_MARKETS or adding dropout.",
            gap * 100.0
        );
    }

    // ── Export ────────────────────────────────────────────────────────────────
    export::export_weights(&result.bytes, &weights_out)?;
    export::verify_weights(&weights_out)?;

    info!("done — weights ready at {weights_out}");
    Ok(())
}
