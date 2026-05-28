'use client';

import { useToast } from '@/context/ToastContext';
import type { Toast as ToastType } from '@/types';

const VARIANT_STYLES: Record<string, string> = {
  success: 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300',
  error:   'border-red-500/30     bg-red-500/10     text-red-300',
  warning: 'border-amber-500/30   bg-amber-500/10   text-amber-300',
  info:    'border-white/10       bg-white/5        text-gray-200',
};

const VARIANT_ICONS: Record<string, string> = {
  success: '✓',
  error:   '✕',
  warning: '⚠',
  info:    'ℹ',
};

function ToastItem({ toast }: { toast: ToastType }) {
  const { dismiss } = useToast();
  const style  = VARIANT_STYLES[toast.variant] ?? VARIANT_STYLES.info;
  const icon   = VARIANT_ICONS[toast.variant]  ?? VARIANT_ICONS.info;

  return (
    <div
      className={`flex items-start gap-3 rounded-xl border p-4 shadow-xl backdrop-blur-sm ${style}`}
      role="alert"
    >
      <span className="mt-0.5 text-sm font-bold">{icon}</span>
      <p className="flex-1 text-sm leading-snug">{toast.message}</p>
      <button
        onClick={() => dismiss(toast.id)}
        className="ml-2 text-xs opacity-60 hover:opacity-100 transition-opacity"
        aria-label="Dismiss"
      >
        ✕
      </button>
    </div>
  );
}

export function ToastContainer() {
  const { toasts } = useToast();
  if (!toasts.length) return null;
  return (
    <div className="pointer-events-none fixed bottom-6 right-6 z-50 flex flex-col gap-3 pointer-events-auto w-[340px]">
      {toasts.map((t) => (
        <ToastItem key={t.id} toast={t} />
      ))}
    </div>
  );
}
