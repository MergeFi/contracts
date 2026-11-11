CONTRACTS := mergefi-escrow mergefi-milestones mergefi-maintenance-pool
WASM_TARGET := wasm32v1-none
WASM_DIR := target/$(WASM_TARGET)/release
NETWORK ?= testnet
SOURCE_ACCOUNT ?= mergefi-admin
ADMIN ?=
ORACLE ?=
TREASURY ?=
FEE_BPS ?=

.PHONY: build build-logs bindings test test-verbose bench fmt clean deploy-escrow deploy-milestones deploy-maintenance-pool deploy

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
	@command -v stellar >/dev/null 2>&1 || { echo "stellar-cli required for WASM optimization; install via: cargo install --locked stellar-cli"; exit 1; }
	@for c in $(CONTRACTS); do \
		stellar contract optimize --wasm $(WASM_DIR)/$$(echo $$c | tr - _).wasm; \
	done

## Build all contracts with the release-with-logs profile (release optimizations
## with debug-assertions enabled, useful for contract debugging and diagnostic logs).
build-logs:
	@if rustup target list --installed | grep -q wasm32v1-none; then \
		cargo build --target $(WASM_TARGET) --profile release-with-logs $(patsubst %,-p %,$(CONTRACTS)); \
	else \
		echo "wasm32v1-none not installed; run: rustup target add wasm32v1-none"; \
		exit 1; \
	fi

## Generate TypeScript bindings for all three contracts under bindings/.
bindings: build
	@for c in $(CONTRACTS); do \
		name=$$(echo $$c | sed 's/^mergefi-//'); \
		stellar contract bindings typescript \
			--wasm $(WASM_DIR)/$$(echo $$c | tr - _).wasm \
			--output-dir bindings/mergefi-$$name \
			--overwrite || exit $$?; \
	done

## Run the full workspace test suite on the native target (soroban-sdk
## supports native test execution via testutils, no wasm target required).
test:
	cargo test --workspace

test-verbose:
	cargo test --workspace -- --nocapture

## Run Soroban CPU instruction and memory byte resource-budget benchmarks across
## all contracts and print the baseline/scaling tables.
bench:
	cargo test --workspace bench_ -- --nocapture --test-threads=1

fmt:
	cargo fmt --all

clean:
	cargo clean

## Example deploy targets. Requires `stellar` (formerly `soroban`) CLI and a
## funded identity named $(SOURCE_ACCOUNT) (see: stellar keys generate).
## Supply ADMIN, ORACLE, TREASURY, and FEE_BPS to deploy and initialize each
## contract. Example: make deploy-escrow ADMIN=G... ORACLE=G... TREASURY=G... FEE_BPS=250
deploy-escrow: build
	@test -n "$(ADMIN)" -a -n "$(ORACLE)" -a -n "$(TREASURY)" -a -n "$(FEE_BPS)" || { echo "Set ADMIN, ORACLE, TREASURY, and FEE_BPS"; exit 1; }
	@set -e; \
	contract_id=$$(stellar contract deploy --wasm $(WASM_DIR)/mergefi_escrow.wasm --source $(SOURCE_ACCOUNT) --network $(NETWORK)); \
	echo "Initializing escrow at $$contract_id"; \
	stellar contract invoke --id "$$contract_id" --source $(SOURCE_ACCOUNT) --network $(NETWORK) -- initialize \
		--admin "$(ADMIN)" --oracle "$(ORACLE)" --treasury "$(TREASURY)" --fee_bps "$(FEE_BPS)" \
		--max_sponsors none --recovery none; \
	echo "Escrow initialized at $$contract_id"

deploy-milestones: build
	@test -n "$(ADMIN)" -a -n "$(ORACLE)" -a -n "$(TREASURY)" -a -n "$(FEE_BPS)" || { echo "Set ADMIN, ORACLE, TREASURY, and FEE_BPS"; exit 1; }
	@set -e; \
	contract_id=$$(stellar contract deploy --wasm $(WASM_DIR)/mergefi_milestones.wasm --source $(SOURCE_ACCOUNT) --network $(NETWORK)); \
	echo "Initializing milestones at $$contract_id"; \
	stellar contract invoke --id "$$contract_id" --source $(SOURCE_ACCOUNT) --network $(NETWORK) -- initialize \
		--admin "$(ADMIN)" --oracle "$(ORACLE)" --treasury "$(TREASURY)" --fee_bps "$(FEE_BPS)" \
		--max_sponsors none --recovery none; \
	echo "Milestones initialized at $$contract_id"

deploy-maintenance-pool: build
	@test -n "$(ADMIN)" -a -n "$(ORACLE)" -a -n "$(TREASURY)" -a -n "$(FEE_BPS)" || { echo "Set ADMIN, ORACLE, TREASURY, and FEE_BPS"; exit 1; }
	@set -e; \
	contract_id=$$(stellar contract deploy --wasm $(WASM_DIR)/mergefi_maintenance_pool.wasm --source $(SOURCE_ACCOUNT) --network $(NETWORK)); \
	echo "Initializing maintenance pool at $$contract_id"; \
	stellar contract invoke --id "$$contract_id" --source $(SOURCE_ACCOUNT) --network $(NETWORK) -- initialize \
		--admin "$(ADMIN)" --oracle "$(ORACLE)" --treasury "$(TREASURY)" --fee_bps "$(FEE_BPS)" \
		--recovery none; \
	echo "Maintenance pool initialized at $$contract_id"

deploy: deploy-escrow deploy-milestones deploy-maintenance-pool
