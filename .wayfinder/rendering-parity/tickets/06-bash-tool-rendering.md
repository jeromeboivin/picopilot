---
label: wayfinder:research
name: Specify bash command and output rendering
status: closed
assignee: research-subagent
blocked_by: [05-tool-call-rendering]
---

# Specify bash command and output rendering

## Question

How is a shell command and its output rendered, on top of the general tool call rules?

Resolve by producing:

- How the command itself is displayed: truncation limits in characters and lines, how a
  multi-line command is shown, and whether any shell syntax styling is applied.
- How stdout and stderr are distinguished, including the palette key for each.
- Exit code presentation: what is shown for zero, what is shown for non-zero, and any
  interpretation text derived from the command.
- Per-line output processing: JSON reformatting, URL linkification, ANSI stripping, and the
  maximum display width.
- Long output truncation: the threshold and the exact indicator text.
- Streaming output while the command is still running, and how that interacts with the
  committed-vs-live boundary.
- What picopilot does today — it prefixes the command with `"$ "` and dumps combined output
  with no truncation and no stderr distinction — and the specific gaps.

## Resolution

Builds on [05](05-tool-call-rendering.md) (header row, `⎿` gutter, the three truncators, the
error dispatch) and [02](02-scrollback-mechanism.md) (committed vs live, the Windows finding).
Those rules are not restated; this section only says what Bash adds or overrides.

Citations are `path:line` inside `C:\dev\git\claude-code`. Same caveat as 05: nothing was run,
so every column count is read off the flex tree, not measured. Items the source does not answer
are marked **not determined from source**, with what was checked.

### 1. The command in the header

The tool's user-facing name is the constant `Bash` (`src/tools/BashTool/toolName.ts:2`,
`export const BASH_TOOL_NAME = 'Bash'`), so the header is `⏺ Bash(<summary>)` / `● Bash(…)`.

`renderToolUseMessage(input, { verbose })` (`src/tools/BashTool/UI.tsx:82-127`) decides the
summary, in this order:

1. **No `command`** → `return null` (`:92-94`). Per 05 this suppresses the entire header row.
2. **`sed -i` style in-place edit** → the *file path only*, never the command
   (`:96-101`): `verbose ? sedInfo.filePath : getDisplayPath(sedInfo.filePath)`. A `sed -i`
   therefore looks like a file edit: `⏺ Bash(src/foo.ts)`.
3. **`verbose === true`** → the whole `if (!verbose)` block at `:102` is skipped and the raw
   `command` is returned untruncated, newlines and all. **Verbose removes every command limit.**
4. **Fullscreen only** (`isFullscreenEnvEnabled()`, Anthropic-internal per 02) → if the first
   line is a `# comment` and not a `#!` shebang, that comment text replaces the command
   (`:104-110`, helper `src/tools/BashTool/commentLabel.ts:8-13`). It is cut at
   `MAX_COMMAND_DISPLAY_CHARS` with `…` appended. **Out of scope for picopilot** — the public
   build never takes this branch.
5. **Normal non-verbose** → the two-stage truncation below.

Exact limits, `UI.tsx:26-27`:

```ts
const MAX_COMMAND_DISPLAY_LINES = 2;
const MAX_COMMAND_DISPLAY_CHARS = 160;
```

The algorithm (`:111-125`), verbatim in effect:

```
needsLineTruncation = lines.length > 2
needsCharTruncation = command.length > 160
if (neither) -> return command unchanged
truncated = command
if (needsLineTruncation) truncated = first 2 lines joined with '\n'
if (truncated.length > 160) truncated = truncated.slice(0, 160)
return <Text>{truncated.trim()}…</Text>
```

Four consequences worth pinning down, because they are easy to get wrong:

- The `…` (U+2026) is appended **only inside the truncating branch**. A command of exactly 160
  chars on 2 lines gets no ellipsis.
- The 160-char test is against the **whole original command**, but the slice is applied to the
  already-line-truncated string. So a 3-line command whose first 2 lines are 40 chars total
  still enters the branch (because the full command is >160), is cut to 2 lines, is *not*
  sliced (40 ≤ 160), and gets `…`. That is correct and intended: `…` means "there was more".
- `.trim()` runs **after** slicing, so trailing whitespace left by the cut is removed before the
  `…` is glued on — the ellipsis touches the last visible character, no space.
- The slice is a raw JS `slice(0, 160)` on UTF-16 code units. No grapheme or width awareness.
  A Rust port must not use `chars().take(160)` if it wants byte-identical output on astral
  characters; **not determined from source** whether this ever matters in practice.

