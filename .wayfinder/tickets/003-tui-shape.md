---
title: What does the picopilot TUI look like?
status: closed
type: prototype
assignee: GitHub Copilot
blocked_by: []
resolution: ../resolutions/003-tui-shape.md
---

## Question

Raise the fidelity of the TUI discussion with a cheap, rough, concrete
artifact (mock screens or a stubbed layout) covering, at minimum:

- The main chat pane: how streamed deltas, tool calls, and tool results are
  rendered as they arrive.
- The session-history picker: how a user browses and selects a previous
  session to resume.
- A status/cost bar surfacing token/cost usage (settled in the map's Notes).
- Where a tool-approval confirmation appears (modal overlay, inline prompt,
  status-line prompt) — enough for the permission-policy ticket to reference
  a concrete interaction, without deciding that ticket's policy itself.

Link the resulting artifact from this ticket.
