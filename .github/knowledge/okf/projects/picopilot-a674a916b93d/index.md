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
- [TUI conventions](tui-conventions.md) — Ctrl-key keyboard model, modals, error/status rendering.
- [System message elimination](system-message-trimming.md) — Zero system-prompt tokens via mode="replace" with empty content.
- [Toolset management](toolset-management.md) — Explicit tool allowlist, Ctrl+K picker, model-aware defaults, context budget testing.
- [Model selection](model-selection.md) — Compact picker, per-model options, cost-focused UX.
- [Local provider support](local-provider-support.md) — BYOK/Ollama additive local model support (implemented).
- [Development workflow](development-workflow.md) — TDD commit-per-step preference, 96+ test validation gates.
- [Known gaps](known-gaps.md) — Residual gaps including live budget test external dependencies.
