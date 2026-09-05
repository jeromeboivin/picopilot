---
label: wayfinder:research
name: Specify the text wrapping and background fill mechanism
status: closed
assignee: research-subagent
blocked_by: [03-user-assistant-messages]
---

# Specify the text wrapping and background fill mechanism

## Question

How does picopilot wrap transcript text and fill a line background to the terminal edge, given
that ratatui cannot do either the way the reference needs?

[Specify user and assistant message rendering](03-user-assistant-messages.md) surfaced two
findings that ratatui's `Paragraph` cannot satisfy, and both are cross-cutting rather than
local to one message type:

1. The reference fills the user message's background across the **full terminal width** on
   every wrapped row. ratatui styles only the cells a glyph occupies.
2. picopilot's existing hanging indent silently collapses to column 0 on any soft-wrapped line,
   because the indent is applied before `Wrap` runs. Assistant text is supposed to wrap back to
   column 2.

Every remaining surface ticket inherits whichever answer this ticket gives, so it is worth
settling once rather than per section.

Resolve by producing:

- The chosen approach, weighed against the alternatives: hand-rolled wrapping producing
  width-padded lines, a post-render pass over the ratatui buffer, or dropping the background
  box and accepting a visible difference.
- How the chosen approach behaves with wide characters, combining marks and emoji.
- How it interacts with `insert_before` and the fixed-height live region from
  [Settle the scrollback rendering mechanism](02-scrollback-mechanism.md), including what
  happens on resize once a line is already committed.
- Whether an existing crate covers this, or whether it has to be written in-tree.
- The rule the spec should state so that each surface section can simply refer to it.

## Resolution

**Answer: write one in-tree wrapper that emits `Vec<Line<'static>>` already padded to the fill
width, and stop using `Paragraph::wrap` for transcript content entirely.** It is the only option
that fixes the background *and* the hanging indent *and* gives `insert_before` an exact line
count, and it is what the reference is doing anyway. Details, evidence and the exact rule below.

Sources: reference read from `C:\dev\git\claude-code` (source only, no `node_modules`, no Bun
runtime on this machine — see *Not verified*). ratatui claims are read from the 0.29.0 source on
docs.rs, npm `wrap-ansi` from `chalk/wrap-ansi@main/index.js`. picopilot claims are read from
this tree.

### 1. What the reference actually does

**Wrapping.** Every `<Text>` with the default `wrap: 'wrap'` goes through one call:

```ts
// src/ink/wrap-text.ts:39-49
export default function wrapText(text, maxWidth, wrapType) {
  if (wrapType === 'wrap') {
    return wrapAnsi(text, maxWidth, { trim: false, hard: true })
  }
```

`wrapAnsi` is `Bun.wrapAnsi` when running under Bun, otherwise the npm `wrap-ansi` package
(`src/ink/wrapAnsi.ts:9-18`). The npm implementation's algorithm, which is the one we can read:

1. `.normalize()` (NFC), `\r\n` → `\n`, split on `\n`, wrap each line **independently**, rejoin.
   Hard newlines are never merged.
2. `expandTabs(line)` first. `TAB_SIZE = 8`; a tab expands to the next multiple-of-8 tab stop,
   measured in visible columns.
3. Words are produced by splitting on the **ASCII space `U+0020` only** (`splitWords`). Not tabs
   (already expanded), not NBSP, not `U+2003`/`U+3000`, not hyphens. So `"foo\u{a0}bar"` and
   `"foo-bar"` are single unbreakable words.
4. Greedy first-fit. There is no optimal-fit / minimum-raggedness pass.
5. `hard: true`: a word wider than `columns` is broken by `wrapWord`, which walks **grapheme
   clusters** and starts a new row when `visible + token.width > columns`. Zero-width tokens
   (escape sequences, combining marks) always stay on the current row. Nothing is ever dropped.
6. `trim: false` is the load-bearing option. It means (a) rows are never left-trimmed, so leading
   indentation survives, and (b) the inter-word space is appended **even when the row is empty**
   (`if (rowLength > 0 || options.trim === false)`), and (c) the final
   `stringVisibleTrimSpacesRight` pass is skipped, so trailing spaces survive.
7. `restoreStylesAcrossRows` closes every open SGR style and OSC 8 hyperlink before each row
   break and reopens it after, so each row is self-contained.

