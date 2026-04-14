//! Lightweight read-only API for the Veil UI.
//!
//! Endpoints:
//!   GET /jobs?limit=N&status=settled   — proof jobs from Mugen gateway
//!   GET /jobs/:id                      — single job
//!   GET /bets?limit=N&paper=true       — Polymarket bets from the agent
//!   GET /bets/:id                      — single bet
//!   GET /events                        — SSE stream for live UI updates
//!   GET /healthz                       — liveness

use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{
    get, middleware,
    web::{self, Bytes, Data, Path, Query},
    App, HttpResponse, HttpServer, Responder,
};
use common::db::DbPool;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct JobsQuery {
    limit:  Option<i64>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct BetsQuery {
    limit: Option<i64>,
    paper: Option<bool>,
}

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    pool: Arc<DbPool>,
    tx:   broadcast::Sender<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

#[get("/healthz")]
async fn healthz() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}

#[get("/jobs")]
async fn list_jobs(state: Data<AppState>, q: Query<JobsQuery>) -> impl Responder {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let result = match &q.status {
        Some(s) => common::repo::list_jobs_by_status(&state.pool, s, limit).await,
        None    => common::repo::list_jobs(&state.pool, limit).await,
    };
    match result {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            tracing::error!("list_jobs failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }))
        }
    }
}

#[get("/jobs/{id}")]
async fn get_job(state: Data<AppState>, path: Path<String>) -> impl Responder {
    let id = match path.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": "invalid uuid" })),
    };
    match common::repo::get_job(&state.pool, id).await {
        Ok(job)  => HttpResponse::Ok().json(job),
        Err(e) if e.to_string().contains("not found") => {
            HttpResponse::NotFound().json(serde_json::json!({ "error": "job not found" }))
        }
        Err(e) => {
            tracing::error!("get_job failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }))
        }
    }
}

#[get("/bets")]
async fn list_bets(state: Data<AppState>, q: Query<BetsQuery>) -> impl Responder {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let result = common::repo::list_bets(&state.pool, limit).await;
    match result {
        Ok(mut rows) => {
            if let Some(paper) = q.paper {
                rows.retain(|b| b.paper == paper);
            }
            HttpResponse::Ok().json(rows)
        }
        Err(e) => {
            tracing::error!("list_bets failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }))
        }
    }
}

#[get("/bets/{id}")]
async fn get_bet(state: Data<AppState>, path: Path<String>) -> impl Responder {
    let id = match path.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": "invalid uuid" })),
    };
    match common::repo::get_bet(&state.pool, id).await {
        Ok(bet)  => HttpResponse::Ok().json(bet),
        Err(e) if e.to_string().contains("not found") => {
            HttpResponse::NotFound().json(serde_json::json!({ "error": "bet not found" }))
        }
        Err(e) => {
            tracing::error!("get_bet failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }))
        }
    }
}

/// GET /events — Server-Sent Events stream for live UI updates.
/// The browser connects once; the server pushes whenever data changes.
#[get("/events")]
async fn sse_stream(state: Data<AppState>) -> impl Responder {
    let rx     = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).map(|msg| {
        let data = match msg {
            Ok(d)  => format!("data: {d}\n\n"),
            Err(_) => "data: {\"type\":\"ping\"}\n\n".to_string(),
        };
        Ok::<Bytes, actix_web::Error>(Bytes::from(data))
    });

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("Connection", "keep-alive"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(stream)
}

// ── Background watcher — polls DB every 3s and broadcasts diffs ───────────────

async fn start_watcher(pool: Arc<DbPool>, tx: broadcast::Sender<String>) {
    let mut last_bet_placed_at:  Option<chrono::DateTime<chrono::Utc>> = None;
    let mut last_job_submitted_at: Option<chrono::DateTime<chrono::Utc>> = None;

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // ── Check for new bets ────────────────────────────────────────────────
        match common::repo::list_bets(&pool, 1).await {
            Ok(bets) => {
                let latest = bets.first().map(|b| b.placed_at);
                if latest != last_bet_placed_at && latest.is_some() {
                    last_bet_placed_at = latest;
                    let payload = serde_json::json!({ "type": "bets" }).to_string();
                    let _ = tx.send(payload);
                    tracing::debug!("SSE: new bet detected");
                }
            }
            Err(e) => tracing::warn!("watcher: list_bets failed: {e}"),
        }

        // ── Check for new/updated jobs ────────────────────────────────────────
        match common::repo::list_jobs(&pool, 1).await {
            Ok(jobs) => {
                let latest = jobs.first().map(|j| j.submitted_at);
                if latest != last_job_submitted_at && latest.is_some() {
                    last_job_submitted_at = latest;
                    let payload = serde_json::json!({ "type": "jobs" }).to_string();
                    let _ = tx.send(payload);
                    tracing::debug!("SSE: new job detected");
                }
            }
            Err(e) => tracing::warn!("watcher: list_jobs failed: {e}"),
        }

        // ── Keepalive ping every ~30s ─────────────────────────────────────────
        // (handled by BroadcastStream Err arm — no explicit ping needed)
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = common::db::build_pool(&database_url)
        .await
        .expect("failed to build DB pool");

    tracing::info!("DB pool ready");

    let pool = Arc::new(pool);
    let (tx, _rx) = broadcast::channel::<String>(128);

    // Start background watcher
    tokio::spawn(start_watcher(Arc::clone(&pool), tx.clone()));

    let state = Data::new(AppState { pool, tx });

    let host = std::env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("API_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3001);

    tracing::info!("Veil API listening on {host}:{port}");

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(middleware::Logger::default())
            .wrap(Cors::permissive())
            .service(healthz)
            .service(list_jobs)
            .service(get_job)
            .service(list_bets)
            .service(get_bet)
            .service(sse_stream)
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}