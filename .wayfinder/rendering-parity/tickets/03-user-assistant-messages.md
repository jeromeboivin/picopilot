---
label: wayfinder:research
name: Specify user and assistant message rendering
status: closed
assignee: research-subagent
blocked_by: []
---

# Specify user and assistant message rendering

## Question

What are the exact rendering rules for a user message and for an assistant message?

This is the base visual language; the tool, bash, diff and picopilot-only tickets all build on
whatever this establishes for glyphs, indent and spacing.

Resolve by producing a spec section covering, for each of the two message types:

- The leading glyph, quoted verbatim, including any per-platform variant, and the palette key
  that colors it.
- Whether the message has a background box, and if so how the background extends across a
  wrapped line. Note that ratatui styles text spans and does not fill a line to the terminal
  edge on its own — say what that costs.
- The indent of the first line and of continuation lines, in columns.
- Blank-line rules: before, after, and between consecutive messages of the same type and of
  different types.
- Wrapping: the column the text wraps at, and how the wrap interacts with the indent.
- Any variation for nested, selected or replayed messages.
- The picopilot equivalent today and what has to change. picopilot currently uses `"❯ "` in
  `Rgb(240,177,94)` bold for user messages and `"● "` in `Rgb(154,230,180)` bold for assistant,
  both with a two-space continuation indent, in `src/tui.rs`.

## Resolution

