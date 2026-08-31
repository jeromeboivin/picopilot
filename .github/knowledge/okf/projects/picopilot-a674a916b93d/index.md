# picopilot — Project Knowledge

Project ID: `picopilot-a674a916b93d`
Repository: `c:\dev\picopilot`

A minimalist Rust TUI coding agent built on the GitHub Copilot SDK.

## Concepts

- [Architecture](architecture.md) — Tech stack, module layout, SDK dependency, and packaging.
- [SDK API surface](sdk-api-surface.md) — Verified public Rust signatures from github-copilot-sdk 1.0.13-preview.2.
- [Permission policy](permission-policy.md) — Hardcoded tool-approval model and workspace confinement.
- [Fleet dispatch design](fleet-dispatch.md) — Explicit `/fleet` command vs normal `session.send`.
- [Transport recovery](transport-recovery.md) — Auto-restart, identity verification, and steering semantics.
- [TUI conventions](tui-conventions.md) — Ctrl+key shortcuts, nano-style command bar, modals, error/status rendering.
- [System message trimming](system-message-trimming.md) — Which Copilot system-message sections are removed or rewritten.
- [Model selection](model-selection.md) — Unconstrained picker, per-model options, cost-focused UX.
- [Known gaps](known-gaps.md) — Audit-discovered residual gaps and their fix status.
- [Development workflow](development-workflow.md) — TDD commit-per-step preference and validation gates.