**No shell syntax styling.** The summary is returned as a bare string or a plain
`<Text>{…}</Text>` with no `color`, `bold`, `dimColor` or highlighter anywhere in the path
(`UI.tsx:82-127`). Per 05 the tool *name* is bold; the command inside the parentheses inherits
the default text style. There is no shell lexer, no keyword coloring, no path highlighting.
Searched `src/tools/BashTool/` for `highlight`, `chalk` and `Ansi` in the render path — nothing.

**Multi-line commands.** The returned string can still contain one `\n` (the 2-line case). The
header row is `flexWrap="nowrap"` (05 §1), which controls wrapping, not embedded newlines, so
the header can genuinely occupy 2 terminal rows with `(` on the first and the rest of the
command plus `)` on the second. Where the second row starts horizontally — column 0, or aligned
under the name — is **not determined from source**; it depends on Ink's own text-node line
splitting inside a `flexShrink={0}` box, which I did not trace into `src/ink/`. Recommended for
picopilot: treat the summary as a single logical string, render the embedded newline as a real
line break, and indent continuation rows to column 2 to match the assistant hanging indent from
[03](03-user-assistant-messages.md). Flag it in the spec as a deliberate choice, not a
reproduction.

### 2. stdout vs stderr

`BashToolResultMessage` (`src/tools/BashTool/BashToolResultMessage.tsx`) renders a
`flexDirection="column"` box with up to five children, in this fixed order (`:172`):

| # | child | condition | style |
| --- | --- | --- | --- |
| 1 | stdout `OutputLine` | `stdout !== ''` (`:114`) | no color key — default `text` |
| 2 | stderr `OutputLine` with `isError` | `stderr.trim() !== ''` (`:121`) | palette key `error` |
| 3 | cwd-reset warning | warning extracted (`:145`) | **dim**, no color key |
| 4 | empty-output line | all three above empty (`:157`) | **dim**, `height={1}` |
| 5 | timeout line | `timeoutMs` set (`:163`) | `ShellTimeDisplay`, see 05 §7 |

The palette mapping is one line, `src/components/shell/OutputLine.tsx:83`:

```ts
const color = isError ? "error" : isWarning ? "warning" : undefined;
```

So: **stdout = default `text`, stderr = `error`, and there is a third `warning` channel that
Bash never uses.** Note the asymmetric guards — stdout is tested with `!== ''` but stderr with
`.trim() !== ''`, so whitespace-only stderr is dropped while whitespace-only stdout is not.

**Each of stdout and stderr is its own `MessageResponse`** (`OutputLine.tsx:98`), so a call with
both produces **two `  ⎿  ` gutter rows**, not one gutter with two colors underneath. Same for
children 3, 4 and 5. This is the single most surprising layout fact in the ticket.

**The important caveat: for Bash, stderr is almost always empty.** The shipped tool runs the
command with a merged fd — `BashTool.tsx:690` comments *"stderr is interleaved in stdout (merged
fd) — result.stdout has both"* — and the returned `Out.stderr` is set to `stderrForShellReset`
(`BashTool.tsx:806`), which is only ever the "Shell cwd was reset to …" notice, and even that is
pulled out into the dim child 3 before rendering. Two things follow:

- **In the reference, error-colored stderr is effectively dead code for Bash.** It is live for
  `PowerShellTool`, which passes real stderr (`src/tools/PowerShellTool/UI.tsx:105-106`).
- Before display, stderr is also scrubbed of `<sandbox_violations>…</sandbox_violations>`
  (`BashToolResultMessage.tsx:22-38`) and of the cwd-reset line, matched by
  `/(?:^|\n)(Shell cwd was reset to .+)$/` (`:18`, extractor at `:44-63`).

**Recommendation for picopilot:** keep the two channels as separate rows with the `text` /
`error` split, because that is the specified behaviour and picopilot's SDK may well deliver them
separately. Do not copy the fd merge.

**Image output short-circuits everything** (`:100-108`): when `isImage`, the entire result block
is one dim single-line row reading exactly `[Image data detected and sent to Claude]`.

### 3. Exit codes

**Zero exit: nothing is shown.** There is no `exit 0`, no checkmark, no code in the result
block. Success is carried only by the header dot turning `success` (05 §3).

**Non-zero exit does not reach `BashToolResultMessage` at all.** `BashTool.tsx:717` throws:

```ts
throw new ShellError('', outputWithSbFailures, result.code, result.interrupted);
```

so a failed command takes the **tool-error path** from 05 §10 — dot in `error`,
`FallbackToolUseErrorMessage`, the 10-line truncator with `… +N lines (ctrl+o to see all)`.
Specifying "how a failing bash command looks" is therefore mostly a matter of specifying the
error text, which is assembled in `src/utils/toolErrors.ts:24-31`:

