---
label: wayfinder:research
name: Specify markdown and code block rendering
status: closed
assignee: research-subagent
blocked_by: []
---

# Specify markdown and code block rendering

## Question

How is markdown styled inside an assistant message, and how should picopilot render
syntax-highlighted fenced code blocks?

Resolve by producing:

- A rule per markdown element as rendered by the reference: headings by level, bold, italic,
  strikethrough, inline code, links, blockquotes, ordered and unordered lists with their
  per-depth indent, task lists, tables, horizontal rules. Give the glyph and the palette key
  for each, and quote the blockquote bar glyph verbatim.
- The fenced code block treatment: indent, any border or background, the language label if
  there is one, and how an unknown or missing language is handled.
- **The syntax highlighting decision.** The reference highlights code; picopilot does not.
  Compare the realistic Rust options — `syntect`, `two-face`, `inkjet`/tree-sitter — on binary
  size, build time, grammar coverage and how closely their themes can be matched to the
  reference's. Recommend one and say why. Dependencies are acceptable, so the bar is
  "justified", not "avoided".
- How the reference's highlight colors map onto the chosen crate's theme format.
- A statement of what picopilot's current `pulldown_cmark` renderer already covers and what it
  is missing, so the gap is explicit.

## Resolution

### Sources

All reference claims are read from source only — there is no `node_modules` and no `claude`
binary on this machine (`Test-Path C:\dev\git\claude-code\node_modules` → `False`), so nothing
below was compared against rendered output. Claims that depend on a third-party package's
runtime behaviour are marked **[unverified]** and were read from the upstream repository on the
web instead.

- `C:\dev\git\claude-code\src\utils\markdown.ts` — the whole token→ANSI formatter.
- `C:\dev\git\claude-code\src\components\Markdown.tsx` — the React wrapper.
- `C:\dev\git\claude-code\src\components\MarkdownTable.tsx` — the box-drawing table renderer.
- `C:\dev\git\claude-code\src\constants\figures.ts` — glyph constants.
- `C:\dev\git\claude-code\src\utils\cliHighlight.ts` — the highlighter loader.
- `C:\dev\git\claude-code\src\utils\hyperlink.ts` — OSC 8 links.
- `C:\dev\git\claude-code\src\utils\theme.ts` — palette values.
- `C:\dev\git\claude-code\src\components\design-system\color.ts` — `color(key, theme)` resolver.

### Architecture in one paragraph

`Markdown.tsx` lexes with **`marked`** (GFM), then splits the token stream: every `table` token
goes to `<MarkdownTable>` (React + Ink box layout, box-drawing glyphs); every other token is
concatenated by `formatToken()` in `markdown.ts` into one ANSI string rendered by `<Ansi>`.
Each contiguous run becomes one element inside `<Box flexDirection="column" gap={1}>`, i.e. a
blank line separates a table from the prose around it. `applyMarkdown()` (the non-React entry
point, used by permission previews) uses `formatToken`'s own ASCII `|`-pipe table branch
instead — so **the reference has two different table renderers** and the assistant-message path
is the box-drawing one.

Two global behaviours:

- **Strikethrough is deliberately disabled.** `configureMarked()` overrides marked's `del`
  tokenizer to return `undefined`, with the comment "the model often uses `~` for
  'approximate' (e.g. ~100)". `~~text~~` therefore renders as literal text including the
  tildes, and the `del` case in `formatToken` returns `''`.
- **Highlighting is lazy and optional.** `Markdown` renders `<MarkdownBody highlight={null}>`
  as a `Suspense` fallback while `cli-highlight` loads, and permanently when the
  `syntaxHighlightingDisabled` setting is on. With `highlight === null`, a fenced block is
  emitted as raw unstyled text.

### (a) Element table — reference rules

Glyphs are quoted verbatim. "Palette key" is the `Theme` key passed to
`color(key, theme)`; dark-theme values are given where the key is used. `chalk.*` means an
attribute-only ANSI style with no palette involvement.

