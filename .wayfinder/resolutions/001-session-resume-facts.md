# Resolution: What facts govern listing and resuming past picopilot sessions?

Findings: [.wayfinder/research/session-resume-facts.md](../research/session-resume-facts.md)

- The Rust SDK exposes `session.list` (wrapped as `Client::list_sessions`,
  filterable by cwd/git_root/repository/branch) returning
  `SessionMetadata { session_id, start_time, modified_time, summary,
  is_remote }` — no working directory or last-message field, so a picker
  needs a follow-up `get_events`/`session.getMessages` call for message
  preview content.
- `ResumeSessionConfig::new(session_id)` only requires the session ID.
  Failure modes: `SessionIdMismatch`, `InvalidConfig` (mutually-exclusive
  GitHub token fields, `ClientMode::Empty` missing `available_tools`),
  `SessionFsProviderRequired`, protocol version-mismatch errors. Resume
  silently starts a new session if the ID isn't found unless
  `suppress_resume_event`/`disableResume` is set — there is no distinct
  "stale session" variant.
- `enable_session_store` is unrelated/optional: it toggles cross-session
  search/retrieval, not single-session resume.
- Session state defaults to `~/.copilot/session-state/{sessionId}/`; docs
  explicitly recommend `session.list` over parsing that directory — no slash
  command or index file is documented for enumeration.

Implication for the TUI-shape ticket (open): the history picker should call
`Client::list_sessions` for the list, then lazily fetch events per selection
for a preview, rather than reading `summary` alone as a full preview.
