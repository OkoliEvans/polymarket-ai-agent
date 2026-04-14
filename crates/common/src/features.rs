// crates/common/src/features.rs
//! Shared types and feature extraction.
//!
//! Used by both `trainer` (historical data) and `agent` (live data).
//! Normalization logic lives here and nowhere else — if it drifts between
//! training and inference the model predictions will be garbage.
//!
//! Feature vector layout (must match inference-guest stdin exactly):
//!   [0] yes_price      — YES token price,          normalized [0, 1]
//!   [1] no_price       — NO token price,            normalized [0, 1]
//!   [2] volume_24h     — 24h volume in USDC,        log-normalized [0, 1]
//!   [3] time_to_expiry — days until market closes,  normalized [0, 1]

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Normalization constants ────────────────────────────────────────────────────
//
// Fixed at training time — never change between trainer and agent builds.
// If you retrain with a different dataset range, bump the model version
// AND update these constants in lockstep.

/// Maximum 24h volume we expect to see (USDC). Markets above this are clamped.
const MAX_VOLUME_24H: f64 = 10_000_000.0;

/// Maximum days to expiry we consider. Markets further out are clamped.
const MAX_DAYS_TO_EXPIRY: f64 = 365.0;

/// Minimum volume floor for log normalization (log(0) is undefined).
const MIN_VOLUME_FLOOR: f64 = 1.0;

// ── Raw market ────────────────────────────────────────────────────────────────

/// Raw market data as returned by the Polymarket API.
/// Produced by `trainer::data` (historical) and `agent::market` (live).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMarket {
    pub market_id: String,
    pub question: String,
    /// Current YES token price in [0, 1].
    pub yes_price: f64,
    /// Current NO token price in [0, 1].
    /// Fetched independently — should be ~1 - yes_price but may drift.
    pub no_price: f64,
    /// 24-hour trading volume in USDC.
    pub volume_24h: f64,
    /// When the market resolves.
    pub end_date: DateTime<Utc>,
    /// Ground-truth outcome — only present in historical training data.
    /// `true` = YES won, `false` = NO won, `None` = live/unknown.
    pub outcome: Option<bool>,
}

// ── Feature extraction ────────────────────────────────────────────────────────

/// Normalized feature vector — the input to the MLP.
/// Shape [1, 4] to match the inference-guest's expected stdin layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureVector {
    /// The four normalized features in model input order.
    pub values: [f64; 4],
    /// Source market — kept for recording and debugging.
    pub market_id: String,
}

impl FeatureVector {
    /// Convert to the `Vec<Vec<f64>>` shape the Veil SDK expects.
    pub fn to_input(&self) -> Vec<Vec<f64>> {
        vec![self.values.to_vec()]
    }
}

/// Extract and normalize features from a raw market.
///
/// Returns `Err` if the market data is clearly invalid (negative prices,
/// already-expired market, etc.). Callers should skip invalid markets
/// rather than propagating the error.
pub fn extract_features(market: &RawMarket, is_training: bool) -> Result<FeatureVector> {
    // ── Validate ──────────────────────────────────────────────────────────────
    if market.yes_price < 0.0 || market.yes_price > 1.0 {
        return Err(anyhow!(
            "market {}: yes_price {} out of [0,1]",
            market.market_id,
            market.yes_price
        ));
    }
    if market.no_price < 0.0 || market.no_price > 1.0 {
        return Err(anyhow!(
            "market {}: no_price {} out of [0,1]",
            market.market_id,
            market.no_price
        ));
    }
    if market.volume_24h < 0.0 {
        return Err(anyhow!(
            "market {}: negative volume_24h {}",
            market.market_id,
            market.volume_24h
        ));
    }

    let now = Utc::now();
    if !is_training && market.end_date <= now {
        return Err(anyhow!(
            "market {}: already expired at {}",
            market.market_id,
            market.end_date
        ));
    }

    // ── Normalize ─────────────────────────────────────────────────────────────

    // [0] yes_price — already in [0,1], pass through
    let yes_price = market.yes_price.clamp(0.0, 1.0);

    // [1] no_price — already in [0,1], pass through
    let no_price = market.no_price.clamp(0.0, 1.0);

    // [2] volume_24h — log normalization
    let vol_clamped = market.volume_24h.max(0.0).min(MAX_VOLUME_24H);
    let log_vol = (vol_clamped + MIN_VOLUME_FLOOR).ln();
    let log_min = MIN_VOLUME_FLOOR.ln();
    let log_max = (MAX_VOLUME_24H + MIN_VOLUME_FLOOR).ln();
    let volume_norm = ((log_vol - log_min) / (log_max - log_min)).clamp(0.0, 1.0);

    // [3] time_to_expiry
    // For training: days_remaining will be negative (market already closed).
    // We clamp to 0.0 — feature carries no signal for historical data but
    // does not corrupt the vector. See note in data.rs about this limitation.
    let days_remaining = (market.end_date - now).num_seconds() as f64 / 86_400.0;
    let expiry_norm = (days_remaining / MAX_DAYS_TO_EXPIRY).clamp(0.0, 1.0);

    Ok(FeatureVector {
        values: [yes_price, no_price, volume_norm, expiry_norm],
        market_id: market.market_id.clone(),
    })
}

