# Session listing & resume facts — GitHub Copilot Rust SDK (`github-copilot-sdk`)

Sources consulted (primary):

- [`rust/README.md`](https://github.com/github/copilot-sdk/blob/main/rust/README.md)
- [`rust/src/types.rs`](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs)
- [`rust/src/session.rs`](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs)
- [`rust/src/errors.rs`](https://github.com/github/copilot-sdk/blob/main/rust/src/errors.rs)
- [Session resume and persistence (docs.github.com)](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-persistence)
- [Copilot SDK docs index](https://docs.github.com/en/copilot/how-tos/copilot-sdk)

Note: `docs.github.com/en/copilot/how-tos/copilot-sdk/resume-a-session` (the URL suggested in the task) returns HTTP 404. The correct doc page is `.../copilot-sdk/features/session-persistence`, used above.

---

## 1. Is there an RPC that enumerates resumable/past sessions? What does it return?

Yes. The wire method is `session.list` (called at the top-level `Client`, not scoped to a specific session), and the Rust SDK wraps it as `Client::list_sessions`.

- The type doc comment on `SessionMetadata` states explicitly: "Metadata for a persisted session, returned by `session.list`." — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs)
- The response wrapper is:
  ```rust
  /// Response from `session.list`.
  pub struct ListSessionsResponse {
      pub sessions: Vec<SessionMetadata>,
  }
  ```
  — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs)
- A test exercises `Client::list_sessions(Some(filter))`, confirming the Rust entry point takes an `Option<SessionListFilter>` — [rust/tests/session_test.rs](https://github.com/github/copilot-sdk/blob/main/rust/tests/session_test.rs) (matched via code search of the `github/copilot-sdk` repo; the SDK's `session.rs`/`lib.rs` house `Client::list_sessions`, which forwards to the `session.list` RPC).
- Filtering support exists via `SessionListFilter { working_directory (cwd), git_root, repository, branch }`, all optional — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs).
- The docs' cross-language quick-start additionally shows a `client.listSessions(filter?)` call (TypeScript) returning an array of session summaries you can filter e.g. by `{ repository: "owner/repo" }` — [Session resume and persistence](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-persistence).
- The Rust SDK also generates a fully-typed low-level namespace, `client.rpc().sessions()`, exposing every `sessions.*` method (list, fork, etc.) beyond the hand-written helpers — e.g. `client.rpc().sessions().fork(SessionsForkRequest { session_id, to_event_id })` — [README.md, "Typed RPC namespace"](https://github.com/github/copilot-sdk/blob/main/rust/README.md). The README explicitly calls out that new RPCs land in this typed namespace immediately as the schema regenerates, and hand-written convenience wrappers (like `list_sessions`) are layered on top only when useful.
- Related single-session lookup RPCs also exist and are documented in `types.rs`: `session.getMetadata` (→ `GetSessionMetadataResponse { session: Option<SessionMetadata> }`), `session.getLastId` (→ `GetLastSessionIdResponse { session_id: Option<SessionId> }`, "the most recently updated session ID"), and `session.getForeground` (→ `GetForegroundSessionResponse { session_id: Option<SessionId> }`) — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs).

## 2. What metadata is available per session for building a picker UI?

The `SessionMetadata` struct (returned by `session.list`, `session.getMetadata`, and embedded in `session.lifecycle` notifications) has:

```rust
pub struct SessionMetadata {
    pub session_id: SessionId,      // unique identifier
    pub start_time: String,         // ISO 8601 timestamp when the session was created
    pub modified_time: String,      // ISO 8601 timestamp of the last modification
    pub summary: Option<String>,    // agent-generated session summary
    pub is_remote: bool,            // whether the session is running remotely
}
```
— [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs)

Observations relevant to a picker UI:

- **Title/summary**: `summary` is an agent-generated summary (optional — may be absent for brand-new/short sessions). There is no separate "title" field.
- **Timestamp**: both `start_time` (created) and `modified_time` (last activity) are provided as ISO 8601 strings.
- **Session id**: `session_id` (the `SessionId` newtype).
- **Working directory**: **not** part of `SessionMetadata` itself. Instead, working directory / git root / repository / branch are exposed only as *filter* criteria on `SessionListFilter` (`working_directory` aka `cwd`, `git_root`, `repository`, `branch`) — you can filter by them but the list response does not appear to echo them back per-session in `SessionMetadata` — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs).
- **Last message**: not present on `SessionMetadata`. To get the last message/content you need a separate call — `Session::get_events()` / the `session.getMessages` RPC, which returns the full timeline (`GetMessagesResponse { events: Vec<SessionEvent> }`) for one session after resuming or via id — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs), [session.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs).
- The TypeScript quick-start example in the docs additionally references a `createdAt` field name (`session.createdAt`) when iterating `listSessions()` results, consistent with `start_time` in the Rust struct — [Session resume and persistence](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-persistence).
- `SessionLifecycleEventMetadata` (attached to `session.lifecycle` push notifications for created/updated/foreground/background events) carries the same `start_time`/`modified_time`/`summary` triple, so live UI updates can reuse the same fields without re-polling `session.list` — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs).

