// crates/agent/src/market.rs
//! Polymarket API client — fetches live active binary markets.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use common::RawMarket;
use reqwest::Client;
use serde::Deserialize;
use tracing::{info, warn};

const PAGE_LIMIT: usize = 50;

// ── Gamma API types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GammaMarket {
    #[serde(rename = "conditionId")]
    condition_id: String,
    question: String,
    #[serde(rename = "bestBid", default)]
    best_bid: Option<f64>,
    #[serde(rename = "bestAsk", default)]
    best_ask: Option<f64>,
    #[serde(default)]
    volume: Option<String>,
    #[serde(rename = "endDate", default)]
    end_date: Option<String>,
    #[serde(rename = "active", default)]
    active: Option<bool>,
    #[serde(rename = "closed", default)]
    closed: Option<bool>,
    #[serde(rename = "liquidity", default)]
    liquidity: Option<String>,
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Fetch active binary markets from Polymarket.
///
/// Returns markets that are:
///   - active = true, closed = false
///   - have valid bid/ask prices
///   - have a parseable end_date in the future
///   - 24h volume >= `min_volume_24h`
pub async fn fetch_active_markets(
    client: &Client,
    api_url: &str,
    min_volume_24h: f64,
    max_markets: usize,
) -> Result<Vec<RawMarket>> {
    let mut markets = Vec::new();
    let mut offset = 0usize;

    loop {
        let url = format!(
            "{api_url}/markets?active=true&closed=false&limit={PAGE_LIMIT}&offset={offset}"
        );

        let resp: Vec<GammaMarket> = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Polymarket API request failed: {e}"))?
            .error_for_status()
            .map_err(|e| anyhow!("Polymarket API error: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Polymarket API JSON parse failed: {e}"))?;

        if resp.is_empty() {
            break;
        }

        let fetched = resp.len();

        for gm in resp {
            match parse_active_market(gm, min_volume_24h) {
                Ok(m) => {
                    markets.push(m);
                    if markets.len() >= max_markets {
                        break;
                    }
                }
                Err(e) => warn!("skipping market: {e}"),
            }
        }

        if markets.len() >= max_markets || fetched < PAGE_LIMIT {
            break;
        }

        offset += PAGE_LIMIT;
    }

    info!("found {} tradeable markets", markets.len());
    Ok(markets)
}

// ── Private helpers ────────────────────────────────────────────────────────────

fn parse_active_market(gm: GammaMarket, min_volume: f64) -> Result<RawMarket> {
    if gm.active != Some(true) {
        return Err(anyhow!("{}: not active", gm.condition_id));
    }
    if gm.closed == Some(true) {
        return Err(anyhow!("{}: already closed", gm.condition_id));
    }

    // YES price from bestBid — best available buy price for YES tokens
    let yes_price = gm
        .best_bid
        .ok_or_else(|| anyhow!("{}: missing bestBid", gm.condition_id))?
        .clamp(0.0, 1.0);

    let no_price = gm
        .best_ask
        .map(|ask| (1.0 - ask).clamp(0.0, 1.0))
        .unwrap_or_else(|| (1.0 - yes_price).clamp(0.0, 1.0));

    let volume_24h = gm
        .volume
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    if volume_24h < min_volume {
        return Err(anyhow!(
            "{}: volume {:.0} below minimum {:.0}",
            gm.condition_id,
            volume_24h,
            min_volume
        ));
    }

    let end_date: DateTime<Utc> = gm
        .end_date
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .ok_or_else(|| anyhow!("{}: missing or invalid end_date", gm.condition_id))?;

    if end_date <= Utc::now() {
        return Err(anyhow!("{}: market already expired", gm.condition_id));
    }

    Ok(RawMarket {
        market_id: gm.condition_id,
        question: gm.question,
        yes_price,
        no_price,
        volume_24h,
        end_date,
        outcome: None,
    })
}
