---
title: How does a user choose/change the model, given no config file?
status: closed
type: grilling
assignee: GitHub Copilot
blocked_by: []
resolution: ../resolutions/006-model-selection-ux.md
---

## Question

Decide the model-selection UX for v1:

- Is a single hardcoded default model enough, overridable only by a startup
  `--model` flag, or should the TUI let the user switch models mid-session
  (`session.set_model`)?
- If switchable mid-session, is the choice unconstrained (any model the CLI
  reports via `models.list`) or a curated allow-list?
