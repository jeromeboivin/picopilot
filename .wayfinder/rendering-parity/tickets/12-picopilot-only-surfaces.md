---
label: wayfinder:prototype
name: Design picopilot-only surfaces in the new language
status: closed
assignee: wayfinder-session
blocked_by: [03-user-assistant-messages, 05-tool-call-rendering]
---

# Design picopilot-only surfaces in the new language

## Question

How should picopilot's own transcript surfaces look, given they have no Claude Code
counterpart to copy?

These are: reasoning lines, subagent status lines, warning and error banners, debug diagnostic
lines, and the permission or approval prompt.

There is nothing to research here — the rules have to be derived from the visual language the
earlier tickets establish. That makes this the one ticket in the map that needs the human,
so keep the round short: build a rough rendering of all five surfaces first and put that in
front of them rather than asking abstract questions.

Resolve by producing:

- A rough rendered sample of each of the five surfaces, using the established glyphs, indents
  and palette keys.
- The chosen rule for each: glyph, palette key, indent, blank-line behaviour.
- For the approval prompt specifically: whether it renders inline in the flow like the pickers
  in [Specify inline pickers](10-inline-pickers.md), what it shows about the pending action,
  and how the choices are presented.
- For subagent lines: how nesting or attribution is shown when several agents are active.
- Whether any of the five should simply be removed rather than restyled.

## Resolution

Resolved with the human on 2026-09-05. A rough rendering of all five surfaces was put in front
of them and accepted, with two amendments (Q17, Q18 below).

### The governing rule

**Every top-level transcript event is a `⏺` row; only the palette key varies.** Body text sits
at column 2 with a hanging indent, subordinate detail sits behind the `  ⎿  ` gutter from
[Specify tool call rendering](05-tool-call-rendering.md). picopilot currently uses five
competing shapes — a `tool ... [state]` header, an `agent ... [state]` header, labelled bold
banners, a `debug` label and a bordered approval box. All five collapse into the one row shape.
No surface introduces a glyph, a label prefix, a border or a colour that is not already in the
palette.

### Per-surface rules

| Surface | Glyph | Palette key | Body | Notes |
| --- | --- | --- | --- | --- |
| Reasoning / thinking | `✻` | `subtle` | dim italic, column 2 | Collapsed by default, see Q17 |
| Subagent | `⏺` | as any tool call | `Task(<description>)` | Becomes a tool call, see Q18 |
| Warning banner | `⏺` | `warning` | column 2 | No `warning:` label, not bold |
| Recoverable error | `⏺` | `warning` | column 2 | Merged with warning; the old salmon tier is dropped |
| Blocking error | `⏺` | `error` | column 2 | No label, not bold |
| Diagnostic / debug | `⏺` | `subtle` | column 2 | Still gated behind `Ctrl+I` |

`✻` is taken from the reference's own `figures.ts` (`TEARDROP_ASTERISK`), so no glyph is invented
by this ticket.

**Dropped:** the purple subagent colour `Rgb(204,166,255)`, the salmon recoverable-error tier
`Rgb(255,169,122)`, the `warning` / `error` / `debug` text labels, and the bold modifier on all
banner text. picopilot's three-tier banner severity collapses to two, because with no label text
the salmon and red tiers are not reliably distinguishable.

### Reasoning (Q17)

**Collapsed by default**, matching the reference. The row renders as `✻ Thinking…` in `subtle`
and expands to the dim italic body on a keypress. This reverses picopilot's current
always-visible behaviour. Reuse the same expand key as the tool-result truncation from
[Specify tool call rendering](05-tool-call-rendering.md) rather than adding a second one.

### Subagents (Q18)

A subagent is an ordinary tool call: `⏺ Task(<description>)`, its own tool calls nested one
gutter level beneath it, and a closing gutter row summarising the run
(`Done (N tool uses · N tokens · Ns)`). **Nesting alone carries attribution** — there is no agent
name and no per-agent colour, even with several agents running at once. The state colouring is
whatever [Specify tool call rendering](05-tool-call-rendering.md) already specifies for tool
headers; no subagent-specific state vocabulary survives.

### Approval prompt

Renders inline exactly like the pickers in [Specify inline pickers](10-inline-pickers.md),
taking the input box's place. Three rows above the choices: the `⏺` header naming the pending
action in tool-call form, a `  ⎿  ` gutter row describing it in plain language, then the
question. Choices are the borderless list with `❯` marking focus. The current bordered,
yellow-titled approval box and its `y / n / a / v` key legend are both removed — the choices are
self-describing rows, so there is nothing left to legend. The `v` details view is folded into
the gutter description rather than being a separate keypress.

### Nothing is removed

All five surfaces are kept. Each carries information with no other home, and the only one that
was a candidate for removal — diagnostics — is already opt-in behind `Ctrl+I`.

### Not verified

The rendering shown to the human was a mock-up written in chat, not a running build. Column
positions and blank-line spacing follow the rules from
[Specify user and assistant message rendering](03-user-assistant-messages.md) and
[Specify tool call rendering](05-tool-call-rendering.md) rather than having been measured.
