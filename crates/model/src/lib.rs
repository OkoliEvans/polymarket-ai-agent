// crates/model/src/lib.rs
//! MLP forward pass — identical architecture to the inference-guest.
//!
//! Architecture: 4 → 8 → 2 (ReLU hidden, linear output)
//!
//! The weights are stored as flat f32 little-endian in `weights.bin`.
//! Layout (must match guest exactly):
//!   W1: [8 × 4] = 32 values   (hidden layer weights)
//!   b1: [8]     =  8 values   (hidden layer bias)
//!   W2: [2 × 8] = 16 values   (output layer weights)
//!   b2: [2]     =  2 values   (output layer bias)
//!   Total: 58 values = 232 bytes

use anyhow::{Result, anyhow};

pub const INPUT_SIZE: usize = 4;
pub const HIDDEN_SIZE: usize = 8;
pub const OUTPUT_SIZE: usize = 2;
pub const WEIGHTS_LEN: usize = HIDDEN_SIZE * INPUT_SIZE   // W1
    + HIDDEN_SIZE              // b1
    + OUTPUT_SIZE * HIDDEN_SIZE // W2
    + OUTPUT_SIZE; // b2
// = 32 + 8 + 16 + 2 = 58

/// Trading decision produced by the model.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Buy YES with the given confidence in (0.5, 1.0].
    BuyYes { confidence: f32 },
    /// Buy NO with the given confidence in (0.5, 1.0].
    BuyNo { confidence: f32 },
    /// Confidence below threshold — skip this market.
    Skip { confidence: f32 },
}

impl Decision {
    pub fn is_actionable(&self) -> bool {
        !matches!(self, Decision::Skip { .. })
    }

    pub fn confidence(&self) -> f32 {
        match self {
            Decision::BuyYes { confidence } => *confidence,
            Decision::BuyNo { confidence } => *confidence,
            Decision::Skip { confidence } => *confidence,
        }
    }
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Decision::BuyYes { confidence } => write!(f, "BUY YES ({:.1}%)", confidence * 100.0),
            Decision::BuyNo { confidence } => write!(f, "BUY NO  ({:.1}%)", confidence * 100.0),
            Decision::Skip { confidence } => write!(f, "SKIP    ({:.1}%)", confidence * 100.0),
        }
    }
}

/// Weights loaded from `weights.bin`.
pub struct Mlp {
    w1: Vec<f32>, // [HIDDEN_SIZE × INPUT_SIZE]
    b1: Vec<f32>, // [HIDDEN_SIZE]
    w2: Vec<f32>, // [OUTPUT_SIZE × HIDDEN_SIZE]
    b2: Vec<f32>, // [OUTPUT_SIZE]
}

impl Mlp {
    /// Load weights from raw bytes (flat f32 LE).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let expected = WEIGHTS_LEN * 4;
        if bytes.len() != expected {
            return Err(anyhow!(
                "weights.bin: expected {} bytes ({} f32s), got {}",
                expected,
                WEIGHTS_LEN,
                bytes.len()
            ));
        }

        let weights: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();

        let mut offset = 0;

        let w1 = weights[offset..offset + HIDDEN_SIZE * INPUT_SIZE].to_vec();
        offset += HIDDEN_SIZE * INPUT_SIZE;

        let b1 = weights[offset..offset + HIDDEN_SIZE].to_vec();
        offset += HIDDEN_SIZE;

        let w2 = weights[offset..offset + OUTPUT_SIZE * HIDDEN_SIZE].to_vec();
        offset += OUTPUT_SIZE * HIDDEN_SIZE;

        let b2 = weights[offset..offset + OUTPUT_SIZE].to_vec();

