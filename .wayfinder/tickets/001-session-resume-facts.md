---
title: What facts govern listing and resuming past picopilot sessions?
status: closed
type: research
assignee: GitHub Copilot
blocked_by: []
resolution: ../resolutions/001-session-resume-facts.md
---

## Question

The TUI must let a user list session history and pick up a previous
conversation (settled in the map's Notes). Investigate, against the
`github/copilot-sdk` primary sources (Rust SDK docs/source, CLI docs on
session persistence, the `~/.copilot/session-state` directory convention, and
the generated RPC schema's `sessions` namespace):

- Is there an RPC (e.g. `sessions.list`) that enumerates resumable sessions,
  or must picopilot scan `~/.copilot/session-state` itself?
- What metadata is available per session for a picker UI (title/summary,
  timestamp, working directory, last message)?
- What does `Client::resume_session` require as input, and what are its
  failure modes (e.g. stale session, CLI version mismatch)?
- How does `enable_session_store` / cross-session search interact with plain
  resume — is it required, optional, or irrelevant to listing history?

Write findings to `.wayfinder/research/session-resume-facts.md`, citing
primary sources for each claim.
