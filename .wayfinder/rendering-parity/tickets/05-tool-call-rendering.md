---
label: wayfinder:research
name: Specify tool call rendering
status: closed
assignee: research-subagent
blocked_by: []
---

# Specify tool call rendering

## Question

What is the exact format of a tool call in the transcript — its header line, its argument
summary, and the result block beneath it?

This is the most visible difference from picopilot today, which prints a flat
`tool <name> [state]` header in blue with the raw output dumped underneath and no truncation.

Resolve by producing:

- The header line format: the leading glyph quoted verbatim with its per-platform variants,
  the palette key per state (pending, running, success, error), the tool's user-facing name,
  and any background color behind that name.
- Whether the glyph animates, and if so the timing. Flag this as a dynamic behaviour that
  depends on a render tick, and say what it would cost picopilot.
- How arguments are summarized per tool, and the character and line limits applied.
- The result block: its indent, whether a continuation glyph prefixes it, and the exact
  wording and threshold of the truncation indicator.
- The error presentation, and the in-progress presentation including any elapsed time or byte
  counter.
- Whether results can be collapsed or expanded, and what a verbose mode changes.

## Resolution

All citations are `path:line` inside `C:\dev\git\claude-code`. The tree is React-compiler
output with the original TSX preserved in trailing sourcemaps; both agree, and quotes below are
taken from the readable compiled JSX. Nothing here was observed rendered — no `bun`, no
`node_modules`, no `claude` binary on this machine — so every column count is derived from the
Ink flex tree, not measured. Items I could not settle are marked **UNVERIFIED**.

### 1. Shape of one tool call

A tool call is two independent transcript rows, not one block:

1. an **assistant** `tool_use` block → the header line (`AssistantToolUseMessage`), plus,
   while unresolved, an inline progress row;
2. a later **user** `tool_result` block → the result block (`UserToolResultMessage`).

Both are top-level rows in the same message list, so anything the model streams between them
(text, another tool call) is interleaved between header and result.

The header row is a single non-wrapping flex row:
`src/components/messages/AssistantToolUseMessage.tsx:228`

```
t12 = <Box flexDirection="row" flexWrap="nowrap" minWidth={t6}>{t7}{t9}{t10}{t11}</Box>
```

- `t7` = the status dot (`:186`)
- `t9` = the tool's user-facing name, bold (`:200`)
- `t10` = the argument summary wrapped in literal parentheses (`:210`)
- `t11` = an optional per-tool tag (`:218`)

There is **no separator between name and `(`** — `t10` is literally
`<Text>({renderedToolUseMessage})</Text>`. Rendered shape:

```
⏺ Bash(npm test)
```

The whole row is wrapped in `<Box … marginTop={t5} width="100%">` where
`t5 = addMargin ? 1 : 0` (`:182`, `:285`) — i.e. **one blank line above the header** when the
row is not a continuation. `minWidth` of the row is
`stringWidth(userFacingToolName) + (shouldShowDot ? 2 : 0)` (`:183`).

### 2. Header glyph, verbatim, per platform

`src/constants/figures.ts:4`

```ts
export const BLACK_CIRCLE = env.platform === 'darwin' ? '⏺' : '●'
```

- macOS: `⏺` (U+23FA BLACK CIRCLE FOR RECORD)
- Windows/Linux: `●` (U+25CF BLACK CIRCLE)

The source comment states the U+23FA form "is better vertically aligned, but isn't usually
supported on Windows/Linux". **picopilot on Windows must use `●`.**

The glyph sits alone in `<Box minWidth={2}>` (`src/components/ToolUseLoader.tsx:33`, and the
queued variant at `AssistantToolUseMessage.tsx:186`). So the dot column is **2 cells wide**:
glyph plus padding to column 2. Whether `⏺` itself measures 1 or 2 cells (and therefore whether
there is a real space after it) is **UNVERIFIED**; for `●` on Windows, treat it as glyph + one
space.

### 3. Header state colors (palette keys)

`src/components/ToolUseLoader.tsx:19`

```ts
const color = isUnresolved ? undefined : isError ? 'error' : 'success'
```

