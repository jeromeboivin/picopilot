## Resolution

Grilled sub-agent tool scope over two rounds plus two inline fact-finding
dispatches (Fleet mode mechanics; Fleet capability detection).

### Scope

**Fleet mode is in scope for v1** — not just single-delegation. picopilot
uses both:
- `task`: single sub-agent, synchronous, parent blocks for its report.
- `session.rpc.fleet.start`: parallel sub-agents coordinated by the
  runtime's own SQL todo state machine (opaque to picopilot — it doesn't
  manage that table itself, just consumes the resulting events).

### TUI rendering

Fits inside the single-column overlay from
[tui-shape](003-tui-shape.md) without reopening that shape: each
sub-agent's `subagent.started`/`completed`/`failed` event renders as its
own **tagged inline line in the same chat stream** tool calls already
use (e.g. `agent 2 ▶ started: <title>`), interleaved by arrival time
across every concurrently-running agent. No side pane, no split view, no
per-agent sub-window.

### Trust cascade

Extends [permission-policy-and-confirmation-ux](004-permission-policy-and-confirmation-ux.md)'s
cascade rule: **one confirmation on `fleet.start`** (categorized as
sub-agent delegation, so it always confirms per that ticket) covers every
sub-agent Fleet spawns for the rest of that run — not a separate
confirmation per spawned agent.

### Concurrency/depth caps

**No hardcoded cap.** `maxConcurrency`/`maxDepth` are left at whatever the
SDK/account defaults are; picopilot doesn't set them.

### Experimental-API handling

Fleet mode is marked experimental in every SDK language, and — per
research — **there is no pre-call capability check**: no `capabilities`
field, no version gate, no dedicated "not supported" error variant. The
only signal is the `started: bool` field `fleet.start()` itself returns.

Decision: picopilot calls `fleet.start()` and checks `started`. If
`false`, it **falls back to single-delegation `task` calls for that run,
silently** — no status message telling the user Fleet mode wasn't
available. The user just sees sub-agents run one at a time instead of in
parallel.

### What this does not decide

- Whether the user can ever see or interact with the Fleet's underlying
  todo coordination table (a richer Fleet UI) — not needed to ship inline
  tagged events, and not sharp enough to ticket yet; left as fog (see
  map's *Not yet specified*).
