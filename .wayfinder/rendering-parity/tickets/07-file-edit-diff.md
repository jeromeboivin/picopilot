---
label: wayfinder:research
name: Specify file edit diff rendering
status: closed
assignee: research-subagent
blocked_by: [05-tool-call-rendering]
---

# Specify file edit diff rendering

## Question

How is a file edit diff rendered, and how much of it should picopilot reproduce?

picopilot has no diff rendering at all today, so this section is written from scratch.

Resolve by producing:

- The container: the border style and its glyphs quoted verbatim, the padding, and the header
  showing the file path.
- Line rendering: the palette keys for added, removed and context lines, the dimmed variants
  and when they apply, the line-number gutter format, and the hunk separator.
- **The word-level diff decision.** The reference highlights changed words inside a changed
  line using more saturated colors. Say what producing this costs in Rust — which diff crate,
  what the algorithm is — and recommend whether to include it or defer it.
- Truncation rules for very large diffs.
- How this interacts with the tool result block from
  [Specify tool call rendering](05-tool-call-rendering.md), since a diff is a tool result.

## Resolution

All `path:line` citations are inside `C:\dev\git\claude-code`. The tree is React-compiler output
with original TSX in trailing sourcemaps; quotes are from the readable compiled JSX or from the
plain `.ts` files, which are not compiled. Nothing was observed rendered — no `bun`, no
`node_modules`, no `claude` binary on this machine — so all column arithmetic is derived from
code, not measured. Unsettled items are marked **UNVERIFIED**.

**picopilot has no diff rendering today. Confirmed:** a case-insensitive regex search for
`diff|patch|old_string|str_replace` across `c:\dev\picopilot\src\**` returns only unrelated
test fixtures whose text happens to contain the word "patch" (`src/tui.rs` lines 3547, 3560,
5090, 5113 and `src/events.rs` lines 416, 424, 633, 641). There is no diff data model, no
gutter, no hunk logic, no `similar`/`diffy`/`imara-diff` in [Cargo.toml](Cargo.toml). This
section is genuinely new code.

### 0. There are two containers, not one

The same hunk renderer is used in two visually different places. Only the first is in this
map's scope (the conversation window); the second is documented because the ticket asks for the
border glyphs and the file-path header, and those exist **only** on the second.

| | transcript result (in scope) | permission dialog (out of scope) |
| --- | --- | --- |
| component | `FileEditToolUpdatedMessage.tsx` | `FileEditToolDiff.tsx` |
| container | the `⎿` result block from ticket 05 | dashed top/bottom border |
| `dim` | `false` | `false` |
| width given to the hunks | `columns - 12` | `columns` (full terminal) |
| header above the hunks | `Added N lines, removed M lines` | dialog title/subtitle |

### 1. The transcript container — it is a tool result, nothing more

`src/tools/FileEditTool/UI.tsx:77-91` — `renderToolResultMessage` returns
`<FileEditToolUpdatedMessage …/>`, and that component's final tree is
(`FileEditToolUpdatedMessage.tsx:103`):

```jsx
<MessageResponse><Box flexDirection="column">{text}{diffList}</Box></MessageResponse>
```

So a diff is **exactly** the result block specified in
[Specify tool call rendering](05-tool-call-rendering.md) §8. Concretely:

- **No border, no box, no padding, no separate header row.** The `⎿` gutter is the entire
  chrome: `'  '` + `⎿` (U+23BF) + `' '` + U+00A0, five cells, dim, first line only; every
  following line sits at column 5 with plain spaces.
- **The file path is not repeated here.** It is already on the tool header line as
  `Update(src/foo.ts)` / `Create(src/foo.ts)` (ticket 05 §6, `FileEditTool/UI.tsx:24-44,57-73`).
  The first line inside the gutter is the change summary, not a path.
