# Replay & Nonce Safety Analysis

## Soroban Authorization Framework Overview

Soroban uses `SorobanAuthorizationEntry` to bind signed authorizations to specific invocations. Each authorization entry binds to:

1. **Contract Address** — the exact contract being called
2. **Network Passphrase** — identifies the target network (e.g., "testnet", "mainnet")
3. **Function Name** — the specific function being invoked
4. **Arguments** — the exact arguments passed to the function
5. **Nonce** — a unique value per authorization entry
6. **Expiration Ledger** — the ledger after which the authorization is invalid

For classic (non-contract) addresses like the admin `GBUXAD...` keypair, Soroban's built-in `Address::require_auth()` enforces this binding automatically through the transaction's authorization list.

## Replay Scenario Analysis

### 1. Cross-Function Within Same Admin (e.g., `release` replayed as `refund`)

**Safe.** The authorization entry includes the function name. A signature authorizing `release` on `mergefi-escrow` cannot be used for `refund` on the same contract, because the function name differs in the authorization entry's invocation details.

### 2. Cross-Contract-Address After Redeploy (e.g., Upgrade to New Address)

**Safe.** The authorization entry binds to the exact contract address. If `mergefi-escrow` is redeployed to a new address, old authorizations for the previous address are invalid against the new address. The admin must sign new authorizations for the new contract.

### 3. Cross-Network with Key Reuse (e.g., Testnet Signature on Mainnet)

**Safe at the protocol level.** The authorization entry binds to the network passphrase. A signature valid on testnet cannot be replayed on mainnet because the network passphrase differs.

### 4. Same Contract, Same Function, Same Args (Replay Within Same Ledger)

**Safe.** Each authorization entry includes a nonce that must be unique. The Soroban host tracks consumed nonces and rejects duplicates.

## What Soroban Guarantees For Free

- **Contract address binding** prevents cross-contract replay
- **Network passphrase binding** prevents cross-network replay
- **Function name binding** prevents cross-function replay
- **Nonce uniqueness** prevents same-entry replay
- **Expiration ledger** limits the time window for authorization validity

## What Soroban Does NOT Prevent (Operational Risks)

1. **Key Compromise**: If the admin private key is compromised on any network, the attacker can sign new authorizations for any contract on that network. Soroban's auth framework cannot prevent this — it only binds *existing* signatures, not key possession.

2. **Key Reuse Across Environments**: While protocol-level replay is prevented by network passphrase binding, reusing the same keypair across testnet and mainnet is still poor practice:
   - A testnet compromise exposes the mainnet key
   - Key rotation on one network doesn't affect the other
   - Operational confusion between environments is more likely

3. **Front-Running of `initialize`**: The current `initialize` pattern (separate from deployment) allows front-running if an attacker submits their own `initialize` call before the legitimate deployer. This requires a Soroban constructor (atomic deploy+init) to fix, which is outside the current contract architecture.

## Operational Recommendations

1. **Separate Keypairs Per Environment**: Use different admin keypairs for testnet, mainnet, and any staging environments. This limits the blast radius of any single key compromise.

2. **Key Rotation Policy**: Implement periodic admin key rotation. The `set_admin` function in milestones and maintenance-pool contracts supports this. For escrow, add a similar mechanism or rotate via redeployment.

3. **Recovery Address**: Use the recovery address feature (available in milestones and maintenance-pool) as a backup for admin key loss.

4. **Multi-Sig Consideration**: For high-value deployments, consider using a multi-signature setup or contract-based authorization (Soroban's `__check_auth` pattern) instead of a single admin keypair.

5. **Monitoring**: Monitor admin-signed transactions for unexpected patterns (e.g., unusual timing, large amounts, new recipients).

## Test Suite Limitations

The existing test suite uses `env.mock_all_auths()` which bypasses real authorization entry construction. To empirically test replay scenarios:

- **What CAN be tested**: Cross-function replay, double-execution of the same function with same args
- **What CANNOT be tested in `testutils::Env`**: True cross-network replay (requires different network passphrases), multi-deployment address binding (requires deploying to different addresses)

For cross-network scenarios, the analysis must rely on the Soroban protocol specification rather than empirical testing.
