'use client';

import { useState } from 'react';
import type { Product } from '@/types';
import { useWallet } from '@/hooks/useWallet';
import { buyPolicy } from '@/lib/api';
import { displayToStroops, basisPointsToPercent } from '@/lib/format';
import { toUserMessage } from '@/lib/errors';
import { Modal } from './Modal';
import { StepProgress } from './ProgressBar';
import { useToast } from '@/context/ToastContext';

interface Props {
  product: Product;
  onClose: () => void;
}

const STEPS = ['Configure', 'Review', 'Sign'];

export function BuyPolicyModal({ product, onClose }: Props) {
  const { address, connect, connecting } = useWallet();
  const { show: showToast }             = useToast();

  const [coverage,  setCoverage]  = useState('');
  const [duration,  setDuration]  = useState(String(Math.min(30, product.maxDuration)));
  const [oracleKey, setOracleKey] = useState('');
  const [step,      setStep]      = useState(0);
  const [busy,      setBusy]      = useState(false);
  const [error,     setError]     = useState('');

  const coverageNum   = parseFloat(coverage) || 0;
  const premiumPct    = product.premiumRate / 10_000;
  const estimatedPrem = (coverageNum * premiumPct).toFixed(2);

  function validate(): string {
    const cov = parseFloat(coverage);
    if (isNaN(cov) || cov < parseFloat(product.coverageMin) || cov > parseFloat(product.coverageMax)) {
      return `Coverage must be between ${product.coverageMin} and ${product.coverageMax} USDC`;
    }
    const dur = parseInt(duration, 10);
    if (isNaN(dur) || dur < 1 || dur > product.maxDuration) {
      return `Duration must be between 1 and ${product.maxDuration} days`;
    }
    if (!oracleKey.trim() || oracleKey.trim().length > 9) {
      return 'Oracle key is required and must be at most 9 characters';
    }
    return '';
  }

  async function handleBuy() {
    if (!address) { await connect(); return; }
    const validationError = validate();
    if (validationError) { setError(validationError); return; }
    if (step < 2) { setStep((s) => s + 1); return; }

    setBusy(true);
    setError('');
    try {
      const { policyId } = await buyPolicy({
        productId: product.id,
        coverage:  displayToStroops(coverage).toString(),
        oracleKey: oracleKey.trim(),
        duration:  parseInt(duration, 10),
        wallet:    address,
        signedXdr: '',
      });
      showToast(`Policy ${policyId.slice(0, 8)}… activated`, 'success');
      onClose();
    } catch (err) {
      setError(toUserMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal open title={`Buy — ${product.name}`} onClose={onClose}>
      <StepProgress steps={STEPS} current={step} />

      <div className="mt-6 space-y-4">
        {step === 0 && (
          <>
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase tracking-widest text-gray-400">
                Coverage Amount (USDC)
              </label>
              <input
                type="number"
                value={coverage}
                onChange={(e) => setCoverage(e.target.value)}
                placeholder={`${product.coverageMin} – ${product.coverageMax}`}
                className="w-full rounded-xl border border-white/10 bg-white/5 px-4 py-2.5 text-sm focus:border-teal-500 focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase tracking-widest text-gray-400">
                Duration (days, max {product.maxDuration})
              </label>
              <input
                type="number"
                value={duration}
                onChange={(e) => setDuration(e.target.value)}
                min={1}
                max={product.maxDuration}
                className="w-full rounded-xl border border-white/10 bg-white/5 px-4 py-2.5 text-sm focus:border-teal-500 focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase tracking-widest text-gray-400">
                Oracle Key
              </label>
              <input
                type="text"
                value={oracleKey}
                onChange={(e) => setOracleKey(e.target.value)}
                placeholder='e.g. "kis2606" for Kisumu June 2026'
                maxLength={9}
                className="w-full rounded-xl border border-white/10 bg-white/5 px-4 py-2.5 text-sm focus:border-teal-500 focus:outline-none"
              />
              <p className="mt-1 text-[10px] text-gray-500">Max 9 chars (Soroban Symbol)</p>
            </div>
          </>
        )}

        {step === 1 && (
          <div className="rounded-xl border border-white/10 bg-white/5 p-5 space-y-3 text-sm">
            <h4 className="font-semibold text-white">Review your policy</h4>
            <div className="flex justify-between text-gray-400">
              <span>Product</span>
              <span className="text-white font-medium">{product.name}</span>
            </div>
            <div className="flex justify-between text-gray-400">
              <span>Coverage</span>
              <span className="text-emerald-400 font-semibold">{coverageNum.toFixed(2)} USDC</span>
            </div>
            <div className="flex justify-between text-gray-400">
              <span>Duration</span>
              <span className="text-white">{duration} days</span>
            </div>
            <div className="flex justify-between text-gray-400">
              <span>Oracle key</span>
              <span className="font-mono text-xs text-white">{oracleKey}</span>
            </div>
            <div className="flex justify-between border-t border-white/10 pt-3 text-gray-400">
              <span>Premium ({basisPointsToPercent(product.premiumRate)})</span>
              <span className="font-bold text-white">{estimatedPrem} USDC</span>
            </div>
          </div>
        )}

        {step === 2 && (
          <div className="space-y-3 text-sm text-gray-400">
            <p>Your Stellar wallet will prompt you to sign the policy transaction.</p>
            <p className="text-xs">
              Premium of <strong className="text-white">{estimatedPrem} USDC</strong> will be deducted from your wallet balance.
            </p>
          </div>
        )}

        {error && <p className="text-sm text-red-400">{error}</p>}
      </div>

      <button
        onClick={handleBuy}
        disabled={busy || connecting}
        className="mt-6 w-full rounded-xl bg-teal-500 py-2.5 font-semibold text-white hover:bg-teal-400 disabled:opacity-60 transition-colors"
      >
        {connecting  ? 'Connecting wallet…' :
         busy        ? 'Submitting…' :
         !address    ? 'Connect Wallet' :
         step === 2  ? 'Sign & Confirm' :
         step === 1  ? 'Confirm details' :
         'Next'}
      </button>
    </Modal>
  );
}
