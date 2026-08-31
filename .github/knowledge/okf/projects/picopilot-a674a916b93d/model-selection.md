---
type: design-decision
title: Model selection UX
description: >
  Unconstrained model picker with per-model options, compact layout, Ctrl+P
  shortcut, and fail-fast --model flag validation.
tags: [picopilot, models, ux, cost-efficiency]
status: verified
sources:
  - .wayfinder/resolutions/006-model-selection-ux.md
  - .wayfinder/resolutions/011-per-model-set-model-options.md
  - src/tui.rs
  - session 7028bf67 (commits 1409704, 6d86c8d)
generated: "2026-08-31T18:04:00Z"
---

# Model selection

## Design principles

- **Cost/token efficiency is the north star.** The model picker surfaces
  billing multiplier and context-window size from `models.list` metadata to
  support informed cost decisions.
- **Unconstrained list**: every model `models.list` returns for the account
  is shown. picopilot hardcodes no model names, so there is no allow-list
  to maintain.

## Picker UX

- Reachable via **Ctrl+P** (full-screen modal, same convention as session
  and usage modals).
- Compact layout — reworked after user feedback that the original was
  "complicated to use." Each row shows model id/name plus cost tier and
  context-window size.
- Arrow-key navigation, Enter to select, Esc to cancel. No fuzzy search.

## Mid-session switching

`session.set_model(SetModelOptions)` is a clean swap — conversation history,
system message, and compaction state are all preserved.

## Per-model options (v1 scope)

| Option            | In scope | Surface |
|-------------------|----------|---------|
| Reasoning effort  | ✅ Yes   | Picker modal + `--reasoning-effort` flag |
| Context tier      | ✅ Yes   | Picker modal + `--context-tier` flag (shown only when supported) |
| Reasoning summary | ❌ No    | Deferred (no clear need) |
| Model capabilities| ❌ No    | Deferred (no clear need) |

## Default model

picopilot **never sets `model`** at session creation unless the user passed
`--model`. Whatever the CLI/account defaults to is picopilot's default —
one less thing to hardcode or keep current.

## `--model` flag validation

The given value is checked against `models.list` **before** creating the
session. Invalid values exit with a clear error listing valid ids (fail-fast
convention, same as other startup validation).
