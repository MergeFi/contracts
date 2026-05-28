'use client';

import { useEffect, type ReactNode } from 'react';

interface ModalProps {
  open:     boolean;
  onClose:  () => void;
  title?:   string;
  children: ReactNode;
  maxWidth?: string;
}

export function Modal({ open, onClose, title, children, maxWidth = 'max-w-md' }: ModalProps) {
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className={`relative w-full ${maxWidth} rounded-2xl border border-white/10 bg-gray-900 p-6 shadow-2xl`}>
        {title && (
          <div className="mb-5 flex items-center justify-between">
            <h2 className="text-lg font-bold text-white">{title}</h2>
            <button
              onClick={onClose}
              className="text-gray-500 transition-colors hover:text-white"
              aria-label="Close modal"
            >
              ✕
            </button>
          </div>
        )}
        {!title && (
          <button
            onClick={onClose}
            className="absolute right-4 top-4 text-gray-500 transition-colors hover:text-white"
            aria-label="Close modal"
          >
            ✕
          </button>
        )}
        {children}
      </div>
    </div>
  );
}
