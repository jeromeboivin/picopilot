## Resolution

Built a throwaway ratatui/crossterm prototype at
[prototypes/tui-shape](../../prototypes/tui-shape) with three structurally
different layouts (fake transcript, fake session list, one simulated pending
tool call), switchable live with ←/→, `p` (picker), `a` (approval), `q`
(quit). Ran it and walked the owner through all three; the answer landed on
**Variant A — Single-column overlay** across every axis, unanimously:

- **Skeleton**: one full-width chat column. Top bar is a thin, full-width
  status strip (model, mode, token count, running cost). Input is a
  single-line box pinned at the bottom. Nothing else competes for width —
  no permanent sidebar, no split pane.
- **Session-history picker**: a full-screen modal list, opened on demand
  (prototype used Ctrl+O as the placeholder binding) rather than a
  permanent sidebar or a fuzzy command palette. Per
  [session-resume-facts](001-session-resume-facts.md), populate it from
  `Client::list_sessions` and lazily fetch events per highlighted entry for
  a preview, since `SessionMetadata` alone has no last-message field.
- **Tool-approval surface**: an **inline banner rendered in the chat
  stream itself**, at the point the tool call happened — not a centered
  modal, not a side log. The transcript keeps flowing; the pending call is
  just the last (highlighted/bold) line until answered. This fixes *where*
  the prompt appears; it does **not** decide the approval policy or which
  keys resolve it (once/always/deny) — that's
  [permission-policy-and-confirmation-ux](004-permission-policy-and-confirmation-ux.md).
- **Status/cost bar**: full-width top bar, as prototyped, not a corner
  strip.

### What this does not decide

- The hardcoded permission policy and exact approval keybindings —
  ticket 004.
- Single-delegation vs. full Fleet mode for the sub-agent tool, and how a
  delegated sub-agent's activity renders inside this single-column view —
  ticket 005.
- The exact keybinding to open the session picker, and general TUI
  error/status conventions — still fog (see map's *Not yet specified*).

### Prototype disposition

No git repo exists yet in this workspace, so the usual "capture on a
throwaway branch" step is adapted: the prototype stays in place at
`prototypes/tui-shape/` (clearly named, `publish = false`, not wired into
any future picopilot workspace member list) as the primary source for this
decision, rather than living on a branch. It should NOT be folded into the
real implementation as-is — it was written under prototype constraints (no
tests, no error handling, fake data) — only the decisions above should be
carried forward.

### Addendum: context/cost usage detail

Extended the same prototype with a `u`-key toggle opening a centered modal
(same convention as the session picker) that shows a detailed token/cost
breakdown, matching VS Code's own context-usage panel: session cost,
context-window gauge (used/limit tokens), and a per-category percentage
breakdown (system instructions, tool definitions, messages, tool results).
This settles the *shape* (on-demand modal, not a permanent part of the thin
status bar) for the "surfaces token/cost usage" bullet; see the map's Notes
for which SDK APIs back each figure.