and `:21` sets `dimColor={isUnresolved}`. Combined with the queued branch at
`AssistantToolUseMessage.tsx:186`:

| state | condition | glyph | style |
| --- | --- | --- | --- |
| queued (not started, not resolved) | `!inProgressToolUseIDs.has(id) && !isResolved` | `●`/`⏺` | no color key, **dim**, static |
| running (in progress) | in `inProgressToolUseIDs` | `●`/`⏺` alternating with `' '` | no color key, **dim**, blinking |
| success | id in `resolvedToolUseIDs`, not errored | `●`/`⏺` | palette key `success` |
| error | id in `erroredToolUseIDs` | `●`/`⏺` | palette key `error`, **never blinks** (`:20`) |

Queued and running share the same dim styling; the only difference is the blink.

There is **no "pending permission" dot state** — permission waiting is expressed on the
progress row instead (section 7).

### 4. Tool name and its background color

`AssistantToolUseMessage.tsx:197-200`

```
const t8 = userFacingToolNameBackgroundColor ? "inverseText" : undefined;
t9 = <Box flexShrink={0}><Text bold wrap="truncate-end"
        backgroundColor={userFacingToolNameBackgroundColor} color={t8}>{userFacingToolName}</Text></Box>
```

- The name is always **bold**, and truncates at the end (`wrap="truncate-end"`) rather than
  wrapping.
- Normally there is **no background** and no explicit foreground key (default text color).
- The only tool that sets one is `AgentTool`: `userFacingNameBackgroundColor()` returns the
  per-agent color key from `agentColorManager` (`src/tools/AgentTool/UI.tsx:776-787`). When a
  background is present the foreground flips to the palette key `inverseText`.
- Names come from each tool's `userFacingName(input)` and are **input-dependent**, e.g.
  `Read` / `Reading Plan` / `Read agent output` (`src/tools/FileReadTool/UI.tsx:165-176`),
  `Update` / `Create` / `Updated plan` (`src/tools/FileEditTool/UI.tsx:24-44`),
  `Search` for Glob (`src/tools/GlobTool/UI.tsx:11-13`).
- `userFacingName() === ''` opts the tool out of all chrome — the whole header renders nothing
  (`AssistantToolUseMessage.tsx:158-160`).

Confirmed palette keys used by tool rendering, all present in `src/utils/theme.ts`:
`success` (:26), `error` (:27), `warning` (:28), `inverseText` (:18), `text` (:17),
`claude` (:7), `permission` (:11).

### 5. Blink animation — the render-tick dependency

`src/hooks/useBlink.ts`

```ts
const BLINK_INTERVAL_MS = 600            // :3
const focused = useTerminalFocus()
const [ref, time] = useAnimationFrame(enabled && focused ? intervalMs : null)
if (!enabled || !focused) return [ref, true]     // :29
const isVisible = Math.floor(time / intervalMs) % 2 === 0   // :32
```

`src/components/ToolUseLoader.tsx:20`

```ts
const t1 = !shouldAnimate || isBlinking || isError || !isUnresolved ? BLACK_CIRCLE : ' '
```

Exact behaviour:

- **600 ms half-period, 1200 ms full cycle.** The dot is replaced by a single space `' '`
  during the off half. The `minWidth={2}` box keeps the column, so the name never shifts.
- All dots on screen blink **in phase**, because `isVisible` is derived from a shared
  animation clock rather than per-component timers.
- The clock is **paused when the terminal is unfocused** and when the element is offscreen.
- Blinking applies only when `isUnresolved && !isError && shouldAnimate`. Errors and finished
  calls are solid; queued dots are solid-dim.
- `shouldAnimate` is further gated: `canAnimate` is false while a tool-confirm prompt, a
  `toolJSX` overlay or the message selector is up (`src/components/Messages.tsx:595`), and per
  row it is true only if that row's tool is in `inProgressToolUseIDs`
  (`src/components/MessageRow.tsx:168-215`). `shouldShowDot` is hard-coded `true` for normal
  transcript rows (`MessageRow.tsx:233`).

