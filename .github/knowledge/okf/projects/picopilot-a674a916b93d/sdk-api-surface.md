---
type: api-reference
title: Copilot SDK API surface (Rust 1.0.13-preview.2)
description: >
  Verified public Rust signatures and response types from github-copilot-sdk
  1.0.13-preview.2 that picopilot depends on for session lifecycle, model
  switching, Fleet dispatch, todo visibility, usage metrics, and transport recovery.
tags: [picopilot, copilot-sdk, rust, api]
status: verified
sources:
  - session b8030d13 (SDK research report)
  - github/copilot-sdk rust/src/
  - session 73ea30cc (SystemMessageConfig replace semantics, disconnect, context attribution)
generated: "2026-08-31T21:30:00Z"
---

# Copilot SDK API surface

Verified against `github-copilot-sdk` 1.0.13-preview.2 crate.

## Session lifecycle

| Method | Wire RPC | Notes |
|--------|----------|-------|
| `Client::list_sessions(filter)` → `Vec<SessionMetadata>` | `session.list` | Returns `session_id`, `start_time`, `modified_time`, `summary`, `is_remote`. No working-dir, last-message, or model ID — cross-process resume cannot know local vs hosted up front. |
| `Client::resume_session(ResumeSessionConfig)` → `Session` | `session.resume` + `session.skills.reload` | Config accepts `.with_model()`, `.with_reasoning_effort()`, `.with_context_tier()`, `.with_continue_pending_work(bool)`. |
| `Session::set_model(SetModelOptions)` | `session.setModel` | Clean swap; history preserved. Options: `.with_reasoning_effort(String)`, `.with_context_tier(ContextTier)`. |

## Fleet dispatch

| Method | Wire RPC | Notes |
|--------|----------|-------|
| `session.rpc().fleet().start(FleetStartRequest)` | `session.fleet.start` | Request carries `prompt: Option<String>`. Response has `started: bool`. |
| `session.rpc().tasks().start_agent(...)` | `session.tasks.startAgent` | Fallback for single `general-purpose` agent delegation when Fleet is unavailable. |

## Todo visibility

| Method | Wire RPC | Notes |
|--------|----------|-------|
| `session.rpc().plan().read_sql_todos_with_dependencies()` | `session.plan.readSqlTodosWithDependencies` | Returns optional row fields (id, title, description, status) plus dependency edges. |
| Event: `session.todos_changed` | — | Signal-only; no payload. Triggers a todo refresh in an open modal. |

## Usage & attribution

| Method | Wire RPC | Notes |
|--------|----------|-------|
| Event: `session.usage_info` | — | Live token/limit gauge per turn. |
| `session.rpc().usage().get_metrics()` → `UsageGetMetricsResult` | `session.usage.getMetrics` | Session-wide cost (`totalNanoAiu`), premium-request cost, request count, duration, active model. |
| `session.rpc().metadata().get_context_attribution()` | `session.metadata.getContextAttribution` | Experimental. Per-category breakdown (system/tool/messages/results). May be absent. |

## Transport / client

- `Client::start(ClientOptions)` spawns the bundled CLI process.
- `ClientOptions::new().with_cwd(path)` — only the working directory is currently configured.
- On transport death: force-stop old client, start a fresh `Client`, resume with the same session ID, verify identity (`session_id` + `start_time` when available).
- Subscription errors are `#[non_exhaustive]`; picopilot uses a generic fallback banner for unknown kinds.

## System message configuration (verified)

| Field | Type | Notes |
|-------|------|-------|
| `mode` | `Option<String>` | `"replace"` or `"customize"`. |
| `content` | `Option<String>` | Replacement content when mode is `"replace"`. Empty string = zero tokens. |
| `section_overrides` | `Option<Vec<SectionOverride>>` | Per-section content overrides (customize mode only). |
| `sections_to_remove` | `Option<Vec<String>>` | Section IDs to remove (customize mode only). |
| `transforms` | registered via builder | `SystemMessageTransform` trait implementations. |

- `mode = "replace"` + `content = ""` yields zero system-prompt tokens.
- `mode = "customize"` + `sections` map applies per-section overrides.
- All fields are `Option` with `skip_serializing_if`.
- Both `SystemMessageConfig` and `SectionOverride` are exported from the Rust SDK 1.0.13-preview.2.

## Tool management constraints

- **No mid-session tool update RPC** — `Session` has no method to change
  `availableTools`/`excludedTools` after creation.
- Tool changes require `Session::disconnect()` + `Client::resume_session()`
  with a new `ResumeSessionConfig` carrying the updated tool list.
- `available_tools: Some(vec![])` disables all tools; `None` restores SDK
  defaults.
- Resume with a different tool list preserves conversation history.
- `excluded_tools` is always sent to block `web_fetch` and `web_search`.

## Context attribution

| Method | Wire RPC | Notes |
|--------|----------|-------|
| `session.rpc().metadata().get_context_attribution()` | `session.metadata.getContextAttribution` | Returns per-category breakdown: `system_prompt`, `custom_instructions`, `system_tools`, `mcp_tools`, `messages`. Experimental. May return `null` before first turn completes. |
| `metadata.contextInfo` | — | Aggregate system/tool-definition tokens. Also may return `null`. |
| Event: `session.usage_info` | — | Includes optional `system_tokens` and `tool_definitions_tokens` fields. Most reliable source. |

## Session disconnect

| Method | Wire RPC | Notes |
|--------|----------|-------|
| `Session::disconnect()` | — | Preserves on-disk history. Session can be resumed with new config (e.g. changed `available_tools`). |

## Absent or generated-only

- No `session.usage.getMetrics` convenience wrapper in the hand-written SDK; accessed via generated RPC layer.
- No distinct "stale session" error from resume; mismatch detected by comparing metadata fields.
- `session.metadata.getContextAttribution` is experimental / generated-only.
- Newly created sessions may not be immediately visible via `get_session_metadata`; picopilot treats missing `start_time` as acceptable during startup (discovered via live testing).
- `recomputeContextTokens` includes protocol/message overhead, so it is **not** a reliable exact-zero check for system-prompt assertions. Use `session.usage_info` fields as the primary source.

## Local provider registry (experimental)

Verified in session `73ea30cc` for additive BYOK/Ollama support.

| Type | Purpose |
|------|---------|
| `NamedProviderConfig` | Registers a named provider (name, provider_type, base_url, wire_api, api_key, bearer_token, transport). |
| `ProviderModelConfig` | Binds a model to a named provider (provider name, wire_model, model_id, capabilities). |

**Session integration:**
- `SessionConfig` accepts `providers: Vec<NamedProviderConfig>` and `models: Vec<ProviderModelConfig>`.
- `Client::list_models()` returns **only** the hosted catalog.
- `session.rpc().model().list()` returns **both** hosted and session-registered models.
- `session.set_model()` accepts qualified IDs like `local/qwen2.5-coder:14b`.
- Provider definitions are per-session and must be resupplied on resume and recovery.

**Ollama minimal config:**
```
ProviderConfig::new("http://localhost:11434/v1")
    .with_provider_type("openai")
    .with_wire_api("completions")
    // no API key needed
```

⚠️ Marked experimental — API may change in future SDK releases.
