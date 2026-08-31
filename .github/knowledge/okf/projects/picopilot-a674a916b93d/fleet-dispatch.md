---
type: design-decision
title: Fleet dispatch design
description: >
  Explicit `/fleet <task>` command for parallel sub-agent orchestration vs
  normal session.send for ordinary messages. Includes silent fallback to
  single task agent delegation.
tags: [picopilot, fleet, sub-agents, dispatch]
status: verified
sources:
  - .wayfinder/resolutions/005-sub-agent-tool-scope.md
  - src/tui.rs
  - src/runtime.rs
  - session 7028bf67 (fix commit f7718ce)
generated: "2026-08-31T17:28:00Z"
---

# Fleet dispatch design

## Routing rule

- **Ordinary messages** (including greetings like "Hi"): go through
  `session.send(prompt)` — never through Fleet.
- **Explicit Fleet commands** (`/fleet <task>`): invoke `fleet.start` for
  parallel sub-agent orchestration.

This separation was established after a live regression where every message
was unconditionally routed through Fleet, causing a simple "Hi" to spawn a
coordinator and sub-agent with todo/SQL coordination noise.

## Fallback behavior

When `fleet.start` returns `started: false` or errors:

1. A single `general-purpose` task agent is started via
   `session.tasks.startAgent` with the original prompt.
2. The Fleet error is **silently swallowed** — the user sees only the
   delegation result, not the Fleet failure.
3. No duplicate prompt is sent: the original prompt goes to exactly one path.

## Transport errors during Fleet

Transport errors during `fleet.start` bypass the silent fallback and enter
the bounded reconnect/recovery loop instead, since the connection itself is
broken.

## Todo visibility

Fleet todo state is visible via an on-demand modal (same convention as
usage/session/model modals), backed by `session.plan.readSqlTodosWithDependencies()`.
The modal is reachable only while a Fleet is running; the toggle is silently
ignored otherwise. `session.todos_changed` events refresh an already-open
modal without polling when closed.