**Cost to picopilot (flagged as required dynamic behaviour):** picopilot redraws only on
events, so this is the one item in this ticket that cannot be met by a static renderer. To
match it, the live region must be re-rendered on a timer while any tool is unresolved — a
600 ms tick (or a faster tick with a 600 ms phase computation) driven from the event loop,
stopping when nothing is unresolved. A single global clock is enough and is what Claude Code
does; per-entry timers would break phase alignment. If picopilot declines the tick, the
correct static fallback is the **solid dim dot** — that is exactly the `!shouldAnimate` branch
at `ToolUseLoader.tsx:20`, so a non-animating picopilot is still *inside* the reference's own
behaviour rather than a deviation. Also note that not ticking loses the elapsed-time counter
in section 7, which is a second, independent reason a tick is wanted.

### 6. Argument summary per tool, with limits

Produced by each tool's `renderToolUseMessage(input, { theme, verbose, commands })`
(dispatch at `AssistantToolUseMessage.tsx:163`, helper at `:295-317`) and printed inside `( )`.
Returning `''` suppresses the parentheses entirely (`:210`); returning `null` suppresses the
whole header (`:179-181`).

| tool | summary | limits |
| --- | --- | --- |
| Bash | the command itself | **2 lines max, 160 chars max**, then `…` appended — `src/tools/BashTool/UI.tsx:26-27,112-125` |
| Bash (`sed -i`) | the target file path only, via `getDisplayPath` | — (`UI.tsx:97-101`) |
| Read | `getDisplayPath(file_path)` as an OSC-8 link; `path · pages N`; verbose adds `path · lines A-B` or `path · from line N` | — (`FileReadTool/UI.tsx:30-65`) |
| Edit / Write | `getDisplayPath(file_path)` as a link; `''` for plan files | — (`FileEditTool/UI.tsx:57-73`) |
| Grep | `pattern: "…"` and, if given, `, path: "…"` | — (`GrepTool/UI.tsx:127-137`) |
| Glob | `pattern: "…", path: "…"` | — (`GlobTool/UI.tsx:14-31`) |

Bash truncation, exactly (`BashTool/UI.tsx:112-125`): first cut to the first
`MAX_COMMAND_DISPLAY_LINES = 2` lines if there are more; then, if still longer than
`MAX_COMMAND_DISPLAY_CHARS = 160`, hard-slice to 160 chars; then `.trim()` and append a single
`…` (U+2026). The ellipsis is appended only when a truncation actually happened. Under
fullscreen mode only, a leading `# comment` label replaces the command and is cut at 160 chars
with `…` (`:104-110`).

`getDisplayPath` (`src/utils/file.ts`) prefers a cwd-relative path, else `~/…`, else the
absolute path.

Separately, `TOOL_SUMMARY_MAX_LENGTH = 50` (`src/constants/toolLimits.ts:56`) with
`truncate()` (`src/utils/truncate.ts`) caps `getToolUseSummary()`. That is **not** the header —
its docstring says it is "for display in compact views … grouped agent rendering". Do not apply
50 to the header line.

### 7. In-progress presentation (elapsed time, byte counter)

While `!isResolved && !isQueued`, a second row renders below the header
(`AssistantToolUseMessage.tsx:240-263`), in priority order:

1. classifier check → `Auto classifier checking…` or `Bash classifier checking…`, dim;
2. `isWaitingForPermission` → `Waiting for permission…`, dim;
3. otherwise the tool's `renderToolUseProgressMessage`.

All of these are wrapped in `<MessageResponse height={1}>` — the same `⎿` gutter as the result
block (section 9), clipped to exactly one line.

Bash's progress (`BashTool/UI.tsx:128-152` → `src/components/shell/ShellProgressMessage.tsx`):

- **No output yet:** `Running… ` (dim, trailing space) then the time display
  (`ShellProgressMessage.tsx:58`).
- **With output:** the **last 5 lines** of stripped-ANSI output, dim, in a box clipped to
  `min(5, lines.length)` rows (`:44`, `:81-99`), then a one-line status row of up to three dim
  fields joined with one space (`gap={1}`, `:120-127`):
  - line status — `~${totalLines} lines` when byte and line totals are known, else
    `+${extraLines} lines` where `extraLines = max(0, totalLines - 5)` (`:74-81`);
  - elapsed/timeout — `ShellTimeDisplay`;
  - byte counter — `formatFileSize(totalBytes)`.

