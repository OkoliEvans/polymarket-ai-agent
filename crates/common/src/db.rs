// crates/common/src/db.rs
//
// Async diesel pool using bb8 + diesel-async.
// Build once at startup, pass as Arc<DbPool> to all handlers.

use diesel_async::{
    pooled_connection::{bb8::Pool, AsyncDieselConnectionManager},
    AsyncPgConnection,
};

pub type DbPool = Pool<AsyncPgConnection>;

/// Build an async Postgres connection pool.
/// Panics with a clear message if DATABASE_URL is unreachable.
pub async fn build_pool(database_url: &str) -> anyhow::Result<DbPool> {
    let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
    let pool = Pool::builder()
        .max_size(10)
        .build(config)
        .await
        .map_err(|e| anyhow::anyhow!("failed to build DB pool: {e}"))?;
    Ok(pool)
}
