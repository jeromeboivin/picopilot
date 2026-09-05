# Picopilot Terminal Rendering Specification

## Normative Language

`MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT`, and `MAY` are normative. Literal strings and glyphs are shown in code formatting. Column numbers are zero-based. `columns` means the current terminal width in cells.

## Purpose And Non-Goals

This specification defines the picopilot conversation window required for exact static visual parity with the public, dark-theme Claude Code terminal UI. It covers screen ownership, layout, wrapping, palette, glyphs, message surfaces, dynamic states, security boundaries, dependencies, and verification fixtures.

The implementation MUST be a separate effort. This document MUST NOT be treated as production Rust implementation.

The following are out of scope:

- Light, ANSI-16 theme, and daltonized theme variants.
- Claude Code's logo header, onboarding, `/help`, and MCP-specific screens.
- Running or modifying Claude Code.
- Reproducing Anthropic-internal alternate-screen/fullscreen branches where the public scrollback branch differs.
- Choosing a truecolor fallback for terminals without 24-bit color support; this remains Unverified.

## Screen Model

### Main Screen And Inline Viewport

Picopilot MUST render on the terminal's main screen buffer. It MUST remove `EnterAlternateScreen` and `LeaveAlternateScreen` while retaining raw mode and bracketed paste.

The ratatui terminal MUST use `Terminal::with_options` with `Viewport::Inline(h)`. Committed transcript rows MUST be inserted above that viewport with `Terminal::insert_before`. A fullscreen viewport MUST NOT be used for the conversation UI: `insert_before` silently does nothing outside an inline viewport.

The architecture MUST expose two states:

| State | Contract |
| --- | --- |
| Committed history | Append-only, write-once rows owned by terminal scrollback. Committed content MUST be unreachable for mutation. |
| Live region | Fixed-height rows at the bottom, redrawn with `Terminal::draw`. Only this state MAY change in place. |

Promotion MUST be explicit and one-way. A terminal-state message MUST be rendered once at the current width, inserted with an exact height equal to the rendered line count, and removed from mutable live state. A late correction MUST be emitted as a new event; committed rows MUST NOT be edited.

A tool header whose dot can still change MUST remain live. For Bash, the unresolved header and progress preview MUST remain live until `tool_result`; the final header and result MAY then be committed together. Intermediate shell progress MUST never be committed.

Assistant streaming MUST commit complete lines only. The trailing partial line MAY remain live. On Windows or when `WT_SESSION` is set, live token-by-token assistant preview MUST be disabled by default; complete lines still commit downward. This Windows exception MUST NOT disable Bash progress.

### Fixed Live-Region Budget

The inline viewport height is fixed for the lifetime of ratatui 0.29's `Terminal`. Routine states MUST fit by internal layout and scrolling, not by changing `h`.

- Input growth MUST be capped at `max(3, budget)` visible rows and cursor-centred internally.
- Completion lists MUST use at most 6 rows.
- General pickers MUST use compact one-row options and at most 5 visible options.
- Long pickers MUST scroll their window; list length MUST NOT change live-region height.
- Optional live rows MUST be dropped before reducing input below 3 rows.
- Picker and input states are alternatives; they MUST NOT coexist.
- A running Bash preview can consume up to 7 rows: a one- or two-row header, up to 5 output rows, and one status row. The implementation MUST budget or shorten the preview without committing it.

No exact `h` is normative. Tickets proposed `h = 12` for compact picker/input states and `h = 14` for more common input room; neither was measured. The implementation MUST prototype and fix one value before coding the full renderer.

### Resize And Scrollback

On resize, the live region MUST re-wrap and repaint at the new width. Committed history MUST NOT be re-rendered or reinserted; terminal-native scrollback reflow owns it. A committed row's wrap and padding are frozen at its commit width.

The implementation MUST accept these consequences:

- Narrowing can re-wrap already padded user-background rows.
- Widening cannot reconstruct earlier line breaks.
- ratatui may append lines and clear/repaint the live viewport during inline resize.
- Wrong commit height is permanent: over-counting leaves blank rows and under-counting truncates content.

The `scrolling-regions` ratatui feature SHOULD remain disabled initially. It MAY be enabled only after measured need and tests on Windows Terminal and legacy conhost.

## Dark Palette

Every key below MUST exist in the palette even when unused. Values are exact dark values; spacing inside source `rgb(...)` strings is not semantically significant in Rust.

| Key | Exact dark value | Picopilot usage rule |
| --- | --- | --- |
| `autoAccept` | `rgb(175,135,255)` | Permission-mode chrome only; otherwise unused. |
| `bashBorder` | `rgb(253,93,177)` | Bash input marker and prompt rules. |
| `claude` | `rgb(215,119,87)` | Default spinner verb/glyph color. |
| `claudeShimmer` | `rgb(235,159,127)` | Spinner shimmer/flash endpoint. |
| `claudeBlue_FOR_SYSTEM_SPINNER` | `rgb(147,165,255)` | System compaction/hook spinner only, if implemented. |
| `claudeBlueShimmer_FOR_SYSTEM_SPINNER` | `rgb(177,195,255)` | Matching system-spinner shimmer only. |
| `permission` | `rgb(177,185,249)` | Inline code and permission/dialog accent. |
| `permissionShimmer` | `rgb(207,215,255)` | Unused. |
| `planMode` | `rgb(72,150,140)` | Plan-mode footer text. |
| `ide` | `rgb(71,130,200)` | Unused in the conversation window. |
| `promptBorder` | `rgb(136,136,136)` | Input top and bottom rules. |
| `promptBorderShimmer` | `rgb(166,166,166)` | Unused. |
| `text` | `rgb(255,255,255)` | Default foreground and assistant glyph. |
| `inverseText` | `rgb(0,0,0)` | Foreground over colored chips, if any. |
| `inactive` | `rgb(153,153,153)` | All `dimColor`/secondary text. |
| `inactiveShimmer` | `rgb(193,193,193)` | Unused as a palette key. |
| `subtle` | `rgb(80,80,80)` | Structural chrome, user glyph, reasoning/diagnostic glyphs. |
| `suggestion` | `rgb(177,185,249)` | Focused picker/completion rows and selected message glyphs. |
| `remember` | `rgb(177,185,249)` | Memory-input marker only, if implemented. |
| `background` | `rgb(0,204,204)` | Background-task foreground accent only; it is not a terminal background. |
| `success` | `rgb(78,186,101)` | Successful tool dot, checked item, success counts. |
| `error` | `rgb(255,107,128)` | Failed tool dot, blocking errors, stderr base style. |
| `warning` | `rgb(255,193,7)` | Warning and recoverable-error dot. |
| `merged` | `rgb(175,135,255)` | Unused in the conversation window. |
| `warningShimmer` | `rgb(255,223,57)` | Unused. |
| `diffAdded` | `rgb(34,92,43)` | Added-line background. |
| `diffRemoved` | `rgb(122,41,54)` | Removed-line background. |
| `diffAddedDimmed` | `rgb(71,88,74)` | Added-line background in rejected/dim diffs. |
| `diffRemovedDimmed` | `rgb(105,72,77)` | Removed-line background in rejected/dim diffs. |
| `diffAddedWord` | `rgb(56,166,96)` | Changed token background inside an added line. |
| `diffRemovedWord` | `rgb(179,89,107)` | Changed token background inside a removed line. |
| `red_FOR_SUBAGENTS_ONLY` | `rgb(220,38,38)` | Unused; picopilot subagents do not receive identity colors. |
| `blue_FOR_SUBAGENTS_ONLY` | `rgb(37,99,235)` | Unused. |
| `green_FOR_SUBAGENTS_ONLY` | `rgb(22,163,74)` | Unused. |
| `yellow_FOR_SUBAGENTS_ONLY` | `rgb(202,138,4)` | Unused. |
| `purple_FOR_SUBAGENTS_ONLY` | `rgb(147,51,234)` | Unused. |
| `orange_FOR_SUBAGENTS_ONLY` | `rgb(234,88,12)` | Unused. |
| `pink_FOR_SUBAGENTS_ONLY` | `rgb(219,39,119)` | Unused. |
| `cyan_FOR_SUBAGENTS_ONLY` | `rgb(8,145,178)` | Unused. |
| `professionalBlue` | `rgb(106,155,204)` | Unused. |
| `chromeYellow` | `rgb(251,188,4)` | Unused. |
| `clawd_body` | `rgb(215,119,87)` | Unused. |
| `clawd_background` | `rgb(0,0,0)` | Unused. |
| `userMessageBackground` | `rgb(55, 55, 55)` | Full-width user-message background. |
| `userMessageBackgroundHover` | `rgb(70, 70, 70)` | Unused without mouse hover/fullscreen mode. |
| `messageActionsBackground` | `rgb(44, 50, 62)` | Full-width selected-message background, if selection exists. |
| `selectionBg` | `rgb(38, 79, 120)` | Mouse-selection background, if picopilot controls selection. |
| `bashMessageBackgroundColor` | `rgb(65, 60, 65)` | User `!` Bash-input message background, if implemented. |
| `memoryBackgroundColor` | `rgb(55, 65, 70)` | User `#` memory-input message background, if implemented. |
| `rate_limit_fill` | `rgb(177,185,249)` | Usage bar only. |
| `rate_limit_empty` | `rgb(80,83,112)` | Usage bar only. |
| `fastMode` | `rgb(255,120,20)` | Fast-mode `↯` marker, if implemented. |
| `fastModeShimmer` | `rgb(255,165,70)` | Unused. |
| `briefLabelYou` | `rgb(122,180,232)` | Unused; brief/Kairos layout is excluded. |
| `briefLabelClaude` | `rgb(215,119,87)` | Unused; brief/Kairos layout is excluded. |
| `rainbow_red` | `rgb(235,95,87)` | Ultrathink trigger only, if implemented. |
| `rainbow_orange` | `rgb(245,139,87)` | Ultrathink trigger only. |
| `rainbow_yellow` | `rgb(250,195,95)` | Ultrathink trigger only. |
| `rainbow_green` | `rgb(145,200,130)` | Ultrathink trigger only. |
| `rainbow_blue` | `rgb(130,170,220)` | Ultrathink trigger only. |
| `rainbow_indigo` | `rgb(155,130,200)` | Ultrathink trigger only. |
| `rainbow_violet` | `rgb(200,130,180)` | Ultrathink trigger only. |
| `rainbow_red_shimmer` | `rgb(250,155,147)` | Ultrathink shimmer only. |
| `rainbow_orange_shimmer` | `rgb(255,185,137)` | Ultrathink shimmer only. |
| `rainbow_yellow_shimmer` | `rgb(255,225,155)` | Ultrathink shimmer only. |
| `rainbow_green_shimmer` | `rgb(185,230,180)` | Ultrathink shimmer only. |
| `rainbow_blue_shimmer` | `rgb(180,205,240)` | Ultrathink shimmer only. |
| `rainbow_indigo_shimmer` | `rgb(195,180,230)` | Ultrathink shimmer only. |
| `rainbow_violet_shimmer` | `rgb(230,180,210)` | Ultrathink shimmer only. |

