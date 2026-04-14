// crates/trainer/src/train.rs
//! MLP training loop using candle.
//!
//! Architecture: 4 → 8 → 2  (ReLU hidden, cross-entropy loss)
//!
//! Training is deliberately simple — this is a demo model for a hackathon
//! reference implementation, not a production quant model. The goal is a
//! weights.bin that produces reasonable predictions so the Veil proof story
//! holds. Accuracy > 55% on held-out data is sufficient.
//!
//! ## Train / val split
//!
//! This module no longer performs its own split. Callers must supply separate
//! `train_markets` and `val_markets` slices (see `data::fetch_and_split`).
//! This enforces a time-based split at the call site rather than a random one
//! here, which was previously causing data leakage.

use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear, loss, ops, AdamW, Linear, Module, Optimizer, VarBuilder, VarMap};
use common::{extract_features, extract_label, RawMarket};
use model::{HIDDEN_SIZE, INPUT_SIZE, OUTPUT_SIZE, WEIGHTS_LEN};
use tracing::{info, warn};

/// Hyperparameters. Intentionally conservative — small dataset, no overfitting.
///
/// Note: `val_split` has been removed. Splitting is the caller's responsibility.
/// Pass separate `train_markets` and `val_markets` to `train()`.
pub struct TrainConfig {
    pub epochs: usize,
    pub learning_rate: f64,
    pub batch_size: usize,
}

impl Default for TrainConfig {
    fn default() -> Self {
        Self {
            epochs: 300,
            learning_rate: 1e-3,
            batch_size: 32,
        }
    }
}

/// Trained weights as raw bytes (flat f32 LE), ready to write to weights.bin.
pub struct TrainedWeights {
    pub bytes: Vec<u8>,
    /// Accuracy on the training set. A large gap vs val_accuracy signals overfitting.
    pub train_accuracy: f32,
    /// Accuracy on the held-out validation set.
    pub val_accuracy: f32,
}

