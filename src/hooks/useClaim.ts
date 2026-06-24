'use client';

import { useState, useCallback } from 'react';
import { submitClaim, fetchClaim } from '@/lib/api';
import type { Claim } from '@/types';
import { toUserMessage } from '@/lib/errors';

type ClaimStep = 'idle' | 'submitting' | 'polling' | 'done' | 'timeout' | 'error';

export function useClaim() {
  const [step,    setStep]    = useState<ClaimStep>('idle');
  const [claimId, setClaimId] = useState<string | null>(null);
  const [claim,   setClaim]   = useState<Claim | null>(null);
  const [error,   setError]   = useState<string | null>(null);

  const submit = useCallback(async (claimant: string, policyId: string) => {
    setStep('submitting');
    setError(null);
    try {
      const id = await submitClaim(claimant, policyId);
      setClaimId(id);
      setStep('polling');

      // Poll up to 20 times with 3-second intervals
      for (let i = 0; i < 20; i++) {
        await new Promise((r) => setTimeout(r, 3000));
        const result = await fetchClaim(id);
        if (result) {
          setClaim(result);
          if (result.status === 'Paid' || result.status === 'Rejected') {
            setStep('done');
            return;
          }
        }
      }
      setStep('timeout');
    } catch (err) {
      setError(toUserMessage(err));
      setStep('error');
    }
  }, []);

  const reset = useCallback(() => {
    setStep('idle');
    setClaimId(null);
    setClaim(null);
    setError(null);
  }, []);

  return { step, claimId, claim, error, submit, reset };
}
