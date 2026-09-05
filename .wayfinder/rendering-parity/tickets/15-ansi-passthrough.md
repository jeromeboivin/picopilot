---
label: wayfinder:research
name: Specify ANSI passthrough in shell output
status: closed
assignee: research-subagent
blocked_by: [06-bash-tool-rendering, 14-wrapping-and-background-fill]
---

# Specify ANSI passthrough in shell output

## Question

How does picopilot preserve the ANSI colors already present in shell output, and how does that
survive its own wrapping?

[Specify bash command and output rendering](06-bash-tool-rendering.md) found that the reference
does **not** strip color from shell output — it keeps it, and strips only the underline
attribute as noise. picopilot has no ANSI parser at all today; it treats tool output as plain
text. So it cannot simply copy the rule.

This collides directly with
[Specify the text wrapping and background fill mechanism](14-wrapping-and-background-fill.md):
a hand-rolled wrapper measures display width, and an escape sequence has display width zero.
Wrapping ANSI-bearing text without understanding it will break both the wrap points and the
colors, and the reference relies on an ANSI-aware wrapper to avoid exactly that.

Resolve by producing:

- Exactly which sequences the reference preserves, which it strips, and which it rewrites.
- What happens to a sequence that spans a wrap point, and whether the styling has to be
  reopened on the following row.
- The Rust approach: which crate parses SGR into ratatui styles, or whether it is written
  in-tree. Check what already exists for converting ANSI text into ratatui `Line`/`Span`.
- How the parsed styles interact with the palette — whether shell colors bypass the palette
  entirely or are remapped onto it.
- Malformed or hostile sequences: cursor movement, screen clears and OSC payloads that a
  subprocess could emit into the transcript, and what picopilot must refuse to pass through.
  This is the one place where the transcript renders untrusted bytes, so state the rule
  explicitly rather than leaving it implied.
- The rule the spec should state, and which surfaces it applies to besides shell output.

## Resolution

**Answer: parse SGR into ratatui `Style` and throw away every other escape sequence.** Colors,
bold, dim, italic, reverse and strikethrough survive; underline is removed after parsing; cursor
movement, screen clears, scroll regions, OSC titles, OSC 52 clipboard and OSC 8 links are dropped
before anything reaches the wrapper. The parse happens **before** the wrapper from
[14](14-wrapping-and-background-fill.md), so by the time text is wrapped it is already
`Vec<Span>` with styles attached and zero escape bytes — the wrap-point problem the ticket worried
about disappears rather than being solved.

Reference citations are `path:line` inside `C:\dev\git\claude-code`. Rust claims are from
crates.io / docs.rs source listings. Nothing was executed on either side; see *Not verified*.

### 1. What the reference preserves, strips and rewrites

The reference does not have one ANSI rule. It has **three layers**, and only the third one is
security-relevant. Reading them as one rule is the main way to get this wrong.

**Layer 1 — the display filter (`OutputLine`).** One regex, one target:

```ts
// src/components/shell/OutputLine.tsx:111-115
export function stripUnderlineAnsi(content: string): string {
  return content.replace(
    /\u001b\[([0-9]+;)*4(;[0-9]+)*m|\u001b\[4(;[0-9]+)*m|\u001b\[([0-9]+;)*4m/g, '')
}
```

It removes **only SGR sequences that contain the bare parameter `4`**, and it removes the *whole*
sequence — so `ESC[1;4;31m` (bold + underline + red) is deleted entirely, taking the bold and the
red with it. That is a bug in the reference, not a rule to copy; picopilot should drop the
underline *attribute* after parsing, which loses nothing else. Everything else at this layer is
passed through untouched to `<Ansi>`, which parses it into Ink `Text` spans
(`src/ink/Ansi.tsx`). The comment at `OutputLine.tsx:104-110` records that full `stripAnsi()` was
shipped once and reverted: *"people complained about losing all formatting"*.

Note the regex does not match `ESC[4:3m` (colon-delimited curly underline) or `ESC[58;…m`
(underline color). Both survive in the reference. Unimportant for us — we drop by attribute.