`ShellTimeDisplay` (`src/components/shell/ShellTimeDisplay.tsx`) emits exactly one of:

```
(timeout 2m)
(1m 5s · timeout 2m)
(12s)
```

The `·` is U+00B7. `formatDuration` gives `0s`, `12s` (floored whole seconds under a minute,
one decimal only below 1 ms), then `Xm Ys` style above a minute. `formatFileSize`
(`src/utils/format.ts`) gives `512 bytes`, `1.5KB`, `2MB`, `1.1GB` — one decimal, trailing
`.0` stripped, no space before the unit.

Queued calls render `Waiting…` dim in the same gutter (`BashTool/UI.tsx:154-158`, text at `:156`).

**Render-tick dependency:** the elapsed-time and byte counters advance because the tool emits
progress messages, so they update on *events*, not on an animation frame — picopilot can drive
them from its existing event redraws. Only the dot blink needs a timer.

### 8. Result block: indent and continuation glyph

Every result, progress, error and cancellation body goes through `MessageResponse`
(`src/components/MessageResponse.tsx:22`):

```jsx
<NoSelect fromLeftEdge flexShrink={0}><Text dimColor>{'  '}⎿ &nbsp;</Text></NoSelect>
```

Verbatim, the prefix is: **space, space, `⎿` (U+23BF), space, U+00A0 no-break space** — five
cells, all rendered **dim**. So:

- **The gutter glyph is `⎿` and it prefixes only the first line of the result block.** It is
  laid out as a fixed-width flex sibling, not repeated per line. Wrapped and subsequent lines
  of the same result align at **column 5** (0-based) with **no continuation glyph** — plain
  spaces.
- Column arithmetic against the header: the dot occupies columns 0-1, so the tool name starts
  at column 2, and result text starts at column 5.
- Nested results do **not** stack glyphs: `MessageResponseContext` makes an inner
  `MessageResponse` render its children bare (`MessageResponse.tsx:16-18, 62`).
- `NoSelect fromLeftEdge` excludes the gutter from terminal text selection.
- `height={1}` variants clip to a single row with `overflowY="hidden"` (`:37`).
- The result subtree is given `width = columns - 5` (`src/components/Message.tsx:408`), i.e.
  the parent reserves exactly the gutter width.

Verbose Grep/Glob results bypass the component and hand-roll the same string
(`GrepTool/UI.tsx:63-95`): `<Text dimColor>  ⎿  </Text>` on the summary line, then
`<Box marginLeft={5}>` (`:82`) for the body — confirming 5 as the canonical indent.

### 9. Result truncation: exact thresholds and wording

There are three distinct truncators. They do not share a threshold.

**(a) Shell stdout/stderr — 3 lines.** `src/utils/terminal.ts`

```ts
const MAX_LINES_TO_SHOW = 3                 // :7
const PADDING_TO_PREVENT_OVERFLOW = 10      // :10
const wrapWidth = Math.max(terminalWidth - PADDING_TO_PREVENT_OVERFLOW, 10)   // :81
```

Content is wrapped to `wrapWidth` (ANSI-aware, each line `trimEnd()`-ed), then:

- if exactly **1** line would remain after the fold, that line is shown instead of a hint —
  `slice(0, MAX_LINES_TO_SHOW + 1)`, remaining forced to 0 (`:44-53`);
- otherwise the first **3** wrapped lines are shown and the indicator is appended on its own
  line (`:104-108`):

```
… +${estimatedRemaining} lines (ctrl+o to expand)
```

Exact wording: `'… +'` + count + `' lines'` (**always plural, even for large-N-only cases**,
since N=1 is handled by the branch above), then a space, then `ctrlOToExpand()` which returns
`(${shortcut} to expand)` where the default shortcut is `ctrl+o`
(`src/components/CtrlOToExpand.tsx:46-49`; `src/keybindings/defaultBindings.ts:44` maps
`ctrl+o` → `app:toggleTranscript`). The whole indicator is dim. The `…` is U+2026.
The expand hint is suppressed (leaving just `… +N lines`) inside a sub-agent, inside the
virtual list, and when `suppressExpandHint` is passed (`terminal.ts:74,107`;
`CtrlOToExpand.tsx:34-36`).

