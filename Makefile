.PHONY: dev gateway agent api ui snapshot migrate

# ── DB ────────────────────────────────────────────────────────────────────────
migrate:
	diesel migration run

# ── Rust services ─────────────────────────────────────────────────────────────
gateway:
	cargo run --bin dashboard

agent:
	cargo run --bin agent

api:
	cargo run --bin api

snapshot:
	cargo run --bin snapshot

trainer:
	cargo run --bin trainer

# ── UI ────────────────────────────────────────────────────────────────────────
ui-install:
	pnpm --filter ui install

ui:
	pnpm --filter ui dev

ui-build:
	pnpm --filter ui build

# ── Dev (all services) ────────────────────────────────────────────────────────
dev:
	$(MAKE) migrate
	@echo "Starting all services..."
	cargo run --bin gateway &
	cargo run --bin api &
	pnpm --filter ui dev &
	cargo run --bin agent