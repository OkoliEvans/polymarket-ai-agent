//! Market snapshot cron — run every hour via cron or systemd timer.
//! Fetches active Polymarket markets and writes snapshots to DB.
//!
//! Usage: cargo run -p snapshot

use anyhow::Result;
use chrono::Utc;
use common::schema::market_snapshots;
use diesel::prelude::*;
use diesel_async::{
    pooled_connection::{bb8::Pool, AsyncDieselConnectionManager},
    AsyncPgConnection, RunQueryDsl,
};
use reqwest::Client;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

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
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    liquidity: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = market_snapshots)]
struct NewMarketSnapshot {
    id: Uuid,
    market_id: String,
    question: String,
    yes_price: f64,
    no_price: f64,
    volume_24h: f64,
    end_date: chrono::DateTime<Utc>,
    captured_at: chrono::DateTime<Utc>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt().with_env_filter("info").init();

    let db_url = std::env::var("DATABASE_URL")?;
    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(db_url);
    let pool: Pool<AsyncPgConnection> = Pool::builder().build(manager).await?;

    let client = Client::builder()
        .user_agent("polymarket-veil-agent/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let url = "https://gamma-api.polymarket.com/markets?active=true&closed=false&limit=100";
    let markets: Vec<GammaMarket> = client.get(url).send().await?.json().await?;

    let now = Utc::now();
    let mut inserted = 0usize;

    // Acquire a single connection for the entire batch rather than one per market.
    let mut conn = pool.get().await?;

    for m in &markets {
        if m.active != Some(true) || m.closed == Some(true) {
            continue;
        }

        // Compute yes_price as the midpoint of bestBid/bestAsk — consistent
        // with how the trainer computes it in parse_gamma_market. Using only
        // bestBid would create a train/inference mismatch.
        let yes_price = match (m.best_bid, m.best_ask) {
            (Some(bid), Some(ask)) if bid > 0.0 && ask >= bid => {
                ((bid + ask) / 2.0).clamp(0.02, 0.98)
            }
            (Some(bid), _) if bid > 0.0 => bid.clamp(0.02, 0.98),
            _ => continue, // no usable price — skip
        };

        // no_price is always 1 - yes_price, consistent with trainer.
        let no_price = (1.0 - yes_price).clamp(0.02, 0.98);

        let volume_24h = m
            .volume
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        let end_date: chrono::DateTime<Utc> = match m
            .end_date
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        {
            Some(dt) => dt.with_timezone(&Utc),
            None => continue,
        };

        if end_date <= now {
            continue;
        }

        let new_snapshot = NewMarketSnapshot {
            id: Uuid::new_v4(),
            market_id: m.condition_id.clone(),
            question: m.question.clone(),
            yes_price,
            no_price,
            volume_24h,
            end_date,
            captured_at: now,
        };

        diesel::insert_into(market_snapshots::table)
            .values(&new_snapshot)
            .execute(&mut conn)
            .await?;

        inserted += 1;
    }

    info!("snapshot complete — {inserted} markets captured");
    Ok(())
}