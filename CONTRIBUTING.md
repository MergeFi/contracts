# Contributing to MergeFi Contracts

Thank you for contributing to MergeFi smart contracts!

## Development Workflow

### Prerequisites

- Rust `1.95.0` (pinned via `rust-toolchain.toml`)
- `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`
- Node.js `24+` for scripts

### Building and Testing

Run tests and verification before opening a pull request:

```bash
# Build WASM contracts
make build

# Run full workspace test suite
make test

# Format and lint checks
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Pull Request Guidelines

1. Link relevant issue(s) using closing keywords (e.g. `Closes #123`).
2. Include comprehensive unit/integration test coverage for any new features or bug fixes.
3. Ensure `cargo fmt` and `cargo clippy` pass cleanly with no warnings.
