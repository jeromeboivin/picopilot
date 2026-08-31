---
title: Which default Copilot system-message sections are safe to trim for token savings?
status: closed
type: research
assignee: GitHub Copilot
blocked_by: []
resolution: ../resolutions/002-system-message-trim-candidates.md
---

## Question

The map settles that picopilot actively trims the default Copilot system
prompt via `SystemMessageTransform` to cut per-turn token overhead.
Investigate, against `github/copilot-sdk` primary sources (the Rust SDK's
System Message Transforms docs, `transforms.rs`, and any CLI reference
enumerating system-message section IDs):

- What section IDs exist in the default system message (e.g. `instructions`,
  skills, MCP, custom-agent boilerplate), and roughly how much of the prompt
  each occupies?
- Which sections are irrelevant given picopilot's settled scope (no skills,
  no MCP, no custom agents) and therefore safe to strip or shrink?
- Which sections are load-bearing for core agent behavior (tool-use
  instructions, safety guidance) and must not be touched?
- Does `transform_section` allow removing a section outright (returning
  `None`/empty) or only rewriting its content?

Write findings to `.wayfinder/research/system-message-trim-candidates.md`,
citing primary sources for each claim.
