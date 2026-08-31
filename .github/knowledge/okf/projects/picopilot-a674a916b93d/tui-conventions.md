---
type: design-decision
title: TUI conventions
description: >
  Keyboard shortcuts, modal patterns, error/status rendering conventions,
  and transcript display rules for the picopilot terminal UI.
tags: [picopilot, tui, ux, keyboard, ratatui]
status: verified
sources:
  - .wayfinder/resolutions/003-tui-shape.md
  - .wayfinder/resolutions/009-tui-error-status-conventions.md
  - src/tui.rs
  - session b8030d13 (implementation)
  - session 7028bf67 (audit fixes a17e9cb, 9bd3d1e, f21987c)
  - session 7028bf67 (Ctrl-key migration 46de582, picker simplification 5d1eafd, debug resume 849c061)
  - session 7028bf67 (Claude Code-like simplification 3a79fff, Markdown rendering, ❯/●/✻ glyphs)
generated: "2026-08-31T19:03:00Z"
---

# TUI conventions

## Layout

- **Borderless, padded single-column** inspired by Claude Code CLI: thin
  top metadata bar (model, mode, reasoning level, live cost in AIU),
  full-width borderless chat transcript with horizontal padding, pinned
  single-line input with subtle top/bottom rules, and a single muted
  **shortcut bar** footer showing keybindings.
- Auto-scroll anchored to the bottom so streamed output stays visible.
- Wrapped-line estimate replaces Ratatui's private `Paragraph::line_count`
  (unstable API workaround for 0.29).
- Modals retain boxed borders; only the main conversation screen is borderless.

## Keyboard shortcuts

All main-window shortcuts require a **Ctrl modifier** to prevent bare
keystrokes from triggering commands while typing in the input field.
This was a regression fix — bare `m`, `u`, `t`, `q` keys previously
conflicted with normal text entry.

| Key       | Context       | Action |
|-----------|---------------|--------|
| Enter     | Normal input  | Send message via `session.send` |
| `/fleet …`| Normal input  | Send via Fleet dispatch |
| `Ctrl+X`  | Any           | Quit (restore terminal) |
| `Ctrl+C`  | Any           | Quit (restore terminal) |
| `Ctrl+O`  | Normal        | Open session picker (full-screen modal) |
| `Ctrl+P`  | Normal        | Open model picker (full-screen modal) |
| `Ctrl+U`  | Normal        | Open usage/cost detail modal |
| `Ctrl+T`  | Fleet active  | Open todo modal |
| `Ctrl+I`  | Any           | Toggle reasoning/tool telemetry visibility |
| `y/n/a`   | Approval      | Approve once / deny once / trust category |
| `v`       | Approval      | Toggle full approval-detail view |
| `r`       | Model picker  | Cycle reasoning effort for selected model |
| `c`       | Model picker  | Cycle context tier for selected model |
| Arrows    | Modal         | Navigate list items |
| Enter     | Modal         | Select item |
| Esc       | Modal         | Close modal |

Press-only guard (`KeyEventKind::Press`) on all key handlers to avoid
double-fire on Windows terminals. Unrecognized `Ctrl+<char>` combinations
are silently ignored rather than passed to the input field.

## Shortcut bar

A 2-line bar rendered below the input box shows discoverable key labels:
`^O Sessions`, `^P Models`, `^U Usage`, `^T Todos`, `^I Internals`,
`^X Exit`. Always visible; no toggle.

## Transcript display

- **User messages** prefixed with `❯` (Unicode glyph matching Claude Code).
- **Assistant messages** prefixed with `●`.
- **Busy indicator**: `✻ Copilot is responding…` shown in dim text while the
  session is active; disappears on idle/completion.
- **Reasoning tokens** are always rendered in **dim gray italics** regardless
  of `Ctrl+I` toggle state. This was a deliberate design change: reasoning
  was previously hidden unless `Ctrl+I` was enabled; now it is a first-class
  always-visible dim stream.
- **Markdown rendering** in assistant and reasoning messages via `pulldown-cmark`:
  headings, bold, italic, strikethrough, lists, task lists, inline code,
  fenced code, links, quotes, and horizontal rules. Reasoning messages use a
  muted gray palette that overrides Markdown accent colors.
- **Tool activity**, sub-agent lifecycle, and Fleet coordination are collapsed
  by default; toggled via `Ctrl+I`.
- **Diagnostic entries** (e.g. "session resumed") are hidden unless `Ctrl+I`
  is toggled on. They appear as `debug:` labeled lines.
- **User messages**: optimistic insert on submit; SDK `user.message` events
  reconcile with (not duplicate) the optimistic row.
- **Streamed deltas** coalesce into a single assistant line.
- **Input caret**: the terminal cursor is positioned after the typed text
  when the main input owns focus. The prompt glyph is `❯`.

## Error and status conventions

- **Startup validation errors** → stderr before TUI launches.
- **`session.warning`** and recoverable **`session.error`** → inline chat-stream banners.
- **Non-recoverable `session.error`** → blocking final message; input is frozen,
  only `Ctrl+X` / `Ctrl+C` are accepted.
- **`session.info`** → ignored in the UI (superseded by usage modal).
- **Cost** displayed as AIU (nano-AIU ÷ 10⁹), not raw nano-AIU.
- **Resume notice** shown as a diagnostic entry (debug-only), not an inline
  warning banner.
- All banners are transient; no persistent error log.

## Modal conventions

- Session/model pickers are **full-screen** (geometry equals terminal bounds).
- Usage, todo, and detail modals follow the same on-demand toggle pattern.
- The **session picker** no longer fetches per-selection previews; navigation
  is purely local (no network round-trip on arrow key). This was simplified
  to keep picker scrolling responsive.
- Network actions (list sessions, fetch usage, load todos) happen in the
  event-loop action handler, not in the renderer.

### Model picker

The model picker uses a three-panel vertical layout:

1. **Top** (flexible height): scrollable `List` widget of all models
   (`name`, cost label, context-window size), with `ListState` highlighting
   and the cursor pre-positioned on the currently active model.
2. **Middle** (5 rows): detail pane for the selected model — full name/ID,
   current reasoning-effort choice vs available options, current context-tier
   choice vs available options.
3. **Bottom** (1 row): key legend (`↑/↓ choose  r reasoning  c context
   Enter apply  Esc cancel`).

Reasoning effort and context tier are cycled via `r` / `c` inside the picker
rather than inlined into the list rows; the detail pane gives immediate
feedback on the selected configuration.