**Width.** `stringWidth` is `Bun.stringWidth(s, { ambiguousIsNarrow: true })` under Bun, else a
hand-written fallback using `get-east-asian-width` with `ambiguousAsWide: false`, `emoji-regex`,
and explicit zero-width tables (`src/ink/stringWidth.ts`). **Ambiguous-width characters are
narrow.** The file's own closing comment says the Bun value is the correct one for terminal cell
allocation and that the JS fallback's grapheme-cluster width is the buggy path.

**Background.** Settled in [Specify user and assistant message rendering](03-user-assistant-messages.md)
§"Background box": Ink pre-fills the whole box rectangle with background-styled spaces and then
draws children on top (`render-node-to-output.ts:1195`), so every row a user message occupies is
background-colored from column 0 to `columns - 1`, including soft-wrapped rows and the short tail
of the last row. Wrap width is `columns - 1`; **fill width is `columns`**. Those two numbers are
deliberately different.

### 2. Why `Paragraph::wrap` cannot be made to do this

**`WordWrapper` is not reachable.** It lives in `ratatui::widgets::reflow`, which is a private
module — `ratatui::widgets` re-exports `block`, `calendar` and `canvas` only. There is no
supported way to call ratatui's wrapper on our own text, or to subclass it.

**And it would be wrong if it were reachable.** Four divergences, all provable from ratatui's own
tests in `widgets/reflow.rs`:

| | ratatui `WordWrapper` | reference `wrap-ansi` |
| --- | --- | --- |
| leading whitespace | `trim: true` deletes it on every row (`line_composer_leading_whitespace_removal`) | preserved (`trim: false`) |
| indented text | `trim: false` emits whitespace-only rows: `"               4 Indent"` at width 10 → `["          ", "    4", "Indent"]` (`line_composer_word_wrapper_preserve_indentation_lots_of_whitespace`) | never emits a row that is only the indent |
| cluster wider than the limit | **silently dropped** — `if symbol_width > self.max_line_width { continue }` (`reflow.rs:87-89`). At width 1 an entire CJK line vanishes: `line_composer_max_line_width_of_1_double_width_characters` asserts `["", "a", "a", "a", "a"]` | `wrapWord` emits it on its own row; nothing is lost |
| word boundary | any whitespace grapheme (NBSP excepted, per `line_composer_word_wrapper_nbsp`) — so `U+2003`, `U+3000`, tabs all break | `U+0020` only |

