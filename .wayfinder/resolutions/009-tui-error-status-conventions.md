## Resolution

Confirmed via research the SDK's actual non-fatal/status event surface —
`session.warning` (`warning_type`: subscription/policy/mcp/notification),
`session.error` (`error_type`: rate_limit/quota/authentication/authorization/
context_limit/query/notification, plus a `recoverable: bool`), and
`session.info` (`info_type`: notification/timing/context_window/mcp/
snapshot/configuration/authentication/model) — and settled how each reaches
the TUI:

- **Startup validation errors** (e.g. an invalid `--model` flag): a plain
  stderr message and non-zero exit, before the TUI ever launches. No
  in-TUI rendering for these.
- **`session.warning`**: rendered as an inline banner in the chat stream,
  the same visual family as tool-approval (ticket 004) and transport-failure
  (ticket 007) banners, differentiated only by color/icon (warning color,
  not error color).
- **`session.error`**: split by the `recoverable` flag.
  - `recoverable: true` — same inline-banner treatment as a warning (error
    color), session keeps going.
  - `recoverable: false` — a blocking final message, since the session is
    ending anyway (comparable to ticket 007's "exhausted retries exit with
    a clear error" — no more input to give, so no need for the input-box-
    hijacking convention used for confirmations).
- **`session.info`**: ignored in the UI for v1, treated as internal-only
  signal. Its `context_window` variant is superseded by the already-settled
  `usage_info` stream event and usage-detail modal (ticket 003's addendum);
  the rest (timing/snapshot/configuration/mcp/authentication/model) reads as
  low-level telemetry, not user-facing.
- **Error/warning history**: transient only — banners scroll past in the
  chat stream like the rest of the transcript. No persistent log/review
  panel for v1.
- **Malformed tool calls**: no special case. They surface as a generic
  `session.error` (message text carries the specifics); same recoverable-
  flag treatment as any other session error.

### What this does not decide

- Tool-approval confirmation UX — ticket 004.
- Transport-level disconnect/reconnect UX — ticket 007.
