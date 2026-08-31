---
type: design-decision
title: Transport recovery semantics
description: >
  Auto-restart and session resume when the CLI transport/process dies mid-session,
  including bounded retries, identity verification, and agent steering.
tags: [picopilot, recovery, transport, resilience]
status: verified
sources:
  - .wayfinder/resolutions/007-transport-failure-recovery.md
  - src/runtime.rs
  - src/tui.rs
  - session b8030d13 (implementation commits)
  - session 7028bf67 (fix commit 8f0ceb7)
generated: "2026-08-31T17:28:00Z"
---

# Transport recovery

## Recovery sequence

1. Detect transport death (event-stream closure or RPC failure).
2. Deny any queued approval prompts (avoids deadlocks waiting for user input
   on a dead session).
3. Mark in-flight tool calls as **unknown** (not assumed succeeded or failed).
4. Show an inline "reconnecting" banner that hijacks input.
5. Force-stop the dead `Client`.
6. Start a fresh `Client` with the **same startup configuration** (preserved
   `AppConfig` ensures future config additions automatically apply to reconnects).
7. Resume via `ResumeSessionConfig` with the original session ID.
8. **Verify identity**: compare `session_id` and `start_time` (when metadata
   is available) against expected values. Mismatch → fail loudly.
9. Send a **steering message** to the resumed agent: instructs it to inspect
   state before assuming or repeating the uncertain tool action.
10. Swap in the replacement client and fresh event subscription.

## Retry policy

- Bounded retries with exponential backoff.
- Exhausted retries exit with a clear error (no infinite retry loop).

## Identity protection

- `start_time` matching is enforced whenever metadata provides it.
- Missing `start_time` is tolerated only for newly created sessions
  (discovered via live smoke testing — SDK does not immediately index new
  session metadata).
- ID mismatch is fatal: picopilot refuses to silently continue in a
  different session.

## Steering message

On successful recovery, a host-originated message is sent with:
- An explicit rule: do **not** assume the outcome of any tool call that was
  in progress when the connection dropped.
- The display prompt is separate from the operational instruction.

## Coverage gaps

No live integration test kills the actual CLI process and verifies
restart/resume end-to-end. This is a known residual risk.
