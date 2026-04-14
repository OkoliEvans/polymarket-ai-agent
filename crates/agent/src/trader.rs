// crates/agent/src/trader.rs
//! Bet placement — Polymarket CLOB API.
//!
//! Paper trading is the default. Set PAPER_TRADING=false to place real bets.
//!
//! The CLOB API requires signed orders. Signing is done via the private key
//! in config. Orders are limit orders at the current best price.

use anyhow::{anyhow, Result};
use model::Decision;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Result of a bet placement attempt.
#[derive(Debug, Clone)]
pub struct BetResult {
    /// Unique bet/order ID from Polymarket.
    pub bet_id: String,
    /// Market the bet was placed on.
    pub market_id: String,
    /// Direction of the bet.
    pub side: BetSide,
    /// Size in USDC.
    pub size_usdc: f64,
    /// Price paid (YES or NO token price).
    pub price: f64,
    /// Whether this was a paper trade.
    pub paper: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BetSide {
    Yes,
    No,
}

impl std::fmt::Display for BetSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BetSide::Yes => write!(f, "YES"),
            BetSide::No => write!(f, "NO"),
        }
    }
}

#[derive(Debug, Serialize)]
struct OrderRequest {
    #[serde(rename = "tokenID")]
    token_id: String,
    price: f64,
    size: f64,
    side: String,
    #[serde(rename = "orderType")]
    order_type: String,
}

#[derive(Debug, Deserialize)]
struct OrderResponse {
    #[serde(rename = "orderID")]
    order_id: String,
}

/// Place a bet based on the model's decision.
///
/// In paper trading mode: generates a synthetic bet_id and returns immediately.
/// In live mode: signs and submits a limit order to the CLOB API.
pub async fn place_bet(
    client: &Client,
    clob_url: &str,
    private_key: &str,
    proxy_address: &str,
    market_id: &str,
    yes_price: f64,
    no_price: f64,
    decision: &Decision,
    max_usdc: f64,
    paper: bool,
) -> Result<BetResult> {
    let (side, price) = match decision {
        Decision::BuyYes { .. } => (BetSide::Yes, yes_price),
        Decision::BuyNo { .. } => (BetSide::No, no_price),
        Decision::Skip { .. } => return Err(anyhow!("cannot place bet on Skip decision")),
    };

    let size_usdc = max_usdc; // flat sizing for MVP — Kelly criterion in Phase 2

    if paper {
        let fake_id = format!(
            "paper-{}-{}",
            market_id.chars().take(8).collect::<String>(),
            uuid::Uuid::new_v4()
        );
        warn!(
            market_id,
            side = %side,
            price,
            size_usdc,
            "PAPER TRADE — no real bet placed"
        );
        return Ok(BetResult {
            bet_id: fake_id,
            market_id: market_id.to_string(),
            side,
            size_usdc,
            price,
            paper: true,
        });
    }

    // ── Live trading ──────────────────────────────────────────────────────────
    //
    // The CLOB API requires EIP-712 signed orders. Full signing implementation
    // requires the Polymarket order signer — this is a simplified skeleton.
    // Wire the actual signing logic once paper trading validates the model.

    let order = OrderRequest {
        token_id: market_id.to_string(),
        price,
        size: size_usdc / price, // convert USDC to token quantity
        side: format!("{side}"),
        order_type: "GTC".into(),
    };

    let resp = client
        .post(format!("{clob_url}/order"))
        .header("POLY_ADDRESS", proxy_address)
        .header("POLY_SIGNATURE", sign_order(private_key, &order)?)
        .json(&order)
        .send()
        .await
        .map_err(|e| anyhow!("CLOB API request failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow!("CLOB API error: {e}"))?
        .json::<OrderResponse>()
        .await
        .map_err(|e| anyhow!("CLOB response parse failed: {e}"))?;

    info!(
        market_id,
        side = %side,
        price,
        size_usdc,
        order_id = %resp.order_id,
        "bet placed"
    );

    Ok(BetResult {
        bet_id: resp.order_id,
        market_id: market_id.to_string(),
        side,
        size_usdc,
        price,
        paper: false,
    })
}

/// Stub — replace with real EIP-712 signing once paper trading is validated.
/// The Polymarket SDK (JS) or a Rust EIP-712 library should be used here.
fn sign_order(_private_key: &str, _order: &OrderRequest) -> Result<String> {
    Err(anyhow!(
        "live order signing not yet implemented — run with PAPER_TRADING=true"
    ))
}
