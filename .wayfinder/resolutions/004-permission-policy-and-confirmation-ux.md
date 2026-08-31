## Resolution

Grilled the hardcoded permission policy and its TUI surface, across two
rounds plus one inline fact-finding dispatch (not a separate ticket — a
fact needed mid-round, per the grilling skill).

### Policy

- **Auto-approve, no confirmation ever**: `read`, `grep`, `glob`, and
  `edit`/`write` — but `edit`/`write` auto-approval is **confined to the
  workspace root**; a write attempt outside it (`$HOME`, `/etc`, another
  project) always confirms regardless of any trust grant.
- **Always confirm**: `shell` and `task`/sub-agent delegation.
- **Nothing is hardcoded default-deny.** Every native tool call can be
  approved live; there's no picopilot config file to pre-authorize or
  permanently block anything.

### Confirmation interaction

Builds on the inline-banner convention from
[tui-shape](003-tui-shape.md): while a call is pending, the message input
box is **hijacked** — it stops accepting free text and only reads the
approval keys (`y` once, `a` always-this-category, `n` deny, plus a
view-full-command key) until the call resolves. No queueing a next
message behind it.

- **Deny (`n`)**: the agent is told the call was denied and continues the
  turn — it adapts or reports back what it couldn't do. Denial does not
  abort the turn or the session.
- **Trust (`a`)**: grants **per-tool-category** trust ("always allow
  shell" / "always allow sub-agent delegation") for the rest of the
  session, not just the exact command that triggered it.
- **Sub-agent delegation cascades**: approving one delegation call also
  grants the spawned sub-agent's own internal tool calls the same trust —
  no separate re-confirmation chain per sub-agent action. (How this
  interacts with single-delegation vs. full Fleet mode is
  [sub-agent-tool-scope](005-sub-agent-tool-scope.md)'s call, not this
  ticket's.)

### Trust persistence across resume

Settled that a trust grant **should** survive a session resume, which
surfaced a fact question: does the SDK offer any supported place to store
custom per-session data? Dispatched a research sub-agent inline; findings:

- `SessionMetadata` (`session.list`/`session.getMetadata`) has exactly
  five fixed fields (`session_id`, `start_time`, `modified_time`,
  `summary`, `is_remote`) — no custom-data field, and the struct is
  `#[non_exhaustive]` but nothing is documented for host-app use today.
- `session_id` **is stable** across resume/restart/compaction.
- `~/.copilot/session-state/{sessionId}/` is undocumented/internal — not
  a safe place for picopilot to drop its own sidecar files.
- The SDK's own extension point for this is `SessionFsProvider`, but that
  replaces the *entire* session filesystem layer — too invasive for what
  picopilot needs.

Decision: **picopilot owns a small sidecar store**, one JSON file per
session keyed by the SDK's own `session_id` — e.g.
`~/.picopilot/sessions/{session_id}.json` — holding just the trusted
tool categories for that session. This is runtime state picopilot
manages itself, not user-facing configuration, so it doesn't reopen the
"no config file" standing preference. It's the first piece of
picopilot-owned persistent state the map has introduced.

### What this does not decide

- The exact JSON schema of the trust sidecar file, and exact keybindings
  beyond y/n/a — implementation detail, not a design fork.
- Whether sub-agent delegation is single-delegation-only or full Fleet
  mode — [sub-agent-tool-scope](005-sub-agent-tool-scope.md).
