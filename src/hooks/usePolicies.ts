"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import { fetchUserPolicies, fetchPolicy } from "@/lib/api";
import type { Policy } from "@/types";
import { POLLING_INTERVAL_MS } from "@/lib/constants";

export function usePolicies(walletAddress: string | null) {
  const [policies, setPolicies] = useState<Policy[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isFirstLoad = useRef(true);

  // Reset immediately when wallet disconnects
  useEffect(() => {
    if (!walletAddress) {
      setPolicies([]);
      isFirstLoad.current = true;
      return;
    }
    if (isFirstLoad.current) setLoading(true);
    setError(null);
    try {
      const data = await fetchUserPolicies(walletAddress);
      if (signal?.aborted) return;
      setPolicies(data);
    } catch (err) {
      if (signal?.aborted) return;
      setError(err instanceof Error ? err.message : "Failed to load policies");
    } finally {
      isFirstLoad.current = false;
      setLoading(false);
    }
  }, [walletAddress]);

  useEffect(() => {
    if (!walletAddress) return;
    const controller = new AbortController();
    void load(controller.signal);
    const interval = setInterval(() => {
      if (!document.hidden) void load(controller.signal);
    }, POLLING_INTERVAL_MS);
    const onVisible = () => { if (!document.hidden) void load(controller.signal); };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      controller.abort();
      clearInterval(interval);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [load, walletAddress]);

  const refetch = useCallback(() => load(), [load]);

  return { policies, loading, error, refetch };
}

export function usePolicy(id: string | null) {
  const [policy, setPolicy] = useState<Policy | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cancelledRef = useRef(false);

  const load = useCallback(async () => {
    if (!id) return;
    cancelledRef.current = false;
    setLoading(true);
    setError(null);
    try {
      const p = await fetchPolicy(id);
      if (!cancelledRef.current) {
        setPolicy(p);
      }
    } catch (err) {
      if (!cancelledRef.current) {
        setError(err instanceof Error ? err.message : "Failed to load policy");
      }
    } finally {
      if (!cancelledRef.current) {
        setLoading(false);
      }
    }
  }, [id]);

  useEffect(() => {
    if (!id) return;
    void load();
    return () => {
      cancelledRef.current = true;
    };
  }, [load, id]);

  return { policy, loading, error, refetch: load };
}