The five source-unused keys are `permissionShimmer`, `promptBorderShimmer`, `inactiveShimmer`, `warningShimmer`, and `fastModeShimmer`.

### Dynamic And Off-Palette Colors

| Effect | Exact rule |
| --- | --- |
| Stall target | Interpolate each RGB channel from the active spinner color to hardcoded `{171,43,63}`. This endpoint is off the dark palette. Non-truecolor fallback switches to `error` after intensity `> 0.5`. |
| Tool-use flash | Interpolate `claude` to `claudeShimmer`; non-truecolor fallback switches at opacity `> 0.5`. |
| Thinking shimmer | Interpolate hardcoded `{153,153,153}` to `{185,185,185}`. The first equals `inactive`; the second is not `inactiveShimmer`. |
| Syntax highlighting | Uses terminal ANSI-16 roles through the syntect mapping; it does not use the dark palette except `permission` for inline code. |
| Shell ANSI | Parsed named/indexed/truecolor values bypass the palette. Channel color is only the base style. |
| Diff primary-source colors | Picopilot MUST NOT use the primary renderer's hardcoded `rgb(2,40,0)`, `rgb(4,71,0)`, `rgb(80,200,80)`, `rgb(61,1,0)`, `rgb(92,2,0)`, `rgb(220,90,90)`, and `rgb(248,248,242)`; the accepted design uses the six palette diff keys instead. |

## Glyph Inventory

| Meaning | Literal glyph(s) | Platform/fallback rule |
| --- | --- | --- |
| User/input pointer | `❯` | Unicode path. Historical non-Unicode fallback is `>` but remains Unverified for `figures@6.1.0`. |
| Assistant/tool/top-level event | `⏺` / `●` | `⏺` on macOS; `●` on Windows and Linux. |
| Result gutter | `⎿` | Prefix is exactly two spaces, `⎿`, then two display spaces; the reference's tool component uses final U+00A0. Render as a five-cell gutter. |
| Blockquote bar | `▎ ` | U+258E plus one space, on every nonblank quote line. |
| Spinner Windows/Linux frames | `· ✢ * ✶ ✻ ✽` | Forward then reverse, endpoints doubled. |
| Spinner macOS frames | `· ✢ ✳ ✶ ✻ ✽` | Forward then reverse, endpoints doubled. |
| Spinner Ghostty frames | `· ✢ ✳ ✶ ✻ *` | Used whenever `TERM=xterm-ghostty`, before OS branching. |
| Reduced-motion spinner | `●` | Static. |
| Reasoning | `✻` | `TEARDROP_ASTERISK`. |
| Picker focus | `❯` | Occupies the same one-cell indicator column as scroll arrows. |
| Picker scroll | `↑`, `↓` | Dim; runtime `figures@6.1.0` values/fallbacks remain Unverified. |
| Multi-select | `[✓]`, `[ ]` | Accepted visual form. Runtime `figures.tick` may be `✔`; see Unverified. |
| Single-select chosen marker | `✓`/`✔` | Exact runtime value remains Unverified; do not finalize without dependency inspection. |
| Todo states | `✓`/`✔`, `◼`, `◻` | Done, in progress, pending; package values/fallbacks remain Unverified. |
| Warning diagnostic | warning figure | Exact `figures.warning` glyph remains Unverified. |
| Plan mode | `⏸` | Followed by ` plan mode on`. |
| Accept/bypass mode | `⏵⏵` | Two source characters. Cell width on Windows remains Unverified. |
| Fast mode | `↯` | Embedded near the right end of the top prompt rule. |
| Prompt rules | `─` | Full terminal width; no corners or vertical sides. |
| Permission-diff rules | `╌` | Top and bottom only; `╎` exists in the border style but is not drawn. |
| Diff sigils | `+`, `-`, space | Repeated on wrapped rows; line number appears only on the first row. |
| Diff hunk separator | `...` | Three ASCII periods, not `…` and not a unified hunk header. |
| Ellipsis | `…` | U+2026 for truncation and spinner verbs. |
| Separator | ` · ` | Space, U+00B7, space. |
| Completion resource icons | `+`, `◇`, `*` | File, MCP resource, agent. |

## Shared Layout And Wrapping

All transcript content MUST be wrapped in-tree before ratatui rendering. Transcript rendering MUST NOT call `Paragraph::wrap`, and transcript blocks MUST NOT add horizontal `Block` padding.

The wrapper MUST:

1. Normalize text to NFC, convert `\r\n` to `\n`, and handle hard-newline segments independently.
2. Expand tabs to the next 8-column tab stop before wrapping.
3. Split words only on U+0020 ASCII space.
4. Use greedy first-fit and hard-break oversized words at grapheme-cluster boundaries.
5. Preserve leading and trailing row whitespace.
6. Measure each grapheme cluster with `unicode_width::UnicodeWidthStr::width`, never `width_cjk`; ambiguous-width characters are narrow.
7. Keep combining marks and zero-width joiners with their grapheme cluster.
8. Emit a single cluster wider than the wrap limit alone, allowing overflow; it MUST never drop that cluster.
9. Treat all spans as one logical character stream; a style boundary MUST NOT create a word boundary.
10. Apply first-row and continuation prefixes after deciding breaks, producing true hanging indents.
11. Pad every background-bearing row with styled spaces to `fill_width`, based on display width rather than character count.
12. Return the exact rendered lines used both for `insert_before(lines.len())` and drawing; height MUST NOT be recomputed independently.