A pre-truncation guard caps work at `3 * wrapWidth * 4` characters and then *estimates* the
remaining count as `ceil(len / wrapWidth) - 3` (`:85-101`) — so for very large outputs the
number in `… +N lines` is an estimate, not a count.

**(b) Tool errors — 10 lines.** `src/components/FallbackToolUseErrorMessage.tsx`

- `MAX_RENDERED_LINES = 10` (`:11`); `plusLines = (newline count + 1) - 10` (`:49`).
- Shows the first 10 lines in palette key `error`, then, when `plusLines > 0` and not verbose
  (`:86`), a dim line with **different wording from (a)**:

```
… +N line (ctrl+o to see all)      // N === 1
… +N lines (ctrl+o to see all)     // otherwise
```

`line`/`lines` is pluralised here, and the verb is **"to see all"**, not "to expand". The
shortcut text inside is rendered **bold**.

**(c) Write results — 10 lines.** `src/tools/FileWriteTool/UI.tsx:26,51,108`: first 10 lines of
content, then `… +N line(s) ` followed by the `(ctrl+o to expand)` hint.

### 10. Error presentation

Dispatch is `UserToolResultMessage.tsx:38-86`, on the `tool_result` block:

- content starts with `CANCEL_MESSAGE` → `UserToolCanceledMessage`;
- content starts with `REJECT_MESSAGE` / equals `INTERRUPT_MESSAGE_FOR_TOOL_USE` →
  `UserToolRejectMessage`;
- `is_error === true` → `UserToolErrorMessage`;
- otherwise → `UserToolSuccessMessage`.

`UserToolErrorMessage.tsx` then special-cases, in order: interrupt → `InterruptedByUser`;
plan rejection; reject-with-reason; classifier denial using `BULLET_OPERATOR` U+2219
(`Denied by auto mode classifier ∙ /feedback if incorrect`, `UserToolErrorMessage.tsx:76`);
else the tool's own `renderToolUseErrorMessage`, else `FallbackToolUseErrorMessage`.

The fallback normalises the text (`FallbackToolUseErrorMessage.tsx:33-47`): extract
`<tool_use_error>`, strip sandbox-violation tags, strip `<error>`/`</error>`, `trim()`, then:

- non-verbose and containing `InputValidationError: ` → the whole body becomes
  `Invalid tool parameters`;
- if it already starts with `Error: ` or `Cancelled: `, keep as-is;
- otherwise **prefix `Error: `**.

The body is rendered in palette key `error`, inside the `⎿` gutter, with the 10-line
truncation from 9(b).

Per-tool short forms replace all of that when not verbose (each inside the gutter):

- Read: `File not found` (error), `Error reading file` (error) — `FileReadTool/UI.tsx:144-160`
- Grep/Glob: `File not found`, `Error searching files` — `GrepTool/UI.tsx:147-160`
- Edit: `File must be read first` (**dim, not error color**, `:141`), `File not found` (error),
  `Error editing file` (error) — `FileEditTool/UI.tsx:128-152`

Cancellation/interruption renders one dim line (`src/components/InterruptedByUser.tsx:8`):

```
Interrupted · What should Claude do instead?
```

(`·` is U+00B7; the whole line is dim, clipped to `height={1}`.)

### 11. Success result bodies (for reference)

- Bash: stdout via `OutputLine`, then stderr via `OutputLine` with palette key `error`; if both
  are empty, one dim line — `(No output)`, or `Done` when the tool declared no output expected,
  or the return-code interpretation, or `Running in the background (↓ to manage)`
  (`BashToolResultMessage.tsx:156`). Image output → `[Image data detected and sent to Claude]`
  (`:103`).
- Read: `Read <bold>N</bold> line`/`lines`, `Read image (1.5KB)`, `Read PDF (…)`,
  `Read <bold>N</bold> cells`, `Read <bold>N</bold> page(s) (…)`, `Unchanged since last read`
  (dim) — `FileReadTool/UI.tsx:77-140`, each `height={1}`.
