---
label: wayfinder:map
name: Claude Code visual parity for picopilot
---

# Claude Code visual parity for picopilot

## Destination

A single handoff document, `docs/rendering-spec.md`, that specifies picopilot's terminal
rendering precisely enough for someone to implement it without looking at the Claude Code
source again: the screen model, the palette, and a section per message type giving exact
glyphs, indents, blank-line rules, truncation wording, colors and states.

The map is done when that spec is written and nothing about the look is still undecided.
Implementing it is a separate effort.

## Notes

**Domain.** picopilot is a Rust CLI coding agent using ratatui + crossterm. Today it is a
full-screen alternate-screen app: a status bar, one scrollable `Paragraph` holding the whole
transcript, an input box, a shortcut bar. All rendering lives in `src/tui.rs` (~176 KB).
The reference is `C:\dev\git\claude-code`, a TypeScript/Bun Ink+React implementation.

**Skills every session should consult:** `grilling` and `domain-modeling`. Tickets that read
the reference source should also use `research`. Tickets that need something to react to
should use `prototype`.

**Standing preferences for this effort** (all settled while charting, do not reopen):

- **Fidelity: identical in static layout.** Glyphs, spacing, indent depth, color roles,
  wrapping and truncation wording are copied exactly. Expensive *dynamic* behaviour
  (syntax highlighting, animation loops, word-level diff) is decided on its own ticket.
- **Branding is copied too.** Claude Code's actual accent color and its spinner verb list
  are in scope, not just the palette structure.
- **Dependencies: whatever it takes.** New crates are acceptable, including large ones.
- **Dark theme only.** No light, ansi-16 or daltonized variants.
- **Screen model: terminal scrollback, not alternate screen.** Committed history flows into
  the user's own scrollback; only a small live region is redrawn. This is settled; how to
  achieve it in ratatui is not.
- **The status bar and shortcut bar are deleted.** Their information follows Claude Code:
  model and token counts ride on the spinner line, the rest moves to a `/status` command.
- **Modals become inline pickers** rendered in the flow, since scrollback leaves no canvas
  for a centered overlay.
- **picopilot-only surfaces are designed in the same visual language**, not left as-is.
- Prefer AFK tickets. The human wants minimal involvement; pull them in only when a
  decision genuinely cannot be made from the reference source.

**Known risk, accepted.** We are working from source reading only — there is no `bun`, no
`node_modules` and no `claude` binary on this machine, so no ticket can compare against real
rendered output. Every "exact" claim in the spec is inferred from code. Where a ticket cannot
tell what the source actually renders, it must say so in its resolution rather than guess.

**Naming.** Colors are referred to by palette key (`claude`, `success`, `error`,
`permission`, `subtle`, …). Tickets may use those keys before
[Capture the Claude Code dark palette](tickets/01-dark-palette.md) supplies their values.

## Decisions so far

<!-- one line per closed ticket -->

- [Capture the Claude Code dark palette](tickets/01-dark-palette.md): 69 keys; accent `claude` is
  `rgb(215,119,87)` with shimmer `rgb(235,159,127)`; only ~20 keys matter for the conversation
  window, 5 are dead, and the spinner's stall, flash and thinking colors are interpolated at
  runtime from two endpoints that are not in the palette at all.
- [Settle the scrollback rendering mechanism](tickets/02-scrollback-mechanism.md): the reference
  has no committed/live split — it renders one oversized frame on the main screen buffer and
  lets terminal scroll carry old rows away. picopilot should instead use ratatui
  `Viewport::Inline` + `Terminal::insert_before` as an explicit one-way commit, accepting a
  fixed-height live region; and it should mirror the reference in disabling live streaming
  preview on Windows Terminal.
- [Specify user and assistant message rendering](tickets/03-user-assistant-messages.md): user is
  `❯ ` in `subtle` on a full-terminal-width background box with continuations back at column 0;
  assistant is `⏺`/`●` in `text` with a hanging indent at column 2; one blank line before every
  message. Both the background fill and the surviving hanging indent require replacing
  ratatui's `Wrap` with hand-rolled wrapping.
- [Specify markdown and code block rendering](tickets/04-markdown-and-code-blocks.md): markdown is
  almost entirely attribute-only — inline code is the one colored element (`permission`),
  blockquotes use `▎ `, strikethrough is deliberately off, and code fences get no indent, border
  or language label. Highlight with `two-face` over `syntect`, with an in-tree span conversion.
- [Specify tool call rendering](tickets/05-tool-call-rendering.md): two rows — a bold
  `⏺ Name(args)` header colored `success`/`error` once resolved, then a result block behind a
  single dim `  ⎿  ` gutter. Truncation is three-tiered with different wording per tier. The
  only behaviour needing a render tick is a shared 600 ms loader blink, which has a legitimate
  static fallback.
- [Specify bash command and output rendering](tickets/06-bash-tool-rendering.md): the command is
  cut to 2 lines / 160 chars with `…` and never syntax-styled; stdout and stderr are two separate
  `⎿` rows; a non-zero exit does not reach the result block at all but routes to the error path
  as `Error: Exit code N`; output keeps its ANSI colors with only underline stripped, and folds
  at `columns - 10`. The Windows streaming disable applies to assistant text only, not to shell
  progress.
