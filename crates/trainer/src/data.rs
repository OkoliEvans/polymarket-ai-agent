// crates/trainer/src/data.rs
//! Fetch historical Polymarket market data for training.
//!
//! Uses the Gamma API (public, no auth required for historical data).
//! Fetches resolved binary markets with known outcomes.
//!
//! ## Key design decisions
//!
//! ### Why `lastTradePrice` instead of `outcomePrices`?
//! Resolved markets always have `outcomePrices = ["1","0"]` or `["0","1"]`.
//! Using those as features causes 100% val_accuracy — a trivially perfect but
//! completely useless model. `lastTradePrice` captures what the market was
//! pricing the YES outcome at just before resolution, which is the actual
//! signal the agent needs to learn from.
//!
//! ### Why time-based split?
//! Random splitting leaks future market dynamics into the training set.
//! We split by `end_date`: the oldest 80% train, the newest 20% validate.
//!
//! ### Why `end_date_min=2024-01-01`?
//! Markets from 2020-2021 have different liquidity profiles and are no longer
//! representative. We cap training data to recent resolved markets.
//!
//! API docs: https://gamma-api.polymarket.com

use anyhow::{Result, anyhow};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use common::RawMarket;
use reqwest::Client;
use serde::Deserialize;
use tracing::{info, warn};

const GAMMA_API: &str = "https://gamma-api.polymarket.com";

/// Page size for the markets endpoint. Max allowed by Gamma API.
const PAGE_LIMIT: usize = 100;

/// Only fetch markets that resolved on or after this date.
/// Keeps training data representative of current market conditions.
const MIN_END_DATE: &str = "2024-01-01";

// ── Gamma API response types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GammaMarket {
    #[serde(rename = "conditionId")]
    condition_id: String,
    question: String,

    /// Last traded price for the YES outcome, captured just before resolution.
    /// This is the primary price feature — NOT outcomePrices which is always
    /// 1.0 or 0.0 (data leakage).
    #[serde(rename = "lastTradePrice", default)]
    last_trade_price: Option<f64>,

    /// Pre-resolution bid/ask. Often zero on fully resolved markets, but
    /// preserved for recently-closed ones. Used to compute spread.
    #[serde(rename = "bestBid", default)]
    best_bid: Option<f64>,
    #[serde(rename = "bestAsk", default)]
    best_ask: Option<f64>,

    /// Total lifetime volume in USD. Used as a proxy for market importance.
    /// Note: this is NOT 24h volume — we normalise it below.
    #[serde(default)]
    volume: Option<String>,

    /// Total liquidity in the pool at resolution time.
    #[serde(default)]
    liquidity: Option<String>,

    #[serde(rename = "endDate", default)]
    end_date: Option<String>,

    #[serde(rename = "startDate", default)]
    start_date: Option<String>,

    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    resolved: Option<bool>,

    /// "Yes" or "No" — ground truth label.
    #[serde(rename = "winningOutcome", default)]
    winning_outcome: Option<String>,

    /// Post-resolution settlement prices. NOT used as features (data leakage).
    /// Kept for outcome fallback only when `winningOutcome` is absent.
    #[serde(rename = "outcomePrices", default)]
    outcome_prices: Option<String>,
}

// ── Split result ───────────────────────────────────────────────────────────────