- Grep/Glob: `Found <bold>N </bold>files` / `lines` / `matches`, optionally
  ` across <bold>M </bold>files`, then the `(ctrl+o to expand)` hint when `N > 0`; the singular
  form is produced by dropping the label's last character (`GrepTool/UI.tsx:35-41`, so
  `files`→`file`, `matches`→`matche` — a latent bug, reproduce it or don't, but note it).
- Edit: `Added <bold>N</bold> lines, removed <bold>M</bold> lines` (capital `R` when there were
  no additions), then the structured diff at `columns - 12`
  (`FileEditToolUpdatedMessage.tsx:36-46, 88`).
- Write: `Wrote <bold>N</bold> lines to <path>` plus up to 10 content lines
  (`FileWriteTool/UI.tsx:79, 392`).

### 12. Collapse / expand and verbose

Three separate mechanisms, all reachable from the truncation hints:

1. **`ctrl+o` → `app:toggleTranscript`** (`src/keybindings/defaultBindings.ts:44`,
   `src/hooks/useGlobalKeybindings.tsx:188`) switches to the transcript screen, which renders
   the same message list with **`verbose={true}`** (`src/screens/REPL.tsx:4402`). This is what
   every "to expand" / "to see all" hint refers to.
2. **Per-row expansion.** `Messages.tsx:624` passes
   `verbose={verbose || isItemExpanded(msg) || (cursor?.expanded && index === selectedIdx)}`,
   so a single row can be expanded via the virtual list's cursor without leaving the main
   screen (`Messages.tsx:572`).
3. **`ExpandShellOutputProvider`** (`src/components/shell/ExpandShellOutputContext.tsx`) forces
   full shell output for a subtree; used to auto-expand the most recent `!` command.
   `OutputLine.tsx:61` reads `shouldShowFull = verbose || expandShellOutput`.

What `verbose = true` changes, concretely:

- shell output is not truncated at all (`OutputLine.tsx:69`);
- error bodies show all lines and skip the `… +N lines` hint
  (`FallbackToolUseErrorMessage.tsx:57, 86`), and per-tool short error forms are bypassed in
  favour of the full text;
- Bash header shows the full command with no 2-line/160-char cut (`BashTool/UI.tsx:104`);
- file paths are shown absolute instead of `getDisplayPath`-shortened, and Read adds
  ` · lines A-B`;
- Grep/Glob switch from the one-line `Found N files` to a `  ⎿  ` header plus the full file
  list indented 5 (`GrepTool/UI.tsx:62-95`);
- shell progress shows the full output instead of the last 5 lines
  (`ShellProgressMessage.tsx:44`) and drops the `+N lines` status.

There is **no per-tool-call fold/unfold affordance** in the transcript — no arrow, no
`[+]` marker. Collapsing is a global/row-level mode, and the only visible affordance is the
`(ctrl+o to expand)` text on the truncation line.

`CollapsedReadSearchContent` (`src/components/messages/CollapsedReadSearchContent.tsx`) and
`GroupedToolUseContent` do collapse *runs* of Read/Search calls into one summarised block, but
that is a grouping concern and belongs to a separate ticket, not to the single-tool-call spec.

### 13. Delta against picopilot today

`src/tui.rs:2992-3031` renders `ChatEntry::Tool` as one `labeled_lines` call: a label
`tool {name} [{state}]` left-padded to 18 columns in `Color::Rgb(139, 181, 255)`, followed by
`$ {command}` and the raw output, with every continuation line indented by a 19-space run in
the same style (`src/tui.rs:3115-3131`). States are the strings `unknown`, `running`, `done`,
`failed`. Non-shell tools are dropped entirely unless `show_internals` is on
(`src/tui.rs:3000`, `src/tui.rs:3133-3137`), and `tool_command` only recognises the argument
keys `command`, `cmd`, `script`, `fullCommandText` (`src/tui.rs:3138-3147`). There is no dot,
no gutter glyph, no truncation, no per-tool argument summary, and no blank line above the
header. Every item in this resolution is therefore new work.
