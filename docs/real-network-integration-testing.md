# Real-Network Integration Testing Design

This document details the architecture and step-by-step procedure for running integration tests against a live Stellar network (Testnet or Mainnet) for the MergeFi contract suite. This integration test suite is designed to be executed by the orchestration layer (`mergefi-backend`) or as a standalone CI/CD pipeline step.

---

## 1. Environment Variables Configuration

To interact with a live Stellar network, the test runner requires access to RPC endpoints, network passphrases, and signing keys. Define the following environment variables in the test runner's environment (e.g., a `.env.testnet` file):

```bash
# The network passphrase of the target Stellar network
# Testnet: "Test Stellar Public Network ; September 2015"
# Mainnet: "Public Global Stellar Network ; October 2005"
export STELLAR_NETWORK_PASSPHRASE="Test Stellar Public Network ; September 2015"

# The Horizon API URL for basic account queries
export STELLAR_HORIZON_URL="https://horizon-testnet.stellar.org"

# The Soroban RPC server endpoint
export STELLAR_RPC_URL="https://soroban-testnet.stellar.org"

# Secret keys for the test accounts (or seed phrases)
# The Admin key represents the mergefi-backend authorization key
export TEST_ADMIN_SECRET="SD..."
# Sponsor key represents the user funding escrows/milestones
export TEST_SPONSOR_SECRET="SC..."
# Recipient keys represent developers receiving payouts
export TEST_RECIPIENT_1_SECRET="SD..."
export TEST_RECIPIENT_2_SECRET="SD..."

# Token Asset to use (e.g., native XLM, or a custom SAC token address)
# If using native XLM on testnet, its contract ID is:
# CAS3J7GYCCKCTUI572THDT3W75567TCT76L7V2Z54K3KBC476HO4TY7T
export TEST_TOKEN_ADDRESS="CAS3J7GYCCKCTUI572THDT3W75567TCT76L7V2Z54K3KBC476HO4TY7T"
```

---

## 2. Test Account Provisioning & Pre-Funding

Before running the sequence, the test accounts must exist on-chain and hold sufficient balances.

### 2.1 On Testnet (Friendbot Automated Script)
On Stellar Testnet, the test runner can programmatically request Friendbot to create and fund the accounts with 10,000 XLM each:

```javascript
const axios = require('axios');

async function fundTestnetAccount(publicKey) {
  try {
    const response = await axios.get(`https://friendbot.stellar.org?addr=${publicKey}`);
    console.log(`Successfully funded account: ${publicKey}`);
  } catch (error) {
    console.error(`Funding failed for ${publicKey}:`, error.message);
  }
}
```

### 2.2 On Mainnet (Manual Seed & Verification)
For Mainnet testing:
1. Pre-fund the Admin and Sponsor accounts manually with native XLM (and/or target custom assets).
2. The test runner must execute a pre-flight check to verify that all accounts have:
   * A minimum reserve balance (typically > 5 XLM for base reserves and trustlines).
   * Active trustlines established for the target non-native asset (`TEST_TOKEN_ADDRESS`).

---

## 3. Contract Compilation & On-Chain Deployment

Before running the liveness sequence, the contracts must be compiled and deployed to the network.

### 3.1 Compilation
Build the WASM binaries optimized for production:
```bash
cargo build --target wasm32-unknown-unknown --release
```

### 3.2 Deployment Sequence via `soroban-cli`
Use `soroban-cli` to install and initialize the contracts on the network:

```bash
# 1. Install the escrow WASM contract byte-code on-chain
WASM_HASH=$(soroban contract install \
  --network testnet \
  --source admin_key \
  --wasm target/wasm32-unknown-unknown/release/mergefi_escrow.wasm)

# 2. Deploy an instance of the contract
CONTRACT_ID=$(soroban contract deploy \
  --network testnet \
  --source admin_key \
  --wasm-hash $WASM_HASH)

# 3. Initialize the contract instance config
soroban contract invoke \
  --network testnet \
  --source admin_key \
  --id $CONTRACT_ID \
  -- \
  initialize \
  --admin $ADMIN_ADDRESS \
  --treasury $TREASURY_ADDRESS \
  --fee_bps 500 \
  --max_sponsors 20
```

---

## 4. Liveness Integration Sequence

Run the following test sequence to verify contract functionality under real-network latency and fee conditions:

### Step 1: Sponsor Funds Escrow
The Sponsor invokes the `fund` endpoint, transferring the bounty amount to the contract:
```bash
soroban contract invoke \
  --network testnet \
  --source sponsor_key \
  --id $CONTRACT_ID \
  -- \
  fund \
  --issue_id 1 \
  --sponsor $SPONSOR_ADDRESS \
  --token $TEST_TOKEN_ADDRESS \
  --amount 1000000000 \
  --deadline 1800 \
  --target None
```

### Step 2: Co-Sponsor Contribution
Verify crowdfunding functionality by having a secondary sponsor top up the escrow:
```bash
soroban contract invoke \
  --network testnet \
  --source co_sponsor_key \
  --id $CONTRACT_ID \
  -- \
  contribute \
  --issue_id 1 \
  --sponsor $CO_SPONSOR_ADDRESS \
  --amount 500000000
```

### Step 3: Admin Releases Bounty to Recipients (with Split)
The backend (Admin) invokes `release`, distributing the escrow balance (deducting the 5% fee to the Treasury) atomically to the developers:
```bash
soroban contract invoke \
  --network testnet \
  --source admin_key \
  --id $CONTRACT_ID \
  -- \
  release \
  --issue_id 1 \
  --recipients '[["G_RECIP_1", 6000], ["G_RECIP_2", 4000]]'
```

---

## 5. Cleanup, Liveness Verification & Recovery Steps

To maintain database cleanliness and prevent on-chain dust accumulation, the test suite must perform post-run verification and balance reclaiming:

### 5.1 Verification Checks
* Query the contract's read-only methods (e.g., `get_escrow`) to verify that the escrow status transitioned to `Paid`.
* Query Horizon balance endpoints to verify the exact payout delivery:
  * Treasury balance increased by `1,500,000,000 * 5% = 75,000,000` units.
  * Recipient 1 balance increased by `1,425,000,000 * 60% = 855,000,000` units.
  * Recipient 2 balance increased by `1,425,000,000 * 40% = 570,000,000` units.

### 5.2 Balance Reclaiming (Cleanup)
Since Friendbot testnet accounts are temporary, cleanups are trivial. However, for persistent testing accounts:
* **Escrow Expiry Test Cleanup:** If testing the refund path, wait or advance the ledger (on a local sandbox) to call `refund` and verify that the remaining balance returns entirely to the Sponsor.
* **Maintenance Pool Reclaim:** For maintenance-pool tests, the Sponsor must invoke `reclaim_deposit` after the inactivity window to retrieve remaining test tokens and prevent lockup.