```ts
if (error instanceof ShellError) {
  return [`Exit code ${error.code}`, error.interrupted ? INTERRUPT_MESSAGE_FOR_TOOL_USE : '', error.stderr, error.stdout]
}
```

joined with `\n` after dropping empties (`:12-13`), and if the joined body exceeds 10 000 chars
it becomes first 5 000 + `\n\n... [N characters truncated] ...\n\n` + last 5 000 (`:17-21`).
Then 05 §10's fallback prefixes `Error: `. **The first rendered line of a failed bash call is
therefore exactly `Error: Exit code 1`**, in palette key `error`, inside the `⎿` gutter, with
the merged output beneath it. If every part is empty the body is the literal
`Command failed with no output` (`toolErrors.ts:14`).

Separately, `BashTool.tsx:695-699` appends a literal `Exit code ${code}` line to the accumulated
stdout when the interpretation says error — model-facing duplication, mentioned only so it is
not mistaken for a second UI element.

**Interpretation text derived from the command.** `src/tools/BashTool/commandSemantics.ts`
maps the *base command* to an exit-code meaning. The base command is the first word of the
**last** segment of the pipeline/chain (`:107-114`, `heuristicallyExtractBaseCommand`, "may get
it super wrong"). The complete table (`:31-85`):

| base command | treated as error when | message on exit 1 |
| --- | --- | --- |
| `grep` | `code >= 2` | `No matches found` |
| `rg` | `code >= 2` | `No matches found` |
| `find` | `code >= 2` | `Some directories were inaccessible` |
| `diff` | `code >= 2` | `Files differ` |
| `test` | `code >= 2` | `Condition is false` |
| `[` | `code >= 2` | `Condition is false` |
| anything else | `code !== 0` | `Command failed with exit code ${exitCode}` |

The default message is **never rendered**: `isError` is true in that branch, so the call throws
into the error path instead. Only the four non-error strings above can reach the screen.

And they only reach it in one narrow case — `BashToolResultMessage.tsx:157` shows
`returnCodeInterpretation` **only when stdout, stderr and the cwd warning are all empty**. So
`grep foo *.txt` with no match renders one dim line `No matches found`; a `grep` that matched
shows its output and no interpretation.

That same slot is a four-way priority chain (`:157`), all **dim**, all `height={1}`:

1. `backgroundTaskId` set → `Running in the background ` + a `KeyboardShortcutHint` for `↓`
   with `parens`, i.e. **`Running in the background (↓ to manage)`**;
2. else `returnCodeInterpretation` if present;
3. else `Done` when `noOutputExpected`;
4. else `(No output)` — note the literal parentheses are part of the string.

`noOutputExpected` is `isSilentBashCommand(command)` (`BashTool.tsx:809`, impl at `:178`), true
when every non-fallback segment's base command is in `BASH_SILENT_COMMANDS` (`BashTool.tsx:81`):

```
mv, cp, rm, mkdir, rmdir, chmod, chown, chgrp, touch, ln, cd, export, unset, wait
```

So `rm foo` shows `Done` and `echo -n ''` shows `(No output)`.

### 4. Per-line output processing

All of it lives in `OutputLine` (`src/components/shell/OutputLine.tsx:47-101`) and runs in this
order:

1. **JSON reformatting** — `tryJsonFormatContent` (`:32-38`). Skipped entirely when the content
   exceeds `MAX_JSON_FORMAT_LENGTH = 10_000` characters (`:31`). Otherwise the content is split
   on `\n` and **each line independently** goes through `tryFormatJson` (`:12-30`): parse,
   re-stringify, and on success return `jsonStringify(parsed, null, 2)` — **2-space indent**.
   A parse failure returns the line untouched. There is also a precision guard: the round-trip
   is compared to the original with whitespace and `\/` escapes normalised away, and on any
   mismatch (large integers past `Number.MAX_SAFE_INTEGER`) the original line is kept
   unformatted. Net effect: a one-line JSON blob from `curl` gets pretty-printed in place; a log
   file does not.
2. **URL linkification** — `linkifyUrlsInText` (`:44-45`), regex
   `/https?:\/\/[^\s"'<>\\]+/g` wrapped in `createHyperlink` (OSC 8). **Gated on the
   `linkifyUrls` prop, and Bash does not pass it** (`BashToolResultMessage.tsx:114,121`).
   Grepped every `OutputLine` call site: only `src/tools/MCPTool/UI.tsx:174,244` sets it.
   **So bash output is not linkified.**
3. **Truncation** — `renderTruncatedContent(formatted, columns, inVirtualList)` (`:75`), skipped
   when `shouldShowFull = verbose || expandShellOutput` (`:60`). See §5.
4. **ANSI stripping — underline only.** `stripUnderlineAnsi` (`:111-115`) runs on **both**
   branches (`:70` and `:75`), and its regex removes only SGR sequences containing parameter
   `4`. Everything else — colors, bold, dim — survives and is rendered by `<Ansi>` (`:88`). The
   comment at `:104-110` is explicit that full `stripAnsi()` was tried and reverted because
   "people complained about losing all formatting". **picopilot must parse and honour ANSI SGR
   in tool output, not strip it and not print it raw.**

**Maximum display width.** `src/utils/terminal.ts:81`:

```ts
const wrapWidth = Math.max(terminalWidth - PADDING_TO_PREVENT_OVERFLOW, 10)   // PADDING = 10 (:10)
```

with the reason spelled out at `:8-9`: 5 for the `  ⎿ ` prefix plus 5 for the parent's
`columns - 5`. So output is folded at `max(columns - 10, 10)` visible columns, ANSI-aware
(`sliceAnsi`), and every produced line is `trimEnd()`-ed (`:29`, `:37`). Ten is a hard floor;
below a 20-column terminal the output simply overflows.

One more pre-render step, upstream of `OutputLine`: `stripEmptyLines`
(`src/tools/BashTool/utils.ts:22-36`) drops leading and trailing blank lines from stdout before
it becomes the result. Interior blank lines are kept.

### 5. Long output truncation

The threshold and wording are 05 §9(a) — 3 wrapped lines, then a dim
`… +N lines (ctrl+o to expand)`, with the N=1 case showing the fourth line instead of a hint,
and N becoming an estimate past `3 * wrapWidth * 4` characters. Bash adds three things:

- **The 3-line limit applies per channel, not per call.** stdout and stderr are separate
  `OutputLine`s, so a call with both can show 3 + indicator *and* 3 + indicator — up to 8 rows
  across two gutters.
- **Verbose disables it** (`OutputLine.tsx:60`) — full output, no indicator.
- **The most recent user `!` command auto-expands.** `ExpandShellOutputProvider` wraps exactly
  one message, chosen by `latestBashOutputUUID` (`src/components/Messages.tsx:423-440`): scan
  backwards for the last **user** message whose text starts with `<bash-stdout` or
  `<bash-stderr`. That is the user's own `!` shell escape, **not** a model Bash tool call — so
  model tool results are always subject to the 3-line fold.

  **This one is structurally incompatible with committed scrollback** and 02's one-way commit.
  When a second `!` command runs, the *previous* message must shrink from full output back to
  3 lines — a retroactive edit to an already-committed row. The reference gets away with it only
  because a row above `viewportY` triggers its `fullResetSequence_CAUSES_FLICKER` path.
  **Recommendation: picopilot commits `!` output expanded and leaves it expanded.** Diverges
  from the reference in a way that is strictly less surprising; record it in the spec as a
  deliberate deviation.

`isResultTruncated` (`BashTool.tsx:822`) is a cheap "is there an expand affordance" predicate
using `isOutputLineTruncated` (`terminal.ts:112-131`) — counts raw `\n` only, more than 3, with
a trailing newline treated as a terminator. It knowingly misses a single very long wrapped line.

### 6. Streaming while the command runs

The progress row shape is 05 §7 (`Running… `, the last-5-lines window, `~N lines` /
`+N lines`, `ShellTimeDisplay`, `formatFileSize`, and `Waiting…` when queued at
`UI.tsx:154-158`). What this ticket adds is the boundary interaction with 02:

- **The progress row is pure live-region content and must never be committed.** It is produced
  from `progressMessagesForMessage.at(-1)` (`UI.tsx:129`) — each tick *replaces* the previous
  render, and the whole row disappears when the `tool_result` arrives and the result block takes
  over. Committing any intermediate frame would leave stale duplicated output in scrollback
  permanently.
- **Live-region height budget.** A running Bash call needs, at worst: 1 row header (2 if the
  command wrapped to two lines) + up to 5 rows of output preview + 1 status row = **7 rows**.
  That is the number the `Viewport::Inline(h)` sizing from 02 §5 step 5 has to accommodate on
  top of the spinner and input box, or the preview has to be shortened. Claude Code hard-clips
  the preview box to `min(5, lines.length)` rows, so 5 is a genuine ceiling, not a typical case.
- **Nothing streams to the terminal directly** — 02 §3 established that all subprocess output is
  captured into progress messages and rendered inside the frame. That holds for Bash and is what
  makes a fixed live region viable.
- **The Windows streaming disable does *not* cover this.** 02 reported
  `showStreamingText = !reducedMotion && !hasCursorUpViewportYankBug()`; grepping every use
  (`src/screens/REPL.tsx:1463,1465,1473,4506`) shows it gates only the **assistant text**
  preview and the sync-vs-async message mode. Bash progress rows are rendered unconditionally on
  Windows. So picopilot should keep the shell progress preview on Windows even while it disables
  assistant token streaming there — the two are independent decisions, and 02's Windows finding
  must not be over-applied here.
- The commit point for a Bash call is unambiguous: the arrival of the `tool_result`. Header and
  result block are two separate transcript rows (05 §1), so in an `insert_before` design the
  header can be committed as soon as it is drawn only if the dot state is already final —
  otherwise header and result must be committed together, once, at completion.

### 7. What picopilot does today, and the gaps

Current behaviour, all in `src/tui.rs`:

- Shell tools are recognised by name substring — `is_shell_tool` matches `shell`, `bash` or
  `powershell`, case-insensitively ([tui.rs](../../../src/tui.rs#L3133-L3136)) — and **every
  other tool is rendered as nothing** unless `show_internals` is on
  ([tui.rs](../../../src/tui.rs#L3000)).
- The header is `format!("tool {} [{}]{}", tool_name, state, …)` with states `unknown`,
  `running`, `done`, `failed`, left-padded to 18 columns and colored a flat
  `Color::Rgb(139, 181, 255)` for every state
  ([tui.rs](../../../src/tui.rs#L3011-L3016), [tui.rs](../../../src/tui.rs#L3136-L3150)).
- The body is `format!("$ {command}\n{output}")`
  ([tui.rs](../../../src/tui.rs#L3017)), where `command` comes from `tool_command`, which reads
  the first of `command`, `cmd`, `script`, `fullCommandText`
  ([tui.rs](../../../src/tui.rs#L3138-L3145)).
- `ChatEntry::Tool` carries a single `output: String` ([tui.rs](../../../src/tui.rs#L121)),
  appended to by `ToolOutput` and then **overwritten** by `ToolCompleted`'s message on success /
  appended on failure ([tui.rs](../../../src/tui.rs#L816-L855)).

The gaps, in rough order of visibility:

1. **No `Bash(cmd)` header.** `tool bash [done]` on its own padded line instead of
   `⏺ Bash(npm test)`. Wrong glyph (none), wrong shape, wrong name casing, and the `$ ` prefix
   is a picopilot invention that the reference does not have anywhere.
2. **State is never colored.** One blue for pending, running, success and failure; the
   reference's `success` / `error` dot is the primary signal that a command failed.
3. **No command truncation.** A 40-line heredoc becomes 40 header-adjacent rows. Needs the
   2-line / 160-char / `…` rule from §1.
4. **No stdout/stderr split.** `output` is one string, so the `error`-colored channel from §2
   cannot be expressed at all without a data-model change. The `Out` shape needs at least
   `stdout`, `stderr`, `exit_code`.
5. **No exit code anywhere.** No `Error: Exit code 1` first line, no `(No output)`, no `Done`,
   no `No matches found`. There is a `success: Option<bool>` but no code, so §3 cannot be
   implemented without plumbing the code through `ToolCompleted`.
6. **No truncation of output.** A 5 000-line build log is dumped in full — and under 02's
   commit-once model that is now *permanent* scrollback, which is materially worse than today's
   scrollable `Paragraph`. Needs §5's 3-line fold before the scrollback migration lands, not
   after.
7. **No ANSI handling.** Escape sequences from `ls --color`, `cargo`, `npm` are pushed into
   `Span::raw` verbatim and will render as literal `ESC[0;32m` garbage. §4 requires the
   opposite of stripping: parse SGR into ratatui `Style`, dropping only underline.
8. **No JSON reformatting**, no width-aware fold at `columns - 10`, no `trimEnd` per line, no
   leading/trailing blank-line strip.
9. **No progress row.** No `Running…`, no elapsed time, no byte counter, no output preview — a
   long command shows a static blue header and nothing else.
10. **Entries are mutated in place after completion** (the `iter_mut().find(...)` at
    [tui.rs](../../../src/tui.rs#L816-L855)), which is exactly the pattern 02 §5 step 4 says must
    become a one-way commit. Bash is the worst case for it, because a single call mutates on
    every output chunk.

Gaps 4, 5 and 10 are data-model work outside `tui.rs`; the rest are rendering.
