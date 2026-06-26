'use client';

import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react';
import { connectWallet, disconnectWallet, getStoredAddress, getConnectedAddress, signAuthMessage, EXPECTED_NETWORK_PASSPHRASE } from '@/lib/stellar';
import { toUserMessage } from '@/lib/errors';
import { fetchChallenge, login, setAuthErrorHandler } from '@/lib/api';
import storage from '@/lib/storage';
import { AUTH_TOKEN_STORAGE_KEY, STELLAR_NETWORK } from '@/lib/constants';

const TESTNET_PASSPHRASE = 'Test SDF Network ; September 2015';
const PUBLIC_PASSPHRASE  = 'Public Global Stellar Network ; September 2015';

function networkLabel(passphrase: string | null): string {
  if (passphrase === PUBLIC_PASSPHRASE)  return 'Mainnet';
  if (passphrase === TESTNET_PASSPHRASE) return 'Testnet';
  return 'an unsupported network';
}

const APP_NETWORK_LABEL = STELLAR_NETWORK === 'PUBLIC' ? 'Mainnet' : 'Testnet';

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
      const { address: addr, networkPassphrase } = await connectWallet();

      // Best-effort: if the wallet reports a network, ensure it matches the app's.
      if (networkPassphrase && networkPassphrase !== EXPECTED_NETWORK_PASSPHRASE) {
        disconnect();
        setError(
          `Your wallet is connected to ${networkLabel(networkPassphrase)} but this app runs on ${APP_NETWORK_LABEL}. Switch your wallet network and try again.`,
        );
        return;
      }

      try {
        // 1. Obtain a challenge nonce (server-issued or locally generated).
        const challenge = await fetchChallenge(addr);
        // 2. Ask the wallet to sign the challenge — this proves key ownership.
        const signedChallenge = await signAuthMessage(challenge);
        // 3. Exchange the signed challenge for a JWT.
        const { token } = await login(addr, signedChallenge);
        storage.setSession(AUTH_TOKEN_STORAGE_KEY, token);
      } catch (authErr) {
        const msg = toUserMessage(authErr);
        setError(`Auth failed: ${msg}`);
        disconnect();
        return;
      }
      setAddress(addr);
    } catch (err) {
      const msg = toUserMessage(err);
      if (!msg.includes('Wallet modal closed')) setError(msg);
    } finally {
      setConnecting(false);
    }
  }, [disconnect]);

  // Restore connected state only when a valid session token also exists.
  // sessionStorage is cleared when the tab closes, so a stored address without
  // a token means the previous session has expired. Clear it rather than
  // briefly exposing a false "connected" state that the 401 interceptor would
  // revoke milliseconds later on the first API call.
  useEffect(() => {
    async function initWallet() {
      const stored = getStoredAddress();
      if (!stored) return;

      const hasSession = !!storage.getSession(AUTH_TOKEN_STORAGE_KEY);
      if (!hasSession) {
        disconnect();
        return;
      }

      try {
        const liveAddress = await getConnectedAddress();
        if (liveAddress && liveAddress === stored) {
          setAddress(stored);
        } else {
          disconnect();
        }
      } catch {
        disconnect();
      }
    }
    initWallet();
  }, [disconnect]);

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