Reference read: `C:\dev\git\claude-code` (source only — no `node_modules`, no `bun`, no
`claude` binary on this machine, per the map's accepted risk). Ink is vendored under
`src/ink/`, so layout and background semantics were verified against real renderer code, not
assumed. Line numbers below are from the files as they exist in that checkout.

### 1. User message

**Component chain.** `MessageRow.tsx:233` → `Message.tsx:195` (`UserMessage`) →
`Message.tsx:295` / `Message.tsx:379` (`UserTextMessage`) → `UserTextMessage.tsx:18`
(`UserPromptMessage`) → `UserPromptMessage.tsx:77` (`HighlightedThinkingText`).
`UserTextMessage.tsx:39-210` is a long dispatch chain; a plain typed prompt falls through all
of it to `UserPromptMessage`. Every other branch (bash input, slash command, memory input,
teammate, resource update, interrupt) is a different message type and belongs to other tickets.

**Leading glyph.** `figures.pointer`, from the npm package `figures`, pinned at `6.1.0`
(`bun.lock`). Rendered at `HighlightedThinkingText.tsx:91` and again at
`HighlightedThinkingText.tsx:145` as `<Text color={pointerColor}>{figures.pointer} </Text>` —
glyph plus **exactly one literal space**, inside the `<Text>`.

- Unicode terminals: `❯` (U+276F, HEAVY RIGHT-POINTING ANGLE QUOTATION MARK ORNAMENT).
- Legacy fallback: `>` (U+003E). **Partially determined.** `figures@6.1.0` depends on
  `is-unicode-supported@^2` (`bun.lock`) and selects a fallback table on that basis; the
  fallback *value* for `pointer` could not be read here because `node_modules` is absent.
  `>` is the value `figures` has shipped for `pointer` historically, but treat it as
  unconfirmed against this exact version.
- Note the contrast with the assistant glyph: Claude Code branches the assistant dot on
  `env.platform === 'darwin'` (`src/constants/figures.ts:4`) but branches the user pointer on
  *unicode support*, not on platform. Two different fallback tests.

**Glyph color.** Palette key `subtle`. When the message is selected in message-actions mode,
`suggestion` (`HighlightedThinkingText.tsx:24`:
`const pointerColor = isSelected ? "suggestion" : "subtle"`). **No bold, no dim modifier.**

**Body color.** Palette key `text` (`HighlightedThinkingText.tsx:99`). One exception: if
ultrathink is enabled, the characters of a detected thinking-trigger phrase are colored
per-character from a rainbow ramp (`HighlightedThinkingText.tsx:87`, `:121-129`, via
`findThinkingTriggerPositions` / `getRainbowColor` in `src/utils/thinking.ts`). Everything
outside the trigger spans stays `text`.

**Background box — yes, and it is real and full width.**

`UserPromptMessage.tsx:76`:

```
<Box flexDirection="column"
     marginTop={addMargin ? 1 : 0}
     backgroundColor={isSelected ? 'messageActionsBackground'
                    : useBriefLayout ? undefined
                    : 'userMessageBackground'}
     paddingRight={useBriefLayout ? 0 : 1}>
```

How wide is that box, exactly:

- The box declares no `width`, and its parent is `<Box flexDirection="column" width={t2}>`
  with `t2 = containerWidth ?? "100%"` (`Message.tsx:191`, `:212`).
- `containerWidth` is the terminal column count: `MessageRow.tsx:227` sets
  `t7 = hasMetadata ? undefined : columns` and passes it as `containerWidth`
  (`MessageRow.tsx:233`). `hasMetadata` is only true for *assistant* messages in transcript
  mode (`MessageRow.tsx:219`), so for a user prompt it is always `columns`.
- Yoga's default `alignItems: stretch` therefore gives the message box the full terminal width.

How the background is painted, and hence how it covers wrapped lines
(`src/ink/render-node-to-output.ts:1163-1179`):

```
const ownBackgroundColor = node.style.backgroundColor
if (ownBackgroundColor || node.style.opaque) {
  const innerWidth  = Math.floor(width)  - borderLeft - borderRight
  const innerHeight = Math.floor(height) - borderTop  - borderBottom
  const spaces   = ' '.repeat(innerWidth)
  const fillLine = applyTextStyles(spaces, { backgroundColor: ownBackgroundColor })
  output.write(x + borderLeft, y + borderTop, Array(innerHeight).fill(fillLine).join('\n'))
}
```

Ink pre-fills the whole box rectangle with background-styled spaces, then renders children on
top, and separately propagates the color down as `inheritedBackgroundColor`
(`render-node-to-output.ts:1195`, `:552-553`) so the text cells carry the same background. The
result: **every row the message occupies is background-colored from column 0 to column
`columns - 1`, including all soft-wrapped rows, the short tail of the last row, and the one
trailing column reserved by `paddingRight={1}`.** It is a solid block, not a highlight behind
the glyphs.

**Indent, in columns.**

- First line: glyph at column 0, space at column 1, body text starts at column **2**.
- Continuation (soft-wrapped) lines: column **0**. There is **no hanging indent**.
  This is deliberate structure, not an accident: the pointer `<Text>` and the body `<Text>`
  are siblings inside a single `<Text>` (`HighlightedThinkingText.tsx:107`, `:153`), and Ink
  squashes nested text nodes into one styled string before wrapping
  (`src/ink/squash-text-nodes.ts:18-45`). The whole `"❯ hello world…"` string is wrapped as a
  unit, so row 2 begins flush left.
- Hard newlines inside the prompt behave the same way: they also start at column 0.

**Wrapping.** Wrap width = `columns - 1` (full terminal width minus `paddingRight={1}`).
`<Text>` defaults to `wrap: 'wrap'` (`src/ink/components/Text.tsx:132`, `:38`), i.e. word wrap.
The interaction with the indent is the point above: the indent does not reduce the wrap width
for continuation rows, because there is no continuation indent.

**Blank-line rules.** One rule, uniform:

- `addMargin` is `!hasMetadata` (`MessageRow.tsx:226`, `:233`), which is `true` for every
  user prompt in the normal REPL.
- `addMargin` maps to `marginTop={1}` on the message box (`UserPromptMessage.tsx:76`).
- Nothing anywhere sets `marginBottom` on a message.

So: **exactly one blank line before every message, zero after.** Consecutive user messages,
consecutive assistant messages, and user→assistant transitions are all separated by exactly
one blank row. Yoga does not collapse margins, but since only `marginTop` is ever used there
is nothing to collapse. The blank row is *outside* the background box (it is margin, not
padding), so it is **not** background-colored.

**Truncation.** `UserPromptMessage.tsx:28-30`, `:64-69`. If the prompt exceeds 10,000
characters it is displayed as the first 2,500 characters, then a literal line
`… +N lines …` (leading `…` = U+2026, then a space, `+`, the count, ` lines `, `…`), then the
last 2,500 characters. `N` is the count of newlines dropped from the middle.

**Variations.**

- *Selected* (message-actions cursor): background key becomes `messageActionsBackground` and
  the pointer becomes `suggestion` (`UserPromptMessage.tsx:76`,
  `HighlightedThinkingText.tsx:24`). Layout is unchanged.
- *Nested* (subagent panes, collapsed-group previews, queued-prompt preview): callers pass
  `addMargin={false}` — e.g. `UI.tsx:85`, `UI.tsx:270`, `UI.tsx:403`, `UI.tsx:561`,
  `PromptInputQueuedCommands.tsx:112`, `RemoteSessionDetailDialog.tsx:884`. The glyph, colors
  and background box are unchanged; only the leading blank line disappears.
- *Brief / "Kairos" layout* (`HighlightedThinkingText.tsx:25-80`): a completely different
  shape — no pointer, no background, a `You` label in palette key `briefLabelYou` (or `subtle`
  when queued) with a dimmed timestamp, and the whole block at `paddingLeft={2}`. It is gated
  behind the `KAIROS` / `KAIROS_BRIEF` build features plus a runtime opt-in
  (`UserPromptMessage.tsx:50-62`). **Recommend excluding from the parity spec** — it is an
  unreleased experiment, not the shipped look.
- *Replayed / resumed* messages: **not determined from source.** Checked `Messages.tsx`,
  `MessageRow.tsx`, `Message.tsx`, `UserPromptMessage.tsx` — the only "static vs live"
  distinction is `shouldRenderStatically` (`Messages.tsx:779`), which selects a render *path*
  for flicker reasons and carries no visual difference. No replay-specific styling was found.
- *Transcript mode*: `isTranscriptMode` reaches `UserPromptMessage` only to force
  `useBriefLayout` off (`UserPromptMessage.tsx:62`). No visual change to the normal layout.

### 2. Assistant message

**Component chain.** `MessageRow.tsx:233` → `Message.tsx:105` (`AssistantMessageBlock`) →
`Message.tsx:469` / `Message.tsx:511` (`AssistantTextMessage`) → the `default:` branch of the
switch at `AssistantTextMessage.tsx:224-266`. All the earlier `case` branches are error and
rate-limit texts that render through `MessageResponse` instead; they are a different visual
form (see "Out of scope" below).

**Leading glyph.** `BLACK_CIRCLE`, `src/constants/figures.ts:4`:

```
export const BLACK_CIRCLE = env.platform === 'darwin' ? '⏺' : '●'
```

- macOS: `⏺` (U+23FA, BLACK CIRCLE FOR RECORD).
- Everything else, including Windows and Linux: `●` (U+25CF, BLACK CIRCLE).
- The source comment says the U+23FA form "is better vertically aligned, but isn't usually
  supported on Windows/Linux". This is a **platform** test, not a unicode-support test.

Rendered at `AssistantTextMessage.tsx:232`:

```
<NoSelect fromLeftEdge={true} minWidth={2}>
  <Text color={isSelected ? "suggestion" : "text"}>{BLACK_CIRCLE}</Text>
</NoSelect>
```

Note there is **no space character** after the glyph. The blank column comes from the gutter
box's `minWidth={2}` (`NoSelect` is a plain `Box` with `noSelect` set —
`src/ink/components/NoSelect.tsx:58`), not from text.