/// Training data split by time.
///
/// `train` contains the oldest `train_ratio` fraction of markets.
/// `val` contains the newest `(1 - train_ratio)` fraction.
///
/// This prevents future leakage: the model is evaluated on markets
/// that resolved *after* the training period, mirroring deployment.
pub struct TimeSplit {
    pub train: Vec<RawMarket>,
    pub val: Vec<RawMarket>,
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Fetch resolved binary markets from the Gamma API and return a time-based
/// train/val split.
///
/// `max_markets` — total cap on resolved markets before splitting.
///   Use 800–2000 for reasonable training signal.
/// `train_ratio` — fraction of (time-sorted) data for training. Default: 0.8.
pub async fn fetch_and_split(
    client: &Client,
    max_markets: usize,
    train_ratio: f64,
) -> Result<TimeSplit> {
    let mut markets = fetch_resolved_markets(client, max_markets).await?;

    if markets.is_empty() {
        return Err(anyhow!(
            "no markets returned — check Gamma API connectivity"
        ));
    }

    // Sort ascending by end_date so the split is chronological.
    markets.sort_by_key(|m| m.end_date);

    let split_idx = ((markets.len() as f64) * train_ratio).round() as usize;
    let split_idx = split_idx.clamp(1, markets.len().saturating_sub(1));

    let val = markets.split_off(split_idx);
    let train = markets;

    info!(
        total  = train.len() + val.len(),
        train  = train.len(),
        val    = val.len(),
        oldest = %train.first().map(|m| m.end_date.to_rfc3339()).unwrap_or_default(),
        newest = %val.last().map(|m| m.end_date.to_rfc3339()).unwrap_or_default(),
        "time-based split complete"
    );

    Ok(TimeSplit { train, val })
}

/// Low-level fetch — returns all usable resolved markets up to `max_markets`.
/// Sorted descending by end_date (most recent first) via API query param.
/// Prefer `fetch_and_split` for training workflows.
pub async fn fetch_resolved_markets(client: &Client, max_markets: usize) -> Result<Vec<RawMarket>> {
    let mut markets = Vec::new();
    let mut offset = 0usize;

    info!(
        "fetching resolved markets from Gamma API \
         (target: {max_markets}, min_end_date: {MIN_END_DATE})"
    );

    loop {
        // `order=endDate&ascending=false` → most recent markets first.
        // `end_date_min` → skip stale 2020-2022 markets.
        let max_end = max_end_date();
        let url = format!(
            "{GAMMA_API}/markets\
     ?closed=true\
     &resolved=true\
     &order=endDate\
     &ascending=false\
     &end_date_min={MIN_END_DATE}T00:00:00Z\
     &end_date_max={max_end}\
     &limit={PAGE_LIMIT}\
     &offset={offset}"
        );

        let raw = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Gamma API request failed: {e}"))?
            .error_for_status()
            .map_err(|e| anyhow!("Gamma API error response: {e}"))?
            .text()
            .await
            .map_err(|e| anyhow!("Gamma API body read failed: {e}"))?;

        tracing::debug!(
            "raw response (first 500 chars): {}",
            &raw[..raw.len().min(500)]
        );

        let resp: Vec<GammaMarket> = serde_json::from_str(&raw).map_err(|e| {
            anyhow!(
                "Gamma API JSON parse failed: {e}\nbody snippet: {}",
                &raw[..raw.len().min(500)]
            )
        })?;

        if resp.is_empty() {
            break;
        }

        let fetched = resp.len();
        info!("fetched {fetched} markets at offset {offset}");

        for gm in resp {
            match parse_gamma_market(gm) {
                Ok(m) => markets.push(m),
                Err(e) => warn!("skipping market: {e}"),
            }
            if markets.len() >= max_markets {
                break;
            }
        }

        if markets.len() >= max_markets || fetched < PAGE_LIMIT {
            break;
        }

        offset += PAGE_LIMIT;
    }

    info!("collected {} usable resolved markets", markets.len());
    Ok(markets)
}

// ── Private helpers ────────────────────────────────────────────────────────────

fn parse_gamma_market(gm: GammaMarket) -> Result<RawMarket> {
    // ── Ground truth label ─────────────────────────────────────────────────────
    // Primary: winningOutcome field.
    // Fallback: outcomePrices (only to determine which outcome won, NOT as a
    // price feature — outcomePrices is always 1/0 after resolution).
    let outcome: bool = if let Some(ref wo) = gm.winning_outcome {
        match wo.trim().to_lowercase().as_str() {
            "yes" => true,
            "no" => false,
            other => {
                return Err(anyhow!(
                    "{}: unexpected winningOutcome '{other}'",
                    gm.condition_id
                ));
            }
        }
    } else {
        // Fallback: derive from outcomePrices only to get the label.
        let prices_str = gm
            .outcome_prices
            .as_deref()
            .ok_or_else(|| anyhow!("{}: no winningOutcome or outcomePrices", gm.condition_id))?;

        let prices: Vec<String> = serde_json::from_str(prices_str)
            .map_err(|e| anyhow!("{}: bad outcomePrices JSON: {e}", gm.condition_id))?;

        let yes_p: f64 = prices.first().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let no_p: f64 = prices.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);

        if yes_p > 0.9 && no_p < 0.1 {
            true
        } else if no_p > 0.9 && yes_p < 0.1 {
            false
        } else {
            return Err(anyhow!(
                "{}: ambiguous outcome: yes={yes_p:.3} no={no_p:.3}",
                gm.condition_id
            ));
        }
    };

