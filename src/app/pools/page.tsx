'use client';

import { useState } from 'react';
import type { PoolStats } from '@/types';
import { formatUSDC } from '@/lib/format';
import { Badge } from '@/components/Badge';
import { ProgressBar } from '@/components/ProgressBar';
import { EmptyState } from '@/components/EmptyState';
import { SkeletonCard } from '@/components/Skeleton';
import { DepositModal } from '@/components/DepositModal';
import { useWallet } from '@/hooks/useWallet';
import { usePools } from '@/hooks/usePools';
import { CATEGORY_ICONS, CATEGORY_LABELS } from '@/lib/constants';

export default function PoolsPage() {
  const { connected } = useWallet();
  const { pools, loading, error, refetch } = usePools();
  const [depositPool,  setDepositPool]  = useState<PoolStats | null>(null);

  return (
    <main className="mx-auto max-w-7xl px-6 py-12">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">Risk Pools</h1>
          <p className="mt-1 text-sm text-gray-400">
            Provide liquidity, underwrite risk, and earn yield on USDC premiums
          </p>
        </div>
        <button
          onClick={() => void refetch()}
          disabled={loading}
          className="rounded-lg bg-teal-500 px-4 py-2 text-xs font-semibold text-white hover:bg-teal-400 disabled:opacity-60 transition-colors flex items-center gap-2"
        >
          {loading && <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-teal-300 border-t-teal-100" />}
          Refresh
        </button>
      </div>

      {loading && (
        <div className="mt-8 grid gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {[1, 2, 3].map((i) => <SkeletonCard key={i} />)}
        </div>
      )}

      {error && (
        <div className="mt-8 rounded-2xl border border-red-500/20 bg-red-500/5 p-6">
          <div className="flex items-start justify-between gap-4">
            <p className="text-sm text-red-400">{error}</p>
            <button
              onClick={() => void refetch()}
              disabled={loading}
              className="shrink-0 rounded-lg bg-red-500 px-4 py-2 text-xs font-semibold text-white hover:bg-red-400 disabled:opacity-60 transition-colors flex items-center gap-2"
            >
              {loading && <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-red-300 border-t-red-100" />}
              Try again
            </button>
          </div>
        </div>
      )}

      {!loading && !error && pools.length === 0 && (
        <EmptyState
          icon="💧"
          title="No pools active yet"
          description="Risk pools will open when the first insurance products go live on mainnet."
          className="mt-12"
        />
      )}

      {!loading && pools.length > 0 && (
        <div className="mt-8 grid gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {pools.map((pool) => (
            <div key={pool.poolId} className="rounded-2xl border border-white/10 bg-white/[0.03] p-6">
              <div className="flex items-center gap-3">
                <span className="text-2xl">{CATEGORY_ICONS[pool.category] ?? '🛡️'}</span>
                <div>
                  <h3 className="font-semibold text-white">{CATEGORY_LABELS[pool.category] ?? pool.category} Pool</h3>
                  <Badge label={pool.category} variant="teal" />
                </div>
              </div>

              <dl className="mt-5 space-y-3 text-sm">
                <div className="flex justify-between">
                  <dt className="text-gray-500">Liquidity</dt>
                  <dd className="font-semibold text-white">{formatUSDC(pool.totalLiquidity)}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-gray-500">Active policies</dt>
                  <dd className="font-semibold text-white">{pool.activePolicies}</dd>
                </div>
                <div className="flex justify-between">
                  <dt className="text-gray-500">APY</dt>
                  <dd className="font-semibold text-emerald-400">{(pool.apy * 100).toFixed(1)}%</dd>
                </div>
              </dl>

              <div className="mt-4">
                <ProgressBar
                  value={pool.utilizationRate * 100}
                  label="Utilization"
                  colour={pool.utilizationRate > 0.8 ? 'red' : pool.utilizationRate > 0.5 ? 'amber' : 'teal'}
                />
              </div>

              <div className="mt-5 flex gap-2">
                {connected && (
                  <button
                    onClick={() => setDepositPool(pool)}
                    className="flex-1 rounded-xl bg-teal-500 py-2 text-xs font-semibold text-white hover:bg-teal-400 transition-colors"
                  >
                    Deposit
                  </button>
                )}
                <button
                  disabled
                  aria-label="Withdraw — coming soon"
                  title="Withdrawals are not yet available"
                  className={`${connected ? 'flex-1' : 'w-full'} rounded-xl border border-white/10 py-2 text-xs font-semibold text-gray-500 cursor-not-allowed`}
                >
                  Withdraw (coming soon)
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {depositPool && (
        <DepositModal pool={depositPool} onClose={() => setDepositPool(null)} />
      )}
    </main>
  );
}