**Glyph color.** Palette key `text`; `suggestion` when selected
(`AssistantTextMessage.tsx:232`). **Not bold, not dim.** The dot is the same color as the body
text — it reads as a bullet, not as an accent.

**Background box.** None in the normal case. `AssistantTextMessage.tsx:258` sets
`backgroundColor={t3}` where `t3 = isSelected ? "messageActionsBackground" : undefined`
(`:229`). So an assistant message only gets a background when the message-actions cursor is on
it, and then it is the same full-width fill mechanism described above (the outer box is
`width="100%"` at `:258`, inside the `width={columns}` wrapper at `Message.tsx:148`).

**Indent, in columns.**

- First line: glyph at column 0, gutter blank at column 1, body starts at column **2**.
- Continuation (soft-wrapped) lines: column **2**. This **is** a true hanging indent, and it
  is structural: the gutter and the body are siblings in a flex **row**
  (`AssistantTextMessage.tsx:249`: `<Box flexDirection="row">{t4}{t5}</Box>`), with the body in
  its own `<Box flexDirection="column">` (`:241`). The body box is a separate layout node
  positioned at x=2, so every row it produces starts at column 2.
- This is the single most important difference from the user message: **user text wraps back
  to column 0; assistant text wraps back to column 2.**

**Wrapping.** Wrap width = `columns - 2`. Word wrap, from the same `<Text>` default. Markdown
block structure (paragraph gaps, list markers, code fences) is produced upstream by
`Markdown.tsx` / `src/utils/markdown.ts` and rendered as ANSI strings; the exact markdown block
rules are a separate concern and were not specified here.

