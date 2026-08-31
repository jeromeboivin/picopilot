---
type: design-decision
title: System message elimination
description: >
  picopilot uses mode="replace" with empty content to achieve zero system-prompt
  tokens. The previous "customize" approach and PicopilotSystemMessageTransform
  have been removed entirely.
tags: [picopilot, system-message, tokens, cost-efficiency]
status: verified
sources:
  - .wayfinder/resolutions/002-system-message-trim-candidates.md
  - src/config.rs
  - session 7028bf67 (fix commit bead54d)
  - session 73ea30cc (prompt pollution elimination)
generated: "2026-08-31T21:30:00Z"
---

# System message elimination

## Current approach (strict empty replacement)

picopilot now uses `SystemMessageConfig::new().with_mode("replace").with_content("")`
on both session creation and resume. This yields **zero system-prompt tokens**.

The previous `PicopilotSystemMessageTransform` struct and `system_message_transform()`
function have been removed entirely. No section overrides or transforms remain.

## Previous approach (superseded)

The original design used `customize` mode which preserved the SDK's default
system prompt and only removed `guidelines` and `custom_instructions` sections,
while replacing `tone` with `"Be concise, direct, and professional."`. This
still resulted in ~4,699 system tokens — the entire Copilot identity, safety,
runtime, environment, and tool-use instructions were preserved.

## SDK semantics (verified for 1.0.13-preview.2)

- `SystemMessageConfig` has fields: `mode`, `content`, `section_overrides`,
  `sections_to_remove`, `transforms`.
- `mode = "replace"` with `content = ""` replaces the entire system prompt
  with nothing.
- `mode = "customize"` preserves the default prompt and allows selective
  section modifications.
- The empty replacement is applied consistently during create, resume, and
  transport recovery.

## Verification

The live context-budget regression test (`tests/context_budget.rs`) asserts
`system_prompt == 0` and `custom_instructions == 0` after session creation,
toolset reconfiguration, and resume. Run with:

```
PICOPILOT_CONTEXT_BUDGET_E2E=1 cargo test --test context_budget -- --ignored --nocapture
```
