# Contributing

## Toolchain

Install Rust with `rustup`. The repository pins the compiler version and
components in [`rust-toolchain.toml`](./rust-toolchain.toml); installing the
toolchain from that file also installs the `wasm32v1-none` target.

The optimized contract build additionally requires `stellar-cli`:

```sh
cargo install --locked stellar-cli
```

## Build, test, format, and lint

Run these commands from the repository root:

```sh
make build
make test
make fmt
make lint
make check
```

`make build` compiles and optimizes the contracts for Soroban. `make test` runs
the workspace's native Rust tests. `make fmt` formats all workspace crates in-place.
`make lint` runs all style and static checks (`fmt --check`, clippy, dead-code detection, and documentation generation).
`make check` runs both `lint` and `test` to reproduce the full CI gate locally.

## Tests

Add or update tests for changed behavior, including failure cases where relevant.
For authorization-sensitive tests, do not use `mock_all_auths()`: configure
specific mocked authorizations or exercise the call without mocked
authorization so the test verifies the access-control requirement.

## Pull requests

- Explain the behavior changed and link every related issue. Use a separate
  `Closes #<number>` line for each issue that the pull request resolves.
- Include tests for behavior changes and report the exact validation commands
  and results. If validation was not run, say so explicitly.
- Run formatting before submitting; include build or test failures that remain.
- Keep changes focused, document any user-visible contract or event changes,
  and avoid committing generated build artifacts or secrets.