**Conclusion for sub-question 2:** Only `session_id`, `start_time`, `modified_time`, `summary`, and `is_remote` are available directly from the list/metadata RPCs. Working directory and "last message" are not part of that struct and must be fetched separately (working directory isn't retrievable per-session at all from documented APIs — it's filter-only input; last message requires a follow-up `get_events`/`session.getMessages` call after resuming).

## 3. What does `Client::resume_session` / `ResumeSessionConfig` require as input, and what are its documented failure modes?

**Required input:** `ResumeSessionConfig::new(session_id: SessionId)` — the only mandatory field is the `session_id` of the session to resume; every other field defaults to `None`/unset and is optional — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs):

```rust
pub fn new(session_id: SessionId) -> Self { /* all other fields None */ }
```

Notable optional re-supply fields (documented as needing to be re-supplied because they are not persisted server-side): `system_message`, `tools`, `commands` (slash commands — "not persisted server-side, so the resume payload re-supplies the registration"), `provider`/BYOK credentials ("API keys are never persisted to disk for security reasons" — must re-provide), `mcp_servers`, `custom_agents`, `managed_settings` ("startup-only and is not persisted... must be re-supplied on resume to remain in effect"), and `exp_assignments` — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs), [Session resume and persistence](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-persistence).

Two resume-specific behavioral flags:
- `suppress_resume_event` (wire name `disableResume`): "Force-fail resume if the session does not exist on disk, instead of silently starting a new session." — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs)
- `continue_pending_work`: "instructs the runtime to continue any tool calls or permission requests that were pending when the previous connection was dropped" — used with `Client::force_stop` to hand a session off between processes — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs)

**Documented failure modes** (from `Client::resume_session`'s implementation and the shared `SessionErrorKind` enum):

- `github_token` and `github_token_provider` set simultaneously → `Error::with_message(ErrorKind::InvalidConfig, "github_token and github_token_provider are mutually exclusive")` — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs) (`into_wire` validation, shared by create and resume).
- CLI returns a session ID different from the one requested → `SessionErrorKind::SessionIdMismatch { requested, returned }` — [session.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs), [errors.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/errors.rs).
- Session not found on disk: by default `session.resume` "silently starts a new session" instead of erroring; setting `suppress_resume_event`/`disableResume` makes this fail instead — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs). The generic **"session not found"** error kind that exists in the SDK is `SessionErrorKind::NotFound(SessionId)` — "The CLI could not find the requested session." — [errors.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/errors.rs) — this is presumably what's raised elsewhere (e.g. explicit lookups), and is the natural failure once `disableResume` forces strict behavior, though the exact code path/RPC error that surfaces it on resume isn't spelled out further in the fetched source.
- `ClientMode::Empty` requires `available_tools` to be set on the config (create *and* resume paths run the same validation) → `Error::with_message(ErrorKind::InvalidConfig, "ClientMode::Empty requires available_tools to be set ...")` — [session.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs).
- `SessionFsProviderRequired` — client was started with `ClientOptions::session_fs` but no `SessionFsProvider` was supplied on resume — [errors.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/errors.rs), enforced in `session.rs`.
- General protocol/version mismatch is a separate, non-session-scoped error: `ProtocolErrorKind::VersionMismatch { server, min, max }` — "The CLI server's protocol version is outside the SDK's supported range" — and `VersionChanged { previous, current }` if it changes mid-connection — [errors.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/errors.rs). These aren't resume-specific but would block any RPC including `session.resume` if the CLI binary is stale/mismatched relative to the SDK.
- Session-scoped errors surfaced generally during a session's life (not resume-exclusive, but relevant): `AgentError`, `Timeout(Duration)` (on `send_and_wait`), `SendWhileWaiting`, `EventLoopClosed`, `ElicitationNotSupported` — [errors.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/errors.rs).