| Surface | Wrap width | Fill width | First-row prefix | Continuation prefix | Fill |
| --- | --- | --- | --- | --- | --- |
| User message | `columns - 1` | `columns` | `❯ ` | empty | `userMessageBackground` |
| Assistant/top-level event | `columns - 2` | `columns - 2` | platform dot plus one cell | two spaces | none |
| Result block | surface-specific content width | none | five-cell `  ⎿  ` gutter | five spaces | none |
| Shell output | `max(columns - 10, 10)` | none | five-cell gutter | five spaces | none |
| Diff hunk rows | `columns - 12` before own gutter | changed rows padded to available content width | standard result gutter, then diff gutter | same | diff background |

## Surfaces

Every normal top-level transcript event MUST have exactly one blank row before it and no blank row after it. That includes the first event. The blank row MUST remain unstyled. Nested rows explicitly marked as nested omit that leading blank.

### User

| Property | Required behavior |
| --- | --- |
| Shape | `❯ ` at column 0, body at column 2. |
| Glyph style | `subtle`, normal weight, not dim. Selected: `suggestion`. |
| Body | `text`; optional ultrathink trigger characters MAY use the rainbow keys. |
| Background | Every occupied row from column 0 through `columns - 1` uses `userMessageBackground`. Selected: `messageActionsBackground`. The preceding blank row is not filled. |
| Wrapping | `columns - 1`; soft and hard continuations return to column 0, not column 2. |
| Truncation | At more than 10,000 characters: first 2,500 characters, literal line `… +N lines …`, last 2,500 characters. `N` is the count of omitted newlines. |
| Nested | Same glyph/colors/background, but no leading blank. |
| Replayed/resumed | No special styling is specified. |

Slash-command echoes use this same surface and render the reconstructed `/{command} {args}` text.

### Assistant

| Property | Required behavior |
| --- | --- |
| Shape | `⏺` on macOS or `●` elsewhere in a two-cell gutter; body begins at column 2. |
| Glyph | `text`, normal weight, no literal trailing space. Selected: `suggestion`. |
| Body | Base `text`, modified by markdown rules below. |
| Background | None. Selected messages use full-width `messageActionsBackground`. |
| Wrapping | `columns - 2`; every soft and hard continuation starts at column 2. |
| Streaming | Same static shape as committed output. Completed lines MAY commit; partial trailing text stays live. Windows disables live token preview by default. |
| Nested | No glyph, no gutter, no leading blank; body and continuations start at the nested container's column 0. |
| Replayed/resumed | No special styling is specified. |

### Markdown And Code

| Element | Exact rendering |
| --- | --- |
| H1 | No marker; bold + italic + underline; inherited foreground; two trailing newlines. |
| H2-H6 | No marker; bold; inherited foreground; two trailing newlines. |
| Bold | Bold only. |
| Italic | Italic only. |
| Strikethrough | Parsing MUST be disabled; `~~text~~` remains literal. |
| Inline code | Backticks removed; foreground `permission`; no background. |
| Link | OSC 8 generated by picopilot MAY wrap display text. Link text uses terminal ANSI blue, not a palette key. `mailto:` displays without scheme and is not clickable. Without OSC 8 support, show plain uncolored URL. |
| Bare issue | `owner/repo#123` links to `https://github.com/owner/repo/issues/123`. |
| Image | Bare href, unstyled. |
| Blockquote | Prefix every nonblank line with dim `▎ `; quote text italic at normal brightness. Blank quote lines have no bar. |
| Unordered list | Literal `- `, inherited style. |
| List indent | Two spaces times zero-based nesting depth. |
| Ordered list | Depth 0-1 Arabic; depth 2 letters; depth 3 lowercase Roman; depth 4+ Arabic. Honor source start number. |
| Task list | Render as plain `- `; checkbox state is omitted. |
| Horizontal rule | Literal `---`, unstyled, no width fill, no automatic trailing newline. |
| Paragraph | Inherited style, one trailing newline. |
| HTML/definitions | Dropped. |

Assistant tables MUST use box drawing: `┌─┬┐`, `├─┼┤`, `└─┴┘`, and `│`. Cells have one space on each side. Headers are centered and not bold. Data follows markdown alignment. A middle rule appears after the header and between every data row. Minimum column width is 3; budget overhead is `1 + columns_count * 3 + 4`. Multi-line cells are vertically centered. If any row exceeds 4 lines or the table exceeds `terminal_width - 4`, render vertical key/value rows as bold `{Header}:` plus value, continuation indent two, with rows separated by `─` repeated `min(terminal_width - 1, 40)`.

Fenced code MUST have no indent, border, background, gutter, or language label. The info string selects a grammar only. Missing or unsupported languages MUST use plaintext. Highlighting MAY temporarily fall back to plain text while unavailable or disabled.

Syntax colors MUST emulate the reference's ANSI-16 roles: keyword/literal/class/name blue; built-in/attribute cyan; type dim cyan; number/comment/doctag/addition green; string/regexp/deletion red; function yellow; meta/tag grey; emphasis italic; strong bold; link underline; other scopes plain. The chosen syntect theme MUST map sentinel RGB values back to ratatui named ANSI colors in the in-tree span converter.

### Tool Calls

A tool call consists of a header event and a result event. Other messages MAY occur between them.

#### Header

The exact shape is `{dot} {Name}({summary})`, with no space between name and `(`. The name starts at column 2, is bold, does not wrap, and truncates at its end when space is exhausted. Parentheses are omitted when the summary is empty. A tool returning `null`/no name suppresses its header.

| State | Dot behavior |
| --- | --- |
| Queued | Static dim dot. |
| Running | Dim dot toggles between dot and one space every 600 ms; two-cell gutter remains fixed. |
| Success | Solid `success` dot. |
| Error | Solid `error` dot; never blinks. |

Normally the name has no background and default `text`. Picopilot subagents MUST NOT add identity backgrounds. One blank line precedes a top-level header.

Argument summaries MUST be tool-specific:

| Tool | Summary |
| --- | --- |
| Bash | Command; 2-line/160-UTF-16-code-unit truncation rule under Bash below. |
| Bash `sed -i` | Display path only. |
| Read | Display path; `path · pages N`; verbose adds `path · lines A-B` or `path · from line N`. |
| Edit/Write | Display path; empty for plan files. |
| Grep | `pattern: "…"`, optional `, path: "…"`. |
| Glob | `pattern: "…", path: "…"`. |

The separate 50-character compact-summary limit MUST NOT be applied to transcript headers.

#### Progress And Results

Every progress, result, cancellation, and error body uses a five-cell dim gutter on its first row: two spaces, `⎿`, one regular space, one no-break space. Content starts at column 5; continuation rows use five spaces and no repeated glyph. Nested result blocks MUST NOT stack gutters.

Unresolved progress priority is classifier text, then `Waiting for permission…`, then tool-specific progress. Queued Bash shows `Waiting…`. One-line progress is clipped to one row.

General result truncation has three distinct rules:

| Surface | Threshold and exact marker |
| --- | --- |
| Shell stdout/stderr | Wrap first. Show 3 rows. If exactly one more row exists, show it instead of a marker. Otherwise append dim `… +N lines (ctrl+o to expand)`. `N` may be estimated after `3 * wrap_width * 4` characters. |
| Tool errors | Show first 10 source lines. Append dim `… +1 line (ctrl+o to see all)` or `… +N lines (ctrl+o to see all)`; shortcut portion is bold. |
| Write results | Show first 10 content lines. Append `… +N line(s) (ctrl+o to expand)` with normal singular/plural. |

In nested/virtual/suppressed contexts, shell markers omit `(ctrl+o to expand)`. Verbose mode removes command/output/error truncation, expands paths, and shows full Grep/Glob bodies. There is no per-call fold icon; the visible affordance is the text hint.

Fallback errors MUST strip wrapper tags, trim, and render `Invalid tool parameters` for nonverbose `InputValidationError`. Preserve an existing `Error: ` or `Cancelled: ` prefix; otherwise add `Error: `. Render in `error`. Cancellation is one dim clipped row: `Interrupted · What should Claude do instead?`.

