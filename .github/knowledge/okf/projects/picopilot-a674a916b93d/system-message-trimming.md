---
type: design-decision
title: System message trimming
description: >
  Which Copilot system-message sections picopilot removes or rewrites for
  token savings, and which are load-bearing and must be preserved.
tags: [picopilot, system-message, tokens, cost-efficiency]
status: verified
sources:
  - .wayfinder/resolutions/002-system-message-trim-candidates.md
  - src/config.rs
  - session 7028bf67 (fix commit bead54d)
generated: "2026-08-31T17:28:00Z"
---

# System message trimming

## Removed sections (via `SystemMessageConfig` "customize" mode)

| Section ID            | Reason |
|-----------------------|--------|
| `guidelines`          | Generic style guidance; picopilot has its own UX. |
| `custom_instructions` | Not applicable; no user-facing config file. |

## Rewritten sections (via `SystemMessageTransform`)

| Section ID | Replacement |
|------------|-------------|
| `tone`     | `"Be concise, direct, and professional."` |

## Preserved (load-bearing)

The following sections are **never** removed or rewritten:

- `safety` — agent safety constraints
- `tool_efficiency` — tool-usage optimization guidance
- `last_instructions` — final prompt-completion anchor
- `runtime_instructions` — **critical**: contains Fleet-specific operational
  guidance and tool-usage context. Initially replaced entirely, which caused
  Fleet to claim it lacked the right tools. Now fully preserved.

## Verification

Token savings can be measured via `session.metadata.contextInfo` after session
creation. The transform is applied on both session creation and session resume.
