# fix/feat: claims retry, polling timeout, wallet errors, policy timeline

## Summary

- **#10 Claims retry button** — Extracts the fetch logic in `ClaimsPage` into a named `loadClaims` callback that can be called on mount and on demand. Adds a "Try again" button inside the error banner with a spinner and disabled state while the retry is in flight; on success the error clears and the table renders.
- **#12 Claim polling timeout** — Adds a `'timeout'` variant to `ClaimStep` in `useClaim`. When the 20-iteration poll exhausts without a terminal status, `step` is set to `'timeout'` instead of the misleading `'done'`. The policy detail page handles this step by showing a yellow advisory message and a direct link to `/claims`.
- **#13 Wallet connection errors** — `WalletButton` now reads `error` from `WalletContext` and renders it as a `text-xs text-red-400` line beneath the connect button when the wallet is not connected and not connecting. The error clears automatically on the next attempt because `connect()` already calls `setError(null)`.
- **#11 PolicyStatusTimeline integration** — `PolicyStatusTimeline` is imported and rendered on `/policies/[id]` between the coverage stats grid and the oracle widget. No new API calls are needed; the component receives the policy object already fetched by `usePolicy`.

## Files changed

| File | Issue |
|---|---|
| `src/components/WalletButton.tsx` | #13 |
| `src/app/claims/page.tsx` | #10 |
| `src/hooks/useClaim.ts` | #12 |
| `src/app/policies/[id]/page.tsx` | #11, #12 |

## Test plan

- [ ] Connect wallet with a failing RPC — verify the error message appears below the button and clears on the next click.
- [ ] Navigate to `/claims` with a simulated network failure — verify the "Try again" button appears, shows a spinner while retrying, and renders the table on success.
- [ ] Submit a claim and let the polling loop expire (mock 20 iterations) — verify step transitions to `'timeout'` and the yellow advisory with the `/claims` link appears on the policy detail page.
- [ ] Visit a policy detail page — verify the `PolicyStatusTimeline` renders between the stats grid and the oracle widget for both Active and terminal-status policies.
