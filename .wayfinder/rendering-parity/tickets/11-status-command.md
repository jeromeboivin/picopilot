---
label: wayfinder:research
name: Specify the status command
status: closed
assignee: research-subagent
blocked_by: [08-spinner-line]
---

# Specify the status command

## Question

What does `/status` print, and which of the deleted status bar's fields land there?

Deleting the status bar orphans: project name, model, reasoning effort, autopilot mode, active
tool and skill counts, token usage against the context limit, and session cost.
[Specify the spinner line](08-spinner-line.md) claims some of them; this ticket houses the
rest.

Resolve by producing:

- What the reference's own status command prints, and its layout.
- A field-by-field placement for every orphaned picopilot field: spinner line, `/status`
  output, somewhere else, or dropped — with a reason for each.
- The rendering of the `/status` output itself as a transcript block: glyph, indent, grouping
  and palette keys.
- Whether any field is urgent enough to need a persistent home despite the bar being gone —
  approaching the context limit is the obvious candidate — and if so where it goes.

## Resolution

Settled from source. Reference read at `C:\dev\git\claude-code`; the files there are compiled
output with an inline base64 sourcemap appended, so line numbers are lines of the file **as it
sits on disk**. picopilot facts read from `src/tui.rs` and `src/events.rs` at the current HEAD.
Nothing was executed — no `bun`, no `node_modules`, no `claude` binary (map "Known risk,
accepted"). Everything I could not confirm is flagged **Unverified**.

### 1. The headline finding: the reference's `/status` is not a printed block

`src/commands/status/index.ts:3-11`

```ts
const status = {
  type: 'local-jsx',
  name: 'status',
  description: 'Show Claude Code status including version, model, account, API connectivity, and tool statuses',
  immediate: true,
  load: () => import('./status.js'),
} satisfies Command
```

`src/commands/status/status.tsx:6` — `return <Settings onClose={onDone} context={context} defaultTab="Status" />`.

So `/status` is the **Settings screen opened on its Status tab**: a `Pane` in colour `permission`
wrapping a `Tabs` widget with three tabs, `Status` / `Config` / `Usage`
(`src/components/Settings/Settings.tsx:73-95,119`), height
`max(15, min(floor(rows * 0.8), 30))` (`Settings.tsx:37`). It is an interactive live surface,
not text committed to the transcript.

**What it leaves behind is one line.** `Settings.tsx:46-48`:

```ts
onClose("Status dialog dismissed", { display: "system" })
```

and `display: 'system'` commits exactly two entries — the command echo and the output block
(`src/utils/processUserInput/processSlashCommand.tsx:594-596`). So the reference's full
transcript trace of a `/status` invocation is:

```
❯ /status
  ⎿  Status dialog dismissed
```

This is the same picker-leaves-one-line rule that
[10-inline-pickers](10-inline-pickers.md) §5 already established.

**Contradiction in the reference, recorded for honesty.** `REPL.tsx:4596-4601` says in a comment
that `/status` is *non-immediate* and therefore renders inside the scrollable transcript column,
while `status/index.ts:7` sets `immediate: true` and `processSlashCommand.tsx:635` derives
`isImmediate: command.immediate === true`. Following the code rather than the comment,
`/status` renders in the **fixed bottom block** (`REPL.tsx:4603`), where the input box normally
sits. **The comment is stale.** This does not change the spec below, but it does mean the
comment cannot be used as evidence for anything.

`/cost` is the opposite shape and is the model to copy. `src/commands/cost/index.ts:9` —
`type: 'local'`; `src/commands/cost/cost.ts:6-25` returns `{ type: 'text', value }`, whose value
is `formatTotalCost()` (`src/cost-tracker.ts:228-245`), a fixed-label block:

```
Total cost:            $0.42
Total duration (API):  1m 5s
Total duration (wall): 4m 12s
Total code changes:    31 lines added, 7 lines removed
       sonnet-4-5: 12.3k input, 4.1k output, 88.0k cache read ($0.42)
```

Whole thing wrapped in `chalk.dim` (`cost-tracker.ts:236`), labels padded to a common column,
per-model rows right-aligned with `padStart(21)` (`cost-tracker.ts:224`).

**Recommendation for picopilot: `/status` is a printed static block, not a pane.** picopilot has
no Config screen and no Usage screen to justify tabs, and
[10-inline-pickers](10-inline-pickers.md) §9 already made the read-only usage surface a printed
block. Adopt `/cost`'s `type: 'local'` shape and the Status tab's *content*. That decision is
what §4 and §5 below specify.

### 2. What the reference's Status tab actually prints

`src/components/Settings/Status.tsx`. Two property sections plus an optional diagnostics
section, in this order.

**Section 1 — identity** (`Status.tsx:19-35`, `buildPrimarySection`):

| Label | Value | Source |
| --- | --- | --- |
| `Version` | build version macro | `Status.tsx:24-26` |
| `Session name` | custom title, or dim `/rename to add a name` | `Status.tsx:22,27-29` |
| `Session ID` | session uuid | `Status.tsx:30-32` |
| `cwd` | **full working-directory path**, not a basename | `Status.tsx:33-34` (`getCwd()`) |
| `Login method` | `${subscription} Account` | `src/utils/status.tsx:206-211` |
| `Auth token`, `API key`, `Organization`, `Email` | conditional | `status.tsx:212-234` |
| `API provider` and friends | only when not first-party | `status.tsx:240-346` |

**Section 2 — configuration** (`Status.tsx:36-51`, `buildSecondarySection`):

| Label | Value | Source |
| --- | --- | --- |
| `Model` | `getModelDisplayLabel(mainLoopModel)` | `Status.tsx:48-51`, `utils/status.tsx:353-357` |
| `IDE` | connection state | `utils/status.tsx:38-87` |
| `MCP servers` | **a count summary, not a list** | `utils/status.tsx:89-115` |
| `Bash Sandbox` | ant-only, absent externally | `utils/status.tsx:28-36` |
| `Setting sources` | array value | `utils/status.tsx:124-172` |

The `MCP servers` row is the pattern picopilot's tool and skill counts should copy verbatim
(`utils/status.tsx:96-115`). Its own comment says why: *"20+ servers wrapped onto many rows,
dominating the Status pane. Show counts by state + /mcp hint."* The value is built as

```ts
if (byState.connected) parts.push(color('success', theme)(`${byState.connected} connected`))
if (byState.needsAuth)  parts.push(color('warning', theme)(`${byState.needsAuth} need auth`))
if (byState.pending)    parts.push(color('inactive', theme)(`${byState.pending} pending`))
if (byState.failed)     parts.push(color('error', theme)(`${byState.failed} failed`))
value: `${parts.join(', ')} ${color('inactive', theme)('· /mcp')}`
```

so: comma-separated counts, each coloured by its state, then a dim `· /command` pointer to the
surface that can change it.

**Section 3 — diagnostics**, only when non-empty (`Status.tsx:186-237`): a bold header
`System Diagnostics` (`Status.tsx:215`), then one row per diagnostic at `paddingX={1}` with
`figures.warning` in colour `error` and `gap={1}` (`Status.tsx:238-240`).

**Row layout** (`Status.tsx:191-193`):

```jsx
<Box flexDirection="row" gap={1} flexShrink={0}>
  {label !== undefined && <Text bold>{label}:</Text>}
  <PropertyValue value={value} />
</Box>
```

- Label is **bold, with a trailing colon, no colour** — it inherits `text`.
- `gap={1}` is exactly one space between label and value. Values are **not** aligned to a common
  column; the value starts one space after each label's own colon. (`/cost` does pad to a common
  column — `cost-tracker.ts:238-243` — so the two surfaces genuinely differ.)
