---
label: wayfinder:task
name: Assemble the rendering spec
status: closed
assignee: wayfinder-session
blocked_by:
  [
    01-dark-palette,
    02-scrollback-mechanism,
    03-user-assistant-messages,
    04-markdown-and-code-blocks,
    05-tool-call-rendering,
    06-bash-tool-rendering,
    07-file-edit-diff,
    08-spinner-line,
    09-input-box,
    10-inline-pickers,
    11-status-command,
    12-picopilot-only-surfaces,
    14-wrapping-and-background-fill,
    15-ansi-passthrough,
  ]
---

# Assemble the rendering spec

## Question

Nothing is left to decide — this ticket does the work of turning twelve resolutions into the
destination artifact, `docs/rendering-spec.md`.

Resolve by producing that file, containing:

- The screen model and the committed-versus-live boundary.
- The palette table: every key, its dark value, and its usage rule.
- The glyph inventory: every glyph quoted verbatim with its per-platform variants.
- A section per surface, in the order a reader meets them: user message, assistant message,
  markdown and code blocks, tool call, bash, diff, spinner line, input box and hints, inline
  pickers, `/status`, and the picopilot-only surfaces.
- The dynamic behaviours that were accepted, each with the cost that was weighed.
- A dependency list with the justification for each new crate.
- An explicit "unverified" section listing every rule inferred from source that was never
  compared against real rendered output, since no reference build exists on this machine.

Do not start implementing. The spec is the handoff.

## Resolution

Created [the rendering specification](../../../docs/rendering-spec.md). It consolidates all
fourteen decision resolutions into one normative handoff: screen ownership, palette, glyphs,
shared wrapping, every conversation surface, ANSI security, animation timing, dependencies,
migration impact, implementation verification, unverified claims, and a source index.

Structural validation confirmed all required major sections, all 69 dark-theme palette keys,
and references to every resolved source ticket. No production Rust implementation was started.
