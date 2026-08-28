# MergeFi Design Documents

This directory contains design documents and analyses for the MergeFi contracts.

- **[access-control-audit.md](access-control-audit.md)** — Function-by-function audit of every public entrypoint across the three contracts, comparing documented access levels against actual runtime enforcement.

- **[escrow-crowdfunding-design.md](escrow-crowdfunding-design.md)** — Design for multi-sponsor crowdfunding in the escrow contract, enabling multiple sponsors to co-fund the same issue.

- **[milestones-crowdfunding-design.md](milestones-crowdfunding-design.md)** — Design for multi-sponsor crowdfunding in the milestones contract, enabling proportional contribution tracking and refund.

- **[refund-permissionless-analysis.md](refund-permissionless-analysis.md)** — Analysis of the `refund` function's permissionless-after-deadline path, including economics, griefing vectors, and sponsor control guarantees.
