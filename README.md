# Polymarket Veil Agent
## Verifiable AI Trading on Prediction Markets

> Every bet is backed by a ZK proof. Every decision is auditable on-chain.

The Polymarket Veil Agent is an autonomous trading agent that places bets on Polymarket prediction markets using a trained MLP model — and proves every inference on-chain via **Mugen** (SP1 zkVM + HashKey testnet). No trust required: the model, the input, and the output are all cryptographically committed before any trade is placed.

---

## Table of Contents

- [Why This Matters](#why-this-matters)
- [Architecture](#architecture)
- [Crate Layout](#crate-layout)
- [Model](#model)
- [Training](#training)
- [Agent Lifecycle](#agent-lifecycle)
- [Dashboard](#dashboard)
- [Quick Start](#quick-start)
- [Environment Variables](#environment-variables)
- [Deployment Guide](#deployment-guide)

---

## Why This Matters

Algorithmic trading agents are black boxes. Anyone can claim their bot won 70% of trades — there is no way to verify the strategy wasn't cherry-picked, back-adjusted, or run on a different model than advertised.

The Polymarket Veil Agent changes this. Every trading decision follows a verifiable chain:

```
market snapshot → feature extraction → MLP inference → SP1 ZK proof → on-chain attestation → bet placed
```

The attestation hash is on HashKey testnet before the bet hits Polymarket. The model weights are registered on IPFS with a `sha256` commitment. The proof ties a specific input to a specific output from a specific model — permanently and unforgably.

This is not a trading bot. It is a demonstration that AI decision-making can be made **accountable by construction** — applicable to prediction markets today, and to finance, credit, and healthcare tomorrow.

---

## Architecture

```
Polymarket Gamma API
        │
        ▼
   snapshot crate          — hourly market snapshots → Postgres
        │
        ▼
   agent/features.rs       — extract [yes_price, no_price, volume_24h, time_to_expiry]
        │
        ▼
   model crate             — MLP forward pass (4→8→2, ReLU hidden, linear output)
   weights/polymarket_mlp_v1.bin
        │
        ▼
   Decision: BuyYes | BuyNo | Skip (confidence threshold)
        │
        ├── Skip → recorder.rs logs TradeRecord { proof_status: Skipped }
        │
        └── Actionable
              │
              ├── trader.rs → Polymarket CLOB API (or paper trade if PAPER_TRADING=true)
              │    → bet_id
              │
              ├── POST /v1/jobs → Mugen Gateway
              │    Phase 1 (~60s): compressed SP1 proof → attestation_hash
              │    Phase 2 (batch): aggregated Groth16 → tx_hash on HashKey
              │
              └── recorder.rs → Postgres bets table
                   { market_id, side, size_usdc, confidence,
                     attestation_hash, tx_hash, placed_at }
```

---

## Crate Layout

```
polymarket-veil-agent/
├── crates/
│   ├── agent/        — main loop, feature extraction, market client, trader, recorder
│   ├── model/        — MLP forward pass (lib.rs), schema.rs, Diesel models
│   ├── trainer/      — fetches resolved Polymarket markets, trains MLP via candle
│   ├── snapshot/     — cron: fetches active markets, writes market_snapshots to DB
│   ├── dashboard/    — Axum HTTP API serving bet history + proof status to frontend
│   └── common/       — shared feature vector type
├── weights/
│   ├── tiny_mlp.bin           — 58-param demo model (4→8→2)
│   └── polymarket_mlp_v1.bin  — trained on resolved Polymarket markets
└── frontend/                  — React dashboard (bet history, attestation hashes, proofs)
```

---

## Model

**Architecture:** 4 → 8 → 2 MLP (ReLU hidden layer, linear output)

**Input features (normalized):**

| Index | Feature | Description |
|---|---|---|
| 0 | `yes_price` | Current YES price from Polymarket order book (0–1) |
| 1 | `no_price` | `1 - yes_price` or bestAsk-derived |
| 2 | `volume_24h` | Normalized 24h trading volume |
| 3 | `time_to_expiry` | Seconds to market close, normalized |

**Output:** `[yes_logit, no_logit]` — softmax applied to get probabilities.

**Decision logic:**

```rust
if yes_prob >= min_confidence  → BuyYes { confidence }
if no_prob  >= min_confidence  → BuyNo  { confidence }
else                           → Skip   { confidence }
```

`min_confidence` defaults to `0.65`. Adjust via `MIN_CONFIDENCE` env var.

**Weights file:** `weights/polymarket_mlp_v1.bin` — 232 bytes, flat f32 little-endian. Same layout as the Mugen inference guest. The `sha256` of this file is the `model_id` committed on-chain.

---

## Training

The trainer fetches resolved Polymarket markets from the Gamma API and trains the MLP using [candle](https://github.com/huggingface/candle) (Rust ML framework).

**Data pipeline:**

```
Gamma API /markets?resolved=true
    │
    ▼ filter: has outcomePrices, has endDate, prices are unambiguous
    │
    ▼ label: outcomePrices[0] > 0.9 → YES won, < 0.1 → NO won
    │
    ▼ features: [yes_price, no_price, volume_24h, time_to_expiry_normalized]
    │
    ▼ train/val split (80/20 by time — not random, to prevent look-ahead)
    │
    ▼ SGD 200 epochs, lr=0.001, cross-entropy loss
    │
    ▼ weights/polymarket_mlp_v1.bin
```

**Run trainer:**

```bash
cd crates/trainer
cargo run --release
# Output: weights/polymarket_mlp_v1.bin (232 bytes)
```

**Trainer config:**

```dotenv
MAX_MARKETS=500        # markets to fetch from Gamma API
EPOCHS=200
LEARNING_RATE=0.001
WEIGHTS_OUT=weights/polymarket_mlp_v1.bin
```

**Note on overfitting:** The Gamma API returns resolved markets going back to 2020. Markets from 2020–2021 have `outcomePrices = [0, 0]` after resolution — these are filtered. If val_accuracy is 100%, the model is likely overfitting on old data with obvious outcomes. Retrain with recent markets (2024–2025) and a larger `MAX_MARKETS` for production use.

---

## Agent Lifecycle

Each cycle the agent:

1. **Fetches active markets** from the Polymarket Gamma API
2. **Extracts features** — `[yes_price, no_price, volume_24h, time_to_expiry]`
3. **Runs inference** via `model::Mlp::decide()` with the loaded weights
4. **Deduplicates** — skips markets already bet on in this session (prevents duplicate bets)
5. **Places bet** — via Polymarket CLOB API (or paper trade stub if `PAPER_TRADING=true`)
6. **Submits proof** — `POST /v1/jobs` to Mugen gateway, tracking job_id
7. **Polls for attestation** — waits for `attestation_hash` (phase 1, ~60s)
8. **Persists to DB** — writes `bets` row with `attestation_hash`, `tx_hash`, `placed_at`

**Proof settlement:** The Mugen gateway batches proofs from all running agents. When the batch threshold is reached (or flush timer fires), one aggregated Groth16 proof settles all N inference attestations on HashKey testnet in a single transaction. Each bet row in Postgres is updated with the settlement `tx_hash`.

---

## Dashboard

The dashboard shows live bet history with full proof provenance.

**Columns:**

| Column | Description |
|---|---|
| MARKET | Question + market ID |
| SIDE | YES / NO badge |
| SIZE | USDC amount staked |
| CONF. | Model confidence (%) |
| ATTESTATION | `keccak256(model_id ‖ input_hash ‖ output_hash)` — links to HashKey explorer |
| TX | Settlement tx hash — links to HashKey testnet explorer |
| P&L | Resolved profit/loss (pending until market closes) |
| PLACED | Relative timestamp |

**Status indicators (top row):**

- `SETTLED PROOFS` — proofs confirmed on HashKey testnet
- `PROVING` — jobs currently in the SP1 prover pipeline
- `FAILED` — proof or settlement failures
- `PAPER TRADING` badge — shown when `PAPER_TRADING=true`

**Run dashboard:**

```bash
cd frontend
npm install && npm run dev
# Opens at http://localhost:3000
```

The dashboard polls `GET /api/bets` and `GET /api/stats` from the dashboard crate (Axum on port 3001 by default) which reads from the same Postgres instance as the agent.

---

## Quick Start

### 1. Clone and build

```bash
git clone https://github.com/kharonlabs/polymarket-veil-agent
cd polymarket-veil-agent
cargo build --release
```

### 2. Set up Postgres

```bash
createdb veil_agent
# Migrations run automatically on first startup
```

### 3. Train the model

```bash
cargo run --release -p trainer
# Produces: weights/polymarket_mlp_v1.bin
```

### 4. Register the model with Mugen

```bash
# Mugen gateway must be running (see Mugen README)
curl -X POST http://localhost:8080/v1/models \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"polymarket_mlp_v1\",
    \"version\": \"0.1.0\",
    \"artifact_b64\": \"$(base64 -i weights/polymarket_mlp_v1.bin)\",
    \"input_shape\": [1, 4]
  }"
```

### 5. Run the agent

```bash
# Paper trading (safe, no real money)
PAPER_TRADING=true cargo run --release -p agent

# Live trading (requires Polymarket credentials)
PAPER_TRADING=false cargo run --release -p agent
```

### 6. Run the snapshot cron

```bash
# Run once manually or add to crontab / systemd timer
cargo run --release -p snapshot

# Crontab (every hour)
0 * * * * /path/to/target/release/snapshot >> /var/log/veil-snapshot.log 2>&1
```

---

## Environment Variables

```dotenv
# Database
DATABASE_URL=postgresql://user:pass@localhost:5432/veil_agent

# Mugen Gateway
MUGEN_GATEWAY_URL=http://localhost:8080
MUGEN_MODEL_ID=polymarket_mlp_v1

# Agent behaviour
PAPER_TRADING=true             # true = no real bets placed
MIN_CONFIDENCE=0.65            # skip decisions below this threshold
BET_SIZE_USDC=5.0              # size per bet in USDC
CYCLE_INTERVAL_SECS=300        # how often the agent cycles (default: 5 min)
MAX_MARKETS_PER_CYCLE=20       # markets to evaluate per cycle

# Polymarket (required for live trading)
POLYMARKET_PRIVATE_KEY=0x...   # EVM wallet private key
POLYMARKET_PROXY_ADDRESS=0x... # your Polymarket CTF Exchange proxy contract

# Model
MODEL_WEIGHTS_PATH=weights/polymarket_mlp_v1.bin

# Trainer
MAX_MARKETS=500
EPOCHS=200
LEARNING_RATE=0.001
WEIGHTS_OUT=weights/polymarket_mlp_v1.bin

# Dashboard API
DASHBOARD_PORT=3001
```

---

## Deployment Guide

### Prerequisites

- Rust 1.75+
- PostgreSQL 14+
- Node.js 18+ (for frontend)
- A running Mugen gateway with `polymarket_mlp_v1` registered

### Production setup

```bash
# Build all binaries
cargo build --release

# Run snapshot cron (systemd timer or crontab)
./target/release/snapshot

# Run agent as a systemd service
[Unit]
Description=Polymarket Veil Agent
After=network.target postgresql.service

[Service]
ExecStart=/path/to/target/release/agent
EnvironmentFile=/etc/veil-agent/env
Restart=on-failure
RestartSec=30

[Install]
WantedBy=multi-user.target
```

### Getting your Polymarket proxy address

1. Go to [polymarket.com](https://polymarket.com) and connect your wallet
2. Complete onboarding — Polymarket deploys a personal proxy contract
3. Find your proxy address:

```bash
curl https://gamma-api.polymarket.com/profiles?address=0xYOUR_WALLET_ADDRESS
# Look for "proxyWallet" in the response
```

### Paper trading mode

Set `PAPER_TRADING=true` (default). The agent evaluates markets and runs proofs exactly as in live mode, but the `trader.rs` stub returns a fake `bet_id` without touching Polymarket's CLOB API. All proof attestations are real and settle on HashKey testnet. The dashboard shows full data. This is the recommended mode for demo and testing.

---

## Database Schema

Key tables (shared Postgres instance with Mugen gateway if co-located):

| Table | Description |
|---|---|
| `bets` | Every bet placed — includes attestation_hash, tx_hash, outcome, pnl |
| `market_snapshots` | Hourly Polymarket market state (yes_price, no_price, volume, end_date) |
| `outcomes` | Resolved market outcomes — used for PnL calculation |
| `training_samples` | Joined snapshot + outcome rows used by the trainer |
| `models` | Registered model metadata (ipfs_cid, on_chain_hash) |
| `jobs` | SP1 proof job lifecycle (queued → proving → done → settled) |

---

## License

MIT