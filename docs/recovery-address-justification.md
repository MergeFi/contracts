Recovery-Address Pattern — Justification
=====================================

Summary
-------
The contracts adopt an optional, single-use `recovery` address stored at
initialize time. That address is permitted to call a `recover_admin` entrypoint
to appoint a new admin if the original admin key is permanently lost.

Why this exists
----------------
- Threat: admin's private key may be irrecoverably lost (device loss, key
  destruction, personnel churn). Without a recovery mechanism the contract
  could be administratively stuck (no ability to rotate treasury, upgrade
  off-chain coordination, or perform urgent admin-only operations).
- Permissions model: the recovery address has *only* one narrowly-scoped
  capability: to set a new admin via `recover_admin`. It cannot itself move
  funds, change treasury, or perform other privileged operations unless the
  new admin performs them after being installed.

Design decisions and tradeoffs
------------------------------
- Set-once at `initialize`: this minimizes runtime surface area. Allowing the
  recovery address to be changed after initialization increases risk and
  complexity; a set-once value is auditable from the chain and predictable.
- Minimal privilege: the recovery address's only power is to replace the
  admin. This keeps the attack surface small while enabling a practical
  recovery path.
- Recommended recovery owner: treat `recovery` as a custody/backstop
  mechanism controlled by an organizational multisig (or HSM-managed key),
  not an individual. This reduces the risk of single-person compromise.
- Audit and operational controls: operators should record the recovery key's
  holder and rotate off-chain practices (rotate multisig signers, monitor key
  compromise channels). Keep the recovery key under strict operational
  controls and incident procedures.

Alternatives considered
-----------------------
- Permissionless fallback: for some contracts (e.g., escrow after deadline)
  permissionless behavior already exists and may make a recovery address
  unnecessary. Where permissionless fallbacks exist, prefer them over a
  recovery key.
- Epoch-based guardians or timelocked governance: more expressive but also
  more complex. Our goal here is a narrow, auditable, low-risk safeguard.

Recommendations
---------------
1. Keep the `recovery` parameter optional and set-only at initialize.
2. Require the recovery holder to be a secure multisig or managed key.
3. Log and document the recovery-holder identity off-chain.
4. Prefer permissionless fallbacks for flows where they suffice (e.g., final
   refunds after a deadline). Use `recovery` only where permissionless
   behavior cannot provide the necessary operational guarantees.

Appendix: example incident flow
-------------------------------
1. Admin key lost. Operators authenticate to the recovery multisig.
2. Recovery multisig calls `recover_admin(new_admin)` to appoint a new admin.
3. New admin performs any outstanding administrative tasks (rotate
   treasury, finalize releases) and then optionally rotates admin again using
   the normal `set_admin` flow.

This pattern balances availability and risk: it avoids permanent lockout with
minimal additional privileges and operational complexity.
