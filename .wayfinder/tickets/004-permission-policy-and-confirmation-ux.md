---
title: What is the hardcoded permission policy, and how does tool-approval surface in the TUI?
status: closed
type: grilling
assignee: GitHub Copilot
blocked_by: [003-tui-shape]
resolution: ../resolutions/004-permission-policy-and-confirmation-ux.md
---

## Question

The map settles that tool-approval is a single `PermissionHandler` hardcoded
in Rust (no runtime config). Decide:

- Which native tool categories/subcommands auto-approve (e.g. read, grep,
  glob) versus require live confirmation (e.g. shell, write, sub-agent
  delegation)?
- For tools that require confirmation, what's the actual confirmation
  interaction, using the TUI's established conventions from the TUI-shape
  prototype (modal overlay vs inline prompt vs status-line prompt)?
- Is there a "trust for the rest of this session" affordance, or is every
  risky call confirmed individually?