/// Train the MLP on pre-split market data and return the serialized weights.
///
/// `train_markets` and `val_markets` must be supplied by the caller — typically
/// via `data::fetch_and_split`. No shuffling or splitting is performed here.
pub fn train(
    train_markets: &[RawMarket],
    val_markets: &[RawMarket],
    cfg: &TrainConfig,
) -> Result<TrainedWeights> {
    let device = Device::Cpu;

    // ── Build training dataset ────────────────────────────────────────────────
    let (train_feat, train_lab) = extract_dataset(train_markets, "train")?;
    let (val_feat, val_lab) = extract_dataset(val_markets, "val")?;

    let train_n = train_feat.len();
    let val_n = val_feat.len();

    if train_n < 10 {
        return Err(anyhow!(
            "only {train_n} usable training samples — need at least 10. \
             Fetch more markets or lower TRAIN_RATIO."
        ));
    }
    if val_n < 5 {
        return Err(anyhow!(
            "only {val_n} usable validation samples — need at least 5. \
             Increase MAX_MARKETS or lower TRAIN_RATIO."
        ));
    }

    info!(train_n, val_n, "samples ready for training");

    let x_train = to_tensor_f32(&train_feat, &device)?;
    let y_train = to_tensor_u32_argmax(&train_lab, &device)?;
    let x_val = to_tensor_f32(&val_feat, &device)?;
    let y_val = to_tensor_u32_argmax(&val_lab, &device)?;

    // ── Build model ───────────────────────────────────────────────────────────
    let varmap = VarMap::new();
    let vb = VarBuilder::from_varmap(&varmap, DType::F32, &device);
    let layer1 = linear(INPUT_SIZE, HIDDEN_SIZE, vb.pp("layer1"))?;
    let layer2 = linear(HIDDEN_SIZE, OUTPUT_SIZE, vb.pp("layer2"))?;

    let mut opt = AdamW::new(
        varmap.all_vars(),
        candle_nn::ParamsAdamW {
            lr: cfg.learning_rate,
            ..Default::default()
        },
    )?;

    // ── Training loop ─────────────────────────────────────────────────────────
    let n_batches = (train_n + cfg.batch_size - 1) / cfg.batch_size;

    for epoch in 0..cfg.epochs {
        let mut epoch_loss = 0.0f32;

        for batch_idx in 0..n_batches {
            let start = batch_idx * cfg.batch_size;
            let end = (start + cfg.batch_size).min(train_n);

            let xb = x_train.narrow(0, start, end - start)?;
            let yb = y_train.narrow(0, start, end - start)?;

            let logits = forward(&layer1, &layer2, &xb)?;
            let batch_loss = loss::cross_entropy(&logits, &yb)?;

            opt.backward_step(&batch_loss)?;
            epoch_loss += batch_loss.to_scalar::<f32>()?;
        }

        if (epoch + 1) % 50 == 0 {
            let val_logits = forward(&layer1, &layer2, &x_val)?;
            let val_acc = accuracy(&val_logits, &y_val)?;
            info!(
                "epoch {:>3}/{} — loss: {:.4} — val_acc: {:.1}%",
                epoch + 1,
                cfg.epochs,
                epoch_loss / n_batches as f32,
                val_acc * 100.0
            );
        }
    }

    // ── Final metrics ─────────────────────────────────────────────────────────
    let train_logits = forward(&layer1, &layer2, &x_train)?;
    let train_accuracy = accuracy(&train_logits, &y_train)?;

    let val_logits = forward(&layer1, &layer2, &x_val)?;
    let val_accuracy = accuracy(&val_logits, &y_val)?;

    info!(
        "training complete — train_acc: {:.1}%  val_acc: {:.1}%",
        train_accuracy * 100.0,
        val_accuracy * 100.0
    );

    // ── Serialize weights ─────────────────────────────────────────────────────
    // Order must match model::Mlp::from_bytes():
    //   W1 [HIDDEN × INPUT], b1 [HIDDEN], W2 [OUTPUT × HIDDEN], b2 [OUTPUT]
    let data = varmap.data().lock().unwrap();

    let w1 = data
        .get("layer1.weight")
        .ok_or_else(|| anyhow!("layer1.weight not found in varmap"))?
        .clone();
    let b1 = data
        .get("layer1.bias")
        .ok_or_else(|| anyhow!("layer1.bias not found in varmap"))?
        .clone();
    let w2 = data
        .get("layer2.weight")
        .ok_or_else(|| anyhow!("layer2.weight not found in varmap"))?
        .clone();
    let b2 = data
        .get("layer2.bias")
        .ok_or_else(|| anyhow!("layer2.bias not found in varmap"))?
        .clone();

    drop(data); // release the lock before tensor ops

    let mut all: Vec<f32> = Vec::with_capacity(WEIGHTS_LEN);
    all.extend(tensor_to_f32_vec(w1.as_tensor())?);
    all.extend(tensor_to_f32_vec(b1.as_tensor())?);
    all.extend(tensor_to_f32_vec(w2.as_tensor())?);
    all.extend(tensor_to_f32_vec(b2.as_tensor())?);

    if all.len() != WEIGHTS_LEN {
        return Err(anyhow!(
            "weight count mismatch: expected {WEIGHTS_LEN}, got {} — \
             check that INPUT_SIZE/HIDDEN_SIZE/OUTPUT_SIZE constants match the architecture",
            all.len()
        ));
    }

    let bytes: Vec<u8> = all.iter().flat_map(|f| f.to_le_bytes()).collect();

    Ok(TrainedWeights {
        bytes,
        train_accuracy,
        val_accuracy,
    })
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Extract feature vectors and labels from a market slice, logging skips.
fn extract_dataset(
    markets: &[RawMarket],
    label: &str,
) -> Result<(Vec<[f64; 4]>, Vec<[f64; 2]>)> {
    let mut features: Vec<[f64; 4]> = Vec::with_capacity(markets.len());
    let mut labels: Vec<[f64; 2]> = Vec::with_capacity(markets.len());

    for market in markets {
        match (extract_features(market, true), extract_label(market)) {
            (Ok(f), Ok(l)) => {
                features.push(f.values);
                labels.push(l);
            }
            (Err(e), _) | (_, Err(e)) => {
                warn!("[{label}] skipping market {}: {e}", market.market_id);
            }
        }
    }

    Ok((features, labels))
}

fn forward(l1: &Linear, l2: &Linear, x: &Tensor) -> Result<Tensor> {
    let h = l1.forward(x)?.relu()?;
    Ok(l2.forward(&h)?)
}

fn to_tensor_f32(data: &[[f64; 4]], device: &Device) -> Result<Tensor> {
    let flat: Vec<f32> = data
        .iter()
        .flat_map(|row| row.iter().map(|&v| v as f32))
        .collect();
    Ok(Tensor::from_vec(flat, (data.len(), INPUT_SIZE), device)?)
}

fn to_tensor_u32_argmax(labels: &[[f64; 2]], device: &Device) -> Result<Tensor> {
    let indices: Vec<u32> = labels
        .iter()
        .map(|l| if l[0] > l[1] { 0u32 } else { 1u32 })
        .collect();
    Ok(Tensor::from_vec(indices, (labels.len(),), device)?)
}

fn accuracy(logits: &Tensor, targets: &Tensor) -> Result<f32> {
    let preds = ops::softmax(logits, 1)?.argmax(1)?;
    let correct = preds.eq(targets)?.to_dtype(DType::F32)?.mean_all()?;
    Ok(correct.to_scalar::<f32>()?)
}

fn tensor_to_f32_vec(t: &Tensor) -> Result<Vec<f32>> {
    Ok(t.flatten_all()?.to_vec1::<f32>()?)
}