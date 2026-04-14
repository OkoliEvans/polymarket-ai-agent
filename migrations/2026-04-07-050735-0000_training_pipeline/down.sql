-- migrations/2026-04-07-000003_training_pipeline/down.sql

-- Drop indexes first (optional, since DROP TABLE cascades indexes, but explicit is safer)

-- ── Bets ─────────────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS bets_job_id_idx;
DROP INDEX IF EXISTS bets_placed_at_idx;
DROP INDEX IF EXISTS bets_market_id_idx;

-- ── Training samples ─────────────────────────────────────────────────────────
DROP INDEX IF EXISTS training_samples_created_at_idx;
DROP INDEX IF EXISTS training_samples_market_id_idx;

-- ── Outcomes ─────────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS outcomes_market_id_idx;

-- ── Market snapshots ─────────────────────────────────────────────────────────
DROP INDEX IF EXISTS snapshots_captured_at_idx;
DROP INDEX IF EXISTS snapshots_market_id_idx;

-- Drop tables in reverse dependency order to avoid FK constraint issues

-- bets depends on jobs (but not vice versa)
DROP TABLE IF EXISTS bets;

-- training_samples depends on market_snapshots and outcomes
DROP TABLE IF EXISTS training_samples;

-- outcomes has no dependencies
DROP TABLE IF EXISTS outcomes;

-- market_snapshots has no dependencies
DROP TABLE IF EXISTS market_snapshots;