### Bash And Shell

#### Command

The header is `Bash(summary)`. The command has no shell syntax styling.

Nonverbose truncation MUST follow this order:

1. Test whether the original has more than 2 lines or more than 160 UTF-16 code units.
2. If neither, return it unchanged with no ellipsis.
3. If line truncation is needed, keep the first 2 lines joined by `\n`.
4. If the retained text still exceeds 160 UTF-16 code units, slice it to 160.
5. Trim the retained text and append `…` with no separating space.

Verbose mode shows the full command. An embedded second command line SHOULD start at assistant continuation column 2; this is a deliberate picopilot choice because the reference position is Unverified.

#### Output Channels And Exit

| Channel/state | Rendering |
| --- | --- |
| stdout | Own `⎿` block; base `text`. |
| stderr | Separate `⎿` block; base `error`. Whitespace-only stderr is dropped. |
| image | One dim line: `[Image data detected and sent to Claude]`. |
| exit 0 | No exit-code text; header dot communicates success. |
| nonzero exit | Error path. First line exactly `Error: Exit code N`, followed by output, subject to the 10-line error truncator. |
| silent success | `Done` for `mv`, `cp`, `rm`, `mkdir`, `rmdir`, `chmod`, `chown`, `chgrp`, `touch`, `ln`, `cd`, `export`, `unset`, `wait`. |
| empty ordinary success | `(No output)`. |
| background | `Running in the background (↓ to manage)`. |
| no-match interpretation | `No matches found` for `grep`/`rg` exit 1 when output is empty. |
| `find` exit 1 | `Some directories were inaccessible` when output is empty. |
| `diff` exit 1 | `Files differ` when output is empty. |
| `test`/`[` exit 1 | `Condition is false` when output is empty. |

Before display, output MUST drop leading/trailing blank lines and preserve interior blanks. For content up to 10,000 characters, each line SHOULD independently attempt JSON parse and pretty-print with two-space indentation only when round-trip precision is preserved. Bash output MUST NOT be URL-linkified.

Output MUST wrap at `max(columns - 10, 10)` visible columns and trim each produced row's trailing whitespace. The 3-row truncation applies separately to stdout and stderr.

#### Live Progress

No output yet: dim `Running… ` plus elapsed/timeout. With output: last 5 sanitized lines, then a dim status row containing available fields separated by one space:

- `~N lines` when totals are known, otherwise `+N lines` beyond the five shown.
- `(12s)`, `(timeout 2m)`, or `(1m 5s · timeout 2m)`.
- `512 bytes`, `1.5KB`, `2MB`, or `1.1GB`.

This preview is mutable live content and MUST disappear when the final result arrives. It remains enabled on Windows.

### File-Edit Diff

A transcript diff is only a result block. It MUST have no border, box, padding, or repeated path header. The file path remains in `Update(path)` or `Create(path)`.

The first result line is:

- `Added ` + bold count + ` line(s)` when additions exist.
- `removed ` + bold count + ` line(s)` after `, ` when both exist.
- `Removed ` with capital `R` when no additions exist.

Hunks receive width `columns - 12`. Use primary-renderer layout and numbering with fallback palette colors; this is an explicit design choice.

The diff gutter is `max_digits + 3` cells: leading space, right-aligned line number, space, sigil. Added rows use the new counter, removed rows the old counter, and context rows display the new counter while advancing both. Wrapped continuations omit the number but repeat the sigil. Changed rows MUST pad their background to the content edge; context rows are unfilled and their gutter is dim.

Between hunks, render dim `...` at the diff left edge with no blank line. Use 3 context lines. There is no row-count cap.

Diff computation SHOULD stop after 5 seconds. On timeout, omit hunks and retain the summary. Inputs over a chosen implementation size limit MAY omit the diff; the 10 MiB reference scan cap is guidance, not a resolved picopilot limit.

Word-level highlighting MUST be included with `similar`. Pair adjacent runs of removals then additions by position. Tokenize into Unicode letter/number/underscore runs, whitespace runs, and individual remaining codepoints. Run Myers over those tokens. Let `total_len = old_len + new_len`; if changed-token length divided by `total_len` is greater than `0.4`, omit word highlighting for the pair. Apply `diffRemovedWord`/`diffAddedWord` before wrapping over `diffRemoved`/`diffAdded`. Dim/rejected diffs MUST disable word highlighting and use dimmed line backgrounds.

The dashed `╌` top/bottom frame in `subtle` belongs only to an inline permission diff, never a transcript diff.

### Spinner

The spinner is one live row with one blank row above it. Column 0 is the glyph in a two-cell box; the verb begins at column 2; a hard trailing space follows the verb; optional status follows in dim parentheses.

The platform's six frames MUST be concatenated with their reverse, preserving doubled endpoints. Windows example:

`· ✢ * ✶ ✻ ✽ ✽ ✻ ✶ * ✢ ·`

Frame duration is 120 ms; full cycle is 1,440 ms. Reduced motion uses a static `●`, no shimmer, no flash, no token animation, and immediate stall color.

One verb MUST be chosen once per busy turn. Precedence is explicit override, active todo form, todo subject, random built-in verb. Append one `…`. The verb MUST NOT rotate during the turn.

The exact built-in list is:

> Accomplishing, Actioning, Actualizing, Architecting, Baking, Beaming, Beboppin', Befuddling, Billowing, Blanching, Bloviating, Boogieing, Boondoggling, Booping, Bootstrapping, Brewing, Bunning, Burrowing, Calculating, Canoodling, Caramelizing, Cascading, Catapulting, Cerebrating, Channeling, Channelling, Choreographing, Churning, Clauding, Coalescing, Cogitating, Combobulating, Composing, Computing, Concocting, Considering, Contemplating, Cooking, Crafting, Creating, Crunching, Crystallizing, Cultivating, Deciphering, Deliberating, Determining, Dilly-dallying, Discombobulating, Doing, Doodling, Drizzling, Ebbing, Effecting, Elucidating, Embellishing, Enchanting, Envisioning, Evaporating, Fermenting, Fiddle-faddling, Finagling, Flambéing, Flibbertigibbeting, Flowing, Flummoxing, Fluttering, Forging, Forming, Frolicking, Frosting, Gallivanting, Galloping, Garnishing, Generating, Gesticulating, Germinating, Gitifying, Grooving, Gusting, Harmonizing, Hashing, Hatching, Herding, Honking, Hullaballooing, Hyperspacing, Ideating, Imagining, Improvising, Incubating, Inferring, Infusing, Ionizing, Jitterbugging, Julienning, Kneading, Leavening, Levitating, Lollygagging, Manifesting, Marinating, Meandering, Metamorphosing, Misting, Moonwalking, Moseying, Mulling, Mustering, Musing, Nebulizing, Nesting, Newspapering, Noodling, Nucleating, Orbiting, Orchestrating, Osmosing, Perambulating, Percolating, Perusing, Philosophising, Photosynthesizing, Pollinating, Pondering, Pontificating, Pouncing, Precipitating, Prestidigitating, Processing, Proofing, Propagating, Puttering, Puzzling, Quantumizing, Razzle-dazzling, Razzmatazzing, Recombobulating, Reticulating, Roosting, Ruminating, Sautéing, Scampering, Schlepping, Scurrying, Seasoning, Shenaniganing, Shimmying, Simmering, Skedaddling, Sketching, Slithering, Smooshing, Sock-hopping, Spelunking, Spinning, Sprouting, Stewing, Sublimating, Swirling, Swooping, Symbioting, Synthesizing, Tempering, Thinking, Thundering, Tinkering, Tomfoolering, Topsy-turvying, Transfiguring, Transmuting, Twisting, Undulating, Unfurling, Unravelling, Vibing, Waddling, Wandering, Warping, Whatchamacalliting, Whirlpooling, Whirring, Whisking, Wibbling, Working, Wrangling, Zesting, Zigzagging.

Shimmer is a three-display-column band. In requesting mode it moves left-to-right one column per 50 ms; otherwise right-to-left per 200 ms. Its cycle spans message width plus 20 steps, starting/ending 10 columns off-text. Base is `claude`; band is `claudeShimmer`.

