# Security Policy

MergeFi takes the security of its smart contracts and decentralized protocols seriously. We appreciate the responsible disclosure of security vulnerabilities by security researchers, contributors, and users.

---

## Supported Versions

Only the latest deployed releases and contract versions on the `main` branch are actively supported for security updates:

| Version / Target | Supported          |
| ---------------- | ------------------ |
| Mainnet Releases | :white_check_mark: |
| Testnet Deployments | :white_check_mark: |
| Development (`main`) | :white_check_mark: |
| Older releases   | :x:                |

---

## Reporting a Vulnerability

If you discover a potential security vulnerability or exploit vector in MergeFi smart contracts or infrastructure:

1. **Do not create a public GitHub issue.**
2. Email your findings directly to the security team at **security@mergefi.org** (or reach out via GitHub Security Advisories if enabled).
3. Include detailed information to help us triage and resolve the issue quickly:
   - **Type of vulnerability** (e.g. reentrancy, unauthorized escrow drain, access control bypass, state griefing).
   - **Steps to reproduce** or a proof-of-concept (PoC) test case against Soroban test environment.
   - **Affected contract(s)** and function(s).
   - **Potential impact** and suggested mitigation.

---

## Response & Disclosure Process

- **Acknowledgment:** We will acknowledge receipt of your vulnerability report within **48 hours**.
- **Assessment:** Our engineering and security team will assess the severity and impact within **5 business days**.
- **Fix & Deployment:** We will develop, test, and deploy a fix or contract upgrade before coordinating public disclosure.
- **Credit & Bounty:** Valid, non-publicly disclosed critical vulnerabilities affecting user/escrow funds are eligible for recognition and bug bounty rewards.

Thank you for helping keep MergeFi and the Stellar ecosystem safe!
