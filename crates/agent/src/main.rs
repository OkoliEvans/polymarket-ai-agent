mod config;
mod market;
mod recorder;
mod trader;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use chrono::Utc;
use common::{db::DbPool, extract_features};
use model::Mlp;
use recorder::{build_record, update_record_with_proof, ProofStatus, Recorder};
use reqwest::Client;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::config::Config;

/// Path to the persisted set of market IDs that have already been bet on.
/// Survives restarts so the agent never double-bets the same market.
const BET_IDS_PATH: &str = "records/bet_market_ids.json";

// ── Bet-ID persistence helpers ────────────────────────────────────────────────

/// Load the persisted bet market-ID set from disk.
/// Returns an empty set if the file does not exist or is malformed.
fn load_bet_ids() -> HashSet<String> {
    match std::fs::read_to_string(BET_IDS_PATH) {
        Ok(raw) => serde_json::from_str::<HashSet<String>>(&raw).unwrap_or_default(),
        Err(_) => HashSet::new(),
    }
}

/// Persist the bet market-ID set to disk.
/// Failures are logged but never propagate — losing the file just means
/// the agent might re-bet on a market after restart, which is acceptable.
fn save_bet_ids(ids: &HashSet<String>) {
    if let Some(parent) = std::path::Path::new(BET_IDS_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string(ids) {
        Ok(json) => {
            if let Err(e) = std::fs::write(BET_IDS_PATH, json) {
                warn!("failed to persist bet_ids to disk: {e}");
            }
        }
        Err(e) => warn!("failed to serialise bet_ids: {e}"),
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env()?;

    if cfg.paper_trading {
        warn!("PAPER TRADING MODE — no real bets will be placed");
    }

    // ── Load model ────────────────────────────────────────────────────────────
    let mlp = Arc::new(Mlp::from_file(&cfg.weights_path).map_err(|e| {
        anyhow::anyhow!("failed to load model weights: {e}\nRun `cargo run -p trainer` first.")
    })?);
    info!(weights = %cfg.weights_path, "model loaded");

    // ── HTTP clients ──────────────────────────────────────────────────────────
    let http = Client::builder()
        .user_agent("polymarket-veil-agent/0.1")
        .timeout(Duration::from_secs(30))
        .build()?;

    let veil_http = Client::builder()
        .user_agent("polymarket-veil-agent/0.1")
        .timeout(Duration::from_secs(300))
        .build()?;

    // ── DB pool ───────────────────────────────────────────────────────────────
    let db_pool: Arc<Option<DbPool>> = Arc::new(match &cfg.database_url {
        Some(url) => match common::db::build_pool(url).await {
            Ok(pool) => {
                info!("DB pool connected — bets will be persisted");
                Some(pool)
            }
            Err(e) => {
                warn!("DB connect failed ({e}) — bets will NOT be persisted");
                None
            }
        },
        None => {
            warn!("DATABASE_URL not set — bets will NOT be persisted");
            None
        }
    });

    // ── Recorder ──────────────────────────────────────────────────────────────
    let records_path = format!(
        "records/trades_{}.jsonl",
        Utc::now().format("%Y%m%d_%H%M%S")
    );
    let recorder = Arc::new(Recorder::open(&records_path)?);
    info!(path = %records_path, "trade recorder ready");

    // ── Bet dedup set ─────────────────────────────────────────────────────────
    // Loaded from disk so it survives restarts.
    let bet_ids: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(load_bet_ids()));
    info!(
        known_bets = bet_ids.lock().unwrap().len(),
        path = BET_IDS_PATH,
        "bet dedup set loaded"
    );

    info!(
        gateway      = %cfg.veil_gateway_url,
        model_id     = %cfg.veil_model_id,
        min_conf     = cfg.min_confidence,
        max_bet_usdc = cfg.max_bet_usdc,
        poll_secs    = cfg.poll_interval_secs,
        "agent starting"
    );

    // ── Main loop ─────────────────────────────────────────────────────────────
    loop {
        match run_cycle(
            &cfg,
            &mlp,
            &http,
            &veil_http,
            Arc::clone(&recorder),
            Arc::clone(&db_pool),
            Arc::clone(&bet_ids),
        )
        .await
        {
            Ok(n) => info!("cycle complete — {n} decisions made"),
            Err(e) => error!("cycle error: {e}"),
        }

        info!("sleeping {}s until next cycle", cfg.poll_interval_secs);
        sleep(Duration::from_secs(cfg.poll_interval_secs)).await;
    }
}

// ── Cycle ─────────────────────────────────────────────────────────────────────

async fn run_cycle(
    cfg: &Config,
    mlp: &Arc<Mlp>,
    http: &Client,
    veil: &Client,
    recorder: Arc<Recorder>,
    db_pool: Arc<Option<DbPool>>,
    bet_ids: Arc<Mutex<HashSet<String>>>,
) -> Result<usize> {
    let markets =
        market::fetch_active_markets(http, &cfg.polymarket_api_url, cfg.min_volume_24h, 100)
            .await?;

    if markets.is_empty() {
        warn!("no tradeable markets found this cycle");
        return Ok(0);
    }

    let mut decisions = 0;
    let max_decisions_per_cycle = 1;

    for raw in &markets {
        // ── Duplicate-bet guard ───────────────────────────────────────────────
        {
            let ids = bet_ids.lock().unwrap();
            if ids.contains(&raw.market_id) {
                info!(market_id = %raw.market_id, "skipping — already bet on this market");
                continue;
            }
        }

        let features = match extract_features(raw, false) {
            Ok(f) => f,
            Err(e) => {
                warn!("feature extraction failed for {}: {e}", raw.market_id);
                continue;
            }
        };

        let input: Vec<f32> = features.values.iter().map(|&v| v as f32).collect();
        let decision = match mlp.decide(&input, cfg.min_confidence) {
            Ok(d) => d,
            Err(e) => {
                error!("inference failed for {}: {e}", raw.market_id);
                continue;
            }
        };

        info!(
            market_id = %raw.market_id,
            question  = %raw.question,
            decision  = %decision,
            yes_price = raw.yes_price,
            no_price  = raw.no_price,
            volume    = raw.volume_24h,
            "decision"
        );

        if !decision.is_actionable() || decisions >= max_decisions_per_cycle {
            let record = build_record(&features, &raw.question, &decision, None);
            let _ = recorder.record(&record);
            continue;
        }

        // ── Mark as bet BEFORE proof submission ───────────────────────────────
        // We mark eagerly so that even if proof submission fails we don't
        // retry the same market in the next cycle.
        {
            let mut ids = bet_ids.lock().unwrap();
            ids.insert(raw.market_id.clone());
            save_bet_ids(&ids);
        }
        info!(market_id = %raw.market_id, "market_id added to bet dedup set");

        decisions += 1;

        let input_data = features.to_input();
        let job_result =
            submit_proof_job(veil, &cfg.veil_gateway_url, &cfg.veil_model_id, input_data).await;

        let mut record = build_record(&features, &raw.question, &decision, None);
        if let Ok(ref job_id) = job_result {
            record.job_id = Some(job_id.clone());
        }
        let _ = recorder.record(&record);

        if let Ok(job_id) = job_result {
            let veil_clone = veil.clone();
            let http_clone = http.clone();
            let cfg_clone = cfg.clone();
            let recorder_c = Arc::clone(&recorder);
            let record_clone = record.clone();
            let raw_clone = raw.clone();
            let decision_clone = decision.clone();
            let db_clone = Arc::clone(&db_pool);

            tokio::spawn(async move {
                poll_and_bet(
                    &veil_clone,
                    &http_clone,
                    &cfg_clone,
                    &job_id,
                    record_clone,
                    recorder_c,
                    raw_clone,
                    decision_clone,
                    db_clone,
                )
                .await;
            });
        }
    }

    Ok(decisions)
}

// ── Proof poller + bet placer ─────────────────────────────────────────────────

async fn poll_and_bet(
    veil: &Client,
    http: &Client,
    cfg: &Config,
    job_id: &str,
    record: recorder::TradeRecord,
    recorder: Arc<Recorder>,
    raw: common::RawMarket,
    decision: model::Decision,
    db_pool: Arc<Option<DbPool>>,
) {
    #[derive(serde::Deserialize)]
    struct StatusResp {
        status: String,
        attestation_hash: Option<String>,
        tx_hash: Option<String>,
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    let mut bet_placed = false;
    let mut attestation_hash_seen: Option<String> = None;
    let mut db_bet_id: Option<uuid::Uuid> = None;

    loop {
        if tokio::time::Instant::now() > deadline {
            warn!(%job_id, "proof poll timed out after 300s");
            let final_record =
                update_record_with_proof(record, job_id, None, None, ProofStatus::Failed);
            let _ = recorder.record(&final_record);
            return;
        }

        sleep(Duration::from_secs(4)).await;

        let resp = match veil
            .get(format!("{}/v1/jobs/{job_id}", cfg.veil_gateway_url))
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            Ok(r) => r,
            Err(e) => {
                warn!(%job_id, "poll request failed: {e}");
                continue;
            }
        };

        let status: StatusResp = match resp.json().await {
            Ok(s) => s,
            Err(e) => {
                warn!(%job_id, "poll parse failed: {e}");
                continue;
            }
        };

        match status.status.as_str() {
            "proving" => {
                if !bet_placed {
                    if let Some(ref h) = status.attestation_hash {
                        info!(%job_id, attestation_hash = %h, "phase 1 attested — placing bet");
                        attestation_hash_seen = Some(h.clone());

                        let bet_result = trader::place_bet(
                            http,
                            &cfg.polymarket_clob_url,
                            &cfg.polymarket_private_key,
                            &cfg.polymarket_proxy,
                            &raw.market_id,
                            raw.yes_price,
                            raw.no_price,
                            &decision,
                            cfg.max_bet_usdc,
                            cfg.paper_trading,
                        )
                        .await;

                        match &bet_result {
                            Ok(b) => info!(%job_id, "bet placed: {:?}", b),
                            Err(e) => warn!(%job_id, "bet failed: {e}"),
                        }

                        // ── Persist bet to DB ─────────────────────────────────
                        if let Some(pool) = db_pool.as_ref() {
                            let side = match &decision {
                                model::Decision::BuyYes { .. } => "YES",
                                model::Decision::BuyNo { .. } => "NO",
                                _ => "SKIP",
                            };
                            let (size, price, paper) = match &bet_result {
                                Ok(b) => (b.size_usdc, b.price, b.paper),
                                Err(_) => (cfg.max_bet_usdc, raw.yes_price, cfg.paper_trading),
                            };

                            let new_bet = common::models::NewBet {
                                id: uuid::Uuid::new_v4(),
                                job_id: None, // linked to jobs table after settlement
                                market_id: raw.market_id.clone(),
                                question: raw.question.clone(),
                                side: side.into(),
                                size_usdc: size,
                                price,
                                paper,
                                confidence: decision.confidence() as f64,
                                yes_price: raw.yes_price,
                                no_price: raw.no_price,
                                volume_24h: raw.volume_24h,
                                attestation_hash: Some(h.clone()),
                                tx_hash: None,
                                placed_at: Utc::now(),
                            };

                            match common::repo::insert_bet(pool, new_bet).await {
                                Ok(bet) => {
                                    info!(%job_id, bet_id = %bet.id, "bet persisted to DB");
                                    db_bet_id = Some(bet.id);
                                }
                                Err(e) => error!(%job_id, "failed to persist bet: {e}"),
                            }
                        }

                        // Update JSONL record with bet
                        let updated = build_record(
                            &common::FeatureVector {
                                values: record.features,
                                market_id: record.market_id.clone(),
                            },
                            &record.question,
                            &decision,
                            bet_result.as_ref().ok(),
                        );
                        let _ = recorder.record(&updated);
                        bet_placed = true;
                    }
                }
            }

            "done" | "settled" => {
                let proof_status = if status.status == "settled" {
                    ProofStatus::Settled
                } else {
                    ProofStatus::Attested
                };

                // Resolve the best attestation_hash we have: prefer what we
                // observed during the "proving" phase; fall back to what the
                // "done" response carries (covers fast proofs that skip the
                // intermediate "proving" poll).
                let resolved_attestation = attestation_hash_seen
                    .as_deref()
                    .or(status.attestation_hash.as_deref());

                // ── Link bet to settled proof in DB ───────────────────────────
                if let (Some(pool), Some(bid)) = (db_pool.as_ref(), db_bet_id) {
                    // job_id must be a valid UUID — it comes from the gateway
                    // which always issues UUIDs. If parse fails, log and skip
                    // the link rather than writing a zeroed UUID.
                    match uuid::Uuid::parse_str(job_id) {
                        Ok(job_uuid) => {
                            if let Err(e) = common::repo::link_bet_to_job(
                                pool,
                                bid,
                                job_uuid,
                                resolved_attestation.unwrap_or(""),
                                status.tx_hash.as_deref().unwrap_or(""),
                            )
                            .await
                            {
                                error!(%job_id, "failed to link bet to job: {e}");
                            } else {
                                info!(%job_id, bet_id = %bid, "bet linked to settled proof");
                            }
                        }
                        Err(e) => {
                            error!(%job_id, "job_id is not a valid UUID — skipping DB link: {e}");
                        }
                    }
                }

                let final_record = update_record_with_proof(
                    record,
                    job_id,
                    resolved_attestation.map(str::to_owned),
                    status.tx_hash,
                    proof_status,
                );
                let _ = recorder.record(&final_record);
                info!(%job_id, "proof record finalised");
                return;
            }

            "failed" => {
                warn!(%job_id, "proof job failed — bet not placed");
                let final_record =
                    update_record_with_proof(record, job_id, None, None, ProofStatus::Failed);
                let _ = recorder.record(&final_record);
                return;
            }

            other => {
                // Unexpected status — log and keep polling.
                warn!(%job_id, status = %other, "unrecognised job status — will retry");
            }
        }
    }
}

// ── Veil gateway ──────────────────────────────────────────────────────────────

async fn submit_proof_job(
    client: &Client,
    gateway: &str,
    model_id: &str,
    input_data: Vec<Vec<f64>>,
) -> Result<String> {
    #[derive(serde::Serialize)]
    struct SubmitReq<'a> {
        model_id: &'a str,
        input_data: Vec<Vec<f64>>,
    }

    #[derive(serde::Deserialize)]
    struct SubmitResp {
        job_id: String,
    }

    let resp = client
        .post(format!("{gateway}/v1/jobs"))
        .json(&SubmitReq {
            model_id,
            input_data,
        })
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Veil submit failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("Veil submit error: {e}"))?
        .json::<SubmitResp>()
        .await
        .map_err(|e| anyhow::anyhow!("Veil submit parse failed: {e}"))?;

    info!(job_id = %resp.job_id, "proof job submitted");
    Ok(resp.job_id)
}