- **The summary line**, verbatim from `FileEditToolUpdatedMessage.tsx:36-49`:
  `Added ` + **bold** N + ` line`/` lines`, then `, ` only when both counts are non-zero, then
  `removed `/`Removed ` + **bold** M + ` line`/` lines`. The capital `R` is used when
  `numAdditions === 0` (`:45`). Singular below 2, plural at 2+. No color key — default text.
  Counts are raw `+`/`-` line counts over all hunks (`:31-32`).
- **The hunks are given `width = columns - 12`** (`FileEditToolUpdatedMessage.tsx:88`), even
  though the gutter only consumes 5 and `Message.tsx:408` already hands the subtree
  `columns - 5`. The extra 7 columns of slack are deliberate but the reason is **UNVERIFIED**;
  reproduce the number, do not re-derive it.

Rendered shape:

```
● Update(src/tui.rs)
  ⎿  Added 2 lines, removed 1 line
     123 -    old line
     123 +    new line
     124 +    another new line
```

### 2. The dashed border (permission dialog only)

`FileEditToolDiff.tsx:96` is the only border in the diff code:

```jsx
<Box flexDirection="column">
  <Box borderColor="subtle" borderStyle="dashed" flexDirection="column"
       borderLeft={false} borderRight={false}>{children}</Box>
</Box>
```

`borderStyle="dashed"` is a Claude-Code-local addition to Ink, not a `cli-boxes` style.
Verbatim, `src/ink/render-border.ts:16-27`:

```ts
export const CUSTOM_BORDER_STYLES = {
  dashed: {
    top: '╌',
    left: '╎',
    right: '╎',
    bottom: '╌',
    // there aren't any line-drawing characters for dashes unfortunately
    topLeft: ' ',
    topRight: ' ',
    bottomLeft: ' ',
    bottomRight: ' ',
  },
} as const
```

- Top and bottom rules only: `╌` U+254C BOX DRAWINGS LIGHT DOUBLE DASH HORIZONTAL, repeated
  `contentWidth` times. `╎` (U+254E) is defined but never drawn here because
  `borderLeft={false} borderRight={false}`; with both sides off, `contentWidth === width`
  and the corner glyphs are omitted (`render-border.ts:118-128, 172-176`).
- Border color is the palette key `subtle`, matching
  [Capture the Claude Code dark palette](01-dark-palette.md) which already lists
  `FileEditToolDiff.tsx` as a `subtle` consumer.
- **No horizontal padding.** `FilePermissionDialog.tsx:169` passes `innerPaddingX={0}` so the
  rule spans the dialog's full inner width; sibling blocks in the same dialog use `paddingX={1}`
  (`:172`), the diff deliberately does not.
- The header/path lives on the dialog, not the frame: title `"Edit file"`, subtitle
  `relative(cwd, file_path)`, question `Do you want to make this edit to `**`basename`**`?`
  (`FileEditPermissionRequest.tsx:67-70`).
- While the file is still being read, the frame renders a single dim `…` (U+2026)
  (`FileEditToolDiff.tsx:88`).

**Recommendation for picopilot:** adopt the dashed `╌` rule if and when the permission prompt is
specified; do **not** put it around transcript diffs, because the reference does not.

### 3. Which line renderer actually runs

`StructuredDiff.tsx:110-133` picks one of two renderers:

1. **Primary: `ColorDiff`** (`src/native-ts/color-diff/index.ts`, a TS port of a Rust NAPI
   module) — syntax-highlighted, returns finished ANSI strings. Used unless
   `CLAUDE_CODE_SYNTAX_HIGHLIGHT` is falsy (`StructuredDiff/colorDiff.ts:18-26`) or
   `settings.syntaxHighlightingDisabled` is set.
2. **Fallback: `StructuredDiffFallback`** (`StructuredDiff/Fallback.tsx`) — no syntax
   highlighting, uses the theme palette.

They agree on gutter width and on the 0.4 word-diff threshold but **disagree on colors and on
line numbering**. Pick one and say so; do not blend them.

### 4. Palette — the two renderers use different colors

**Fallback path** uses the six `diff*` palette keys from
[Capture the Claude Code dark palette](01-dark-palette.md) as **background** colors
(`Fallback.tsx:329, 403`):