Tabs are a fifth divergence and the worst-behaved. `UnicodeWidthStr::width("\t")` is 0 (control
character), and `Buffer::set_stringn` documents that it *"skips zero-width graphemes and control
characters"* — so under `Paragraph` a tab contributes nothing and then disappears. The reference
expands tabs to 8-column stops before wrapping. picopilot's own input editor already disagrees
with both: `input_character_width` in [src/tui.rs](src/tui.rs#L2286) hardcodes `'\t' => 4` with no
tab-stop arithmetic.

**No per-line background fill.** `Line::style` colors the cells its glyphs occupy and stops.
`Paragraph` writes per grapheme. The only ratatui primitive that colors empty cells is
`Buffer::set_style(rect, style)`, which needs `&mut Buffer` — i.e. a custom widget or a post-pass.

**Height precompute is a trap.** `Paragraph::line_count(width)` exists and picopilot already uses
it at [src/tui.rs](src/tui.rs#L2912) with the `unstable-rendered-line-info` feature enabled in
[Cargo.toml](Cargo.toml#L15). It is explicitly unstable (*"no stability guarantees, could be
changed or removed at any time"*, tracking issue ratatui#293) and it re-runs the same divergent
wrapper. Once we own the wrapping, the count is just `lines.len()` and the feature flag can be
dropped.

### 3. The three candidate approaches, weighed

**(a) Hand-rolled wrapper producing width-padded lines — chosen.**

Produce `Vec<Line<'static>>` where every `Line` is already exactly `fill_width` cells wide,
padded with a real `Span::styled(" ".repeat(n), bg_style)`. Render with plain
`Paragraph::new(lines)` — no `.wrap(...)`, no `Block::padding`.

- Background: solved, and solved uniformly for the live region and for `insert_before`, because
  it is carried in the `Line` itself rather than in a render-time side effect.
- Hanging indent: solved, because the indent prefix is applied **after** the break decision, per
  produced row, instead of before (which is exactly why today's indent collapses — see
  [03](03-user-assistant-messages.md) §"The 2-column indent must survive soft wrapping").
- `insert_before` height: solved exactly, no unstable feature.
- Cost: ~150 lines in-tree plus one new declared dependency (`unicode-segmentation`, already in
  the tree transitively via ratatui).

**(b) Post-render pass over the ratatui buffer — rejected.**

`insert_before`'s `draw_fn` does receive `&mut Buffer`, so `set_style` over each row rect is
mechanically possible. But it only paints. It leaves ratatui's wrapper in the loop, so all four
divergences in §2 remain, the hanging indent stays broken, and you *still* need the wrapped line
count up front — which sends you back to the unstable `line_count`. It solves the smaller half of
the ticket and none of the larger half.

**(c) Drop the background — rejected, with a note.**

Cheapest, and it is the only option that keeps `Paragraph::wrap`. But the user-message background
is a distinctive part of the reference's look, and dropping it does not fix the hanging indent,
which is a real defect rather than a stylistic difference. If schedule pressure ever forces this,
drop the background and *still* hand-roll the wrapping.

### 4. Wide characters, combining marks and emoji

The wrapper must measure **grapheme clusters, not chars**, and measure each cluster with
`UnicodeWidthStr::width` on the whole cluster string.

- Per-char summation is wrong. unicode-width 0.2 documents that emoji ZWJ sequences, emoji
  modifier sequences and emoji presentation sequences have width 2 *as strings*, and that
  canonically equivalent strings get the same width. `"👩\u{200d}💻"` summed per char is 4; as a
  string it is 2, which is what the terminal allocates.
- Combining marks and default-ignorables are width 0 and must stay attached to their base
  cluster, never starting a new row on their own. This falls out of clustering.
- **Ambiguous width must stay narrow.** Use `UnicodeWidthStr::width`, never `width_cjk`. This
  matches the reference's `ambiguousAsWide: false` / `ambiguousIsNarrow: true`.
- A cluster that does not fit in the remaining columns moves to the next row whole. A cluster
  wider than the entire wrap width is emitted on its own row and allowed to overflow by one
  column — it is **never dropped** (this is the one place we deliberately behave unlike ratatui).
- When padding to `fill_width`, pad by `fill_width - measured_width`, not by character count. A
  row ending in a width-2 cluster at `fill_width - 1` gets one pad cell, not zero.

Residual known mismatch, accepted: the reference's *JS fallback* measures a Devanagari conjunct
as width 1 (it takes the first non-zero-width char), while `Bun.stringWidth` and `unicode-width`
both say 2. The reference's own source comment says the Bun answer is correct and the fallback
desyncs Ink from the terminal. Matching `unicode-width` therefore matches real Claude Code.

### 5. Interaction with `insert_before` and resize

[Settle the scrollback rendering mechanism](02-scrollback-mechanism.md) §5 settled that history is
rendered to `Vec<Line>` against the current width and handed to
`insert_before(lines.len() as u16, …)`, and that committed rows are never re-wrapped by us.

- **Height.** `insert_before` takes the height as a `u16` *before* `draw_fn` runs, and overshoot
  leaves unreclaimable blank rows while undershoot silently truncates. The hand-rolled wrapper
  makes `lines.len()` the exact, cheap, stable answer. This is the single strongest argument for
  approach (a).
- **Inside `draw_fn`.** Render the already-padded lines with `Paragraph::new(lines)` and nothing
  else. Do not attach a `Block` with padding — the background must reach column 0, so
  [03](03-user-assistant-messages.md) §"Horizontal padding" already requires
  `Padding::horizontal(2)` to go. Any indent is now part of the line content.
- **Fill width vs wrap width.** Wrap at `fill_width - 1` for user messages (reference:
  `columns - 1`, from `paddingRight={1}`), pad to `fill_width`. Keeping these as two separate
  numbers in the wrapper API avoids re-deriving the off-by-one at every call site.
- **Resize after commit.** The wrap width is frozen at commit time. A later narrow resize makes
  the terminal re-wrap our padded rows, and a padded 100-column colored row re-wrapped at 80
  becomes an 80-column colored row plus a 20-column colored row — visibly worse than plain text
  would degrade. This is unavoidable: [02](02-scrollback-mechanism.md) §3 records that the
  reference does not reflow committed scrollback either (it full-resets the live region on
  `FlickerReason: 'resize'` and leaves history to the terminal). Two mitigations, both cheap:
  never pad beyond the width in force at commit time, and keep the background confined to user
  messages, which are short.
- **Live region.** Same wrapper, re-run every frame against the current `area.width`. Resize is
  then just a re-wrap, with no special case.

### 6. Crate survey — write it in-tree

- **`textwrap`** is the closest fit and still wrong. It has genuine hanging indent
  (`Options::subsequent_indent`) and correct unicode-width measurement, but it operates on
  `&str`: it cannot carry ratatui `Span` styles across a break, so styles would have to be
  re-attached by byte offset afterwards. Its default `WordSeparator` is UAX #14 line breaking,
  which breaks after hyphens and between CJK characters — `wrap-ansi` does neither. Making it
  match would mean disabling `unicode-linebreak` *and* post-processing, at which point the
  in-tree version is smaller.
- **`ratatui::widgets::reflow::WordWrapper`** — private module, unreachable (§2).
- Nothing on crates.io wraps a `Vec<Span>` to a width while preserving per-span style and padding
  the result. This is a ratatui-shaped gap.

So: in-tree, built on `unicode-segmentation` (declare it; ratatui already depends on it) plus the
existing `unicode-width`. Suggested shape:

```rust
pub struct WrapSpec {
    pub wrap_width: usize,     // columns available to text
    pub fill_width: usize,     // columns to pad each row to; >= wrap_width
    pub first_prefix: Vec<Span<'static>>,   // e.g. "❯ " or "● "
    pub rest_prefix: Vec<Span<'static>>,    // e.g. "  "  -> the hanging indent
    pub fill_style: Option<Style>,          // Some(bg) => pad rows to fill_width
}

pub fn wrap_spans(spans: &[Span<'static>], spec: &WrapSpec) -> Vec<Line<'static>>;
```

The prefixes are applied per produced row *after* the break decision, and `wrap_width` is reduced
by the prefix width for the rows the prefix applies to. That is the whole fix for the hanging
indent.

### 7. The rule for the spec

> **Transcript wrapping rule.** All transcript content is wrapped by picopilot's own wrapper
> before it reaches ratatui. `Paragraph::wrap` is not used for transcript content, and no
> transcript `Block` carries horizontal padding.
>
> The wrapper is greedy first-fit over grapheme clusters. Words are separated by `U+0020` only.
> Tabs are expanded to 8-column tab stops before wrapping. Hard newlines wrap independently.
> Leading and trailing whitespace on a row is preserved. A word wider than the wrap width is
> broken at cluster boundaries at exactly the wrap width; a single cluster wider than the wrap
> width is emitted alone and allowed to overflow, never dropped.
>
> Width is `unicode_width::UnicodeWidthStr::width` applied per grapheme cluster. Never
> `width_cjk` — ambiguous-width characters are narrow.
>
> A surface declares four things: **wrap width**, **fill width**, **first-row prefix** and
> **continuation prefix**. Prefixes are applied per produced row after the break decision, so a
> continuation prefix is a true hanging indent. When a surface declares a background, every
> produced row is padded with background-styled spaces out to the fill width — including
> soft-wrapped rows and the short last row. Fill width and wrap width are independent; user
> messages use `fill = columns`, `wrap = columns - 1`.
>
> Committed rows are padded to the width in force at commit time and are never re-wrapped.

Per-surface values known so far, from [03](03-user-assistant-messages.md):

| surface | wrap width | fill width | first prefix | continuation prefix | fill style |
| --- | --- | --- | --- | --- | --- |
| user message | `columns - 1` | `columns` | `"❯ "` | `""` (column 0) | `userMessageBackground` |
| assistant message | `columns - 2` | `columns - 2` | `"● "` | `"  "` (column 2) | none (`messageActionsBackground` when selected) |

### Not verified

- **`Bun.wrapAnsi` / `Bun.stringWidth` were not read or executed.** There is no Bun runtime and no
  `node_modules` on this machine. Real Claude Code runs the Bun paths; the algorithm above is the
  npm `wrap-ansi` fallback that the reference explicitly falls back to. They are documented as
  interchangeable and the reference's own comments treat Bun's width as the reference answer, but
  any residual divergence between the two implementations is unmeasured.
- **No reference build exists to diff against**, so none of the wrapping claims were confirmed
  against rendered output — this is the same limitation recorded in
  [Assemble the rendering spec](13-assemble-spec.md).
- **The "other Unicode spaces" divergence in §2 is reasoned, not executed.** It follows from Rust's
  `char::is_whitespace` being true for `U+2003`/`U+3000` and from `wrap-ansi` splitting on
  `U+0020` only; ratatui has no test covering it.
- **Resize behaviour of a padded, committed row was not observed on a real terminal.** The
  degradation described in §5 is derived from how terminals reflow scrollback, and inherits
  [02](02-scrollback-mechanism.md)'s note that `insert_before` on Windows Terminal was never
  executed.