| Element | Glyph / prefix (verbatim) | Style | Palette key | Trailing blank lines | Source |
| --- | --- | --- | --- | --- | --- |
| H1 | none — no `#`, no underline rule | `chalk.bold.italic.underline` | none (inherits body fg) | `\n\n` | `markdown.ts` `case 'heading'` depth 1 |
| H2 | none | `chalk.bold` | none | `\n\n` | same, depth 2 |
| H3+ | none | `chalk.bold` (identical to H2 — depths 3–6 are not distinguished) | none | `\n\n` | same, `default` branch |
| Bold | none | `chalk.bold` | none | — | `case 'strong'` |
| Italic | none | `chalk.italic` | none | — | `case 'em'` |
| Strikethrough | n/a | **not parsed** — renders as literal `~~text~~` | none | — | `configureMarked()`, `case 'del'` |
| Inline code | none — backticks stripped, no background | plain fg color | **`permission`** = `rgb(177,185,249)` (dark) | — | `case 'codespan'`; `theme.ts` `darkTheme.permission` |
| Link (text ≠ href) | none | OSC 8 hyperlink around the text, text painted `chalk.blue` (basic ANSI blue, *not* a palette key — comment says RGB is not preserved by `wrap-ansi` across OSC 8) | none | — | `case 'link'`; `hyperlink.ts` `createHyperlink` |
| Link (text = href, or empty text) | none | the bare URL, same OSC 8 + `chalk.blue` treatment | none | — | `case 'link'` |
| Link, `mailto:` | none | scheme stripped, plain text email, **not** clickable | none | — | `case 'link'` |
| Link, no OSC 8 support | none | plain URL text, uncolored (`createHyperlink` returns `url` before applying chalk) | none | — | `hyperlink.ts` |
| Bare `owner/repo#123` | none | auto-linkified to `https://github.com/owner/repo/issues/123`, display text `owner/repo#123` | none | — | `ISSUE_REF_PATTERN`, `linkifyIssueReferences` |
| Image | none | rendered as the bare `href` string, unstyled | none | — | `case 'image'` |
| Blockquote | `"▎ "` — `BLOCKQUOTE_BAR` = `'\u258e'` (left one-quarter block) **plus one space**, prefixed to *every* line | bar is `chalk.dim`; the line text is `chalk.italic` at normal brightness (comment: "chalk.dim is nearly invisible on dark themes") | none | none of its own | `figures.ts` `BLOCKQUOTE_BAR`; `case 'blockquote'` |
| Blockquote, blank inner line | line emitted unchanged, **no bar** | — | none | — | `stripAnsi(line).trim()` guard |
| Unordered list item | `"- "` (hyphen + space) | inherits | none | `\n` per item | `case 'text'` with `parent.type === 'list_item'` |
| Ordered list item | `"N. "` where N depends on depth (below) | inherits | none | `\n` per item | same + `getListNumber` |
| List indent | `"  "` (two spaces) × depth, depth 0 at top level | — | — | — | `case 'list_item'`: `'  '.repeat(listDepth)` |
| Ordered numbering by depth | depth 0–1 → arabic `1.`; depth 2 → letters `a.` `b.`; depth 3 → lowercase roman `i.` `ii.`; depth ≥4 → arabic | — | — | — | `getListNumber`, `numberToLetter`, `numberToRoman` |
| Ordered list start | honours `token.start` (`token.start + index`) | — | — | — | `case 'list'` |
| Task list `- [ ]` / `- [x]` | **no checkbox glyph is emitted** — `formatToken` has no `task`/`checked` branch, so a task item renders as a plain `- ` bullet and the checkbox state is lost | — | — | — | absence of any task handling in `markdown.ts` |
| Horizontal rule | `"---"` — literal three hyphens, **no trailing newline**, not width-filled | unstyled | none | none | `case 'hr'` |
| Paragraph | none | inherits; `dimColor` prop on `<Markdown>` dims the whole block | none | `\n` | `case 'paragraph'`; `Markdown.tsx` `<Ansi dimColor>` |
| Soft break inside paragraph | `\n` | — | — | — | `case 'br'`, `case 'space'` |
| HTML, link defs | dropped entirely (empty string) | — | — | — | `case 'html'`, `case 'def'` |

