---
type: design-decision
title: Permission policy
description: >
  Hardcoded tool-approval model: auto-approve reads and workspace-confined writes,
  always confirm shell and sub-agent delegation, with per-session trust and
  canonical path resolution.
tags: [picopilot, permissions, security, workspace-confinement]
status: verified
sources:
  - .wayfinder/resolutions/004-permission-policy-and-confirmation-ux.md
  - src/permissions.rs
  - session b8030d13 (implementation)
  - session 7028bf67 (audit fix commit 2430746)
generated: "2026-08-31T17:28:00Z"
---

# Permission policy

## Auto-approve rules

- **Reads** (`view`, `grep`, `glob`): always approved, no prompt.
- **Writes** (`edit`, `create`): auto-approved **only** when the target path is
  inside the workspace root, verified via filesystem canonicalization of the
  nearest existing ancestor (not lexical `..` normalization).

## Always-confirm rules

- **Shell** (`bash`): always prompts `y/n/a`.
- **Sub-agent delegation** (`task`, Fleet): always prompts `y/n/a`.

## External writes

- Writes **outside** the workspace root are presented for user confirmation
  (not silently denied). External writes are **never** eligible for session
  trust — each one must be explicitly approved.

## Trust model

- Pressing `a` grants per-category trust for the rest of the session.
- Trust cascades to delegated sub-agents' own calls.
- Trust survives resume via a picopilot-owned sidecar file:
  `~/.picopilot/sessions/{session_id}.json`.
- Denial lets the agent continue (it can try alternatives).

## Approval UX

- Approval renders as an **inline banner** in the chat stream that hijacks
  the input line.
- `y` = approve once, `n` = deny once, `a` = trust this category for the session.
- Approval requests and their outcomes are persisted inline in the transcript.
- Press-only guard (`KeyEventKind::Press`) avoids double-fire on Windows terminals.

## Symlink / junction protection

- Path containment uses `std::fs::canonicalize` on the nearest existing ancestor
  directory, so symlinks and Windows junctions targeting outside the workspace
  are correctly classified as external.
