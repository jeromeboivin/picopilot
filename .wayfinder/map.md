---
label: wayfinder:map
title: Design picopilot — a minimalist Rust CLI coding agent on the GitHub Copilot SDK
---

## Destination

A build-ready design spec for **picopilot**: a minimalist Rust CLI coding agent
built on the GitHub Copilot Rust SDK (`github-copilot-sdk`), covering
architecture, the tool/permission model, and the TUI shape — decided well
enough to hand to a fresh implementation session. No code is produced by this
map; "assess the necessary development" means charting what needs deciding,
not building it. No benchmarking/evaluation scorecard is in scope.

## Notes

- **Domain**: picopilot wraps the `copilot` CLI process via JSON-RPC
  (`github/copilot-sdk`, `rust/` crate). The CLI owns the actual agent loop
  and native tools (shell, read/edit/write, grep/glob, web_fetch/web_search,
  task/fleet sub-agent delegation) — picopilot never reimplements these, only
  curates and permissions them via `available_tools`/`excluded_tools`, a
  hardcoded `PermissionHandler`, and (only if ever needed) `ToolHandler`/
  `with_tools`.
- **Standing preferences**: minimalist harness; cost/token efficiency is the
  north star; full control over which native tools are exposed; no skills
  support; no MCP compatibility; no picopilot-owned config file (flags/env
  only).
- **Settled architecture** (fixed by charting; redraw the destination before
  reopening any of these):
  - Interactive full-screen TUI is the primary UX; a one-shot mode is a later
    thin wrapper, not v1.
  - Default working mode is autopilot-style (keeps going until
    `task_complete` or nothing is left to do).
  - Streams assistant/reasoning deltas live.
  - Ships as a single self-contained binary (`bundled-cli` feature) — no
    separate `copilot` CLI install required.
  - Session resume is in scope; the TUI must let the user list history and
    pick up a previous conversation.
  - Surfaces token/cost usage to the user: a thin always-on status bar
    (model, mode, live token count, running cost) plus an on-demand detail
    modal (`u` to toggle, same convention as the session picker) matching
    VS Code's own context-usage panel. Backed by `session.usage.getMetrics()`
    for session-wide cost (`totalNanoAiu`), the `session.usage_info` stream
    event for the live token/limit gauge, and the experimental
    `session.metadata.getContextAttribution()` for the per-category
    breakdown (system instructions/tool definitions/messages/tool results) —
    prototyped at [prototypes/tui-shape](../prototypes/tui-shape).
  - Native tool set for v1: shell, file read/edit/write, grep/glob, and
    task/sub-agent delegation (single `task` calls and Fleet mode's
    `fleet.start` for parallel sub-agents, falling back to single-delegation
    silently if Fleet is unavailable). Excluded: `web_fetch`, `web_search`.
  - No custom tools (`ToolHandler`) in v1 — "full control over tools" is
    satisfied by curating the native set plus the permission policy, not by
    adding new tools.
  - Permission policy is hardcoded: `read`/`grep`/`glob`/`edit`-`write`
    auto-approve (write confined to the workspace root); `shell` and
    sub-agent delegation always confirm, with a per-category "trust for the
    rest of this session" affordance. picopilot owns a small per-session
    sidecar file for that trust state (`~/.picopilot/sessions/{session_id}.json`)
    — this is app-managed runtime state, not the ruled-out user-facing config
    file.
- **Skills per ticket**: grilling + domain-modeling by default; research
  tickets call the research skill; the prototype ticket calls the prototype
  skill.

## Decisions so far