**Task-list caveat.** That a task item loses its checkbox is inferred from the *absence* of code,
not from observed output — marked does expose `task`/`checked` on the `list_item` token, so this
is the strongest available reading but is **[unverified]** against a running binary.

#### Tables — assistant-message path (`MarkdownTable.tsx`)

Full box drawing, rendered as one `<Ansi>` block so Ink cannot wrap mid-row.

- Border glyphs, verbatim: top `'┌'` `'─'` `'┬'` `'┐'`, middle `'├'` `'─'` `'┼'` `'┤'`,
  bottom `'└'` `'─'` `'┴'` `'┘'`, vertical `'│'`. A middle rule is drawn after the header **and
  between every pair of data rows**.
- Cell padding is one space each side: `'│' + ' ' + padded + ' │'`.
- Header cells are always **center**-aligned; data cells use the markdown column alignment,
  default left. Header cells are **not** bolded in this path.
- Column widths: ideal (unwrapped) widths if they fit, else min (longest-word) widths plus a
  proportional share of the slack, else hard word-breaking. `MIN_COLUMN_WIDTH = 3`.
  Overhead budget is `1 + numCols * 3` plus `SAFETY_MARGIN = 4`.
- Multi-line cells are vertically centered: `Math.floor((maxLines - lines.length) / 2)`.
- **Vertical fallback.** If any row would exceed `MAX_ROW_LINES = 4` lines, or the assembled
  table is wider than `terminalWidth - 4`, the whole table is re-rendered as key/value pairs:
  `\x1b[1m{Header}:\x1b[22m {value}` per cell, continuation lines indented `'  '`, and rows
  separated by a `'─'` rule of `Math.min(terminalWidth - 1, 40)`.
- No palette color anywhere in the table — borders are unstyled.

#### Tables — `applyMarkdown` path (`markdown.ts` `case 'table'`)

ASCII only, used by permission previews, not by assistant messages. Header row
`'| ' + cells.join(' | ')`, separator `'|' + '-'.repeat(width + 2) + '|'` per column,
alignment colons deliberately not shown, minimum column width 3, trailing `\n\n`.

#### Fenced code blocks

Read from `markdown.ts` `case 'code'` and `cliHighlight.ts`:

- **Highlighting library: `cli-highlight`, which wraps `highlight.js`.** Loaded lazily via a
  single shared `import('cli-highlight')` promise; `highlight.js` is imported after it as a
  module-cache hit.
- **No indent.** The highlighted text is returned as-is plus one `\n`. No leading spaces.
- **No border, no background, no gutter, and no language label.** The info string is used only
  to pick a grammar; it is never displayed.
