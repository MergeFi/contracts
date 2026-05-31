'use client';

import { Badge } from './Badge';
import { EmptyState } from './EmptyState';
import type { Claim } from '@/types';
import { formatUSDC, formatDateTime, shortenAddress } from '@/lib/format';

interface ClaimHistoryTableProps {
  claims:    Claim[];
  className?: string;
}

export function ClaimHistoryTable({ claims, className }: ClaimHistoryTableProps) {
  if (!claims.length) {
    return (
      <EmptyState
        icon="📋"
        title="No claims yet"
        description="When you submit a claim, it will appear here with its current status."
        className={className}
      />
    );
  }

  return (
    <div className={`overflow-x-auto ${className ?? ''}`}>
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-white/10 text-left text-xs uppercase tracking-wider text-gray-500">
            <th className="pb-3 pr-4">Claim ID</th>
            <th className="pb-3 pr-4">Policy</th>
            <th className="pb-3 pr-4">Trigger</th>
            <th className="pb-3 pr-4">Payout</th>
            <th className="pb-3 pr-4">Submitted</th>
            <th className="pb-3">Status</th>
          </tr>
        </thead>
        <tbody>
          {claims.map((claim) => (
            <tr
              key={claim.id}
              className="border-b border-white/5 transition-colors hover:bg-white/[0.02]"
            >
              <td className="py-4 pr-4 font-mono text-xs text-gray-400">
                {claim.id.slice(0, 8)}…
              </td>
              <td className="py-4 pr-4 font-mono text-xs text-gray-400">
                {claim.policyId.slice(0, 8)}…
              </td>
              <td className="py-4 pr-4">
                <span className={`text-xs font-semibold ${claim.triggerMet ? 'text-emerald-400' : 'text-gray-500'}`}>
                  {claim.triggerMet ? '✓ Met' : '✕ Not met'}
                </span>
              </td>
              <td className="py-4 pr-4 text-xs font-semibold text-emerald-400">
                {claim.payoutAmount ? formatUSDC(claim.payoutAmount) : '—'}
              </td>
              <td className="py-4 pr-4 text-xs text-gray-400">
                {formatDateTime(claim.submittedAt)}
              </td>
              <td className="py-4">
                <Badge label={claim.status} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