Tool-use mode replaces the sweep with a whole-message sine interpolation from `claude` to `claudeShimmer`, period 2 seconds. A stall begins after 3 seconds without new output and no active tools, ramps over 2 seconds, and smooths 10% toward target every 50 ms. It stops shimmer and fades glyph and verb toward `{171,43,63}`.

Timer and output-token estimate appear together only when elapsed time is greater than 30 seconds, or immediately in verbose/team mode. Tokens estimate current-turn streamed output as rounded characters divided by 4. Format below 1,000 as integer; at/above 1,000 use lowercase compact notation with exactly one decimal, such as `1.0k` or `12.4k`. The counter climbs per 50 ms tick: +3 for gap <70, max(8, ceil(15% gap)) for gap <200, otherwise +50. Reduced motion snaps.

Elapsed format is `0s`, `12s`, `1m 5s`, `1h 2m 3s`, or `1d 2h 3m`, with no zero padding. Status parts are admitted in order: thinking, timer, tokens. Join with ` · ` inside parentheses. Thinking may reduce from `thinking with {level} effort` to `thinking` to fit. The model MUST NOT appear on this line.

### Input And Hints

The prompt area is:

```text
[one blank row]
────────────────────────────────
❯ input
────────────────────────────────
  hint
```

Top and bottom rules MUST be `─` repeated to full terminal width in `promptBorder`, with no corners/sides, no dim, and no animation. Bash mode changes both rules and `! ` marker to `bashBorder`. Fast mode MAY splice ` ↯ ` or ` ↯ /fast ` into the right end of the top rule; `/fast` appears for the first 5 seconds.

The normal prompt marker is `❯ ` at column 0 in inherited foreground, dim while working. Input begins at column 2; continuation rows also begin at column 2. The marker appears only on row one. Cursor presentation SHOULD use a reverse-video software cell with no blink; retaining the native cursor is an accepted implementation choice only if placeholder behavior remains correct.

An empty input MAY show dim placeholder text with its first character inverse. Priority is teammate message, queued-message hint `Press up to edit queued messages`, first-session rotating example, then no placeholder. Steady-state empty input shows only the marker and inverse blank cursor.

Visible input rows MUST equal `max(3, available_budget)`. Hidden input scrolls cursor-centred with `start = clamp(cursor_row - floor(max/2), 0, total_rows - max)`. No truncation marker appears. Input above 10,000 characters SHOULD be replaced by `[Pasted text #N]` and stored out of band.

The footer starts at column 2. Parts use dim ` · ` separators. It disappears on the first typed character, except mode/task/status parts that are not hints. Default empty idle text is `? for shortcuts`; busy text is `esc to interrupt`. It MUST truncate at the tail, never wrap. Below 80 columns, left and right footer groups stack.

Resolved mode text:

| State | Exact text/style |
| --- | --- |
| Plan | `⏸ plan mode on` in `planMode`; optional dim `(shift+tab to cycle)`. |
| Accept edits | `⏵⏵ accept edits on` in `autoAccept`. |
| Bypass | `⏵⏵ bypass permissions on` in `error`. |
| Don't ask | `⏵⏵ don't ask on` in `error`. |
| Vim insert | `-- INSERT --` dim; replaces other left hints. No `-- NORMAL --`. |
| Bash | `! for bash mode` in `bashBorder`; replaces other hints. |

Completions replace the hint slot below the input, indented 2, borderless and titleless. Show at most 6 rows, cursor-centred. Selected row uses `suggestion`; others are dim. No pointer/background. Commands align names in a column capped at 40% width, then optional `[tag] ` and description. Resource rows use `{icon} {name} – {description}`. Paths truncate in the middle; rows truncate at tail. `Tab` accepts, `Esc` dismisses, arrows navigate, and `Enter` remains input submission.

### Inline Pickers

A picker replaces the input box in the live region. It MUST NOT clear or overlay transcript rows and MUST leave none of its list rows in history. On confirmation or cancellation it SHOULD commit one short outcome line; cancellation SHOULD state the unchanged value. A picker MAY explicitly choose no trace.

Rows are borderless and have a one-cell indicator column. Focus uses `❯` in `suggestion`. When the first/last visible row hides options, dim `↑`/`↓` replaces the pointer in that same column. Labels use `inactive` when disabled, `success` when selected, `suggestion` when focused, otherwise inherited text. No inverse-video/background selection is allowed.

Single-select compact rows MAY use 1-based aligned numeric prefixes. Default visible count is 5. Long lists MUST add `(n of m)` to the title. Window movement is one row at an edge; navigation wraps to the opposite end. Keys: `↑`/`k`/`ctrl+p`, `↓`/`j`/`ctrl+n`, `PageUp`/`PageDown`, `Enter`, `Esc`, and immediate numeric `1`-`9` selection where indices are shown.

Multi-select rows use `[✓]`/`[ ]`; checked marker is `success`, focus remains `suggestion`. `Space` toggles, `Enter` applies, `Esc` cancels. Tool/skill shortcuts `s` and `a` MAY remain.

| Existing surface | Required destination |
| --- | --- |
| Sessions | Single-select picker titled `Select a session to resume (n of m):`; aligned `Updated` and `Session Title`; 5 rows. |
| Models | Single-select compact picker; context/cost becomes inline description; drop detail pane; `←`/`→` adjust focused-row effort/context setting. |
| Tools | Multi-select checklist. |
| Skills | Multi-select checklist; description inline; source/directory move to static `/skills` output if retained. |
| Usage | Static transcript block, not a picker. |
| Todos | Noninteractive live block above input, toggled by `Ctrl+T`, never committed; cap rows and summarize hidden work as ` … +N pending, M in progress`. |
| Completion | Footer alternate state described above. |

An approval is a picker: tool-call-form header, one `⎿` plain-language detail row, question, then self-describing borderless choices. Remove the border, yellow title, `y / n / a / v` legend, and separate details view.

### `/status` And Usage

`/status` MUST print a static transcript block, despite the reference using an interactive settings pane. It consists of a normal user echo then a local-output result block. The result begins at column 5 behind one dim gutter and has no background.

Rows use bold `Label:` in `text`, one space, then value; values are not column-aligned. No blank rows occur inside a section; exactly one blank row separates identity and configuration sections.

Minimum picopilot fields:

```text
❯ /status
  ⎿  Version: 0.1.0
     Session ID: …
     cwd: C:\dev\picopilot

     Model: …
     Tools: 7 enabled, 2 disabled · /tools
     Skills: 3 enabled, 38 disabled · /skills