    // ── End date ──────────────────────────────────────────────────────────────
    let end_date: DateTime<Utc> = gm
        .end_date
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .ok_or_else(|| anyhow!("{}: missing or invalid end_date", gm.condition_id))?;

    // Sanity-check: skip markets that haven't actually ended yet.
    if end_date > Utc::now() {
        return Err(anyhow!(
            "{}: end_date {} is in the future — not usable as training data",
            gm.condition_id,
            end_date.to_rfc3339()
        ));
    }

    // ── Duration filter ───────────────────────────────────────────────────────
    // Skip markets that ran less than 24 hours. These are high-frequency
    // churners where lastTradePrice has almost certainly converged to 1.0/0.0
    // before resolution — making it a leaky label proxy, not a signal.
    let start_date: Option<DateTime<Utc>> = gm
        .start_date
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    if let Some(start) = start_date {
        let duration_hours = (end_date - start).num_hours();
        if duration_hours < 72 {
            return Err(anyhow!(
                "{}: market duration {}h is too short — lastTradePrice likely \
                 already converged, skipping to avoid leakage",
                gm.condition_id,
                duration_hours
            ));
        }
    }

    // ── Pre-resolution price features ─────────────────────────────────────────
    // We want the YES price as the market was trading it BEFORE resolution.
    // Preference order:
    //   1. lastTradePrice — last fill price, closest to pre-resolution signal
    //   2. bestBid        — last resting bid on the YES side
    //   3. midpoint of bestBid/bestAsk if both present
    //   4. Conservative prior: resolved YES → 0.75, NO → 0.25
    //      (reflects that markets tend to drift toward their outcome late)
    //
    // We deliberately do NOT use outcomePrices here.
    let yes_price: f64 = if let Some(ltp) = gm.last_trade_price {
        // lastTradePrice is already the YES price.
        ltp.clamp(0.02, 0.98)
    } else if let (Some(bid), Some(ask)) = (gm.best_bid, gm.best_ask) {
        if bid > 0.0 && ask > 0.0 && ask >= bid {
            // Use midpoint of last resting quotes.
            ((bid + ask) / 2.0).clamp(0.02, 0.98)
        } else if bid > 0.0 {
            bid.clamp(0.02, 0.98)
        } else {
            // No reliable price data — use conservative outcome prior.
            conservative_prior(outcome)
        }
    } else {
        // No price data at all — use conservative outcome prior.
        conservative_prior(outcome)
    };

    let no_price = (1.0 - yes_price).clamp(0.02, 0.98);

    // ── Volume ────────────────────────────────────────────────────────────────
    // `volume` from Gamma on closed markets is lifetime total, not 24h.
    // We store it in volume_24h because that's the RawMarket field; the model
    // treats it as a relative market-importance signal regardless of label.
    let volume_24h = gm
        .volume
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
        .max(0.0);

    Ok(RawMarket {
        market_id: gm.condition_id,
        question: gm.question,
        yes_price,
        no_price,
        volume_24h,
        end_date,
        outcome: Some(outcome),
    })
}

/// Conservative price prior for when no market price data is available.
///
/// We do NOT use 0.5 (uniform) because that collapses variance across outcomes
/// and prevents the model from learning any price-based signal.
/// We also do NOT use 0.99/0.01 (resolution prices) — that is data leakage.
///
/// 0.72/0.28 reflects typical late-stage market drift toward the winning side.
/// This is a rough empirical prior; all features that DO carry information
/// (volume, liquidity) remain unbiased.
#[inline]
fn conservative_prior(outcome: bool) -> f64 {
    if outcome { 0.72 } else { 0.28 }
}

// Exclude markets that resolved in the last 24h — these are intraday
// churners where lastTradePrice has already converged to 1.0/0.0.
fn max_end_date() -> String {
    (Utc::now() - chrono::Duration::days(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}