**Layer 2 — the wrapper (`wrap-ansi`).** Read from `chalk/wrap-ansi@main/index.js`, the fallback
the reference uses when not on Bun (`src/ink/wrapAnsi.ts:9-18`). It tracks SGR state and OSC 8
hyperlinks; its own source comment says *"ordinary CSI sequences and other complete 7-bit OSC
commands are preserved as opaque zero-width units"* and that DCS/SOS/PM/APC, C0 bytes inside
sequences and 8-bit forms are unsupported. So the wrapper **carries hostile sequences through**.
It is not a sanitiser and must not be treated as one.

**Layer 3 — the screen writer (`Ink`), where the actual filtering happens.** This is the finding
that answers the security bullet. `src/ink/output.ts:677-681`:

> *ESC (0x1B): skip incomplete escape sequences that ansi-tokenize didn't recognize.
> ansi-tokenize only parses SGR sequences (ESC[...m) and OSC 8 hyperlinks (ESC]8;;url BEL).
> Other sequences like cursor movement, screen clearing, or terminal title become individual
> char tokens that we need to skip here.*

`writeLineToScreen` then walks the tokens and **discards** (`output.ts:665-757`):

| input | action |
| --- | --- |
| `ESC ( X`, `ESC ) X`, `ESC * X`, `ESC + X` | charset designation — skip 2 chars |
| `ESC [ … final(0x40-0x7E)` | **any** CSI — skip to and including the final byte |
| `ESC ] …`, `ESC P …`, `ESC _ …`, `ESC ^ …`, `ESC X …` | OSC/DCS/APC/PM/SOS — skip to BEL or `ESC \` |
| `ESC` + single char 0x30-0x7E | Fp/Fe/Fs (`ESC 7`, `ESC 8`, `ESC c`, `ESC D`, `ESC M`) — skip 2 |
| `\t` | expanded to the next 8-column tab stop, as spaces |
| `\r`, `\b`, BEL, VT, FF, all other C0 | `continue` — dropped, never written |

Only *recognised* SGR and OSC 8 ever become cell attributes, because they were consumed by
`tokenize()` upstream and never reach this loop as characters. Everything else is thrown away at
the last moment before the terminal sees it. Nothing hostile is ever re-emitted.

**Rewrites.** Two, both at row boundaries, both in `wrap-ansi`'s `restoreStylesAcrossRows`:
styles are re-emitted (§2), and OSC 8 links are closed and reopened. Separately the reference
*adds* OSC 8 by rewriting URLs (`OutputLine.tsx:44-45` via `createHyperlink`), but that is gated
on the `linkifyUrls` prop and [06](06-bash-tool-rendering.md) §4 established Bash does not pass
it — **shell output is never linkified.**

**The rule this yields for picopilot, stated as preserve / strip / rewrite:**

| | |
| --- | --- |
| **Preserve** | SGR only: `0`, `1`, `2`, `3`, `7`, `9`, `21-29`, `22`, `23`, `27`, `29`, `30-37`, `39`, `40-47`, `49`, `90-97`, `100-107`, `38/48;5;N`, `38/48;2;R;G;B` |
| **Strip** | SGR `4`/`21`/`24` and `58`/`59` (underline family) — the attribute, not the sequence. Every non-SGR escape. Every C0 control except `\n` and `\t`. |
| **Rewrite** | `\t` → spaces to the next 8-column stop, before wrapping. `\r\n` → `\n`; a lone `\r` → **dropped**, not a line break. |

### 2. A style that spans a wrap point

The reference's rule, from `restoreStylesAcrossRows`: before every row break it emits the
**close code of each active style, in reverse order**, and immediately after the break it re-emits
the **open codes in order**. Closes are per-family, not `SGR 0` — foreground closes with `39`,
background with `49`, underline color with `59`, and each modifier with its own close code from
`ansi-styles`. An active OSC 8 link is closed with an empty link and reopened with the same URI.
Two edge cases it handles: an empty row reopens nothing, and leading SGR resets on the next row
are applied to the reopen set first (`applyLeadingSgrResets`) so a row starting with `ESC[0m`
does not get styles reopened just to close them again.

**picopilot does not need any of this.** Because the parse happens before the wrap, a style is not
"open" across a wrap point — it is a `Style` value on a `Span`, and splitting a `Span` at a column
copies the `Style` onto both halves. Reopening is automatic and cannot leak. The rule to state is
therefore the negative one:

> **Wrap-point rule.** ANSI is parsed to styled spans *before* wrapping, never after. The wrapper
> from [14](14-wrapping-and-background-fill.md) operates on `&[Span]` and never sees an escape
> byte, so escape sequences cannot contribute to a width measurement and a break inside a styled
> run simply produces two spans with the same `Style`. No style is ever "reopened".

Two consequences worth writing down:

- The wrapper's word-splitting is unaffected: a style change mid-word does not split the word,
  because splitting is done on `U+0020` in the *text*, which is now span-boundary-independent. The
  wrapper must therefore treat a `Line`'s spans as one logical character stream, not wrap each
  span separately.
- The reference's own `wrapWord` has a documented quirk — a grapheme cluster split by an escape
  sequence is measured once per part. Parsing first removes that quirk. This is a divergence in
  picopilot's favour; record it, do not reproduce it.

### 3. Rust: which crate

`Cargo.toml` today has **no ANSI crate at all** — `crossterm`, `ratatui`, `pulldown-cmark`,
`unicode-width` and nothing else. `src/` has no escape handling either (grepped for `\x1b`,
"escape sequence", "ansi": zero hits outside unrelated identifiers). This is a green field.

| crate | SGR coverage | yields ratatui types? | non-SGR sequences | maintenance | incremental / streaming |
| --- | --- | --- | --- | --- | --- |
| **`ansi-to-tui`** 8.0.1 | named, indexed 8-bit, truecolor, bold/dim/italic/underline/blink/reverse/hidden/crossed-out **and their off-codes** | **yes** — `Text<'static>` of `Line`/`Span` with `Style` | consumed and **discarded** by `any_escape_sequence` | owned by the ratatui org (orhun, joshka, uttarayan21); 5.1M downloads; 8.0.1 published ~8 months before this ticket | **no** — `IntoText::into_text(&self)` on a complete buffer only; no resumable parser state exposed |
| `cansi` 2.2.1 | SGR only, and only the 8 standard colors (`Color` enum is 8 variants) — **no truecolor, no 256-color** | no — its own `CategorisedSlice` | CSI-only scope; ignores others | single maintainer, v1 API deprecated in-place | no |
| `vte` 0.15 | none — it is a state machine, you write the SGR semantics | no | gives you `csi_dispatch` / `osc_dispatch` / `esc_dispatch` callbacks; **you decide** | alacritty; excellent | **yes** — `Parser::advance(&mut perform, bytes)` is byte-at-a-time resumable |
| `anstyle-parse` 1.0 | none — same shape as `vte` | no | same | rust-cli / `anstyle` family; excellent | **yes**, same API |

**Recommendation: `ansi-to-tui`.** It is the only candidate that produces `Line`/`Span`/`Style`
directly, its SGR coverage is a superset of what the reference honours, it is maintained by the
ratatui organisation itself, and — the decisive point for the security bullet — its
`any_escape_sequence` parser *consumes and returns nothing* for every non-SGR sequence
(`parser.rs:259-278`), so cursor movement, clears and OSC payloads are dropped by construction,
not by a filter we have to remember to apply. `cansi` is disqualified on colors alone: 8-color
output would be a visible regression against a reference that renders truecolor. `vte` and
`anstyle-parse` are the right answer only if streaming forces it — see the caveats.

**Pin `ansi-to-tui = "7.0.0"`, not 8.x, until ratatui is upgraded.** 8.0.1 depends on
`ratatui-core ^0.1`; ratatui 0.29 — which picopilot uses — has **no `ratatui-core` dependency at
all** (checked its full dependency list). The `Text` that 8.0.1 returns would therefore be a
different type from `ratatui::text::Text` and will not compile against this tree. 7.0.0 depends on
`ratatui ^0.29` and is the compatible one. Move to 8.x in the same change that moves to
ratatui 0.30+.

**Four caveats that must be handled in-tree regardless of version:**

1. **No streaming.** Both versions parse a whole buffer and start from `Style::new()` every call.
   For the live shell progress row this means re-parsing the accumulated output each frame. That
   is acceptable — [06](06-bash-tool-rendering.md) §6 caps the preview at 5 lines and §5 caps the
   committed block at 3 wrapped lines, so the buffer being re-parsed is small and bounded. Do not
   feed it chunk-by-chunk: a chunk boundary inside an escape sequence would corrupt the style, and
   there is no API to carry state across calls. If a future surface genuinely needs incremental
   parsing, that is the case for switching to `vte` and writing the SGR table in-tree.
2. **Carriage return.** 7.0.0 splits lines on `\n` only, so a lone `\r` — every progress bar ever
   written — lands **inside a `Span`**. 8.0.1 treats `\r` as a line break, which is also wrong (it
   turns one overwritten row into N rows). Neither matches the reference, which drops `\r`
   outright (`output.ts:750-755`). **Normalise the input before parsing:** `\r\n` → `\n`, then
   delete remaining `\r`.
3. **An OSC without a BEL terminator eats the rest of the line.** `any_escape_sequence` handles
   `ESC ]` with `take_till(|c| c == b'\x07')` — it does not know about `ESC \` (ST). An
   ST-terminated OSC, which is legal and common, therefore consumes to end of buffer/line and the
   text after it is silently lost. Non-alpha CSI final bytes have a smaller version of the same
   problem. This is a **correctness** bug, not a security hole — dropping too much is the safe
   direction — but it is a reason to pre-strip non-SGR sequences ourselves (§5) rather than rely
   on the crate to do it tidily. *Reasoned from the parser source; not executed.*
4. **7.0.0 is missing the SGR off-codes.** `NotItalic` (23), `UnderlineOff` (24), `BlinkOff` (25),
   `InvertOff` (27) and `CrossedOutOff` (29) are only handled in 8.x. Under 7.0.0 an attribute
   turned on stays on until an explicit `SGR 0`. Accepted for now; it resolves on the 8.x upgrade.

Underline stripping becomes trivial and lossless: after parsing, clear `Modifier::UNDERLINED` on
every produced `Style`. No regex, and none of the collateral damage the reference's regex causes.

### 4. Interaction with the palette

**Shell colors bypass the palette entirely.** In the reference the parsed spans are rendered by
`<Ansi>` as Ink `Text` nodes carrying their own concrete colors; the theme is not consulted. The
enclosing `<Text color={color}>` from `OutputLine.tsx:83` supplies the palette key `error` for
stderr / `warning` for the unused third channel, and an inner span's own color wins over it — so
the `error` tint only reaches the parts of stderr that carry no ANSI color of their own.
*Reasoned from the component structure; Ink's color inheritance was not executed.*

picopilot should copy that shape, which falls out of ratatui naturally: build the result block
with the channel's palette `Style` as the base and `patch()` each parsed span's style over it.
Parsed truecolor becomes `Color::Rgb`, `38;5;N` becomes `Color::Indexed(N)`, and the 16 named
colors become ratatui's named `Color`s — which the terminal resolves from the *user's* scheme, not
ours. That is correct and is what `ls --color` expects. Do **not** remap parsed colors onto the
palette from [01](01-dark-palette.md): the palette describes picopilot's own chrome, and
rewriting a subprocess's red into `error` would misrepresent the tool's output.

One open consequence, deferred: this is a second source of ANSI-16 in the transcript, alongside
the markdown path already noted under *Not yet specified → Truecolor fallback* in the map. It
does not need an answer here.

### 5. Untrusted sequences — the explicit allowlist

Shell output is the one place the transcript renders bytes chosen by something other than
picopilot. Treat it as untrusted input, and use an **allowlist**, never a denylist: a denylist has
to enumerate every dangerous sequence, and the interesting ones are the ones nobody thought of.

> **Sanitisation rule.** Before shell output is parsed or measured, everything that is not in this
> list is deleted:
>
> **Allowed**
> - `CSI … m` — SGR, and only the parameters in §1's preserve column. An SGR with an unrecognised
>   parameter is dropped as a whole sequence, not passed on.
> - `U+000A` line feed, as a line break.
> - `U+0009` tab, rewritten to spaces at the next 8-column stop.
> - Printable text, including combining marks and emoji.
>
> **Dropped, unconditionally**
> - **All other CSI** — `ESC [ … final`, any final byte in 0x40–0x7E.
> - **All OSC** — `ESC ] …` to BEL or ST, including OSC 8.
> - **DCS / APC / PM / SOS** — `ESC P`, `ESC _`, `ESC ^`, `ESC X` to their terminator.
> - **Charset designation** — `ESC ( X`, `ESC ) X`, `ESC * X`, `ESC + X`.
> - **Single-char escapes** — `ESC` followed by 0x30–0x7E (`ESC 7`, `ESC 8`, `ESC c`, `ESC D`,
>   `ESC M`).
> - **A lone `ESC`** that starts nothing recognisable, and any truncated sequence.
> - **Every other C0 control**, including `\r`, `\b`, BEL, VT, FF, NUL.
> - **C1 8-bit forms** (0x80–0x9F), including `U+009B` CSI. Nothing in this codebase needs them and
>   `wrap-ansi` documents its own support for them as absent.

Why each dropped category matters, concretely:

- **Cursor movement** (`CSI A/B/C/D/H/f`, `ESC 7`/`ESC 8`) lets output written *later* overwrite
  rows written *earlier*. Under [02](02-scrollback-mechanism.md) those earlier rows are committed
  scrollback that picopilot can no longer redraw, so a subprocess could permanently rewrite the
  visible record of what the agent did — including a permission prompt or a tool result the user
  already read. This is the strongest reason in the list. It is also why `\r` and `\b` are in it:
  they are cursor movement in one byte.
- **Screen clears and scroll-region changes** (`CSI 2J`, `CSI 3J`, `CSI r`, `CSI S`/`T`) let a
  subprocess erase the transcript or confine picopilot's own drawing to a strip of the screen. The
  live region's geometry is picopilot's invariant; handing it to a subprocess breaks every height
  calculation `insert_before` depends on.
- **OSC 52** writes the **system clipboard**. A build script that prints one escape sequence can
  replace whatever the user was about to paste. There is no legitimate reason for tool output to
  do this and no way for the user to notice it happened.
- **OSC 0/1/2** set the terminal **title**, which many shells echo into their own prompt and which
  some multiplexers persist. Cheap, silent spoofing.
- **OSC 8 hyperlinks** are the interesting one, because the reference *does* emit them — but only
  for MCP output that it linkified itself (`src/tools/MCPTool/UI.tsx:174,244`), never for shell
  output ([06](06-bash-tool-rendering.md) §4). Honouring an OSC 8 that the *subprocess* supplied
  means rendering attacker-chosen display text over an attacker-chosen URL: the classic phishing
  primitive, with the URL invisible until clicked. **Drop it.** This also settles half of the
  map's open *Hyperlinks* question in the restrictive direction: whatever picopilot decides about
  emitting its own links, it must not forward one it did not create.
- **DCS/APC/PM** carry device-control payloads — Sixel, Kitty graphics, tmux passthrough. Passing
  them to a terminal picopilot has no model of is an unbounded surface for zero gain.
- **Charset designation** (`ESC ( 0`) switches the terminal into line-drawing mode, so subsequent
  *plain ASCII* renders as different glyphs. It desynchronises text from what picopilot measured
  and persists past the end of the output.
- **Truncated sequences** matter because the 3-line fold from [06](06-bash-tool-rendering.md) §5
  cuts output at an arbitrary byte. Sanitise **before** truncating, so a cut can never manufacture
  a half-sequence — and drop a trailing partial sequence rather than emitting it.

Two implementation notes:

- **Sanitise on the way in, once, at the boundary where subprocess bytes enter a `ChatEntry`** —
  not at render time. Sanitising once means the truncator, the wrapper, `insert_before`'s height
  calculation and any future export path all see the same bytes, and it removes the standing risk
  of a new surface forgetting to filter.
- **Defence in depth is real but must not be relied on.** `ansi-to-tui` discards unknown escapes
  (§3), and ratatui's `Buffer::set_stringn` skips zero-width graphemes and control characters
  (established in [14](14-wrapping-and-background-fill.md) §2), so a stray `ESC` that reached a
  `Span` would very likely be swallowed. "Very likely" is not a security property, and it is a
  property of two dependency versions we do not control. Filter explicitly.

### 6. The rule for the spec, and where it applies

> **ANSI rule.** Text that picopilot did not generate is sanitised at ingestion against the
> allowlist above: SGR, line feed and tab survive; every other escape sequence and control
> character is deleted. The surviving SGR is parsed into ratatui `Style` before any width is
> measured, then `Modifier::UNDERLINED` is cleared. Parsed colors are used as-is and are not
> remapped onto the palette; the surface's palette color is the base style that parsed spans are
> patched over. The wrapper never sees an escape byte.

Surfaces this applies to, beyond Bash stdout/stderr:

- **The shell progress preview** ([06](06-bash-tool-rendering.md) §6) — same bytes, arriving
  earlier. Sanitising at ingestion covers it for free.
- **Tool error text** ([05](05-tool-call-rendering.md) §10) — `Error: Exit code 1` is followed by
  the merged subprocess output, so the error path renders untrusted bytes too. Easy to miss,
  because it looks like picopilot's own string.
- **Any other tool's result block.** picopilot currently renders nothing for non-shell tools
  ([06](06-bash-tool-rendering.md) §7), but [05](05-tool-call-rendering.md) requires that to
  change, and an MCP server's output is exactly as untrusted as a subprocess's.
- **Assistant text: no.** Model output is not passed through the ANSI parser — it goes through the
  markdown path from [04](04-markdown-and-code-blocks.md). But it is still not picopilot-authored
  text, so it must be sanitised at ingestion on the same rule. A model that emits `ESC[2J` into a
  code fence must not clear the screen.
- **File contents echoed into the transcript** (diffs, read results) — same reasoning; the bytes
  come from disk.

Put plainly: **the only text exempt from this rule is text picopilot's own code wrote.**

### Not verified

- **Nothing was executed on either side.** No Bun, no `node_modules`, no `claude` binary — the
  standing limitation from the map's *Known risk*. No Rust was compiled either; the
  ansi-to-tui/ratatui version incompatibility in §3 is read from crates.io dependency metadata,
  not from a failed build.
- **`Bun.wrapAnsi` was not read.** §1 layer 2 and §2 describe the npm `wrap-ansi` fallback, per
  [14](14-wrapping-and-background-fill.md)'s same caveat. Real Claude Code runs Bun's version.
- **Ink's inner-over-outer color inheritance (§4) is reasoned from component structure**, not
  observed. If Ink actually resolves the outer `<Text color>` last, stderr would be uniformly
  `error` and the parsed colors would be lost — which would make the reference's whole
  "don't strip color" position incoherent, so the reading in §4 is very likely right, but it is
  not proven.
- **The `ansi-to-tui` OSC/ST defect (§3 caveat 3) is reasoned from the nom combinators**, not
  reproduced. It does not change the recommendation either way, since §5 requires pre-stripping.
- **No claim is made about which of these sequences any specific terminal actually honours.** The
  allowlist is written on the assumption that some terminal somewhere honours all of them, which
  is the only safe assumption for untrusted input.
