# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| `main`  | Yes       |

Older tagged releases are not actively patched. Please upgrade to the latest commit on `main` before reporting a bug.

## Reporting a vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Send a report by email to the maintainers listed in [CODEOWNERS](.github/CODEOWNERS) or use GitHub's private [Security Advisory](../../security/advisories/new) feature.

Include:

- A clear description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept (PoC)
- The affected contract(s) and function(s)
- Any suggested mitigation

We will acknowledge receipt within **48 hours** and aim to provide a resolution timeline within **7 days**.

## Scope

This policy covers all Soroban smart contracts under the `contracts/` directory of this repository.

Off-chain tooling (`scripts/`, `bindings/`) is in scope for disclosure but carries lower severity by default.