        Ok(Self { w1, b1, w2, b2 })
    }

    /// Load weights from a file path.
    pub fn from_file(path: &str) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| anyhow!("failed to read weights from '{path}': {e}"))?;
        Self::from_bytes(&bytes)
    }

    /// Forward pass — returns raw logits [yes_logit, no_logit].
    ///
    /// Input: flat slice of length INPUT_SIZE.
    pub fn forward(&self, input: &[f32]) -> Result<[f32; OUTPUT_SIZE]> {
        if input.len() != INPUT_SIZE {
            return Err(anyhow!(
                "expected input of length {INPUT_SIZE}, got {}",
                input.len()
            ));
        }

        // Hidden layer: h = relu(W1 @ x + b1)
        let mut hidden = [0.0f32; HIDDEN_SIZE];
        for i in 0..HIDDEN_SIZE {
            let mut sum = self.b1[i];
            for j in 0..INPUT_SIZE {
                sum += self.w1[i * INPUT_SIZE + j] * input[j];
            }
            hidden[i] = sum.max(0.0); // ReLU
        }

        // Output layer: y = W2 @ h + b2  (linear — no activation)
        let mut output = [0.0f32; OUTPUT_SIZE];
        for i in 0..OUTPUT_SIZE {
            let mut sum = self.b2[i];
            for j in 0..HIDDEN_SIZE {
                sum += self.w2[i * HIDDEN_SIZE + j] * hidden[j];
            }
            output[i] = sum;
        }

        Ok(output)
    }

    /// Run inference and return a trading Decision.
    ///
    /// `min_confidence` — decisions below this threshold become Skip.
    /// Recommended: 0.60–0.70 for real trading.
    pub fn decide(&self, input: &[f32], min_confidence: f32) -> Result<Decision> {
        let logits = self.forward(input)?;
        let probs = softmax(logits);

        let yes_prob = probs[0];
        let no_prob = probs[1];

        if yes_prob >= no_prob && yes_prob >= min_confidence {
            Ok(Decision::BuyYes {
                confidence: yes_prob,
            })
        } else if no_prob > yes_prob && no_prob >= min_confidence {
            Ok(Decision::BuyNo {
                confidence: no_prob,
            })
        } else {
            let max_prob = yes_prob.max(no_prob);
            Ok(Decision::Skip {
                confidence: max_prob,
            })
        }
    }
}

/// Softmax over a fixed-size array.
fn softmax(logits: [f32; OUTPUT_SIZE]) -> [f32; OUTPUT_SIZE] {
    let max = logits[0].max(logits[1]); // numerical stability
    let exp0 = (logits[0] - max).exp();
    let exp1 = (logits[1] - max).exp();
    let sum = exp0 + exp1;
    [exp0 / sum, exp1 / sum]
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform_weights() -> Vec<u8> {
        let weights = vec![0.1f32; WEIGHTS_LEN];
        weights.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    #[test]
    fn load_and_forward() {
        let bytes = uniform_weights();
        let mlp = Mlp::from_bytes(&bytes).unwrap();
        let input = [0.5f32, 0.5, 0.5, 0.5];
        let out = mlp.forward(&input).unwrap();
        // With uniform weights the outputs should be equal
        assert!(
            (out[0] - out[1]).abs() < 1e-5,
            "outputs should be equal with uniform weights"
        );
    }

    #[test]
    fn wrong_weight_size_errors() {
        let bytes = vec![0u8; 100]; // wrong size
        assert!(Mlp::from_bytes(&bytes).is_err());
    }

    #[test]
    fn wrong_input_size_errors() {
        let bytes = uniform_weights();
        let mlp = Mlp::from_bytes(&bytes).unwrap();
        assert!(mlp.forward(&[0.5, 0.5]).is_err());
    }

    #[test]
    fn softmax_sums_to_one() {
        let s = softmax([1.0, 2.0]);
        assert!((s[0] + s[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decide_skip_when_low_confidence() {
        let bytes = uniform_weights();
        let mlp = Mlp::from_bytes(&bytes).unwrap();
        // Uniform weights + equal input → equal logits → 50/50 → below any threshold
        let d = mlp.decide(&[0.5, 0.5, 0.5, 0.5], 0.65).unwrap();
        assert!(matches!(d, Decision::Skip { .. }));
    }

    #[test]
    fn weights_len_constant() {
        assert_eq!(WEIGHTS_LEN, 58);
    }
}