I did not find a distinct, explicitly-named "stale session" error variant in the fetched source — the closest documented concept is the default silently-start-new-session behavior on a missing/absent session, gated by `suppress_resume_event`.

## 4. How does `enable_session_store` relate to plain single-session resume?

`enable_session_store` is **separate from, and not required for**, single-session resume/history. Concretely:

- `SessionConfig::enable_session_store` / `ResumeSessionConfig::enable_session_store` doc comment: "When true, enables the session store for this session." — [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs).
- The README states directly: "`enable_session_store` on `SessionConfig` enables **the cross-session store for search and retrieval across sessions**. When unset in the default client mode, the runtime default applies (enabled). In `Empty` mode, defaults to disabled." — [README.md, "Infinite Sessions"](https://github.com/github/copilot-sdk/blob/main/rust/README.md).
- Plain resume of one session by ID (`Client::resume_session(ResumeSessionConfig::new(session_id))`) works independent of this flag — resuming a specific session by its own ID is the base persistence mechanism (conversation history/checkpoints on disk), documented separately from the session store — [Session resume and persistence](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-persistence).
- In `ClientMode::Empty`, `enable_session_store` is explicitly forced to `Some(false)` by default (alongside several other feature flags) unless the caller overrides it — [session.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs) (`create_session`/`resume_session` mode-default block: `if config.enable_session_store.is_none() { config.enable_session_store = Some(false); }`).

**Conclusion:** `enable_session_store` governs an additional cross-session *search/retrieval* capability (a different feature from "infinite sessions" compaction, despite being documented in the same README section) layered on top of the runtime. It is optional and orthogonal to the ability to list (`session.list`) and resume (`session.resume`) a session by its own ID — those RPCs work regardless of this flag's value.

## 5. Where does session state live on disk, and is there a simpler way to enumerate session ids than parsing that directory?

- Confirmed convention: **`~/.copilot/session-state/{sessionId}/`** is the default location. The docs give this concrete layout:
  ```text
  ~/.copilot/session-state/
  └── user-123-task-456/
      ├── checkpoints/           # Conversation history snapshots
      │   ├── 001.json
      │   ├── 002.json
      │   └── ...
      ├── plan.md                # Agent's planning state (if any)
      └── files/                 # Session artifacts
  ```
  — [Session resume and persistence, "What gets persisted?"](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-persistence)
- The Rust README corroborates this for the infinite-sessions feature specifically: "Workspace state lives under `~/.copilot/session-state/{sessionId}` by default — override with `workspace_path` to relocate." — [README.md, "Infinite Sessions"](https://github.com/github/copilot-sdk/blob/main/rust/README.md).
- Container/serverless guidance confirms the same path is what needs to be mounted to persistent storage (e.g. Azure Container Instance example mounts a volume at `/home/app/.copilot/session-state`) — [Session resume and persistence, "Azure dynamic sessions"](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-persistence).
- **Simpler enumeration than parsing the directory:** Yes — the docs explicitly recommend the RPC/API over filesystem inspection. The "Managing session lifecycle → Listing active sessions" section says to call `client.listSessions()` / (Rust) `Client::list_sessions(filter)` rather than reading the session-state directory yourself, and the "Summary" table lists `client.listSessions(filter?)` as the documented way to enumerate sessions — [Session resume and persistence](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-persistence). There is no documented CLI slash command (e.g. `/resume`) or JSON index file for this in the sources fetched — the docs point to the `session.list` RPC as the canonical, supported enumeration mechanism, not directory parsing or a slash command.

---

## Summary of primary-source gaps

- No dedicated "stale session" error variant was found; the closest documented behavior is silently starting a new session unless `suppress_resume_event`/`disableResume` is set (sub-question 3).
- `SessionMetadata` does not include working directory or last-message content; these require separate calls or aren't retrievable per-session at all from the documented list/metadata RPCs (sub-question 2).
- The `docs.github.com/.../resume-a-session` URL suggested in the task does not exist (404); the correct doc is `features/session-persistence`.