```

Counts use state colors (`success`, `warning`, `inactive`, `error`), with dim `· /command`. Empty local output is dim `(no content)`.

Field placement is normative:

| Field | Placement |
| --- | --- |
| Full cwd/project | `/status`. |
| Model | `/status`; never spinner. |
| Reasoning effort | Spinner thinking status only. |
| Busy/autopilot ready/working | Dropped. |
| Tool and skill counts | `/status`. |
| Current-turn output estimate | Spinner after 30 seconds. |
| Context current/limit and attribution | `/usage` static block. |
| Session cost/request counts | `/usage` static block. |
| Version and Session ID | `/status`. |
| Session name | Only if a rename feature exists. |

`/usage` MUST use the same user echo plus five-cell local-output gutter. It MUST retain picopilot's existing `Session cost:` and `Context window: current / limit tokens` information and attribution breakdown. It SHOULD be dim like Claude Code `/cost`, with fixed labels where the existing formatter already defines them. Exact additional field wording is Unverified because no resolution enumerated the complete current usage block.

The one persistent status is context pressure. Hide it until current usage is within 20,000 tokens of the effective threshold. Then render dim `{percent}% until auto-compact`, right-aligned in the footer and tail-truncated. Below 80 columns stack it under the hint. Suppress it for the rest of a turn after compaction. Picopilot MUST use its own effective token limit; whether that limit already reserves compaction space is Unverified.

### Picopilot-Only Reasoning, Subagents, Banners, Diagnostics, Approval

All top-level picopilot-only events use the assistant hanging-indent shape and normal one-blank-line-before rule unless they are live picker content.

| Surface | Exact form and state |
| --- | --- |
| Reasoning | `✻ Thinking…`; glyph `subtle`, body dim italic at column 2. Collapsed by default. Expand with the same command used for full tool output. Expanded body preserves the two-column hanging indent. |
| Subagent | Ordinary `Task(description)` tool call with normal tool dot/state. Nested calls add one result-gutter level. No agent name or identity color. Closing row: `Done (N tool uses · N tokens · Ns)`. |
| Warning | Platform dot in `warning`, body column 2, no `warning:` label, no bold. |
| Recoverable error | Same warning form; the former salmon tier is removed. |
| Blocking error | Platform dot in `error`, body column 2, no `error:` label, no bold. |
| Diagnostic | Platform dot in `subtle`, body column 2, no `debug` label, gated by `Ctrl+I`. |
| Approval | Inline picker described above; it replaces input and remains live until resolved. |

## ANSI Sanitization And Shell-Output Security

All text not generated by picopilot code MUST be sanitized at ingestion, once, before storage, truncation, width measurement, export, or rendering. This includes shell progress, stdout/stderr, tool errors/results, assistant text, file content, and diff content.

### Allowlist

Allowed input is limited to:

- Printable Unicode text, including combining marks and emoji.
- U+000A LF as a line break.
- U+0009 tab, rewritten to the next 8-column stop.
- `CSI … m` SGR containing only recognized supported parameters.

Supported SGR roles are reset; bold; dim; italic; reverse; strikethrough; their off/reset codes where supported; named foreground/background 30-37/40-47 and bright 90-97/100-107; default foreground/background 39/49; indexed `38/48;5;N`; truecolor `38/48;2;R;G;B`. Underline family `4`, `21`, `24`, `58`, and `59` MUST be removed as attributes. A sequence containing an unknown parameter MUST be dropped as a whole.

Normalize `\r\n` to `\n`; delete every remaining `\r`.

### Mandatory Drops

The sanitizer MUST delete:

- Every non-SGR CSI, including cursor movement, erase, scroll, and scroll-region commands.
- Every OSC through BEL or ST, including OSC 8 links, OSC 52 clipboard, and title controls.
- DCS, APC, PM, and SOS payloads.
- Charset designation `ESC (`, `ESC )`, `ESC *`, and `ESC +` forms.
- Single-character escapes such as `ESC 7`, `ESC 8`, `ESC c`, `ESC D`, and `ESC M`.
- Lone/truncated/unrecognized escape sequences.
- Every C0 control except LF/tab, including backspace, BEL, VT, FF, and NUL.
- C1 8-bit controls U+0080-U+009F.

No untrusted escape byte may reach ratatui/crossterm. Picopilot-generated hyperlinks MAY be emitted separately; subprocess-supplied OSC 8 MUST never survive.

After sanitization, `ansi-to-tui` MUST parse SGR into ratatui spans before wrapping. Clear `Modifier::UNDERLINED` on every parsed style. Patch parsed styles over the surface base style so explicit subprocess colors win; do not remap them to the Claude palette. A span split at wrapping copies its style to both parts; no escape reopening is needed.

## Accepted Dynamic Behavior

All animation phases MUST derive from one `t` sampled once per frame. The existing 50 ms tick is the master clock. Rendering SHOULD occur only when live state is dirty or animation is active; commits MUST occur only on state transitions.

| Behavior | Timing | Cost/constraint |
| --- | --- | --- |
| Master clock | 50 ms | Existing wake-up. Live-region cell diff only; no scrollback write. |
| Spinner frame | 120 ms | About 8 glyph writes/s. |
| Request shimmer | 50 ms/column | Up to 20 small updates/s. |
| Normal shimmer | 200 ms/column | About 5 updates/s. |
| Tool dot blink | 600 ms half-period, 1,200 ms cycle | Shared phase; static dim dot is valid fallback when animation is disabled. |
| Tool-use flash | 2,000 ms sine | Whole verb color interpolation. |
| Stall | Starts at 3,000 ms; target ramp 2,000 ms; 10% smoothing per 50 ms | Disabled while tools are active; instant in reduced motion. |
| Timer/tokens | Show after `> 30,000 ms` | Adds status text and counter updates; no model. |
| Thinking state | Hold `thinking` for minimum 2,000 ms, then `thought for Ns` for 2,000 ms | Thinking glow uses a 2,000 ms sine. The source's 3,000 ms delay is vestigial and MUST NOT be copied. |
| Focus lost | SHOULD reduce tick to 250 ms if focus events work | Optimization only; stay at 50 ms if no events arrive. |
| Reduced motion | Static `●`; no shimmer/flash/counter easing; instant stall color | Must preserve all static information. |

## Dependencies

### Required

| Crate | Version/features | Purpose and rule |
| --- | --- | --- |
| `ratatui` | Existing `0.29`; remove `unstable-rendered-line-info` | Inline viewport and `insert_before`; owned wrapping makes unstable line counting unnecessary. |
| `crossterm` | Existing `0.28` | Main-screen terminal, events, focus, bracketed paste. |
| `pulldown-cmark` | Existing `0.13` | Markdown parsing; disable strikethrough behavior and adapt emitted structure. |
| `unicode-width` | Existing `0.2` | Grapheme display width; use non-CJK width. |
| `unicode-segmentation` | Add a compatible current version; exact version Unverified | Grapheme boundaries for wrapping. It is currently only transitive and MUST become direct. |
| `syntect` | `5.3.0`; default/onig path preferred, exact feature declaration Unverified | Syntax engine and tmTheme loading. |
| `two-face` | `0.5.2`; syntax assets including extra newlines | Embedded bat syntax set, including TypeScript, TOML, Dockerfile, PowerShell on the onig path. Use `two_face::syntax::extra_newlines()`. |
| `similar` | `3.2.0`, features `inline`, `unicode` | Line and token Myers diff plus deadline support. Use custom tokenization, not its inline tokenizer. |
| `ansi-to-tui` | Pin exactly `7.0.0` | Parse SGR into ratatui 0.29 types. Version 8.x uses `ratatui-core` and is incompatible until ratatui is upgraded. |

The syntect-to-ratatui span conversion MUST be implemented in-tree. `syntect-tui` is not required.

### Existing Crates Reused

`tokio` `1` supplies the existing 50 ms clock; `serde_json` `1` supports bounded line-wise JSON formatting. Existing application dependencies remain unchanged unless implementation proves otherwise.

### Candidate Or Unverified

| Candidate | Status |
| --- | --- |
| `two-face` bundled `Ansi` theme | Inspect before creating a custom tmTheme; whether it matches required ANSI roles is Unverified. |
| `syntect` onig build | Preferred for speed/PowerShell grammar, but Windows build success and binary/build-time cost MUST be measured. `fancy` loses PowerShell and other grammars. |
| `ratatui` `scrolling-regions` | Off initially; candidate only after performance measurement and Windows console tests. |
| `vte`/`anstyle-parse` | Not required. Candidate only if future unbounded incremental ANSI parsing is needed. |
| ratatui 0.30+ / `ansi-to-tui` 8.x | Future coordinated upgrade; runtime-resizable inline viewport availability remains Unverified. |

## Migration Impact

The implementation MUST remove or replace these current picopilot mechanisms:

| Current mechanism | Required replacement |
| --- | --- |
| Alternate screen entry/exit | Main screen with `Viewport::Inline` and `insert_before`. |
| App-owned transcript scroll offset | Terminal-native scrollback. |
| Persistent status bar | Spinner status, `/status`, `/usage`, and context footer warning. |
| Persistent shortcut bar | Conditional hint/footer row. |
| Centered modal areas, `Clear`, full borders, inverse selection | Inline compact pickers or static transcript output. |
| One scrollable transcript `Paragraph` | Per-event prewrapped lines, explicit committed/live boundary. |
| `Paragraph::wrap` for transcript | In-tree grapheme/span wrapper. |
| ratatui `unstable-rendered-line-info` | Exact `Vec<Line>::len()` from owned wrapper. |
| Scattered hardcoded `Color::Rgb` values | Named dark palette above. |
| User orange bold `❯ ` | Normal `subtle` pointer. |
| Assistant green `● ` and agent-id prefix | Platform dot in `text`; nested subagents use `Task(...)` without identity label/color. |
| Blue `tool name [state]` and `$ command` | Dot-state `Name(args)` header plus gutter results. |
| Combined unbounded shell output | Separate sanitized stdout/stderr blocks with width and truncation rules. |
| Bordered approval box and `y/n/a/v` legend | Inline picker with self-describing choices. |
| Purple subagent, salmon retry, labeled bold banners | Normal tool states and warning/error/subtle top-level rows. |

## Verification Checklist

A later implementation session MUST run unit/property tests and visual fixtures at multiple widths. No production rollout is complete until Windows Terminal checks pass.

### Structural And Automated

- [ ] Verify alternate-screen sequences are absent and native scrollback remains selectable after exit.
- [ ] Verify `insert_before` receives exactly the same line vector length that it draws; property-test widths, hard newlines, empty lines, CJK, combining marks, emoji ZWJ, tabs, and a cluster wider than width.
- [ ] Verify committed events cannot be mutated by type/API and late changes append a new event.
- [ ] Verify resize repaints only live rows and never recommits history.
- [ ] Verify transcript rendering has no `Paragraph::wrap`, horizontal transcript padding, or `unstable-rendered-line-info` feature.
- [ ] Verify every one of the 69 palette keys and exact RGB values.
- [ ] Verify the 50 ms clock drives all phases from one timestamp and stops drawing when idle.
- [ ] Verify reduced motion freezes all motion while preserving text and state.
- [ ] Verify ANSI sanitization fuzz cases never emit ESC/control bytes to the backend.
- [ ] Verify SGR named/indexed/truecolor parsing, underline removal, base-style patching, malformed/truncated sequence drops, CR progress output, and chunk boundaries.
- [ ] Verify syntax highlighting loads embedded grammars without runtime files and unsupported languages become plaintext.
- [ ] Verify diff timeout, 3-line context, two-counter numbering, wrapped repeated sigils, 0.4 word threshold, and no row cap.

### Visual Fixtures

- [ ] User: short, wrapped, hard newline, 10,001-character truncation, selected, consecutive user messages; confirm full-width fill and unfilled separator.
- [ ] Assistant: macOS/Windows dots, wrapped hanging indent, streaming partial line, selected, nested no-dot form.
- [ ] Markdown: H1/H2/H6, bold/italic/literal strikethrough, inline code, OSC/no-OSC links, mailto, issue reference, image, multiline blockquote with blank line, nested ordered/unordered lists, task item, rule, HTML drop.
- [ ] Code: known language, unknown language, missing language, highlighting disabled, long line, ANSI-role colors.
- [ ] Table: aligned normal table, wrapped cells, 4-line boundary, 5-line vertical fallback, width fallback.
- [ ] Tool: queued/running/success/error dots, 600 ms blink, no-summary/no-name cases, every argument-summary shape, progress priority, nested gutter suppression.
- [ ] Truncation: shell 3 rows, exactly fourth row, estimated large count; error 10 rows singular/plural; write 10 rows; verbose expansion.
- [ ] Bash: exact 160, 161, two-line, three-line, `sed -i`, JSON line, stdout+stderr, image, exit 0/nonzero, silent/no-output/interpretation/background, five-line live preview.
- [ ] Diff: add/remove/context, same-number replacement, wrapped lines, multiple hunks and `...`, summary grammar, dim rejection, 0.4 boundary, timeout summary-only, permission-only dashed frame.
- [ ] Spinner: all three platform frame sequences, doubled endpoints, every width-gating stage, 30-second threshold, compact tokens, effort text, requesting shimmer, tool flash, stall, reduced motion.
- [ ] Input/hints: normal/Bash/plan/vim/fast, empty/typed/busy, placeholder cursor, 3-row minimum, long cursor-centred prompt, no hidden-row marker, context warning wide/narrow.
- [ ] Completion: 1/6/>6 rows, selected/unselected, command/resource forms, middle/tail truncation, `Tab` versus `Enter`.
- [ ] Pickers: sessions/models/tools/skills, 500-item five-row window, arrows/pointer collision, numeric selection, wrap navigation, `[✓]`, cancel outcome, no surviving picker rows.
- [ ] `/status`: exact echo/gutter, section blank, labels, colored count summary, empty output.
- [ ] `/usage`: cost, context totals, attribution, long wrapping.
- [ ] Picopilot-only: collapsed/expanded reasoning, nested concurrent Tasks without names/colors, warning/recoverable/blocking/diagnostic rows, approval picker.

### Windows Terminal

- [ ] Run in Windows Terminal, VS Code integrated terminal, and legacy conhost where available.
- [ ] Confirm no viewport jump while assistant output streams; live assistant token preview is off by default under Windows/`WT_SESSION`.
- [ ] Confirm Bash progress remains live on Windows.
- [ ] Confirm LF-based commits preserve scrollback and do not duplicate or erase rows.
- [ ] Confirm resize narrow/wide behavior, including committed user backgrounds.
- [ ] Confirm focus events; if absent, clock remains correct at 50 ms.
- [ ] Confirm Unicode glyph widths for `❯`, `●`, `⎿`, `⏵⏵`, checkmarks, todo squares, spinner frames, and emoji.
- [ ] Confirm ANSI truecolor, indexed color, named terminal color, and stripped underline.
- [ ] Do not enable `scrolling-regions` unless both ANSI ConPTY and WinAPI fallback behavior pass.

## Unverified

This section is authoritative: implementations MUST NOT invent values for these items.

### Environment And Reference

- No ticket observed real Claude Code output. This machine had **no `bun`, no `node_modules`, and no `claude` binary**, so there was **no real rendered-output comparison**.
- Source-only column arithmetic, Ink/Yoga spacing, backgrounds, and state behavior were not screenshot-tested.
- Bun's actual `wrapAnsi` and `stringWidth` paths were not read or executed; wrapping was derived from the npm fallback and source comments.
- `cli-highlight` default colors were read from upstream `master`, not the exact installed package.
- Published crate size/build-time figures were not locally measured.

### Glyphs And Layout

- `figures@6.1.0` runtime values and non-Unicode fallbacks were unavailable. This affects `❯`/`>`, `↑`, `↓`, warning, checkmark, and todo glyphs.
- The `cli-boxes` `round` table was not available locally. The `─` top/bottom glyph is taken from package documentation; the source does verify that corners and sides are suppressed.
- `ListItem` documentation says `✓`, while `figures.tick` documentation suggests `✔`; exact single-select and `[tick]` runtime glyph is unresolved.
- Multi-select sibling gap behavior may produce `❯ 1. [✓] Label` or tighter spacing; it was not rendered.
- The cell width of `⏵⏵`, and whether macOS `⏺` consumes one or two cells, was not measured.
- Bash embedded-newline continuation placement is unresolved; this spec deliberately chooses column 2.
- Picker blank-line-above convention differs by caller and remains unresolved; implementation fixtures must choose consistently with the global event rule.
- Replayed/resumed message-specific styling was not found.
- The assistant outer row contains an unused right-hand layout slot; whether anything can occupy it is unknown.
- The rotating first-prompt example returned by `getExampleCommandFromCache()` was not enumerated.
- The shipped reference prompt's uncapped growth was derived from `maxVisibleLines === undefined`; a very large paste was not stress-tested.

### Screen Model And Terminal

- `insert_before` was not executed on Windows Terminal.
- `microsoft/terminal#14774` was not independently inspected.
- The fixed live viewport height is unresolved: `12` and `14` are arithmetic proposals, not measurements.
- ratatui 0.30+ runtime viewport resizing was not checked.
- `scrolling-regions` was not exercised on any platform.
- Very long `insert_before` commits and the need for chunking were not measured.
- Resize behavior for padded committed rows was inferred, not observed.
- Crossterm focus events on Windows Terminal were not tested.