- **Unknown or unsupported language** → `supportsLanguage(lang)` is false → falls back to
  `language: 'plaintext'` and writes a debug log line ("Language not supported while
  highlighting code, falling back to plaintext: …"). **Missing** info string → `'plaintext'`
  directly.
- **Highlighter unavailable** (`highlight === null`: setting disabled, still loading, or the
  dynamic import threw) → `token.text + '\n'`, completely unstyled.
- The block inherits `<Box gap={1}>` separation from neighbouring elements, but has no
  blank-line rule of its own beyond the single trailing `\n`.

#### The reference's highlight colors

`markdown.ts` calls `highlight.highlight(text, { language })` with **no `theme` option**, so
`cli-highlight`'s `DEFAULT_THEME` applies. From
`https://raw.githubusercontent.com/felixfbecker/cli-highlight/master/src/theme.ts`
**[unverified against the installed version — read from `master`, and the pinned version in
this repo could not be checked because `node_modules` is absent]**:

| highlight.js class | chalk style |
| --- | --- |
| `keyword`, `literal`, `class`, `name` | `chalk.blue` |
| `built_in`, `attr` | `chalk.cyan` |
| `type` | `chalk.cyan.dim` |
| `number`, `comment`, `doctag`, `addition` | `chalk.green` |
| `string`, `regexp`, `deletion` | `chalk.red` |
| `function` | `chalk.yellow` |
| `meta`, `tag` | `chalk.grey` |
| `emphasis` / `strong` / `link` | `chalk.italic` / `chalk.bold` / `chalk.underline` |
| `title`, `params`, `subst`, `symbol`, `variable`, `attribute`, all selectors, `default` | `plain` (no styling) |

**This matters for the port.** These are chalk's *basic ANSI-16* colors, not RGB. Their actual
appearance is the user's terminal palette. Nothing in the code-block path touches the Claude
Code theme — the only palette key markdown uses at all is `permission` for inline code.

### (b) Rust syntax-highlighting decision

#### Comparison

Sizes and counts are from the crates' own pages on lib.rs (retrieved 2026-09-05). No build was
run, so **every build-time and binary-size figure below is the crate's own published claim, not
a local measurement — [unverified]**.

| | `syntect` (defaults only) | `two-face` (+ `syntect`) | `inkjet` (tree-sitter) |
| --- | --- | --- | --- |
| Crate size / SLoC | 1 MB, 8K SLoC | 3.5 MB, 415 lines of its own (rest is data) | **515 MB, ~16M SLoC** (bundled C parser sources) |
| Binary impact | one compressed dump of the default Sublime syntax set, linked in; exact size not published | README: "~0.6 MiB increase" for `syntax::extra_newlines()` over syntect's defaults; `theme::extra()` ≈ 61 KiB. Linker discards unused assets. | author's FAQ: "consider disabling languages you don't need… LTO can shave off a few megabytes" |
| Build time | normal Rust. Default `regex-onig` builds the **Oniguruma C library** ("many users experience difficulty building the `onig` crate, especially on Windows"). `default-fancy` is pure Rust, no C toolchain. | same as syntect + a few hundred KB of dump decoding | author's FAQ: **"Why is Inkjet taking so long to build?" — "it has to compile and link in dozens of C/C++ programs"**. Cached after the first build. |
| Grammars: embedded or runtime | **embedded** compressed dump; ~23 ms to load and link all definitions from the internal dump | **embedded** dumps (bat's `syntaxes.bin` / `themes.bin`, versioned) | **embedded** — "language grammars are linked into the executable as C functions - no need to load anything at runtime" |
| Grammar coverage | Sublime default package set. **Missing TOML, TypeScript, Dockerfile** and others (two-face exists precisely to fix this) | superset: adds TOML, TypeScript, TypescriptReact, Dockerfile, Nix, Zig, Svelte, Vue, Terraform, Kotlin, Swift, Elixir, Fish, JQ, WGSL, Typst… curated by the bat project | "over seventy languages" + `Language::Runtime` escape hatch |
| Themes | tmTheme; bundled defaults incl. `base16-ocean.dark`, `base16-eighties.dark`, Solarized, InspiredGitHub | tmTheme; adds Nord, Dracula, Catppuccin ×4, gruvbox, OneHalf, Monokai Extended, TwoDark, Zenburn, `Ansi` | **Helix editor themes** (TOML), vendored collection |
| ratatui integration | `syntect-tui` 3.0.6 (May 2025) — `into_span(segment) -> ratatui::Span`, 156 lines, MIT | same (works through syntect types) | none. Ships an HTML formatter and an optional `termcolor` formatter; ratatui glue must be hand-written |
| Maintenance signal | 3.6M downloads/month, 5.3.0 Sep 2025 | 1.8M downloads/month, 0.5.2 Aug 2026 | 28K downloads/month, **last release 0.11.1 Sep 2024**, used by 9 crates |
| Concurrency | `SyntaxSet` is `Send + Sync + Clone` | same | needs a `&mut` highlighter; one per thread |

Caveats worth recording:

- `syntect`'s `fancy-regex` engine "is about half the speed" of Oniguruma and is "absurdly slow
  in debug mode" — relevant because picopilot developers run debug builds.
- `two-face`'s `fancy` feature **excludes** a handful of grammars that need Oniguruma-only regex
  features (ARM Assembly, PowerShell, JavaScript (Babel), Salt State SLS). Losing PowerShell is
  a real cost for a Windows-first agent.
- `syntect-tui` declares `ratatui` as a dependency that lib.rs flags "outdated"; picopilot is on
  ratatui 0.29. **Compatibility is [unverified]** — check before adopting, and note the glue is
  ~150 lines that could simply be written in-tree.

#### Recommendation: `two-face` on top of `syntect`, own span conversion

Take `two-face` with `syntect` underneath, `two_face::syntax::extra_newlines()` as the syntax
set, and convert `syntect` styled ranges to `ratatui::text::Span` in-tree (roughly what
`syntect-tui::into_span` does) rather than depending on `syntect-tui`.

Why:

1. **Grammar coverage is the deciding factor.** picopilot is a coding agent; the languages it
   will print most are TypeScript, TOML, Dockerfile, YAML, Rust, Python, shell. Plain `syntect`
   is missing three of those. `two-face` covers them for a published ~0.6 MiB.
2. **`inkjet` is not justified.** A 515 MB crate that compiles dozens of C/C++ parsers on first
   build, with no ratatui integration, no release since September 2024, and a `&mut`
   highlighter — for a *superset of nothing we need*. Its Helix TOML themes are also further
   from what we must reproduce than tmTheme is (see below).
3. **`syntect` alone is the same work with fewer grammars.** `two-face` is a data package over
   the same API; dropping back to plain `syntect` later is a one-line change.
4. Everything stays **embedded** — no theme or grammar files to ship or find at runtime, which
   matters for a single-binary CLI installed by `install.ps1` / `install.sh`.
5. Regex engine: prefer `syntect-onig` (the default) if the Oniguruma C build is acceptable on
   the developer machines, because `fancy` costs both speed and four grammars including
   PowerShell. **This one choice should be settled by actually attempting a build** — it is the
   only claim here that a five-minute experiment can turn from [unverified] to fact.

#### Mapping the reference's highlight colors onto tmTheme

The reference paints **highlight.js token classes with chalk's ANSI-16 colors**. `syntect`
paints **TextMate/Sublime scopes with RGBA**. The bridge has two halves and neither is free:

1. **Scope → token-class.** Write a small `claude-parity.tmTheme` (plist XML), embedded with
   `include_str!` and loaded via `syntect::highlighting::ThemeSet::load_from_reader`
   (`plist-load` feature, on by default). Proposed scope selectors, derived by matching each
   highlight.js class description to its nearest TextMate scope — **this correspondence is a
   design decision, not something the reference states, so it is [unverified] as "identical"**:

   | tmTheme scope selector | reference class | target color |
   | --- | --- | --- |
   | `keyword`, `storage`, `constant.language`, `entity.name.class`, `entity.name.tag` (s-expr `name`) | `keyword`, `literal`, `class`, `name` | blue |
   | `support.function`, `support.class`, `support.constant`, `entity.other.attribute-name` | `built_in`, `attr` | cyan |
   | `support.type`, `entity.name.type` | `type` | cyan + dim |
   | `constant.numeric` | `number` | green |
   | `comment` | `comment`, `doctag` | green |
   | `string`, `constant.character`, `string.regexp` | `string`, `regexp` | red |
   | `entity.name.function` | `function` | yellow |
   | `meta.preprocessor`, `punctuation.definition.tag` | `meta`, `tag` | grey |
   | `markup.inserted` / `markup.deleted` | `addition` / `deletion` | green / red |
   | everything else | `default`, `plain`, `title`, `params`, `variable` | inherit body fg |

2. **RGB → ANSI-16.** This is the trap. `syntect::highlighting::Color` is RGBA `u8`, so a
   tmTheme cannot express "chalk blue" — it can only express one concrete RGB, which will
   *not* track the user's terminal palette the way the reference does. Two options, and the
   spec must pick one:
   - **Sentinel colors (recommended for parity).** Give each of the eight roles a distinct
     unmistakable RGB in the tmTheme, then in the span conversion map those exact values back to
     `ratatui::style::Color::Blue` / `Cyan` / `Green` / `Red` / `Yellow` / `DarkGray` and to
     `Modifier::DIM` for `type`. Output is then ANSI-16, i.e. genuinely identical to chalk.
     This is why the conversion should be written in-tree: `syntect-tui::into_span` passes RGB
     straight through and gives you no place to intercept.
   - **Concrete RGB.** Pick fixed dark-theme-friendly RGB values. Simpler, but knowingly
     different from the reference on any terminal with a customised palette.

   The `Ansi` theme shipped by `two-face` is worth inspecting first — its name suggests it
   already targets the 16-color space, which could remove the need for a hand-written tmTheme
   entirely. **[unverified — not inspected.]**

### What picopilot already has, and what is missing

Current state, from `src/tui.rs` (`markdown_lines`, `MarkdownRenderer`, `render_table`) and
`Cargo.toml`:

Already covered:

- `pulldown-cmark` 0.13 with `ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS | ENABLE_TABLES`.
- Headings, bold, italic, strikethrough, links, inline code, blockquotes, lists, task-list
  markers, rules, code blocks, and tables, each producing `ratatui` `Line`/`Span` with styles.
- A `muted` mode (`accent()` collapses to the base style when `base_style.fg == DarkGray`) —
  the structural equivalent of the reference's `dimColor` prop.
- `markdown_prefixed_lines` puts a caller-supplied prefix on line 1 and `"  "` on the rest.
- A working table renderer with per-column widths and left/center/right alignment.
- Tests exist for lists/emphasis/code and for table alignment.

Missing or divergent, element by element:

| Gap | picopilot today | reference |
| --- | --- | --- |
| **Syntax highlighting** | none — code block body is one flat `Rgb(180,190,200)` | `cli-highlight` / highlight.js, ANSI-16 per token class |
| Code block indent | prepends `"  "` to every line | no indent |
| Heading levels | all levels identical: `Rgb(139,181,255)` + bold | H1 bold+italic+underline, H2+ bold, **no color at all** |
| Heading spacing | one `flush_line` | two newlines |
| Inline code color | `Rgb(242,204,96)` (amber) | `permission` = `rgb(177,185,249)` |
| Bullet glyph | `"* "` | `"- "` |
| Bullet color | `Rgb(240,177,94)` | unstyled, inherits |
| List indent | `"  " * (depth-1)`, applied to the marker span | `"  " * depth` |
| **Ordered lists** | not handled — `Tag::Item` always emits `*`, numbers are dropped | `1.`, then `a.` at depth 2, `i.` at depth 3, honouring `start` |
| Blockquote glyph | `"> "`, emitted **once at block start** | `"▎ "` (`U+258E`) on **every** line, bar dim, text italic |
| Blockquote color | `Rgb(132,147,160)` | `chalk.dim` bar only |
| Strikethrough | enabled, renders `CROSSED_OUT` | **deliberately disabled** — must be turned off to match |
| Task list marker | `"[x] "` / `"[ ] "` in `Rgb(240,177,94)` | no marker emitted at all |
| Horizontal rule | 40 literal `-` in DarkGray | literal `"---"`, unstyled |
| Links | colored `Rgb(139,181,255)` + underline, href discarded | OSC 8 hyperlink, `chalk.blue`, `mailto:` unwrapped, bare `owner/repo#123` auto-linked |
| Images | dropped | rendered as the bare href |
| Table borders | space-pipe-space separators, `-`/`+` divider after header only | full box drawing `┌─┬┐ ├─┼┤ └─┴┘ │`, rule between **every** row |
| Table header | bold | centered, not bold |
| Table narrow fallback | none — can overflow | vertical key/value format past 4 lines per row or `width - 4` |
| Table wrapping | none — cell width is `chars().count()`, no wrap, no CJK width | ANSI-aware wrap, `stringWidth`, hard-break mode |
| Cell width measure | `chars().count()` | `stringWidth` (`unicode-width` is already a picopilot dependency and should be used) |
| Block separation | none between blocks | `<Box gap={1}>` around table/non-table runs |
| Paragraph spacing | `flush_line` only | trailing `\n` per paragraph |

Two structural notes for whoever writes the spec:

- The reference emits **ANSI strings** and lets Ink parse them; picopilot builds **styled
  spans** directly. That is a better position to be in, but it means the highlighter must
  produce spans, not escape sequences — which is exactly why the syntect→`Span` conversion
  should live in-tree.
- The reference's markdown path carries **no theme color except `permission`**. Copying it
  means *removing* four of picopilot's five current markdown colors, not adding to them.