**Blank-line rules.** Identical to the user message: `marginTop={addMargin ? 1 : 0}` at
`AssistantTextMessage.tsx:258` (`t2` from `:227`), `addMargin` true in the normal REPL, no
`marginBottom`. One blank line before, none after.

**Streaming.** While the assistant reply is still streaming it is rendered outside the message
list, at `Messages.tsx:703-711`, in the **same** shape: `marginTop={1}`, `width="100%"`, a
`<Box minWidth={2}>` gutter holding `BLACK_CIRCLE` in palette key `text`, and the text in a
sibling `<Box flexDirection="column">`. The only differences are `StreamingMarkdown` instead of
`Markdown` and a plain `Box` instead of `NoSelect`. Visually indistinguishable from the
committed form, so there is no "settling" jump when streaming ends.

**Variations.**

- *Nested* (subagent output, collapsed groups, progress lines, remote-session preview): callers
  pass `shouldShowDot={false}` — `UI.tsx:85`, `UI.tsx:270`, `UI.tsx:403`, `UI.tsx:561`,
  `PromptInputQueuedCommands.tsx:112`, `RemoteSessionDetailDialog.tsx:884`. At
  `AssistantTextMessage.tsx:232` the whole gutter node becomes `false`, so it is not rendered:
  **no glyph and no 2-column gutter, body text at column 0 of its container**, and continuation
  lines also at column 0. These callers also pass `addMargin={false}`, so no leading blank line.
- *Selected*: dot → `suggestion`, background → `messageActionsBackground`. Layout unchanged.
- *Transcript mode with metadata*: `MessageRow.tsx:219-228` — when an assistant message in
  transcript mode has a timestamp or model, `hasMetadata` is true, which sets `addMargin={false}`
  (no leading blank line) and drops `containerWidth` (the width wrapper becomes `"100%"` of
  whatever the transcript container is, rather than the raw terminal column count).
- *Replayed / resumed*: **not determined from source**, same checks as for user messages.

**Out of scope of this ticket but adjacent:** the non-default branches of
`AssistantTextMessage` (API errors, context-limit, credit balance, invalid key, token revoked,
timeout, user abort, rate limit) render through `MessageResponse`, which prefixes with the
literal string `"  ⎿  "` — two spaces, `⎿` (U+23BF), two spaces — in a dim style, inside a
`NoSelect fromLeftEdge` gutter (`MessageResponse.tsx:22`). That is the "response/continuation"
visual form shared with tool results and belongs to the tool ticket.

### 3. Summary table