### Palette And Dynamic Behavior

- `subtle` contrast depends on the user's terminal background and was not measured.
- The off-palette stall target `{171,43,63}` appears to be the light-theme error value; intent is unknown.
- Truecolor fallback behavior remains unresolved.
- Reduced-motion pulse appears dead in the reference path; static `●` is inferred.
- One random verb per turn depends on spinner unmount/remount behavior and was inferred.
- `figures.arrowUp`/`arrowDown` values in token status were not read from the package.
- The recovered spinner-mode union may omit modes that render identically.

### Markdown, Highlighting, And Unicode

- Task-list checkbox loss is inferred from absent handling, not observed.
- The exact installed `cli-highlight` version and theme could not be inspected without `node_modules`.
- tmTheme scope-to-highlight.js-class mapping is an implementation design, not a reference rule.
- `two-face`'s `Ansi` theme was not inspected.
- `syntect-tui` compatibility was not tested; it is intentionally not selected.
- Oniguruma build success, compile time, and binary size on Windows are unmeasured; exact `syntect`/`two-face` feature declarations remain to be pinned by a build spike.
- The Unicode-space divergence between ratatui and wrap-ansi was reasoned, not executed.
- Unknown differences between Bun and npm wrappers may remain.

### Tool, Bash, Diff, And ANSI