// ── Label extraction (trainer only) ──────────────────────────────────────────

/// Ground-truth label for supervised training.
/// [1.0, 0.0] = YES won, [0.0, 1.0] = NO won.
pub fn extract_label(market: &RawMarket) -> Result<[f64; 2]> {
    match market.outcome {
        Some(true) => Ok([1.0, 0.0]),
        Some(false) => Ok([0.0, 1.0]),
        None => Err(anyhow!(
            "market {}: no outcome label — cannot use for training",
            market.market_id
        )),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn make_market(yes: f64, no: f64, vol: f64, days: i64, outcome: Option<bool>) -> RawMarket {
        RawMarket {
            market_id: "test-market".into(),
            question: "Will X happen?".into(),
            yes_price: yes,
            no_price: no,
            volume_24h: vol,
            end_date: Utc::now() + Duration::days(days),
            outcome,
        }
    }

    #[test]
    fn features_in_unit_range() {
        let m = make_market(0.7, 0.3, 500_000.0, 30, None);
        let f = extract_features(&m, false).unwrap();
        for (i, v) in f.values.iter().enumerate() {
            assert!(*v >= 0.0 && *v <= 1.0, "feature[{i}] = {v} out of [0,1]");
        }
        assert_eq!(f.values[0], 0.7);
        assert_eq!(f.values[1], 0.3);
    }

    #[test]
    fn zero_volume_is_valid() {
        let m = make_market(0.5, 0.5, 0.0, 10, None);
        let f = extract_features(&m, false).unwrap();
        assert_eq!(f.values[2], 0.0);
    }

    #[test]
    fn max_volume_normalizes_to_one() {
        let m = make_market(0.5, 0.5, MAX_VOLUME_24H, 10, None);
        let f = extract_features(&m, false).unwrap();
        assert!((f.values[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn expired_market_is_rejected() {
        let m = make_market(0.5, 0.5, 1000.0, -1, None);
        assert!(extract_features(&m, false).is_err()); // agent rejects expired
        assert!(extract_features(&m, true).is_ok()); // trainer accepts expired
    }

    #[test]
    fn label_yes() {
        let m = make_market(0.9, 0.1, 1000.0, 5, Some(true));
        assert_eq!(extract_label(&m).unwrap(), [1.0, 0.0]);
    }

    #[test]
    fn label_no() {
        let m = make_market(0.1, 0.9, 1000.0, 5, Some(false));
        assert_eq!(extract_label(&m).unwrap(), [0.0, 1.0]);
    }

    #[test]
    fn label_missing_errors() {
        let m = make_market(0.5, 0.5, 1000.0, 5, None);
        assert!(extract_label(&m).is_err());
    }

    #[test]
    fn to_input_shape() {
        let m = make_market(0.6, 0.4, 100_000.0, 14, None);
        let f = extract_features(&m, false).unwrap();
        let input = f.to_input();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0].len(), 4);
    }
}
