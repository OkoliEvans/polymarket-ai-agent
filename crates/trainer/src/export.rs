// crates/trainer/src/export.rs
//! Serialize trained weights to weights.bin.
//!
//! Output format: flat f32 little-endian, 58 values = 232 bytes.
//! This is the exact format model::Mlp::from_bytes() and the inference-guest expect.

use anyhow::{Result, anyhow};
use model::WEIGHTS_LEN;
use std::path::Path;
use tracing::info;

/// Write weight bytes to `path`.
///
/// Creates parent directories if they don't exist.
/// Overwrites any existing file at `path`.
pub fn export_weights(bytes: &[u8], path: &str) -> Result<()> {
    let expected = WEIGHTS_LEN * 4;
    if bytes.len() != expected {
        return Err(anyhow!(
            "export: expected {expected} bytes ({WEIGHTS_LEN} f32s), got {}",
            bytes.len()
        ));
    }

    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create output directory: {e}"))?;
    }

    std::fs::write(p, bytes).map_err(|e| anyhow!("failed to write weights to '{path}': {e}"))?;

    info!(
        path,
        size_bytes = bytes.len(),
        num_weights = WEIGHTS_LEN,
        "weights exported"
    );

    Ok(())
}

/// Verify a written weights file can be read back by model::Mlp.
pub fn verify_weights(path: &str) -> Result<()> {
    let bytes = std::fs::read(path).map_err(|e| anyhow!("verify: failed to read '{path}': {e}"))?;

    model::Mlp::from_bytes(&bytes).map_err(|e| anyhow!("verify: weights.bin is corrupt: {e}"))?;

    info!(
        path,
        "weights verified — model::Mlp::from_bytes() succeeded"
    );
    Ok(())
}