- The reason transcript diff width is `columns - 12` rather than the five-cell gutter width is unknown.
- The primary reference diff's ANSI-256 add/delete asymmetry may be a bug; intent is unknown.
- The visual join between the five-cell result gutter and diff background was not observed.
- Syntax highlighting inside diff lines remains unresolved; the reference highlights additions/context but not removals.
- JavaScript's 160 UTF-16-code-unit Bash slice can split astral characters; practical impact is unknown.
- `ansi-to-tui` 7.0.0 compatibility was inferred from dependency metadata, not compiled locally.
- `ansi-to-tui` 7.0.0 lacks modifier off-codes 23/24/25/27/29; styles may persist until SGR 0. This is accepted until a coordinated upgrade.
- Its OSC/ST over-consumption behavior was reasoned from parser combinators, not reproduced.
- Ink's inner ANSI color overriding outer stderr color was inferred from component structure.
- No terminal-specific dangerous-sequence behavior was tested; the allowlist assumes the safest case.

### Status And Product Decisions

- Whether picopilot's `token_limit` already excludes a compaction buffer is unknown.
- The reference context warning's warning-color branch appears unreachable because warning/error thresholds are both 20,000; this was inferred.
- `/status`, `/usage`, `/tools`, and `/skills` command registration is not specified here.
- Session name is conditional on a future rename feature.
- Exact additional `/usage` line wording beyond resolved cost/context/attribution fields was not enumerated.
- `Clauding` remains in the copied spinner verb list; replacing it with picopilot branding is a product decision, not a rendering fact.
- The accepted picopilot-only mock-up was not run; its spacing inherits the shared rules.

## Source Index

### Local Resolution Tickets

- [01 dark palette](../.wayfinder/rendering-parity/tickets/01-dark-palette.md): all 69 keys, dynamic/off-palette colors, current hardcoded-color delta.
- [02 scrollback mechanism](../.wayfinder/rendering-parity/tickets/02-scrollback-mechanism.md): main-screen behavior, explicit commit/live model, ratatui limits, Windows exception, resize.
- [03 user and assistant](../.wayfinder/rendering-parity/tickets/03-user-assistant-messages.md): glyphs, columns, backgrounds, blank lines, truncation.
- [04 markdown and code](../.wayfinder/rendering-parity/tickets/04-markdown-and-code-blocks.md): markdown elements, tables, syntax-highlighting choice.
- [05 tool calls](../.wayfinder/rendering-parity/tickets/05-tool-call-rendering.md): header states, gutter, summaries, progress, truncators, errors.
- [06 Bash rendering](../.wayfinder/rendering-parity/tickets/06-bash-tool-rendering.md): command/output/exit/progress rules.
- [07 file-edit diff](../.wayfinder/rendering-parity/tickets/07-file-edit-diff.md): result container, gutter, palette, word diff, timeout.
- [08 spinner](../.wayfinder/rendering-parity/tickets/08-spinner-line.md): frames, verbs, timings, status fields, master clock.
- [09 input and hints](../.wayfinder/rendering-parity/tickets/09-input-box.md): prompt rules, marker, modes, footer, input growth, completions.
- [10 inline pickers](../.wayfinder/rendering-parity/tickets/10-inline-pickers.md): picker rows, keyboard model, modal mapping, live budget.
- [11 status](../.wayfinder/rendering-parity/tickets/11-status-command.md): status/usage placement and context warning.
- [12 picopilot-only surfaces](../.wayfinder/rendering-parity/tickets/12-picopilot-only-surfaces.md): reasoning, Task subagents, banners, diagnostics, approval.
- [14 wrapping and fill](../.wayfinder/rendering-parity/tickets/14-wrapping-and-background-fill.md): shared in-tree wrapper and Unicode rules.
- [15 ANSI passthrough](../.wayfinder/rendering-parity/tickets/15-ansi-passthrough.md): parse-first SGR handling and security allowlist.

### Picopilot Files

- [Cargo.toml](../Cargo.toml): current dependency versions and unstable ratatui feature.
- [src/tui.rs](../src/tui.rs): current alternate-screen renderer, transcript paragraph, bars, modals, colors, glyphs, and 50 ms loop.
- [src/events.rs](../src/events.rs): usage/context snapshots and tool event data.
- [src/input_editor.rs](../src/input_editor.rs): prompt editing behavior.

### Claude Code Reference Files

External paths are plain because they are outside this workspace.

| Rules | Key source files under `C:\dev\git\claude-code` |
| --- | --- |
| Palette | `src\utils\theme.ts` |
| Main-screen renderer | `src\ink\log-update.ts`, `src\ink\screen.ts`, `src\ink\terminal.ts`, `src\ink\output.ts`, `src\components\Messages.tsx`, `src\screens\REPL.tsx` |
| User/assistant | `src\components\messages\UserPromptMessage.tsx`, `src\components\messages\AssistantTextMessage.tsx`, `src\components\messages\MessageRow.tsx`, `src\constants\figures.ts` |
| Markdown | `src\utils\markdown.ts`, `src\components\Markdown.tsx`, `src\components\MarkdownTable.tsx`, `src\utils\cliHighlight.ts` |
| Tools/results | `src\components\messages\AssistantToolUseMessage.tsx`, `src\components\ToolUseLoader.tsx`, `src\components\MessageResponse.tsx`, `src\components\FallbackToolUseErrorMessage.tsx` |
| Bash | `src\tools\BashTool\UI.tsx`, `src\tools\BashTool\BashToolResultMessage.tsx`, `src\components\shell\OutputLine.tsx`, `src\components\shell\ShellProgressMessage.tsx`, `src\utils\terminal.ts`, `src\utils\toolErrors.ts` |
| Diff | `src\tools\FileEditTool\FileEditToolUpdatedMessage.tsx`, `src\components\StructuredDiff\Fallback.tsx`, `src\native-ts\color-diff\index.ts`, `src\utils\diff.ts` |
| Spinner | `src\components\Spinner\SpinnerAnimationRow.tsx`, `src\components\Spinner\SpinnerGlyph.tsx`, `src\components\Spinner\GlimmerMessage.tsx`, `src\components\Spinner\useStalledAnimation.ts`, `src\constants\spinnerVerbs.ts` |
| Input | `src\components\PromptInput\PromptInput.tsx`, `src\components\PromptInput\PromptInputModeIndicator.tsx`, `src\components\PromptInput\PromptInputFooter.tsx`, `src\components\PromptInput\PromptInputFooterLeftSide.tsx`, `src\components\TextInput.tsx`, `src\utils\Cursor.ts` |
| Pickers | `src\components\CustomSelect\select.tsx`, `src\components\CustomSelect\SelectMulti.tsx`, `src\components\design-system\ListItem.tsx`, `src\components\PromptInput\PromptInputFooterSuggestions.tsx` |
| Status | `src\components\Settings\Status.tsx`, `src\commands\status\status.tsx`, `src\commands\cost\cost.ts`, `src\components\TokenWarning.tsx`, `src\services\compact\autoCompact.ts` |
| Wrapping/ANSI | `src\ink\wrap-text.ts`, `src\ink\wrapAnsi.ts`, `src\ink\stringWidth.ts`, `src\ink\output.ts`, `src\components\shell\OutputLine.tsx` |