| | user | assistant |
|---|---|---|
| glyph | `❯` (fallback `>`, unicode test) | `⏺` on macOS, `●` elsewhere (platform test) |
| glyph source | `figures@6.1.0` `.pointer` | `BLACK_CIRCLE`, `src/constants/figures.ts:4` |
| glyph palette key | `subtle` (`suggestion` selected) | `text` (`suggestion` selected) |
| glyph weight | normal | normal |
| separator after glyph | one literal space, inside the text | none; 2-col gutter box |
| body palette key | `text` | markdown-dependent, base `text` |
| background | `userMessageBackground`, always, full width | none (only `messageActionsBackground` when selected) |
| first-line text column | 2 | 2 |
| continuation column | **0** | **2** |
| wrap width | `columns - 1` | `columns - 2` |
| blank line before | 1 | 1 |
| blank line after | 0 | 0 |

### 4. What the user-message background costs in ratatui

The ticket asked specifically whether the background is real. It is: a solid rectangle
`columns` wide × (number of wrapped rows) tall. Reproducing it in ratatui is the single
most expensive item in this ticket.

**Why it is not free.** `Paragraph` applies a *widget-level* style once —
`buf.set_style(area, self.style)` at `ratatui-0.29.0/src/widgets/paragraph.rs:426` and again on
the inner text area at `:439` — and after that only writes the cells that actually contain
glyphs: `render_text` loops over `StyledGrapheme`s and calls `set_symbol().set_style()`
per grapheme (`paragraph.rs:465-476`). There is no per-`Line` background fill. A `Line` with a
background style colors its glyph cells and stops at the end of the text.

**The three options, and their real costs:**

1. **Pad each line to the wrap width with spaces and style the padding.** Requires giving up
   `Paragraph::wrap`, because `WordWrapper` re-wraps whatever you hand it and discards trailing
   whitespace at break points — padded lines would be re-broken or stripped. So picopilot must
   do its own unicode-aware word wrapping (grapheme + east-asian width) before building the
   `Line`s, and re-do it on every terminal resize. That is a genuine subsystem, but it is
   probably wanted anyway: the current `Wrap { trim: false }` is also what destroys the
   assistant hanging indent (see below), so hand-rolled wrapping fixes two problems at once.
2. **Post-render buffer pass.** Render normally, remember which buffer rows each user message
   occupies, then walk those rows and `set_bg` on every cell. Cheap and exact, but requires
   knowing row spans after layout, which means the renderer must report them — awkward with
   `Paragraph`, natural if picopilot is already producing pre-wrapped lines.
3. **Drop the background.** Cheapest. Loses a distinctive part of the look; the map's stated
   fidelity bar ("identical in static layout") argues against it.

**Two extra constraints, whichever option is chosen:**

- picopilot's chat block currently has `Padding::horizontal(2)` (`src/tui.rs:2905`) and computes
  `inner_width = area.width - 4` (`:2906`). A "full width" background inside that block would
  stop 2 columns short on each side and would not look like Claude Code. The horizontal padding
  has to go, or the background has to be painted outside the block.
- Claude Code's background does **not** cover the separating blank line (it is `marginTop`,
  outside the box). So the blank row between two consecutive user messages must stay
  unstyled — two adjacent user messages read as two separate blocks, not one tall block.

### 5. picopilot today, and the delta

Current behavior, `src/tui.rs`:

- `entry_lines`, `src/tui.rs:2960-2967` — user: prefix `"❯ "`, prefix style
  `Rgb(240,177,94)` + `BOLD`, body style `Style::default()`.
- `entry_lines`, `src/tui.rs:2976-2982` — assistant: prefix from
  `speaker_prefix("●", agent_id)` (`src/tui.rs:3450-3455`), which yields `"● "` when there is
  no agent id and `"● {agent_id} "` when there is; prefix style
  `Rgb(154,230,180)`, **no bold**. (The ticket text says the assistant prefix is bold — it is
  not; only the user prefix carries `Modifier::BOLD`.)