| role | key | dark value | applies when |
| --- | --- | --- | --- |
| added line bg | `diffAdded` | `rgb(34,92,43)` | `type === 'add'`, `!dim` |
| removed line bg | `diffRemoved` | `rgb(122,41,54)` | `type === 'remove'`, `!dim` |
| added line bg, dimmed | `diffAddedDimmed` | `rgb(71,88,74)` | `type === 'add'`, `dim` |
| removed line bg, dimmed | `diffRemovedDimmed` | `rgb(105,72,77)` | `type === 'remove'`, `dim` |
| added word bg | `diffAddedWord` | `rgb(56,166,96)` | inside an added line (`:275,279`) |
| removed word bg | `diffRemovedWord` | `rgb(179,89,107)` | inside a removed line (`:275,286`) |

Context (`nochange`) lines get **no background** and are rendered `dimColor` on the gutter
(`Fallback.tsx:410`, `dimColor={dim || type === 'nochange'}`). Foreground is left to the
terminal default unless a theme override is passed, in which case it is the key `text` (`:410`).

**Primary path does not use the palette at all.** `buildTheme()` hardcodes its own dark set
(`native-ts/color-diff/index.ts:305-327`), truecolor branch:

| role | value |
| --- | --- |
| `addLine` | `rgb(2,40,0)` |
| `addWord` | `rgb(4,71,0)` |
| `addDecoration` (the `+` and its line number) | `rgb(80,200,80)` |
| `deleteLine` | `rgb(61,1,0)` |
| `deleteWord` | `rgb(92,2,0)` |
| `deleteDecoration` (the `-` and its line number) | `rgb(220,90,90)` |
| foreground | `rgb(248,248,242)` |

Note the asymmetry: in `color256` mode (`COLORTERM` neither `truecolor` nor `24bit`,
`:95-99`) the *add* colors switch to explicit ANSI-256 indices `22`/`28` while the *delete*
colors stay RGB and get quantised downstream. That looks accidental; **UNVERIFIED** whether it
is intentional.

**Recommendation for picopilot:** use the **Fallback palette keys** (`diffAdded`, `diffRemoved`,
`diffAddedWord`, `diffRemovedWord`, plus the two `*Dimmed`). They are already captured in
ticket 01, they are theme-driven rather than hardcoded, and they are far more legible than
`rgb(2,40,0)`. Take the *layout* from the primary path (§5) and the *colors* from the fallback.
This is a deliberate, documented split.

### 5. Line rendering — gutter, sigil, wrapping, padding

