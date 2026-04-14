// crates/agent/src/recorder.rs
//! Trade and proof record keeping.
//!
//! Every trade decision — including skips — is recorded with its full audit trail:
//!   { market_id, question, features, decision, bet_id, job_id,
//!     attestation_hash, tx_hash, timestamp }
//!
//! Records are written to JSONL (newline-delimited JSON) for easy ingestion
//! into any downstream analytics system. One file per run, rotated daily.

use anyhow::Result;
use chrono::Utc;
use common::FeatureVector;
use model::Decision;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};
use tracing::info;

use crate::trader::BetResult;

/// A fully resolved trade record — written once the proof job completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub timestamp: chrono::DateTime<Utc>,
    pub market_id: String,
    pub question: String,
    /// Normalized feature vector fed to the model.
    pub features: [f64; 4],
    /// Model output.
    pub decision: String,
    pub confidence: f32,
    /// YES or NO.
    pub side: Option<String>,
    pub size_usdc: Option<f64>,
    pub price: Option<f64>,
    pub paper: bool,
    /// Polymarket order ID — None if skipped or paper trade.
    pub bet_id: Option<String>,
    /// Veil proof job ID.
    pub job_id: Option<String>,
    /// keccak256 attestation — available after phase 1 (~60s).
    pub attestation_hash: Option<String>,
    /// On-chain tx hash — available after phase 2 + settlement (~120s).
    pub tx_hash: Option<String>,
    /// Status of the proof job.
    pub proof_status: ProofStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProofStatus {
    Pending,
    Attested,
    Settled,
    Failed,
    Skipped, // decision was Skip — no proof submitted
}

/// Thread-safe append-only record store.
pub struct Recorder {
    file: Arc<Mutex<File>>,
    path: String,
}

impl Recorder {
    /// Open (or create) the records file at `path`.
    pub fn open(path: &str) -> Result<Self> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| anyhow::anyhow!("failed to open records file '{path}': {e}"))?;

        info!(%path, "recorder opened");
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            path: path.to_string(),
        })
    }

    /// Append a trade record.
    pub fn record(&self, r: &TradeRecord) -> Result<()> {
        let line = serde_json::to_string(r)? + "\n";
        let mut f = self.file.lock().unwrap();
        f.write_all(line.as_bytes())
            .map_err(|e| anyhow::anyhow!("failed to write record: {e}"))?;
        f.flush()?;
        Ok(())
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Build an initial TradeRecord at decision time (before proof is submitted).
pub fn build_record(
    features: &FeatureVector,
    question: &str,
    decision: &Decision,
    bet: Option<&BetResult>,
) -> TradeRecord {
    let (side, size_usdc, price, bet_id, paper) = match bet {
        Some(b) => (
            Some(format!("{}", b.side)),
            Some(b.size_usdc),
            Some(b.price),
            Some(b.bet_id.clone()),
            b.paper,
        ),
        None => (None, None, None, None, true),
    };

    TradeRecord {
        timestamp: Utc::now(),
        market_id: features.market_id.clone(),
        question: question.to_string(),
        features: features.values,
        decision: format!("{decision}"),
        confidence: decision.confidence(),
        side,
        size_usdc,
        price,
        paper,
        bet_id,
        job_id: None,
        attestation_hash: None,
        tx_hash: None,
        proof_status: if decision.is_actionable() {
            ProofStatus::Pending
        } else {
            ProofStatus::Skipped
        },
    }
}

/// Update a record after the proof job completes.
/// Called by the background proof poller.
pub fn update_record_with_proof(
    mut record: TradeRecord,
    job_id: &str,
    attestation_hash: Option<String>,
    tx_hash: Option<String>,
    status: ProofStatus,
) -> TradeRecord {
    record.job_id = Some(job_id.to_string());
    record.attestation_hash = attestation_hash;
    record.tx_hash = tx_hash;
    record.proof_status = status;
    record
}