- [What facts govern listing and resuming past picopilot sessions?](tickets/001-session-resume-facts.md): `Client::list_sessions` (RPC `session.list`) returns `SessionMetadata{session_id, start_time, modified_time, summary, is_remote}` — no working dir/last-message, so the picker needs a follow-up events fetch per selection. `ResumeSessionConfig::new(session_id)` is the only required input; no distinct "stale session" error exists. Session state lives at `~/.copilot/session-state/{sessionId}/`.
- [Which default Copilot system-message sections are safe to trim for token savings?](tickets/002-system-message-trim-candidates.md): 12 documented section IDs exist, none dedicated to skills/MCP; best trim candidates are `guidelines`, `custom_instructions` (removable via `SystemMessageConfig` "customize" mode), and shrinking `tone`/`runtime_instructions` via `SystemMessageTransform` rewrites; `safety`, `tool_efficiency`, `last_instructions` are load-bearing. Verify savings via `session.metadata.contextInfo`.
- [What does the picopilot TUI look like?](tickets/003-tui-shape.md): single-column overlay skeleton (full-width chat, thin top status/cost bar, pinned single-line input); session-history picker is a full-screen modal opened on demand; tool-approval renders as an inline banner in the chat stream, in place, not a modal or a side log. A later addendum added an on-demand context/cost detail modal (`u` toggle), matching VS Code's usage panel. Prototype at [prototypes/tui-shape](../prototypes/tui-shape).
- [What is the hardcoded permission policy, and how does tool-approval surface in the TUI?](tickets/004-permission-policy-and-confirmation-ux.md): `read`/`grep`/`glob`/`edit`-`write` auto-approve (write confined to the workspace root); `shell` and sub-agent delegation always confirm via an input-box-hijacking y/n/a prompt. Denial lets the agent continue; `a` grants per-category trust for the rest of the session, cascading to a delegated sub-agent's own calls. Trust survives resume via a picopilot-owned sidecar file keyed by `session_id` (`~/.picopilot/sessions/{session_id}.json`) since the SDK has no supported custom-metadata mechanism.
- [Should picopilot's sub-agent tool be single-delegation only, or full Fleet mode?](tickets/005-sub-agent-tool-scope.md): Fleet mode is in scope alongside single `task` delegation. Concurrent sub-agents render as tagged inline lines in the same single-column chat stream (no new pane). One `fleet.start` confirmation cascades trust to every agent it spawns. No hardcoded concurrency/depth cap. Fleet mode has no pre-call capability check, so picopilot calls it, checks the `started` result, and falls back to single-delegation silently if unsupported.
- [How does a user choose/change the model, given no config file?](tickets/006-model-selection-ux.md): mid-session switching is in scope via `session.set_model` (a clean swap, history preserved), surfaced as a full-screen modal picker listing every model `models.list` returns (unconstrained, no curated allow-list) with cost/context metadata. picopilot never hardcodes a default model; a `--model` flag is validated against `models.list` before session creation.
- [How should picopilot behave when the CLI transport/process dies mid-session?](tickets/007-transport-failure-recovery.md): auto-restart the `Client` and auto-resume via `ResumeSessionConfig`, bounded retries with backoff, reconnecting shown as an inline banner that hijacks input like a confirmation prompt. Verifies the resumed session's identity and fails loudly on mismatch rather than silently continuing in a new session (guards the gap found in ticket 001). An in-flight tool call at failure time is treated as unknown, not assumed successful or failed. Exhausted retries exit with a clear error.
- [How should picopilot be packaged and published beyond the bundled binary?](tickets/008-packaging-and-publishing.md): no crates.io release for v1; install via `git clone` + `cargo build`/`cargo install --path .`, no prebuilt binaries. Depend on `github-copilot-sdk` with a normal semver range plus a committed `Cargo.lock` (not a hard version pin) — the SDK's own build.rs already keeps the bundled CLI in lockstep. No self-update.
- [What are picopilot's general TUI error/status conventions?](tickets/009-tui-error-status-conventions.md): startup validation errors go to stderr before the TUI launches. `session.warning` and recoverable `session.error` render as inline chat-stream banners (same family as approval/transport banners); non-recoverable `session.error` is a blocking final message. `session.info` is ignored in the UI (its `context_window` variant is superseded by the usage modal). All banners are transient, no persistent error log.
- [Should the TUI expose Fleet mode's todo-coordination table?](tickets/010-fleet-todo-visibility.md): yes — an on-demand modal (same convention as the usage/session/model modals) backed by `session.plan.readSqlTodosWithDependencies()`, showing a flat list with dependency annotations, live-refreshed via `session.todos_changed` while open. Reachable only while a Fleet is running; the toggle is silently ignored otherwise.
- [Should picopilot expose per-model SetModelOptions?](tickets/011-per-model-set-model-options.md): yes for reasoning effort (the SDK's main cost/latency lever) and contextTier (when the model supports it), set as extra fields in the existing model-picker modal plus startup flags. reasoningSummary and modelCapabilities overrides are out of scope for v1.

## Not yet specified

(none — all fog graduated into tickets 008-011)

## Out of scope

(none yet)