Both renderers produce a gutter of **`max_digits + 3` cells**, where
`max_digits = len(str(max(oldStart + oldLines - 1, newStart + newLines - 1)))`
(`native-ts/color-diff/index.ts:832-836`, mirrored at `StructuredDiff.tsx:44-48`,
whose comment states the layout verbatim: *"marker (1) + space + right-aligned line number
(max_digits) + space"*).

Primary path, first line of a logical diff line
(`addLineNumber` `:737-745` then `addMarker` `:747-756`, both `unshift`, so the line-number
block ends up leftmost):

```
' ' + lineNumber.padStart(max_digits) + ' ' + marker + content
```

Continuation (wrapped) lines: `' '.repeat(max_digits + 2)` + marker + content (`:740-741`).
**The sigil repeats on wrapped lines; the line number does not.**

- Sigils are exactly `'+'`, `'-'`, `' '` (`Marker` type, `:168`; `parseMarker` `:838-840`).
  The space sigil is a real cell, so context lines align with changed lines.
- Content width is `effectiveWidth = max(1, width - max_digits - 2 - 1)` (`:872`).
- Changed lines are **padded with spaces to the full content width** so the background bar
  reaches the right edge (`wrapText` `:713-721`). Context lines are not padded.
- The line-number cell is emitted dim for context lines and only for context lines
  (`shouldDim = h.marker === null || h.marker === ' '`, `:736`).

**Line numbering semantics** (primary path, `:877-895`): `+` lines take the *new* counter,
`-` lines take the *old* counter, context lines display the *new* counter and advance both.
So on a replaced line the `-` and the `+` can show the same number — that is correct and
intended, not a bug.

The fallback path lays the gutter out as `lineNumber.padStart(maxWidth) + ' ' + sigil` where
`maxWidth = len(str(maxLineNumber)) + 1` (`Fallback.tsx:367, 399-400`), which is the same
total width with the leading pad space folded into `maxWidth`. Its numbering is different and
worse — `numberDiffLines` (`:422-482`) starts from `patch.oldStart` and rewinds the counter by
the number of consecutive removals. **Ignore the fallback numbering; implement the primary
path's two-counter scheme.**

Gutter selectability: both wrap the gutter cell in `<NoSelect fromLeftEdge>`
(`Fallback.tsx:408-409`, `StructuredDiff.tsx:150`) so a mouse selection yields clean code.
picopilot gets this for free once it moves to terminal scrollback, since it will not be drawing
a selectable canvas — but the columns must still be a distinct region if OSC-8/selection work
is ever done.

### 6. Hunk separator

`StructuredDiffList.tsx:24-28` interposes, between every pair of hunks and nowhere else:

```jsx
<NoSelect fromLeftEdge key={`ellipsis-${i}`}><Text dimColor>...</Text></NoSelect>
```

The separator is **three ASCII full stops `...`**, dim, at the diff's left edge — *not* `…`,
*not* a `@@ -a,b +c,d @@` header. Claude Code never renders a unified-diff hunk header. No
blank line accompanies it.

Context is `CONTEXT_LINES = 3` (`src/utils/diff.ts:9`), so hunks are the standard 3-line-context
kind and the `...` stands in for the elided region.

### 7. The `dim` variant and when it applies

`dim` is a prop threaded from the top: `false` for a successful edit
(`FileEditToolUpdatedMessage.tsx:88`), `true` for a **rejected** edit
(`FileEditToolUseRejectedMessage.tsx:149`). Effects:

- Backgrounds swap to `diffAddedDimmed` / `diffRemovedDimmed` (fallback path).
- **Word-level highlighting is switched off entirely** — primary path guards the whole
  word-diff pass with `if (!dim)` (`native-ts/color-diff/index.ts:896`); fallback bails with
  `changeRatio > CHANGE_THRESHOLD || dim` (`Fallback.tsx:256`). The source comment for the
  primary guard is *"skip when dim — too loud"*.
- Every emitted line is wrapped in SGR 2 (`intoLines(…, dim, …)`, `:150`).

The rejected-edit block also prepends its own header line, in palette key `subtle`
(`FileEditToolUseRejectedMessage.tsx:41-59`): `User rejected update to ` (or `write`) followed
by the **bold** cwd-relative path, all inside the same `⎿` gutter.

### 8. Truncation rules

There is **no line-count cap on the diff itself**. A 400-line edit prints 400 diff rows. What
exists instead:

1. **Context is capped at 3 lines per side** (`CONTEXT_LINES = 3`, `utils/diff.ts:9`), which is
   what keeps normal edits short. Unchanged regions collapse to the `...` separator.
2. **A 5-second diff timeout.** `DIFF_TIMEOUT_MS = 5_000` is passed to `structuredPatch`
   (`utils/diff.ts:10, 100, 160`); on timeout `structuredPatch` returns falsy and the code
   returns `[]` — **an empty hunk list, i.e. the diff silently disappears** and only the
   `Added N lines, removed M lines` summary remains (`:104-106, 164-166`).
3. **Input caps before diffing** (`src/utils/readEditContext.ts:4-5`):
   `CHUNK_SIZE = 8 * 1024`, `MAX_SCAN_BYTES = 10 * 1024 * 1024`. Past 10 MiB the scan gives up
   (`truncated: true`) and `FileEditToolDiff.tsx:139-141` falls back to diffing the tool inputs
   against each other rather than against the file. A single `old_string` of `≥ CHUNK_SIZE`
   skips the file read outright (`:110-112`).
4. **The one real `… +N lines` truncation is on rejected *writes*, not on diffs.**
   `FileEditToolUseRejectedMessage.tsx:11` `MAX_LINES_TO_RENDER = 10`; `:91-92` slices the
   new file content to 10 lines and `:116` emits `<Text dimColor>… +{plusLines} lines</Text>`.
   Note this variant has **no `(ctrl+o to expand)` hint** — unlike the three truncators in
   ticket 05 §9. `plusLines = numLines - 10` and the row is guarded by `plusLines > 0`, so the
   count is always 1 or more, and the word is always plural.
5. **Per-hunk render cache is capped at 4 entries** keyed by
   `theme|width|dim|gutterWidth|firstLine|filePath` (`StructuredDiff.tsx:86-89`). Performance
   only; not visible.

**Recommendation for picopilot:** implement 1 and 2 (a computation budget with a "diff omitted"
degradation), skip 3 (picopilot can just refuse to diff files over a size limit), and reproduce
4 only if a Write-rejection surface is specified. Do **not** invent a `… +N lines` cap on diff
rows — the reference has none, and inventing one would be a visible deviation.

### 9. The word-level diff decision

**Algorithm, exactly as the reference does it** (`native-ts/color-diff/index.ts:546-634`):

1. `findAdjacentPairs(markers)` (`:576-602`) — walk the hunk's marker array; on a run of `k`
   `-` lines immediately followed by a run of `m` `+` lines, pair index `i` of the delete run
   with index `i` of the add run for `min(k, m)` pairs. Unpaired leftovers get no word diff.
2. `tokenize(text)` (`:550-574`) — three token classes, greedy runs:
   - a run of `[\p{L}\p{N}_]` (word characters, Unicode-aware),
   - a run of `\s` (whitespace collapses into one token),
   - otherwise a single codepoint (surrogate-pair aware).
   The comment states this mirrors the npm `diff` package's `diffWordsWithSpace` splitting.
3. Myers diff over the **token slices** (`diffArrays`, `:607`), accumulating byte ranges of
   added tokens on the new side and removed tokens on the old side.
4. **Threshold:** `CHANGE_THRESHOLD = 0.4` (`:546`). With
   `totalLen = len(oldStr) + len(newStr)` and `changedLen` the summed length of all changed
   tokens on both sides, if `changedLen / totalLen > 0.4` the function returns
   `[[], []]` — **no word highlighting at all for that pair**, fall back to a plain full-line
   background (`:632-635`). The fallback renderer computes the identical ratio and refuses the
   same way (`Fallback.tsx:250-257`).
5. Ranges are painted as a *second* background layer inside the line background: `addWord` /
   `deleteWord` inside `addLine` / `deleteLine` (`applyBackground` `:766-800`,
   `wordBackground` `:379-388`).

Note step 5 happens **before** wrapping (`render` `:918-928`: `applyBackground` then
`wrapText`), so a highlighted range survives a line wrap correctly. The fallback renderer wraps
per-part instead and is measurably sloppier; ignore it.

**Rust options compared.** All three were checked against their current docs.rs pages.

| | `similar` 3.2.0 | `imara-diff` 0.2.0 | `diffy` 0.5.2 |
| --- | --- | --- | --- |
| line diff with N context | yes — `TextDiff::from_lines` + `grouped_ops(3)` | yes — `Diff::hunks`, context only via the `unified_diff` printer | yes — `DiffOptions::set_context_len` |
| diff over an arbitrary token slice | **yes** — `capture_diff_slices(Algorithm, &[T], &[T])` | yes, but you must intern tokens yourself (`TokenSource`/`compute_with`) | **no** — text/bytes only |
| built-in intraline refinement | **yes** — `inline` feature: `iter_inline_changes`, `InlineChange`, `InlineChangeMode` | no | no |
| word/char/grapheme tokenizers | yes — `from_words`, `from_chars`, `from_graphemes` (`unicode` feature) | no | no |
| timeout / deadline (needed for §8.2) | **yes** — deadline + timeout on `TextDiffConfig` and `capture_diff_*_deadline` | no | no |
| dependencies | none by default | small | none by default (`no_std` by default) |
| maintenance | 3.2.0 released ~3 weeks ago, 190M downloads all-time, MSRV 1.85, Apache-2.0 | 0.2.0, actively used by gitoxide, claims 10–30× faster than `similar` | 0.5.2, maintained, focused on patch parse/apply/merge |

**Recommendation: include word-level diff now, using `similar` with
`features = ["inline", "unicode"]`.**

Reasons:

- **It is a static computation, not a render tick.** Unlike the 600 ms dot blink from ticket 05,
  nothing here needs a timer. The diff is computed once when the tool result arrives and the
  rows are then committed to scrollback and never repainted. This is exactly the kind of cost
  the map's standing preferences say to pay ("dependencies: whatever it takes"), and it does not
  touch the live-region budget.
- **`similar` is the only one of the three that covers both levels with one dependency.** Use
  `TextDiff::from_lines(...).grouped_ops(3)` for the hunks and `capture_diff_slices` over your
  own token vector for the word pass. The `deadline`/`timeout` support maps 1:1 onto
  `DIFF_TIMEOUT_MS = 5_000`, which neither `imara-diff` nor `diffy` offers and which §8.2 needs.
- **Do not use `iter_inline_changes` for the word pass.** Its tokenization is `similar`'s own,
  not the reference's. To match the reference exactly, port `tokenize()` (§9 step 2) by hand —
  it is ~20 lines — and feed the token vector to `capture_diff_slices(Algorithm::Myers, …)`.
  `similar` is then being used as a diff *engine*, not as a formatter, which is the right seam.
- `diffy` is disqualified: no sequence-level API at all, so word diff is impossible without a
  second crate.
- `imara-diff` is the better *line* engine on paper (10–30× faster) and can be made to do word
  diff via `compute_with`, but it has no deadline support and no inline story, so it would cost
  a hand-rolled budget mechanism. Its speed advantage is irrelevant here: the reference caps the
  input at 10 MiB and picopilot's diffs are hunk-sized.
- The threshold logic is trivial to port and is where the visual quality actually comes from —
  the 0.4 ratio is what stops a rewritten line from turning into confetti. Shipping the word
  diff without it would look *worse* than no word diff.

**Cost estimate:** one dependency, one ~20-line tokenizer, one `capture_diff_slices` call per
paired line, one ratio check, and a two-layer background painter. No new runtime machinery.

**If it is deferred anyway,** the correct fallback is the reference's own `dim` branch: skip the
word pass and paint the whole changed line with `diffAdded` / `diffRemoved`. That is a code path
Claude Code already ships (rejected edits), so a picopilot without word diff is still inside the
reference's behaviour rather than a deviation — same argument as the static dot in ticket 05.

### 10. Open items

- **`columns - 12`** (§1) — the 7 columns of slack beyond the 5-cell gutter are unexplained.
  **UNVERIFIED.**
- **`color256` add/delete asymmetry** (§4) — likely a bug in the reference; irrelevant if
  picopilot uses the palette keys as recommended. **UNVERIFIED.**
- **Whether `⎿` and the diff's own background bar interact visually at column 5** — the gutter
  is a flex sibling with no background, the diff rows carry a full-width background starting at
  column 5, so the bar should start at column 5 and run to `columns - 7`. Not measured.
  **UNVERIFIED.**
- **Syntax highlighting inside diff lines.** The primary path highlights `+` and context lines
  but explicitly does **not** highlight `-` lines (`native-ts/color-diff/index.ts:915-918`:
  `marker === '-' ? [[defaultStyle(theme), code]] : highlightLine(...)`). Whether picopilot
  should reproduce that belongs to
  [Specify markdown and code block rendering](04-markdown-and-code-blocks.md)'s `two-face`/
  `syntect` decision, not here. Flagged, not resolved.