- `markdown_prefixed_lines`, `src/tui.rs:3147-3171` — puts the prefix on index 0 and a literal
  `"  "` on every subsequent line.
- `chat_lines`, `src/tui.rs:2926-2935` — pushes one `Line::default()` after each entry and pops
  the trailing one, i.e. one blank line *between* entries, none before the first and none at the
  end.
- `draw_chat`, `src/tui.rs:2904-2911` — one `Paragraph` for the whole transcript,
  `Block::padding(Padding::horizontal(2))`, `Wrap { trim: false }`.

Delta to reach parity:

1. **User glyph color and weight.** `Rgb(240,177,94)` bold → palette key `subtle`, no bold.
   The orange accent moves off the user pointer entirely.
2. **Assistant glyph.** `●` unconditionally → `⏺` on macOS, `●` elsewhere. Color
   `Rgb(154,230,180)` → palette key `text`, i.e. the same color as the body. picopilot currently
   uses a green accent where Claude Code uses none.
3. **Selection colors.** picopilot has no message-actions selection concept. If it stays absent,
   the `suggestion` variants are dead rules; if a selection mode is added later, both glyphs and
   both backgrounds switch as described.
4. **User continuation indent must be removed.** picopilot indents user continuation lines by 2
   (`markdown_prefixed_lines`); Claude Code does not indent them at all. The assistant's 2-column
   indent is correct in intent.
5. **The 2-column indent must survive soft wrapping.** Today it does not.
   `markdown_prefixed_lines` only prefixes lines that already exist — i.e. hard newlines from the
   markdown renderer. ratatui's `Wrap` then re-wraps those lines and every soft-wrapped
   continuation returns to column 0 of the inner area. So picopilot's hanging indent is
   currently cosmetic and breaks on any line longer than the terminal. Fixing this requires the
   same hand-rolled wrapper as the background work in §4 — the indent must be applied *after*
   wrapping, not before.
6. **Blank lines.** picopilot: one blank line *between* entries, none before the first. Claude
   Code: one blank line *before* every entry, including the first. One extra leading blank row,
   and the rule becomes uniform per-message rather than per-gap. Same visible spacing between
   messages either way.
7. **User background box.** Entirely absent today. See §4.
8. **Horizontal padding.** `Padding::horizontal(2)` must go if the background is implemented,
   and in any case Claude Code has no global left inset — its glyphs sit at column 0 of the
   terminal, picopilot's sit at column 2.
9. **Prompt truncation.** Not implemented. Add the 10,000 / 2,500 / 2,500 rule with the literal
   `… +N lines …` marker.
10. **The `agent_id` in `speaker_prefix`.** `"● subagent "` has no Claude Code equivalent — the
    reference suppresses the dot entirely for nested/subagent output rather than labelling it.
    This is a picopilot-only surface and belongs to the picopilot-only ticket; flagged here only
    because it shares the prefix code path.

### 6. Explicitly not determined from source

- The exact `figures@6.1.0` fallback character for `pointer`. `node_modules` is absent;
  `bun.lock` gives the version and the `is-unicode-supported` dependency but not the table.
- Any replayed/resumed-message styling. Checked `Messages.tsx`, `MessageRow.tsx`, `Message.tsx`,
  `UserPromptMessage.tsx`, `AssistantTextMessage.tsx`; found only `shouldRenderStatically`
  (`Messages.tsx:779`), which is a render-path choice with no visual effect.
- How markdown blocks inside an assistant message space themselves (paragraph gaps, list
  indents, code fences). Produced by `src/utils/markdown.ts` via `marked`; not read for this
  ticket.
- Whether anything renders *between* the glyph column and the terminal edge on the right for
  either message type. Nothing was found, but the `justifyContent="space-between"` on the
  assistant's outer box (`AssistantTextMessage.tsx:258`) suggests a right-hand slot once existed.
  Currently that box has a single child, so it has no effect.
