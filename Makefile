CONTRACTS := mergefi-escrow mergefi-milestones mergefi-maintenance-pool
WASM_TARGET := wasm32v1-none
WASM_DIR := target/$(WASM_TARGET)/release
NETWORK ?= testnet
SOURCE_ACCOUNT ?= mergefi-admin

.PHONY: build test test-verbose fmt clean deploy-escrow deploy-milestones deploy-maintenance-pool deploy

## Build all contracts to optimized wasm (wasm32v1-none, the target Soroban's
## host currently requires for Rust 1.84+; falls back instructions below if
## you're on an older toolchain that only has wasm32-unknown-unknown).
build:
	@if rustup target list --installed | grep -q wasm32v1-none; then \
		cargo build --target $(WASM_TARGET) --release $(patsubst %,-p %,$(CONTRACTS)); \
	else \
		echo "wasm32v1-none not installed; run: rustup target add wasm32v1-none"; \
		exit 1; \
	fi
	@command -v stellar >/dev/null 2>&1 && \
		for c in $(CONTRACTS); do \
			stellar contract optimize --wasm $(WASM_DIR)/$$(echo $$c | tr - _).wasm || true; \
		done || echo "stellar-cli not found; skipping wasm optimize step (optional)"

## Run the full workspace test suite on the native target (soroban-sdk
## supports native test execution via testutils, no wasm target required).
test:
	cargo test --workspace

test-verbose:
	cargo test --workspace -- --nocapture

fmt:
	cargo fmt --all

clean:
	cargo clean

## Example deploy targets. Requires `stellar` (formerly `soroban`) CLI and a
## funded identity named $(SOURCE_ACCOUNT) (see: stellar keys generate).
## Usage: make deploy-escrow NETWORK=testnet SOURCE_ACCOUNT=mergefi-admin
deploy-escrow: build
	stellar contract deploy \
		--wasm $(WASM_DIR)/mergefi_escrow.wasm \
		--source $(SOURCE_ACCOUNT) \
		--network $(NETWORK)

deploy-milestones: build
	stellar contract deploy \
		--wasm $(WASM_DIR)/mergefi_milestones.wasm \
		--source $(SOURCE_ACCOUNT) \
		--network $(NETWORK)

deploy-maintenance-pool: build
	stellar contract deploy \
		--wasm $(WASM_DIR)/mergefi_maintenance_pool.wasm \
		--source $(SOURCE_ACCOUNT) \
		--network $(NETWORK)

deploy: deploy-escrow deploy-milestones deploy-maintenance-pool
