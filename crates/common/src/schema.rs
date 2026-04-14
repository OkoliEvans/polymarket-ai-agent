// @generated automatically by Diesel CLI.

diesel::table! {
    batches (id) {
        id -> Uuid,
        status -> Text,
        job_count -> Int4,
        aggregated_proof_path -> Nullable<Text>,
        tx_hash -> Nullable<Text>,
        gas_used -> Nullable<Int8>,
        created_at -> Timestamptz,
        aggregated_at -> Nullable<Timestamptz>,
        settled_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    bets (id) {
        id -> Uuid,
        job_id -> Nullable<Uuid>,
        market_id -> Text,
        question -> Text,
        side -> Text,
        size_usdc -> Float8,
        price -> Float8,
        paper -> Bool,
        confidence -> Float8,
        yes_price -> Float8,
        no_price -> Float8,
        volume_24h -> Float8,
        attestation_hash -> Nullable<Text>,
        tx_hash -> Nullable<Text>,
        outcome -> Nullable<Bool>,
        pnl_usdc -> Nullable<Float8>,
        placed_at -> Timestamptz,
        resolved_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    jobs (id) {
        id -> Uuid,
        model_id -> Uuid,
        status -> Text,
        input_hash -> Text,
        proof_path -> Nullable<Text>,
        error -> Nullable<Text>,
        submitted_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        completed_at -> Nullable<Timestamptz>,
        settled_at -> Nullable<Timestamptz>,
        tx_hash -> Nullable<Text>,
        batch_id -> Nullable<Uuid>,
        attestation_hash -> Nullable<Text>,
        proof_bytes -> Nullable<Bytea>,
    }
}

diesel::table! {
    market_snapshots (id) {
        id -> Uuid,
        market_id -> Text,
        question -> Text,
        yes_price -> Float8,
        no_price -> Float8,
        volume_24h -> Float8,
        end_date -> Timestamptz,
        captured_at -> Timestamptz,
    }
}

diesel::table! {
    models (id) {
        id -> Uuid,
        name -> Text,
        version -> Text,
        ipfs_cid -> Text,
        input_shape -> Jsonb,
        on_chain_hash -> Text,
        registered_at -> Timestamptz,
    }
}

diesel::table! {
    outcomes (id) {
        id -> Uuid,
        market_id -> Text,
        question -> Text,
        resolved_at -> Timestamptz,
        outcome -> Bool,
    }
}

diesel::table! {
    training_samples (id) {
        id -> Uuid,
        snapshot_id -> Uuid,
        outcome_id -> Uuid,
        market_id -> Text,
        yes_price -> Float8,
        no_price -> Float8,
        volume_24h -> Float8,
        time_to_expiry -> Float8,
        outcome -> Bool,
        created_at -> Timestamptz,
    }
}

diesel::joinable!(bets -> jobs (job_id));
diesel::joinable!(jobs -> batches (batch_id));
diesel::joinable!(jobs -> models (model_id));
diesel::joinable!(training_samples -> market_snapshots (snapshot_id));
diesel::joinable!(training_samples -> outcomes (outcome_id));

diesel::allow_tables_to_appear_in_same_query!(
    batches,
    bets,
    jobs,
    market_snapshots,
    models,
    outcomes,
    training_samples,
);
