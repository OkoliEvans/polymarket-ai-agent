// crates/common/src/repo.rs
//
// Async repository functions — all DB access goes through here.
// Returns anyhow::Result so callers don't need to import diesel errors.

use anyhow::Result;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use crate::{
    db::DbPool,
    models::{Bet, Job, NewBet},
    schema::{bets, jobs},
};

// ── Jobs ──────────────────────────────────────────────────────────────────────

/// Fetch the most recent `limit` jobs, newest first.
pub async fn list_jobs(pool: &DbPool, limit: i64) -> Result<Vec<Job>> {
    let mut conn = pool.get().await?;
    let rows = jobs::table
        .order(jobs::submitted_at.desc())
        .limit(limit)
        .select(Job::as_select())
        .load(&mut conn)
        .await?;
    Ok(rows)
}

/// Fetch a single job by UUID.
pub async fn get_job(pool: &DbPool, id: uuid::Uuid) -> Result<Job> {
    let mut conn = pool.get().await?;
    let row = jobs::table
        .find(id)
        .select(Job::as_select())
        .first(&mut conn)
        .await?;
    Ok(row)
}

/// Fetch jobs filtered by status (e.g. "settled", "failed").
pub async fn list_jobs_by_status(pool: &DbPool, status: &str, limit: i64) -> Result<Vec<Job>> {
    let mut conn = pool.get().await?;
    let rows = jobs::table
        .filter(jobs::status.eq(status))
        .order(jobs::submitted_at.desc())
        .limit(limit)
        .select(Job::as_select())
        .load(&mut conn)
        .await?;
    Ok(rows)
}

// ── Bets ──────────────────────────────────────────────────────────────────────

/// Fetch the most recent `limit` bets, newest first.
pub async fn list_bets(pool: &DbPool, limit: i64) -> Result<Vec<Bet>> {
    let mut conn = pool.get().await?;
    let rows = bets::table
        .order(bets::placed_at.desc())
        .limit(limit)
        .select(Bet::as_select())
        .load(&mut conn)
        .await?;
    Ok(rows)
}

/// Fetch a single bet by UUID.
pub async fn get_bet(pool: &DbPool, id: uuid::Uuid) -> Result<Bet> {
    let mut conn = pool.get().await?;
    let row = bets::table
        .find(id)
        .select(Bet::as_select())
        .first(&mut conn)
        .await?;
    Ok(row)
}

/// Insert a new bet record. Called by the agent after placing a trade.
pub async fn insert_bet(pool: &DbPool, new_bet: NewBet) -> Result<Bet> {
    let mut conn = pool.get().await?;
    let row = diesel::insert_into(bets::table)
        .values(&new_bet)
        .returning(Bet::as_returning())
        .get_result(&mut conn)
        .await?;
    Ok(row)
}

/// Update a bet's outcome and PnL after market resolution.
pub async fn resolve_bet(
    pool: &DbPool,
    id: uuid::Uuid,
    outcome: bool,
    pnl_usdc: f64,
) -> Result<Bet> {
    let mut conn = pool.get().await?;
    let row = diesel::update(bets::table.find(id))
        .set((
            bets::outcome.eq(outcome),
            bets::pnl_usdc.eq(pnl_usdc),
            bets::resolved_at.eq(chrono::Utc::now()),
        ))
        .returning(Bet::as_returning())
        .get_result(&mut conn)
        .await?;
    Ok(row)
}

/// Link a bet to its proof job once the job is settled.
pub async fn link_bet_to_job(
    pool: &DbPool,
    bet_id: uuid::Uuid,
    job_id: uuid::Uuid,
    attestation_hash: &str,
    tx_hash: &str,
) -> Result<Bet> {
    let mut conn = pool.get().await?;
    let row = diesel::update(bets::table.find(bet_id))
        .set((
            bets::job_id.eq(job_id),
            bets::attestation_hash.eq(attestation_hash),
            bets::tx_hash.eq(tx_hash),
        ))
        .returning(Bet::as_returning())
        .get_result(&mut conn)
        .await?;
    Ok(row)
}
