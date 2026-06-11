import { STELLAR_NETWORK } from '@/lib/constants';

interface TransactionLinkProps {
  txHash:    string;
  label?:    string;
  className?: string;
}

const EXPLORER_BASE =
  STELLAR_NETWORK === 'PUBLIC'
    ? 'https://stellar.expert/explorer/public/tx/'
    : 'https://stellar.expert/explorer/testnet/tx/';

export function TransactionLink({ txHash, label, className }: TransactionLinkProps) {
  return (
    <a
      href={`${EXPLORER_BASE}${txHash}`}
      target="_blank"
      rel="noopener noreferrer"
      className={`inline-flex items-center gap-1.5 font-mono text-xs text-teal-400 hover:text-teal-300 hover:underline transition-colors ${className ?? ''}`}
    >
      {label ?? `${txHash.slice(0, 8)}…${txHash.slice(-6)}`}
      <span className="text-[10px] text-gray-600">↗</span>
    </a>
  );
}
