'use client';

import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react';
import { connectWallet, disconnectWallet, getStoredAddress } from '@/lib/stellar';
import { toUserMessage } from '@/lib/errors';
import { login, setAuthErrorHandler } from '@/lib/api';
import storage from '@/lib/storage';
import { AUTH_TOKEN_STORAGE_KEY } from '@/lib/constants';

interface WalletContextValue {
  address:    string | null;
  connected:  boolean;
  connecting: boolean;
  error:      string | null;
  connect:    () => Promise<void>;
  disconnect: () => void;
}

const WalletContext = createContext<WalletContextValue | null>(null);

export function WalletProvider({ children }: { children: ReactNode }) {
  const [address,    setAddress]    = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [error,      setError]      = useState<string | null>(null);

  const disconnect = useCallback(() => {
    disconnectWallet();
    storage.removeSession(AUTH_TOKEN_STORAGE_KEY);
    setAddress(null);
    setError(null);
  }, []);

  const connect = useCallback(async () => {
    setConnecting(true);
    setError(null);
    try {
      const addr = await connectWallet();
      try {
        const challenge = `${addr}:${Date.now()}`;
        const { token } = await login(addr, challenge);
        storage.setSession(AUTH_TOKEN_STORAGE_KEY, token);
      } catch (authErr) {
        const msg = toUserMessage(authErr);
        setError(`Auth failed: ${msg}`);
        disconnectWallet();
        return;
      }
      setAddress(addr);
    } catch (err) {
      const msg = toUserMessage(err);
      if (!msg.includes('closed')) setError(msg);
    } finally {
      setConnecting(false);
    }
  }, []);

  useEffect(() => {
    const stored = getStoredAddress();
    if (stored) setAddress(stored);
  }, []);

  useEffect(() => {
    setAuthErrorHandler(disconnect);
  }, [disconnect]);

  return (
    <WalletContext.Provider
      value={{ address, connected: !!address, connecting, error, connect, disconnect }}
    >
      {children}
    </WalletContext.Provider>
  );
}

export function useWalletContext(): WalletContextValue {
  const ctx = useContext(WalletContext);
  if (!ctx) throw new Error('useWalletContext must be used inside <WalletProvider>');
  return ctx;
}
