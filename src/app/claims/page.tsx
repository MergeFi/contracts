'use client';

import { useState, useEffect, useCallback } from 'react';
import { useWallet } from '@/hooks/useWallet';
import { fetchUserClaims } from '@/lib/api';
import { ConnectWalletPrompt } from '@/components/ConnectWalletPrompt';
import { ClaimHistoryTable } from '@/components/ClaimHistoryTable';
import { FullPageSpinner, LoadingSpinner } from '@/components/LoadingSpinner';
import type { Claim } from '@/types';

export default function ClaimsPage() {
  const { address, connected } = useWallet();
  const [claims,   setClaims]  = useState<Claim[]>([]);
  const [loading,  setLoading] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [error,    setError]   = useState<string | null>(null);

  const loadClaims = useCallback(async () => {
    if (!address) return;
    setLoading(true);
    try {
      const data = await fetchUserClaims(address);
      setClaims(data);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load claims');
    } finally {
      setLoading(false);
    }
  }, [address]);

  useEffect(() => {
    loadClaims();
  }, [loadClaims]);

  const handleRetry = useCallback(async () => {
    setRetrying(true);
    await loadClaims();
    setRetrying(false);
  }, [loadClaims]);

  if (!connected) {
    return (
      <main className="mx-auto max-w-7xl px-6 py-20">
        <ConnectWalletPrompt message="Connect your wallet to view your claims" />
      </main>
    );
  }

  return (
    <main className="mx-auto max-w-7xl px-6 py-12">
      <h1 className="text-2xl font-bold">Claim History</h1>
      <p className="mt-1 text-sm text-gray-400">
        All claims submitted from your connected wallet
      </p>

      {loading && !retrying ? (
        <FullPageSpinner />
      ) : error ? (
        <div className="mt-8 rounded-2xl border border-red-500/20 bg-red-500/5 p-6 text-sm text-red-400">
          <p>{error}</p>
          <button
            onClick={handleRetry}
            disabled={retrying}
            className="mt-3 flex items-center gap-2 rounded-lg border border-red-500/30 px-4 py-2 text-xs text-red-400 hover:bg-red-500/10 disabled:opacity-60 transition-colors"
          >
            {retrying && <LoadingSpinner size="sm" className="h-3 w-3 border-red-400/30 border-t-red-400" />}
            Try again
          </button>
        </div>
      ) : (
        <div className="mt-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6">
          <ClaimHistoryTable claims={claims} />
        </div>
      )}
    </main>
  );
}
