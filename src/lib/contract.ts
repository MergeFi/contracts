'use client';

import {
  Contract,
  TransactionBuilder,
  BASE_FEE,
  Networks,
  rpc as StellarRpc,
  nativeToScVal,
  scValToNative,
  xdr,
} from '@stellar/stellar-sdk';
import { SOROBAN_RPC_URL, POLICY_CONTRACT_ID, CLAIMS_CONTRACT_ID, STELLAR_NETWORK } from './constants';
import { signTransaction } from './stellar';
import { ContractError } from './errors';

const NETWORK_PASSPHRASE =
  STELLAR_NETWORK === 'PUBLIC' ? Networks.PUBLIC : Networks.TESTNET;

function getRpc(): StellarRpc.Server {
  return new StellarRpc.Server(SOROBAN_RPC_URL);
}

export async function simulateContractCall(
  contractId: string,
  method: string,
  args: xdr.ScVal[],
  callerAddress: string,
): Promise<xdr.ScVal | null> {
  const rpc     = getRpc();
  const account = await rpc.getAccount(callerAddress);
  const contract = new Contract(contractId);

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(contract.call(method, ...args))
    .setTimeout(30)
    .build();

  const result = await rpc.simulateTransaction(tx);
  if (StellarRpc.Api.isSimulationError(result)) {
    throw new ContractError(`Simulation failed: ${result.error}`);
  }
  if (!result.result?.retval) return null;
  return result.result.retval;
}

export async function buildBuyPolicyTx(
  walletAddress: string,
  productId: string,
  coverageStroops: bigint,
  oracleKey: string,
  durationDays: number,
): Promise<string> {
  const rpc      = getRpc();
  const account  = await rpc.getAccount(walletAddress);
  const contract = new Contract(POLICY_CONTRACT_ID);

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(
      contract.call(
        'buy_policy',
        nativeToScVal(walletAddress,    { type: 'address' }),
        nativeToScVal(productId,        { type: 'string'  }),
        nativeToScVal(coverageStroops,  { type: 'i128'    }),
        nativeToScVal(oracleKey,        { type: 'symbol'  }),
        nativeToScVal(durationDays * 86400, { type: 'u64' }),
      ),
    )
    .setTimeout(60)
    .build();

  const simResult = await rpc.simulateTransaction(tx);
  if (StellarRpc.Api.isSimulationError(simResult)) {
    throw new ContractError(`buy_policy simulation failed: ${simResult.error}`);
  }

  const assembled = StellarRpc.assembleTransaction(tx, simResult).build();
  return assembled.toXDR();
}

export async function invokeBuyPolicy(
  walletAddress: string,
  productId: string,
  coverageStroops: bigint,
  oracleKey: string,
  durationDays: number,
): Promise<string> {
  const rpc      = getRpc();
  const account  = await rpc.getAccount(walletAddress);
  const contract = new Contract(POLICY_CONTRACT_ID);

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(
      contract.call(
        'buy_policy',
        nativeToScVal(walletAddress,    { type: 'address' }),
        nativeToScVal(productId,        { type: 'string'  }),
        nativeToScVal(coverageStroops,  { type: 'i128'    }),
        nativeToScVal(oracleKey,        { type: 'symbol'  }),
        nativeToScVal(durationDays * 86400, { type: 'u64' }),
      ),
    )
    .setTimeout(60)
    .build();

  const simResult = await rpc.simulateTransaction(tx);
  if (StellarRpc.Api.isSimulationError(simResult)) {
    throw new ContractError(`buy_policy simulation failed: ${simResult.error}`);
  }

  const assembled    = StellarRpc.assembleTransaction(tx, simResult).build();
  const signedXdr    = await signTransaction(assembled.toXDR());
  const signedTx     = TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE);
  const submitResult = await rpc.sendTransaction(signedTx);

  if (submitResult.status === 'ERROR') {
    throw new ContractError(`Transaction rejected: ${JSON.stringify(submitResult.errorResult)}`);
  }
  return submitResult.hash;
}

export async function invokeSubmitClaim(
  walletAddress: string,
  policyId: string,
): Promise<string> {
  const rpc      = getRpc();
  const account  = await rpc.getAccount(walletAddress);
  const contract = new Contract(CLAIMS_CONTRACT_ID);

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(
      contract.call(
        'submit_claim',
        nativeToScVal(walletAddress, { type: 'address' }),
        nativeToScVal(policyId,      { type: 'string'  }),
      ),
    )
    .setTimeout(60)
    .build();

  const simResult = await rpc.simulateTransaction(tx);
  if (StellarRpc.Api.isSimulationError(simResult)) {
    throw new ContractError(`submit_claim simulation failed: ${simResult.error}`);
  }

  const assembled    = StellarRpc.assembleTransaction(tx, simResult).build();
  const signedXdr    = await signTransaction(assembled.toXDR());
  const signedTx     = TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE);
  const submitResult = await rpc.sendTransaction(signedTx);

  if (submitResult.status === 'ERROR') {
    throw new ContractError(`Claim transaction rejected: ${JSON.stringify(submitResult.errorResult)}`);
  }
  return submitResult.hash;
}

export async function buildDepositTx(
  poolId: string,
  amount: bigint,
  wallet: string,
): Promise<string> {
  const rpc      = getRpc();
  const account  = await rpc.getAccount(wallet);
  const contract = new Contract(poolId);

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(
      contract.call(
        'deposit',
        nativeToScVal(amount, { type: 'i128' }),
        nativeToScVal(wallet, { type: 'address' }),
      ),
    )
    .setTimeout(60)
    .build();

  const simResult = await rpc.simulateTransaction(tx);
  if (StellarRpc.Api.isSimulationError(simResult)) {
    throw new ContractError(`deposit simulation failed: ${simResult.error}`);
  }

  return StellarRpc.assembleTransaction(tx, simResult).build().toXDR();
}

export async function submitSignedTransaction(signedXdr: string): Promise<string> {
  const rpc          = getRpc();
  const signedTx     = TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE);
  const submitResult = await rpc.sendTransaction(signedTx);

  if (submitResult.status === 'ERROR') {
    throw new ContractError(`Transaction rejected: ${JSON.stringify(submitResult.errorResult)}`);
  }
  return submitResult.hash;
}
