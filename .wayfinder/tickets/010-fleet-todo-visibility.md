---
title: Should the TUI expose Fleet mode's todo-coordination table?
status: closed
type: grilling
assignee: GitHub Copilot
blocked_by: []
resolution: ../resolutions/010-fleet-todo-visibility.md
---

## Question

Ticket 005 settled that concurrent Fleet sub-agents render as tagged inline
lines in the single-column chat stream, and left richer Fleet UI as fog.
Decide:

- Does the SDK actually expose a structured todo/coordination view for a
  running Fleet (a queryable list of sub-tasks and their status), or is the
  inline event stream the only signal available? (Needs a research dispatch
  to confirm before deciding the UX.)
- If such a structure exists, is it worth a dedicated view (e.g. a
  toggleable panel/modal, same convention as the usage-detail modal from
  ticket 003's addendum), or does the inline tagged-line rendering already
  cover it well enough for v1?
- If out of scope for v1, is this something to explicitly rule out-of-scope
  on the map, or leave as post-v1 fog?
