---
title: Should picopilot's sub-agent tool be single-delegation only, or full Fleet mode?
status: closed
type: grilling
assignee: GitHub Copilot
blocked_by: [003-tui-shape]
resolution: ../resolutions/005-sub-agent-tool-scope.md
---

## Question

The map keeps task/sub-agent delegation in v1's native tool set. Decide,
using the TUI-shape prototype's established conventions:

- Is single-delegation (`task` tool, one sub-agent at a time) sufficient, or
  does v1 need full Fleet mode (parallel sub-agents via the `task` tool with
  SQL-todo-style coordination)?
- If Fleet mode is in scope, how does the TUI represent multiple concurrent
  sub-agent streams without abandoning the "minimalist" destination?
- If out of scope, is Fleet mode fog for a later effort, or explicitly ruled
  out of scope for picopilot entirely?
