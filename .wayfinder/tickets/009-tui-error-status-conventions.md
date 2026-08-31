---
title: What are picopilot's general TUI error/status conventions?
status: closed
type: grilling
assignee: GitHub Copilot
blocked_by: []
resolution: ../resolutions/009-tui-error-status-conventions.md
---

## Question

Ticket 004 settled tool-approval confirmation UX, and ticket 007 settled
transport-failure recovery banners. Decide the remaining, more general
conventions:

- How are non-fatal warnings surfaced (e.g. a tool call succeeded but
  produced a stderr warning, a partial file write, a deprecated model)?
- How are SDK-side error events (e.g. a malformed tool call, a rejected
  request) shown to the user — same inline-banner family as approvals/
  transport failures, or something distinct?
- Is there a persistent place to review past errors/warnings within a
  session (a log view), or are they transient (scroll past in the chat
  stream and gone)?
- Any conventions for validation errors on user input (e.g. an invalid
  `--model` at startup, already partially settled in ticket 006) that should
  generalize to in-TUI input validation?
