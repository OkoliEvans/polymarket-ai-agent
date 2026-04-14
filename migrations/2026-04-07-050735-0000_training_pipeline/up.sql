-- Your SQL goes here
-- migrations/2026-04-07-000003_training_pipeline/up.sql

-- ── Market snapshots — hourly captures of live market state ──────────────────
CREATE TABLE market_snapshots (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    market_id      TEXT        NOT NULL,
    question       TEXT        NOT NULL,
    yes_price      DOUBLE PRECISION NOT NULL,
    no_price       DOUBLE PRECISION NOT NULL,
    volume_24h     DOUBLE PRECISION NOT NULL,
    end_date       TIMESTAMPTZ NOT NULL,
    captured_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX snapshots_market_id_idx  ON market_snapshots(market_id);
CREATE INDEX snapshots_captured_at_idx ON market_snapshots(captured_at DESC);

-- ── Outcomes — resolved market results ───────────────────────────────────────
CREATE TABLE outcomes (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    market_id      TEXT        NOT NULL UNIQUE,
    question       TEXT        NOT NULL,
    resolved_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    outcome        BOOLEAN     NOT NULL  -- true = YES won, false = NO won
);

CREATE INDEX outcomes_market_id_idx ON outcomes(market_id);

-- ── Training samples — snapshot + outcome join ────────────────────────────────
CREATE TABLE training_samples (
    id             UUID             PRIMARY KEY DEFAULT gen_random_uuid(),
    snapshot_id    UUID             NOT NULL REFERENCES market_snapshots(id),
    outcome_id     UUID             NOT NULL REFERENCES outcomes(id),
    market_id      TEXT             NOT NULL,
    yes_price      DOUBLE PRECISION NOT NULL,
    no_price       DOUBLE PRECISION NOT NULL,
    volume_24h     DOUBLE PRECISION NOT NULL,
    time_to_expiry DOUBLE PRECISION NOT NULL, -- days at snapshot time
    outcome        BOOLEAN          NOT NULL,
    created_at     TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);

CREATE INDEX training_samples_market_id_idx ON training_samples(market_id);
CREATE INDEX training_samples_created_at_idx ON training_samples(created_at DESC);

-- ── Bets — agent bet history ──────────────────────────────────────────────────
CREATE TABLE bets (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id           UUID        REFERENCES jobs(id),
    market_id        TEXT        NOT NULL,
    question         TEXT        NOT NULL,
    side             TEXT        NOT NULL CHECK (side IN ('YES', 'NO')),
    size_usdc        DOUBLE PRECISION NOT NULL,
    price            DOUBLE PRECISION NOT NULL,
    paper            BOOLEAN     NOT NULL DEFAULT true,
    confidence       DOUBLE PRECISION NOT NULL,
    yes_price        DOUBLE PRECISION NOT NULL,
    no_price         DOUBLE PRECISION NOT NULL,
    volume_24h       DOUBLE PRECISION NOT NULL,
    attestation_hash TEXT,
    tx_hash          TEXT,
    outcome          BOOLEAN,        -- filled in when market resolves
    pnl_usdc         DOUBLE PRECISION, -- filled in when market resolves
    placed_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at      TIMESTAMPTZ
);

CREATE INDEX bets_market_id_idx  ON bets(market_id);
CREATE INDEX bets_placed_at_idx  ON bets(placed_at DESC);
CREATE INDEX bets_job_id_idx     ON bets(job_id);