## Resolution

Grilled transport-failure recovery in one round, informed directly by
[session-resume-facts](001-session-resume-facts.md)'s finding that resume
has no distinct "stale session" error — it silently starts a new session
if the id isn't found.

### Behavior

When `Error::is_transport_failure()` fires mid-session:

1. **Auto-restart and auto-resume**: picopilot restarts the `Client`
   process and calls `ResumeSessionConfig::new(session_id)` automatically.
2. **Bounded retries with backoff**: a small fixed number of attempts
   (e.g. 3) with short exponential backoff between them, then give up —
   not indefinite, not a single bare retry.
3. **TUI**: an inline banner in the chat stream (e.g. "connection lost,
   reconnecting… attempt 2/3"), input hijacked the same way a pending
   tool-approval hijacks it (per
   [permission-policy-and-confirmation-ux](004-permission-policy-and-confirmation-ux.md)),
   until reconnected or retries are exhausted.
4. **Guard against the silent-new-session risk**: after each resume
   attempt returns, picopilot verifies the returned session's identity
   (id and start_time) against what it expected. A mismatch is a
   **fatal resume failure**, surfaced clearly — picopilot never silently
   continues in an unrelated new session just because the SDK resume call
   itself didn't error.
5. **In-flight tool call at the moment of failure**: treated as
   **unknown, not assumed**. After reconnecting, the agent is told the
   last action's outcome is unresolved and should check before assuming
   success (e.g. re-read a file it was mid-write on) rather than blindly
   retrying or assuming it went through.
6. **Exhausted retries**: picopilot exits with a clear error naming the
   session id, so the user can resume manually later. No lingering
   "press r to retry" affordance.

### What this does not decide

- The exact retry count/backoff timing (e.g. 3 attempts, 500ms/1s/2s) —
  implementation detail, not a design fork.
- General TUI error/status conventions for failures *other than* transport
  loss (e.g. malformed tool output, disk-full on write) — still fog, this
  ticket only covers transport/process death.