- [Specify file edit diff rendering](tickets/07-file-edit-diff.md): a diff is just the `⎿` result
  block — no border and no path header, only `Added N lines, removed M lines` then rows at
  `columns - 12` behind a `max_digits + 3` gutter with `...` hunk separators and no row cap. The
  dashed border exists only around the permission-dialog diff. Do word-level highlighting now,
  with `similar` and the reference's 0.4 change threshold.
- [Specify the spinner line](tickets/08-spinner-line.md): one row driven by a single 50 ms clock —
  a 12-frame glyph at 120 ms with per-platform variants, one of 187 verbs fixed for the turn, a
  3-column shimmer, and a timer plus a per-turn output-token estimate that both appear only after
  30 s. The render-tick worry was unfounded: picopilot already redraws every 50 ms, and 50 ms
  divides every interval in the spec. Only the token count and reasoning effort survive from the
  deleted status bar.
- [Specify the input box and hint line](tickets/09-input-box.md): it is not a box — two full-width
  `─` rules in `promptBorder`, a `❯ ` at column 0 that dims while the agent works, and a dim
  `" · "`-joined hint row that vanishes on the first keystroke. Growth is capped at
  `max(3, budget)` with cursor-centred scrolling inside the box and no truncation marker; the
  completion list moves below the box, borderless.
- [Specify inline pickers](tickets/10-inline-pickers.md): the reference has no centered modal at
  all — a picker takes the input box's place as a borderless list with `❯` for focus, `↑`/`↓` in
  that same column, `[✓]` checkboxes, a 5-row window and an `(n of m)` counter. That fixed window
  is what makes the inline viewport work: a 500-item list is still 5 rows. Usage prints a static
  block, todos become a live block above the input.
- [Specify the text wrapping and background fill mechanism](tickets/14-wrapping-and-background-fill.md):
  hand-roll the wrapper and stop using `Paragraph::wrap` for transcript content. ratatui's
  `WordWrapper` is private and diverges from the reference in four ways, including silently
  dropping a grapheme wider than the wrap limit. Owning the wrapping also gives `insert_before`
  its exact height for free and lets the `unstable-rendered-line-info` feature be dropped.
- [Specify the status command](tickets/11-status-command.md): the reference's `/status` is an
  interactive settings pane, not a print, so picopilot's becomes a `/cost`-shaped block behind
  the `  ⎿  ` gutter carrying cwd, model and count summaries. Cost and full token accounting go
  to the usage block, the busy flag is dropped because the spinner already says it, and only the
  context-limit warning keeps a persistent home — dim and right-aligned on the hint row, hidden
  until 20k tokens from the threshold.
- [Specify ANSI passthrough in shell output](tickets/15-ansi-passthrough.md): parse SGR into a
  ratatui `Style` **before** wrapping, which dissolves the wrap-point problem entirely since a
  split span just copies its style and no escape ever reaches a width measurement. Use
  `ansi-to-tui` pinned to 7.0.0, because 8.x needs `ratatui-core` and ratatui 0.29 has none.
  Untrusted subprocess output is sanitised at ingestion against an allowlist of SGR, LF and tab;
  everything else is dropped — cursor movement most of all, since under a committed-scrollback
  model it would let a subprocess permanently rewrite rows the user has already read.
- [Design picopilot-only surfaces in the new language](tickets/12-picopilot-only-surfaces.md):
  every top-level event becomes a `⏺` row and only the palette key varies, so picopilot's five
  competing shapes collapse into one. Thinking gets `✻`, collapsed by default; subagents become
  ordinary `Task(...)` tool calls carried by nesting alone, with no agent name and no per-agent
  colour; banners lose their labels, their bold and their middle severity tier; the approval
  prompt becomes an inline picker. The purple agent colour and the salmon error tier are dropped.
- [Assemble the rendering spec](tickets/13-assemble-spec.md): consolidated all fourteen
  resolutions into the normative [picopilot terminal rendering specification](../../docs/rendering-spec.md),
  including implementation checks and an explicit inventory of every source-only claim that
  remains unverified.

## Not yet specified

_None. The way to the destination is clear._

## Out of scope

- **Light, ansi-16 and daltonized theme variants.** Dark only was chosen; the other variants
  are a separate effort if they are ever wanted.
- **Running claude-code locally to capture reference screenshots.** Explicitly declined; the
  spec is written from source reading. Recorded as a risk above, not as work.
- **Implementing the rewrite in `src/tui.rs`.** The destination is a spec; building it is a
  separate effort.
- **Claude Code's logo header, onboarding, `/help` and MCP surfaces.** The destination is the
  conversation window.
- **Truecolor fallback.** Dark truecolor is the destination; ANSI-16 degradation belongs to a
  separate compatibility effort.
- **Performance tuning for very long transcripts.** The spec starts with ratatui's
  `scrolling-regions` feature disabled and requires measurement before enabling it; that is an
  implementation validation, not another visual decision.
- **A replacement shortcut legend.** The bottom shortcut bar is deleted. `Ctrl+I` remains the
  opt-in diagnostic toggle; discoverability beyond the context-sensitive input hint is a
  separate interaction-design effort.
