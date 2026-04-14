// crates/common/src/lib.rs

pub mod db;
pub mod features;
pub mod models;
pub mod repo;
pub mod schema;

// Re-export the types trainer and agent use directly
// so they can write `common::RawMarket` instead of `common::features::RawMarket`
pub use features::{extract_features, extract_label, FeatureVector, RawMarket};
