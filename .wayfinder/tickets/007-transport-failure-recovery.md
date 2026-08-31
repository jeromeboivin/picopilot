---
title: How should picopilot behave when the CLI transport/process dies mid-session?
status: closed
type: grilling
assignee: GitHub Copilot
blocked_by: [001-session-resume-facts]
resolution: ../resolutions/007-transport-failure-recovery.md
---

## Question

Decide picopilot's behavior when `Error::is_transport_failure()` fires
mid-session:

- Auto-restart the `Client` and attempt to resume the same session (using the
  facts from the session-resume-facts research), or surface an error to the
  user and exit?
- If auto-resume is attempted, how many retries before giving up, and what
  does the TUI show while recovering?
