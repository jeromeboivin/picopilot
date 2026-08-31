---
type: user-preference
title: Development workflow
description: >
  TDD commit-per-step preference and validation gates for picopilot
  development sessions.
tags: [picopilot, tdd, workflow, testing]
status: verified
sources:
  - session b8030d13 (explicit user instruction)
  - session 7028bf67 (applied consistently)
  - session 73ea30cc (96-test validation)
generated: "2026-08-31T21:30:00Z"
---

# Development workflow

## Commit-per-step

The user explicitly requested: **"Commit at each successful step."**

Each implementation slice follows a red → green → format → commit cycle:

1. Write a **focused failing test** at the intended seam.
2. Implement the **smallest code** that makes the test green.
3. Run `cargo fmt` and `cargo clippy`.
4. Commit the passing slice with a descriptive message.
5. Move to the next slice.

Formatting-only changes get their own cleanup commit to keep behavior commits
reviewable.

## Validation gates (full suite)

Before considering a feature complete, run:

```
cargo fmt --all -- --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --locked
```

All 96+ library tests must pass. A live smoke test (start TUI, send a message,
quit via `q`) is the final executable verification.

## Context budget regression test (opt-in)

A live integration test (`tests/context_budget.rs`) asserts zero system-prompt
tokens and enforces tool schema token ceilings. Requires Copilot authentication:

```
PICOPILOT_CONTEXT_BUDGET_E2E=1 cargo test --test context_budget -- --ignored --nocapture
```

With a configured local provider, the test also verifies a real shell tool
call completes. Any nonzero system prompt, tool-budget regression, or lifecycle
mismatch is release-blocking.

## Test organization

- All tests live in `#[cfg(test)] mod tests` blocks within each source file.
- Tests are focused on public API seams, not internal implementation details.
- SDK types are constructed directly in tests (the preview SDK exposes
  constructible public types).
- No mock framework; behavior is tested through typed adapter boundaries.

## Code review preference

Read-only review passes are requested after implementation, focusing on
**correctness bugs and behavioral regressions** — not style-only findings.
Review explicitly checks transport recovery, session identity, prompt
duplication, event subscriptions, Fleet fallback, permission channels, and
startup configuration preservation.
