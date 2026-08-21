# Security Policy

## Reporting a Vulnerability

The MergeFi team takes the security of our smart contracts and funds custody seriously. If you discover a vulnerability or potential exploit, please report it responsibly so we can address it before disclosure.

### Private Disclosure

Please report security issues directly via:
- GitHub Security Advisories: Submit a private advisory via the Security tab on the repository.
- Email: Send vulnerability details to `security@mergefi.org` (or contact maintainers directly).

Please **do not** open a public GitHub issue for undisclosed vulnerabilities or fund-draining exploits.

### Scope

- `contracts/escrow`: Escrow lifecycle, fund custody, deadline handling, release, and refund logic.
- `contracts/milestones`: Milestone lifecycle, issue allocations, release, and crowdfunding refund logic.
- `contracts/maintenance-pool`: Maintenance pool deposits, withdrawals, and fee calculations.
- `contracts/common`: Shared administration, TTL extension, and access control helpers.

### Response Timeline

- Initial response & acknowledgment: Within 24-48 hours.
- Status update and triage assessment: Within 5 business days.
