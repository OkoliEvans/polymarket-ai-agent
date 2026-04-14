// crates/dashboard/src/main.rs
//! Terminal dashboard — live trade + proof feed.
//! Reads the JSONL records file written by the agent and renders a live view.
//!
//! Usage:
//!   RECORDS_PATH=records/trades_20240405_120000.jsonl cargo run -p dashboard

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    time::Duration,
};

const MAX_DISPLAY: usize = 20;
const REFRESH_MS:  u64   = 2_000;

#[derive(Debug, Clone, Deserialize)]
struct TradeRecord {
    timestamp:        DateTime<Utc>,
    market_id:        String,
    question:         String,
    decision:         String,
    confidence:       f32,
    side:             Option<String>,
    size_usdc:        Option<f64>,
    price:            Option<f64>,
    paper:            bool,
    bet_id:           Option<String>,
    job_id:           Option<String>,
    attestation_hash: Option<String>,
    tx_hash:          Option<String>,
    proof_status:     String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    let records_path = std::env::var("RECORDS_PATH")
        .unwrap_or_else(|_| find_latest_records().unwrap_or_else(|| "records/trades.jsonl".into()));

    println!("\x1b[2J\x1b[H"); // clear screen
    println!("Polymarket Veil Agent — Live Dashboard");
    println!("Records: {records_path}");
    println!("{}", "─".repeat(80));

    let mut file     = File::open(&records_path)
        .map_err(|e| anyhow::anyhow!("cannot open records file '{records_path}': {e}\nIs the agent running?"))?;
    let mut position = 0u64;
    let mut records: VecDeque<TradeRecord> = VecDeque::new();

    loop {
        // Tail the file — read any new lines since last check
        file.seek(SeekFrom::Start(position))?;
        let mut reader = BufReader::new(&file);
        let mut new_records = 0;

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    position += line.len() as u64;
                    if let Ok(r) = serde_json::from_str::<TradeRecord>(line.trim()) {
                        records.push_front(r);
                        new_records += 1;
                    }
                }
                Err(_) => break,
            }
        }

        // Keep only the most recent MAX_DISPLAY records
        while records.len() > MAX_DISPLAY {
            records.pop_back();
        }

        // Render
        print!("\x1b[H"); // move cursor to top
        println!("Polymarket Veil Agent — Live Dashboard         [{} UTC]",
            Utc::now().format("%H:%M:%S"));
        println!("Records: {records_path}  ({} trades)", records.len());
        println!("{}", "─".repeat(100));

        // Header
        println!(
            "{:<8} {:<14} {:<6} {:<6} {:<10} {:<10} {:<12} {:<10}",
            "Time", "Market", "Side", "Conf%", "Size", "Price", "Proof", "TxHash"
        );
        println!("{}", "─".repeat(100));

        for r in &records {
            let time     = r.timestamp.format("%H:%M:%S").to_string();
            let market   = r.market_id.chars().take(12).collect::<String>();
            let side     = r.side.as_deref().unwrap_or("SKIP").to_string();
            let conf     = format!("{:.0}%", r.confidence * 100.0);
            let size     = r.size_usdc.map(|s| format!("${s:.2}")).unwrap_or_else(|| "—".into());
            let price    = r.price.map(|p| format!("{p:.3}")).unwrap_or_else(|| "—".into());

            let proof_status = proof_status_display(&r.proof_status);
            let tx      = r.tx_hash.as_deref()
                .map(|h| format!("{}…", &h[..std::cmp::min(8, h.len())]))
                .unwrap_or_else(|| "pending".into());

            let paper_flag = if r.paper { " [P]" } else { "" };

            println!(
                "{:<8} {:<14} {:<6} {:<6} {:<10} {:<10} {:<12} {:<10}{}",
                time, market, side, conf, size, price, proof_status, tx, paper_flag
            );

            // Show attestation hash if available and not yet settled
            if let Some(ref h) = r.attestation_hash {
                if r.tx_hash.is_none() {
                    println!(
                        "         attestation: {}…",
                        &h[..std::cmp::min(18, h.len())]
                    );
                }
            }
        }

        if records.is_empty() {
            println!("  Waiting for agent to produce trade records…");
        }

        println!("{}", "─".repeat(100));
        println!(
            "  {} actionable  |  {} skipped  |  {} attested  |  {} settled",
            records.iter().filter(|r| r.side.is_some()).count(),
            records.iter().filter(|r| r.side.is_none()).count(),
            records.iter().filter(|r| r.proof_status == "attested").count(),
            records.iter().filter(|r| r.proof_status == "settled").count(),
        );
        if new_records > 0 {
            println!("  [{new_records} new records this refresh]");
        }

        tokio::time::sleep(Duration::from_millis(REFRESH_MS)).await;
    }
}

fn proof_status_display(status: &str) -> String {
    match status {
        "pending"  => "\x1b[33mpending\x1b[0m".into(),   // yellow
        "attested" => "\x1b[36mattested\x1b[0m".into(),  // cyan
        "settled"  => "\x1b[32msettled\x1b[0m".into(),   // green
        "failed"   => "\x1b[31mfailed\x1b[0m".into(),    // red
        "skipped"  => "\x1b[90mskipped\x1b[0m".into(),   // grey
        other      => other.into(),
    }
}

fn find_latest_records() -> Option<String> {
    let dir = std::fs::read_dir("records").ok()?;
    let mut files: Vec<_> = dir
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "jsonl"))
        .collect();
    files.sort_by_key(|e| e.file_name());
    files.last().map(|e| e.path().to_string_lossy().into_owned())
}