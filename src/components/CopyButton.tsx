'use client';

import { useState } from 'react';

interface CopyButtonProps {
  text:      string;
  label?:    string;
  className?: string;
}

export function CopyButton({ text, label, className }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* clipboard permission denied */ }
  }

  return (
    <button
      onClick={handleCopy}
      className={`inline-flex items-center gap-1.5 rounded-lg border border-white/10 px-2 py-1 text-xs text-gray-400 transition-all hover:border-white/20 hover:text-white active:scale-95 ${className ?? ''}`}
      aria-label={`Copy ${label ?? 'value'}`}
    >
      {copied ? '✓ Copied' : label ?? 'Copy'}
    </button>
  );
}
