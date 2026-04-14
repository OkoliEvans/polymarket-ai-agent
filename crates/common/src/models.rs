// crates/common/src/models.rs
//
// Diesel Queryable structs for the jobs and bets tables.
// These are read-only from the API's perspective — the agent writes, the API reads.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

// ── Job ───────────────────────────────────────────────────────────────────────

/// A proof job from the Mugen gateway.
/// status lifecycle: queued → running → proving → done → settled | failed
#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = crate::schema::jobs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Job {
    pub id: Uuid,
    pub status: String,
    pub input_hash: String,
    pub proof_path: Option<String>,
    pub error: Option<String>,
    pub submitted_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub settled_at: Option<DateTime<Utc>>,
    pub tx_hash: Option<String>,
    pub attestation_hash: Option<String>,
}

// ── Bet ───────────────────────────────────────────────────────────────────────

/// A Polymarket bet placed by the agent.
#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = crate::schema::bets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Bet {
    pub id: Uuid,
    pub job_id: Option<Uuid>,
    pub market_id: String,
    pub question: String,
    pub side: String,
    pub size_usdc: f64,
    pub price: f64,
    pub paper: bool,
    pub confidence: f64,
    pub yes_price: f64,
    pub no_price: f64,
    pub volume_24h: f64,
    pub attestation_hash: Option<String>,
    pub tx_hash: Option<String>,
    pub outcome: Option<bool>,
    pub pnl_usdc: Option<f64>,
    pub placed_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

// ── Insertable ────────────────────────────────────────────────────────────────

/// Used by the agent to record a new bet.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::bets)]
pub struct NewBet {
    pub id: Uuid,
    pub job_id: Option<Uuid>,
    pub market_id: String,
    pub question: String,
    pub side: String,
    pub size_usdc: f64,
    pub price: f64,
    pub paper: bool,
    pub confidence: f64,
    pub yes_price: f64,
    pub no_price: f64,
    pub volume_24h: f64,
    pub attestation_hash: Option<String>,
    pub tx_hash: Option<String>,
    pub placed_at: DateTime<Utc>,
}
