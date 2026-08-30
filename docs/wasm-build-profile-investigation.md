# WASM Build Profile Investigation

## Current Profile

```toml
[profile.release]
opt-level = "z"           # optimize for size
overflow-checks = true    # arithmetic overflow checks enabled
debug = 0
strip = "symbols"         # strip debug symbols
debug-assertions = false
panic = "abort"           # no unwinding
codegen-units = 1         # single codegen unit (better optimization)
lto = true                # full link-time optimization
```

## Measured WASM Sizes

| Contract | Before Optimization | After `stellar contract optimize` | Reduction |
|----------|---------------------|-----------------------------------|-----------|
| mergefi-escrow | 42,874 bytes | 33,583 bytes | 21.7% |
| mergefi-maintenance-pool | 31,584 bytes | 23,930 bytes | 24.2% |
| mergefi-milestones | 47,418 bytes | 37,794 bytes | 20.3% |

## Key Findings

### 1. `overflow-checks = true` with `opt-level = "z"`

This combination is intentional defense-in-depth:
- `opt-level = "z"` minimizes code size
- `overflow-checks = true` adds runtime safety at the cost of ~2-5% size increase
- For a financial protocol, the safety benefit outweighs the small size cost
- The alternative (unchecked arithmetic with manual `checked_add`/`checked_mul` at specific sites) requires more code changes and risks missing sites

**Recommendation**: Keep `overflow-checks = true`. The size cost is minimal compared to the safety guarantee.

### 2. `stellar contract optimize` Impact

The `stellar contract optimize` step (using `soroban-sdk`'s optimizer) provides significant additional size reduction (~20-24%) beyond `rustc`'s own optimizations. This is because:
- It applies Soroban-specific WASM transformations
- It strips unnecessary sections from the WASM binary
- It applies additional compression

**Recommendation**: Make `stellar contract optimize` a required part of the build process, not optional.

### 3. Profile Settings Analysis

| Setting | Current | Impact | Recommendation |
|---------|---------|--------|----------------|
| `opt-level = "z"` | Yes | Best for size | Keep |
| `lto = true` | Yes | Better optimization, slower builds | Keep |
| `codegen-units = 1` | Yes | Better optimization, slower builds | Keep |
| `strip = "symbols"` | Yes | Reduces binary size | Keep |
| `panic = "abort"` | Yes | No unwinding overhead | Keep |
| `overflow-checks = true` | Yes | Runtime safety | Keep |

## Changes Made

1. **Makefile**: Made `stellar contract optimize` a required build step (previously optional/best-effort)
2. **Documentation**: This file records the investigation findings

## Cost Implications

For Soroban deployment and invocation costs:
- Smaller WASM = lower deployment cost
- `overflow-checks = true` adds ~2-5% compute overhead per arithmetic operation
- For a protocol deducting small percentage fees, this overhead is negligible compared to the safety benefit
