---
type: verified-fact
title: Known implementation gaps
description: >
  Audit-discovered residual gaps in the picopilot v1 implementation that were
  identified but not yet fixed. Tracked here to inform future sessions.
tags: [picopilot, gaps, audit, v1]
status: verified
sources:
  - session 7028bf67 (audit report)
  - src/tui.rs
  - src/runtime.rs
generated: "2026-08-31T18:04:00Z"
---

# Known implementation gaps

These gaps were discovered during the post-implementation audit. Some have
been fixed in subsequent commits; remaining items are accepted v1 limitations.

## ~~Status bar cost shows `--` until usage modal is opened~~ (Fixed)

Fixed in commit `3d03b16`: cost now auto-refreshes via `refresh_status_cost`
after each turn completion, without requiring the usage modal.

## ~~Model picker omits billing and context-window metadata~~ (Fixed)

Fixed in commit `6d86c8d` + `1409704`: the simplified model picker now shows
billing multiplier and context-window size from `models.list`.

## ~~Resume does not replay the conversation transcript~~ (Fixed)

Fixed: `replace_history()` now clears the prior transcript and walks the
resumed session's events to reconstruct `ChatEntry` rows before subscribing
to new events. A diagnostic "session resumed" entry is added (debug-only).

## No live integration test for transport recovery

The transport recovery path (kill CLI → auto-restart → resume → verify
identity) has unit tests for each component, but no end-to-end integration
test that actually kills the CLI process mid-session. This is a known
residual risk.
