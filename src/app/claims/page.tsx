'use client';

import { useState, useEffect, useCallback } from 'react';
import { useWallet } from '@/hooks/useWallet';
import { fetchUserClaims } from '@/lib/api';
import { ConnectWalletPrompt } from '@/components/ConnectWalletPrompt';
import { ClaimHistoryTable } from '@/components/ClaimHistoryTable';
import { LoadingSpinner } from '@/components/LoadingSpinner';
import { SkeletonTable } from '@/components/Skeleton';
import type { Claim } from '@/types';
import { CLAIMS_REFRESH_INTERVAL_MS } from '@/lib/constants';

export default function ClaimsPage() {
  const { address, connected } = useWallet();
  const [claims,      setClaims]      = useState<Claim[]>([]);
  const [loading,     setLoading]     = useState(false);
  const [refreshing,  setRefreshing]  = useState(false);
  const [error,       setError]       = useState<string | null>(null);
  const [lastFetched, setLastFetched] = useState<Date | null>(null);

  const loadClaims = useCallback(async (silent = false) => {
    if (!address) return;
    if (!silent) setLoading(true);
    try {
      const data = await fetchUserClaims(address);
      setClaims(data);
      setLastFetched(new Date());
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load claims');
    } finally {
      if (!silent) setLoading(false);
    }
  }, [address]);

  useEffect(() => {
    void loadClaims();
    if (!address) return;
    const interval = setInterval(() => {
      if (!document.hidden) void loadClaims(true);
    }, CLAIMS_REFRESH_INTERVAL_MS);
    const onVisible = () => {
      if (!document.hidden) void loadClaims(true);
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      clearInterval(interval);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [loadClaims, address]);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    await loadClaims();
    setRefreshing(false);
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
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold">Claim History</h1>
          <p className="mt-1 text-sm text-gray-400">
            All claims submitted from your connected wallet
          </p>
        </div>
        <div className="flex flex-col items-end gap-1">
          <button
            onClick={handleRefresh}
            disabled={refreshing || loading}
            className="flex items-center gap-2 rounded-lg border border-white/10 px-4 py-2 text-xs text-gray-300 hover:border-white/20 hover:text-white disabled:opacity-60 transition-colors"
          >
            {refreshing && <LoadingSpinner size="sm" className="h-3 w-3" />}
            Refresh
          </button>
          {lastFetched && (
            <span className="text-[10px] text-gray-500">
              Updated {lastFetched.toLocaleTimeString()}
            </span>
          )}
        </div>
      </div>

      {loading && !refreshing ? (
        <div className="mt-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6">
          <SkeletonTable rows={5} />
        </div>
      ) : error ? (
        <div className="mt-8 rounded-2xl border border-red-500/20 bg-red-500/5 p-6 text-sm text-red-400">
          <p>{error}</p>
          <button
            onClick={handleRefresh}
            disabled={refreshing}
            className="mt-3 flex items-center gap-2 rounded-lg border border-red-500/30 px-4 py-2 text-xs text-red-400 hover:bg-red-500/10 disabled:opacity-60 transition-colors"
          >
            {refreshing && <LoadingSpinner size="sm" className="h-3 w-3 border-red-400/30 border-t-red-400" />}
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
