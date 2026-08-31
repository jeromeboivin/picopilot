## Resolution

Confirmed via research that Fleet's coordination state is a real, documented,
stable client-facing surface — not hidden internal state:
`session.plan.readSqlTodosWithDependencies()` (rows: `id`/`title`/
`description`/`status`/`createdAt`, plus `todoId`/`dependsOn` dependency
edges) and a `session.todos_changed` signal event designed for client-side
tracking. This changes the answer from "maybe not worth building" (the fog's
premise) to a straightforward yes:

- **Add a dedicated todo-list view**: an on-demand modal, same convention as
  the usage-detail modal (ticket 003's addendum) and the session/model
  pickers, not a permanent part of the layout.
- **Content**: a flat list of todos (title + status), with a short
  dependency annotation per row (e.g. "blocked by: alpha") rather than a
  full tree rendering — kept simple for v1.
- **Availability**: only reachable while a Fleet is actually running. The
  toggle key is silently ignored when no Fleet is active — no status-bar
  hint, by explicit choice (diverges from the error/status-conventions
  ticket's general preference for feedback, but this is a narrower,
  deliberately-chosen exception, not a reversal of that decision).
- **Refresh**: live-refreshes while the modal is open, re-querying
  `readSqlTodosWithDependencies()` on each `session.todos_changed` signal
  (debounced). No background polling while the modal is closed — fetch
  once, fresh, on open.

This still doesn't decide the exact toggle keybinding — an implementation
detail, same as the session picker's placeholder `Ctrl+O` binding in ticket
003.

### What this does not decide

- The inline tagged-agent-line rendering for concurrent sub-agents in the
  main chat stream — already settled in ticket 005; unaffected by this.