- A label may be omitted entirely, giving a value-only row (`utils/status.tsx:270-273`).
- Array values render as a wrapping row with `columnGap={1}` and a `,` appended to every item
  except the last (`Status.tsx:62,79`).
- **Sections are separated by one blank line** — the container is
  `<Box flexDirection="column" gap={1}>` (`Status.tsx:152`) — and rows *inside* a section have no
  blank line between them.
- The whole thing is closed by a dim `Esc to cancel` hint (`Status.tsx:161`), which is a
  consequence of it being an interactive pane and does not survive picopilot's printed-block form.

**Not on the reference's `/status`:** reasoning effort, session cost, token usage, context
percentage, and any tool inventory. Effort is only on the spinner line and the model picker
(`utils/status.tsx:353-357` and `modelDisplayString`, `src/utils/model/model.ts:556-567`, neither
of which appends an effort suffix). Cost is `/cost`; context accounting is `/context`.

### 3. Field-by-field placement

picopilot's real bar is `status_bar` at [tui.rs#L2065](src/tui.rs#L2065), whose format string is
[tui.rs#L2086](src/tui.rs#L2086):

```rust
" {project}{model}  ·  {reasoning} reasoning  ·  autopilot {mode}  ·  tools {}/{}  ·  skills {}/{}  ·  {context} tokens  ·  {cost} "
```

Eight fields. Placement:

| # | picopilot field | Source | Goes to | Why |
| --- | --- | --- | --- | --- |
| 1 | **Project name** — `working_directory_name`, the cwd *basename* | [tui.rs#L2067](src/tui.rs#L2067), [tui.rs#L2096](src/tui.rs#L2096) | **`/status`**, as `cwd`, **widened to the full path** | Static for the whole session, so it earns nothing from being on screen every frame. The reference has exactly this row and prints the full path (`Status.tsx:33-34`); a basename is ambiguous across worktrees, and `/status` is the one place with room for the full path. |
| 2 | **Model** | [tui.rs#L2072](src/tui.rs#L2072) | **`/status`**, as `Model:` | Ticket 08 claims only the token count and effort for the spinner line, and the model is not in `SpinnerAnimationRow`'s row anatomy at all. The map's older note that "model rides the spinner line" is superseded by [08-spinner-line](08-spinner-line.md) §1. Mid-session the model is announced by the picker's commit line, `Set model to <name>` ([10-inline-pickers](10-inline-pickers.md) §5), so the standing display is only needed on demand. |
| 3 | **Reasoning effort** | [tui.rs#L2073](src/tui.rs#L2073) | **Spinner line — already claimed**, and **deliberately not on `/status`** | [08-spinner-line](08-spinner-line.md) §9 owns it as the `thinking with high effort` status part. Do not duplicate it on `/status`: the reference's `Model` row has no effort suffix (`utils/status.tsx:353-357`; `model.ts:556-567`). It is also editable on the model picker's `←`/`→` axis ([10-inline-pickers](10-inline-pickers.md) §9). |
| 4 | **Autopilot mode** — literally `working` / `ready` from `status.busy` | [tui.rs#L2074](src/tui.rs#L2074) | **Dropped** | It is a busy flag with a misleading name, and the busy state is already carried three ways: the spinner row exists only while busy (`REPL.tsx:4587`), the input `❯` dims while the agent works ([09-input-box](09-input-box.md)), and the tool dots blink. The reference has no idle-state text anywhere. Nothing is lost. |
| 5 | **Tool count** `tools n/TOOL_COUNT` | [tui.rs#L2087](src/tui.rs#L2087) | **`/status`**, one row in the `MCP servers` count-summary shape | A number that only changes when the user opens the tool picker, and the picker's own commit line reports the change. Row shape from `utils/status.tsx:96-115`: `Tools: 7 enabled` in `success`, `, 2 disabled` in `inactive`, then a dim `· /tools`. |
| 6 | **Skill count** `skills n/m` | [tui.rs#L2089](src/tui.rs#L2089) | **`/status`**, same shape, dim pointer `· /skills` | Same reasoning. This also gives [10-inline-pickers](10-inline-pickers.md) §9 the home it wanted for the dropped skill detail pane's `Source:` and `Directory:` diagnostics — a `/skills` listing, not `/status` itself. |
| 7 | **Token usage vs context limit** `{current}/{limit} tokens` | [tui.rs#L2075](src/tui.rs#L2075), `format_tokens` [tui.rs#L3461](src/tui.rs#L3461) | **Split three ways: spinner line (different number), printed usage block, and a persistent footer warning** | The reference never puts context usage on the spinner line — that number is `characters / 4` of *this turn's output* ([08-spinner-line](08-spinner-line.md) §8), a different quantity. Full accounting belongs in the printed usage block, which already renders `Context window: x / y tokens` plus the category attribution (`usage_detail_lines`, [tui.rs#L2853](src/tui.rs#L2853)) — the analog of the reference's `/context`. Only the *threshold crossing* needs to be always-visible; see §6. |
| 8 | **Session cost** — `format_cost`, AIU or premium requests | [tui.rs#L2081](src/tui.rs#L2081), [tui.rs#L2886](src/tui.rs#L2886) | **Printed usage block (`/usage`), not `/status`** | Aligns with [10-inline-pickers](10-inline-pickers.md) §9, which already routed the usage modal to a printed block, and with the reference's own split: cost is `/cost` (`cost/index.ts:9`), never the Status tab. `usage_detail_lines` already opens with `Session cost:` ([tui.rs#L2842](src/tui.rs#L2842)). |

Nothing is dropped except field 4.

**One addition `/status` should gain**, because the reference has it and picopilot's bar never
did: `Version` and `Session ID` (`Status.tsx:24-32`). Both are pure support-ticket data and both
are free once the block exists. `Session name` is only worth adding if picopilot gains a rename
command; **Unverified** — no rename command exists in the codebase today
(only `/fleet` is registered, [tui.rs#L186](src/tui.rs#L186)).

### 4. Rendering the `/status` output as a transcript block

Because §1 makes `/status` a `type: 'local'` printed block, its rendering is entirely determined
by the reference's local-command output path, which is already fixed by tickets 03, 04 and 05.
Two committed entries, in order.

**a. The echo line.** `src/components/messages/UserCommandMessage.tsx:93-99`:

```jsx
<Box flexDirection="column" marginTop={addMargin ? 1 : 0}
     backgroundColor="userMessageBackground" paddingRight={1}>
  <Text><Text color="subtle">{figures.pointer} </Text><Text color="text">{content}</Text></Text>
</Box>
```

- One blank line above (`marginTop={1}`).
- `❯` in palette key `subtle`, one space, then `/status` in `text`.
- The full-terminal-width `userMessageBackground` fill of
  [03-user-assistant-messages](03-user-assistant-messages.md), with `paddingRight={1}`.
- Content is rebuilt as `` `/${[command, args].filter(Boolean).join(' ')}` ``
  (`UserCommandMessage.tsx:80`), so arguments are echoed after a single space.

**b. The output block.** `src/components/messages/UserLocalCommandOutputMessage.tsx:73-77`:

```jsx
<Box flexDirection="row">
  <Text dimColor>{"  \u23BF  "}</Text>
  <Box flexDirection="column" flexGrow={1}><Markdown>{children}</Markdown></Box>
</Box>
```

- Gutter is **two spaces, `⎿` (U+23BF), two spaces** — five columns, dim — so the content column
  is **column 5**. Byte-identical to the tool-result gutter of
  [05-tool-call-rendering](05-tool-call-rendering.md)
  (`src/components/MessageResponse.tsx:22` builds the same string).
- The gutter is emitted **once**, on the first row; continuation rows are produced by the inner
  column box and sit at column 5 with no repeated glyph.
- **No background fill.** The echo line is filled; the output block is not.
- Content is `stdout.trim()` (`UserLocalCommandOutputMessage.tsx:37`) and goes through the
  **markdown renderer** of [04-markdown-and-code-blocks](04-markdown-and-code-blocks.md), not
  raw text. picopilot may emit plain lines and get the same result, but bold and inline code are
  available if wanted.
- Empty output renders `(no content)` — `NO_CONTENT_MESSAGE`,
  `src/constants/messages.ts:1` — dim, behind the standard `  ⎿  ` gutter
  (`UserLocalCommandOutputMessage.tsx:27`).
- stdout and stderr are two separate gutter blocks when both are present
  (`UserLocalCommandOutputMessage.tsx:37-40`), the same two-row rule as
  [06-bash-tool-rendering](06-bash-tool-rendering.md). `/status` only ever has stdout.

**c. Body layout and palette keys**, adapted from `Status.tsx` (§2) into the column-5 block:

| Element | Rule | Palette key |
| --- | --- | --- |
| Property row | `Label:` + one space + value, no column alignment | label `text` **bold**; value `text` |
| Value-only row | value at column 5, no label | `text` |
| Section separator | exactly one blank line between sections, none between rows | — |
| Count summary value | `` `${parts.join(', ')} · /command` `` | per state: `success` / `warning` / `inactive` / `error`; the `· /command` tail `inactive` |
| Array value | items joined by `,` and one space, wrapping | `text` |
| Placeholder value | e.g. `/rename to add a name` | dim |
| Diagnostics header | bold `System Diagnostics`, blank line before | `text` bold |
| Diagnostic row | one space indent, `figures.warning`, one space, wrapped text | glyph `error`; text `text` |

**d. Concrete shape** for picopilot, assembling §3:

```
❯ /status
  ⎿  Version: 0.4.1
     Session ID: 0f1c…
     cwd: C:\dev\picopilot

     Model: gpt-5
     Tools: 7 enabled · /tools
     Skills: 3 of 41 enabled · /skills
```

The blank line between `cwd:` and `Model:` is the section gap; the gutter is not repeated on it.

**Unverified.** `figures.warning` and `figures.pointer` are read from the `figures@6.1.0`
documented table, not from a file on this machine — `node_modules` is absent. Same caveat as
tickets 03 and 10.

### 5. Do not print these on `/status`

Stated explicitly so the spec does not drift:

- **Reasoning effort** — spinner line only ([08-spinner-line](08-spinner-line.md) §9).
- **Session cost, request counts, API duration** — the printed usage block
  ([10-inline-pickers](10-inline-pickers.md) §9). `usage_detail_lines`
  ([tui.rs#L2836](src/tui.rs#L2836)) is already that block; it needs the rendering of §4 applied
  to it and nothing more.
- **Context token totals and the category attribution** — same block. The reference keeps this in
  a separate `/context` command with its own coloured grid
  (`src/commands/context/index.ts:4-9`, `src/components/ContextVisualization.tsx`); whether
  picopilot wants a grid is out of scope here.
- **Busy state** — dropped, §3 field 4.

### 6. The one field that keeps a persistent home: approaching the context limit

**Yes, the reference warns about this, and it warns in the footer, not in a command.**

`src/components/TokenWarning.tsx` is rendered by `Notifications`
(`src/components/PromptInput/Notifications.tsx:321`), which in the **non-fullscreen (scrollback)
build** lives on the right-hand side of the prompt footer row
(`src/components/PromptInput/PromptInputFooter.tsx:139-147`):

```jsx
<Box flexDirection={isNarrow ? 'column' : 'row'}
     justifyContent={isNarrow ? 'flex-start' : 'space-between'}
     paddingX={2} gap={isNarrow ? 0 : 1}>
  <Box flexDirection="column" flexShrink={isNarrow ? 0 : 1}>
    … PromptInputFooterLeftSide …
  </Box>
  <Box flexShrink={1} gap={1}>
    {isFullscreen ? null : <Notifications … isNarrow={isNarrow} />}
  </Box>
</Box>
```

So: **the same row as the hint line from [09-input-box](09-input-box.md), right-aligned, at
`paddingX={2}`, `wrap="truncate"`.** `isNarrow` is `columns < 80`
(`PromptInputFooter.tsx:105`); below 80 columns the row becomes a column and the warning stacks
*below* the hint instead of beside it. This is the single fact ticket 09 does not yet carry:
its hint row is the *left* half of a two-sided footer, and this warning is the right half.

**Thresholds**, `src/services/compact/autoCompact.ts:62-64,93-141`:

```ts
export const AUTOCOMPACT_BUFFER_TOKENS = 13_000
export const WARNING_THRESHOLD_BUFFER_TOKENS = 20_000
export const ERROR_THRESHOLD_BUFFER_TOKENS = 20_000
export const MANUAL_COMPACT_BUFFER_TOKENS = 3_000
…
const threshold = isAutoCompactEnabled() ? contextWindow - 13_000 : contextWindow
const percentLeft = Math.max(0, Math.round(((threshold - tokenUsage) / threshold) * 100))
const isAboveWarningThreshold = tokenUsage >= threshold - 20_000
const isAboveErrorThreshold   = tokenUsage >= threshold - 20_000
```

- The row is **hidden entirely** until `isAboveWarningThreshold` (`TokenWarning.tsx:107-110`),
  i.e. until usage is within **20 000 tokens** of the effective threshold. There is no
  always-on context readout outside `--verbose`.
- It is also suppressed for a while after a successful `/compact`
  (`useCompactWarningSuppression`, `TokenWarning.tsx:107`; see the comment at
  `src/commands/compact/compact.ts:114`).
- **`percentLeft` is a percentage of the threshold, not of the window** — with autocompact on,
  the 13 000-token buffer is already subtracted, so it reaches 0 % before the window is actually
  full.

**Wording and colour**, `TokenWarning.tsx:166,169` — two mutually exclusive forms:

| Condition | Text | Style |
| --- | --- | --- |
| autocompact enabled (the default) | `35% until auto-compact` | **dim**, no colour |
| autocompact disabled | `Context low (12% remaining) · Run /compact to compact & continue` | `error` |

Both truncate rather than wrap. An upsell string, when present, is appended after a
` · ` separator (`TokenWarning.tsx:169`) — picopilot has no equivalent and should omit it.

> **Finding, inferred from source.** `WARNING_THRESHOLD_BUFFER_TOKENS` and
> `ERROR_THRESHOLD_BUFFER_TOKENS` are **both 20 000** (`autoCompact.ts:63-64`), so
> `isAboveWarningThreshold` and `isAboveErrorThreshold` flip at the identical token count. The
> row only renders when the warning flag is set, at which point the error flag is set too —
> therefore the `warning`-coloured branch of `isAboveErrorThreshold ? "error" : "warning"` is
> **unreachable in the shipped build**. Palette key `warning` is not used by this row.
> picopilot should implement the single `error` colour and not build a two-tier ramp.
> **Unverified** against a running binary.

**In `--verbose` only**, a plain dim `{tokenUsage} tokens` sits on the same right-hand side
(`Notifications.tsx:317-320`), unconditionally. That is the reference's whole answer to "show me
the number all the time": it is opt-in behind a flag.

**Recommendation for picopilot.** picopilot has both a context window and compaction — its
`UsageSnapshot` carries `current_tokens` / `token_limit`
([events.rs#L21](src/events.rs#L21)) and `ContextAttributionSnapshot` carries `compactions`
([events.rs#L39](src/events.rs#L39)) — so use the autocompact-enabled form:

1. Compute `percent_left = max(0, round((limit - current) / limit * 100))` from `status.usage`.
   picopilot has no 13 000-token buffer of its own; use `token_limit` directly unless a buffer is
   introduced. **Unverified:** whether the SDK reserves a compaction buffer inside `token_limit`
   was not established.
2. Render **nothing** while `current < limit - 20_000`.
3. Above that, render one dim row on the right of the footer, same row as the hint line, at
   `paddingX = 2`, truncated not wrapped: `` `{percent_left}% until auto-compact` ``.
4. Below 80 columns, stack it under the hint line instead of beside it.
5. Suppress it for the remainder of the turn after a compaction is observed
   (`compactions` increments).

This is the only status-bar-derived field that keeps a permanent place on screen. Everything else
in §3 is on demand.

### 7. Live-region budget consequence

The warning row is **not** a new row. It shares the footer row that
[09-input-box](09-input-box.md) already budgets for the hint line, so
[10-inline-pickers](10-inline-pickers.md) §8's `h = 12` is unchanged — except at
`columns < 80`, where the footer becomes two rows and the budget needs one more. Two options,
both fine and not decided here: keep `h = 12` and accept that narrow terminals lose a row of
picker slack, or drop the hint line while the warning is showing. **Flagged for the spec, not
resolved.**

### 8. Unverified

- **`figures` glyph values** (`pointer`, `warning`) — read from the package's documented table,
  not from disk. Same caveat as tickets 03 and 10.
- **The `warning`-colour branch being dead** (§6) — inferred from two equal constants, not
  observed.
- **Whether picopilot's `token_limit` already excludes a compaction buffer** — not established;
  affects whether `percent_left` reaches 0 before or at the real limit.
- **`Session name`** — proposed for `/status` only if picopilot gains a rename command; none
  exists today ([tui.rs#L186](src/tui.rs#L186) registers only `/fleet`).
- **How `/status`, `/usage`, `/tools`, `/skills` get registered at all.** picopilot has no slash
  command registry beyond `BUILTIN_COMMANDS` ([tui.rs#L186](src/tui.rs#L186)); this ticket
  specifies what they print, not the plumbing that dispatches them.
