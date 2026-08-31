---
type: design-decision
title: Toolset management
description: >
  Mask-backed Toolset domain type with selectable tool profiles, Ctrl+K picker,
  model-aware defaults, and transactional same-session resume for tool changes.
tags: [picopilot, toolset, tools, context-efficiency, tui]
status: verified
sources:
  - src/toolset.rs
  - src/config.rs
  - src/runtime.rs
  - src/tui.rs
  - session 73ea30cc (prompt pollution elimination)
generated: "2026-08-31T21:30:00Z"
---

# Toolset management

## Problem

Even with zero system-prompt tokens, seven eager built-in tool schemas still
consume ~4,593 tokens (~657 tokens/tool). For local models with small context
windows, this overhead is significant.

## Solution: selectable tool profiles

Users choose which built-in tools are active via `Ctrl+K`. Tool schemas
are included in the request only for selected tools.

## Canonical tools (7)

| Index | Tool (Unix) | Tool (Windows) |
|-------|-------------|----------------|
| 0     | `bash`      | `powershell`   |
| 1     | `view`      | `view`         |
| 2     | `edit`      | `edit`         |
| 3     | `create`    | `create`       |
| 4     | `grep`      | `grep`         |
| 5     | `glob`      | `glob`         |
| 6     | `task`      | `task`         |

`web_fetch` and `web_search` are always excluded.

## Toolset domain type (`src/toolset.rs`)

A `u8` bitmask over the 7 canonical tools with `ToolsetProvenance` tracking:

- **`Default`** — automatic based on model type
- **`User`** (explicit) — manually configured via picker

### Constructors and API

- `empty()`, `all()`, `shell_only()`, `default_for_model(is_local)`,
  `from_tools(iter)`.
- `toggle_at(index)` for picker interaction.
- `available_tools()` returns the selected subset in canonical order.
- `from_tools` rejects unknown tool names with `ToolsetError::UnknownTool`.

### Serialization

`available_tools` is always set explicitly on `SessionConfig` and
`ResumeSessionConfig`. `Some([])` vs omission has distinct SDK semantics:
`Some([])` disables all tools; `None` enables the default set.

## Model-aware defaults

- **Hosted models**: all 7 tools by default.
- **Local models** (provider-qualified IDs): shell-only by default when a
  new conversation starts, to minimize context consumption.
- Before the first message, changing models recomputes the default tool set
  unless the user has manually selected tools. After a conversation has
  history, model changes preserve the current tool selection.
- **Historical resume** with unknown model: bootstraps shell-only, detects
  model via `usage.get_metrics().current_model`, reconnects with all tools
  if the restored model is hosted.

## Picker UX (`Ctrl+K`)

- Full-height modal accessible via **Ctrl+K** while idle.
- `Space` toggles the highlighted tool, `s` selects shell only, `a` selects
  all tools, `Enter` applies, `Esc` cancels.
- Can only apply when the session is **idle** (not streaming, not in an
  approval, not reconnecting). Busy-state attempts show an error.
- The picker can be opened during an approval or reconnect, but applying
  waits until the current operation is finished.
- Status bar shows `tools N/7` count.

## Reconnect on tool change

Application is **transactional**: disconnect the current session, resume
the same session with the new `available_tools` allowlist. On failure,
roll back to the previous toolset. Conversation history is preserved
because `Session::disconnect()` keeps on-disk state.

## SDK constraints

- No `Session::set_available_tools()` — mid-session tool changes are
  impossible.
- Tool changes require `Session::disconnect()` + `Client::resume_session()`
  with the new config.
- The active toolset is preserved across transport recovery.

## Per-model option interaction

- `--reasoning-effort` and `--context-tier` produce clear unsupported-option
  errors when selected with capability-empty local models. Empty capability
  lists render as `none` (not blank).

## Context-budget regression test

An opt-in live test verifies zero system-prompt tokens:

```
PICOPILOT_LIVE_BUDGET=1 cargo test live_budget -- --ignored --nocapture
```

Requires Copilot authentication. With a configured local provider, also
requires a tool-capable local model.
