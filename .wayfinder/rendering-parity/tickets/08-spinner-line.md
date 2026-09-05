---
label: wayfinder:research
name: Specify the spinner line
status: closed
assignee: research-subagent
blocked_by: [02-scrollback-mechanism]
---

# Specify the spinner line

## Question

What exactly does the "thinking" line show while the agent is working, and what does picopilot
need in order to animate it?

This line also inherits some of the deleted status bar: model and token counts ride here.

Resolve by producing:

- The animation frames, quoted verbatim, with their per-platform and reduced-motion variants,
  the frame interval, and the forward-then-reverse cycling.
- The verb list. Branding is being copied, so capture the actual list from
  `src/constants/spinnerVerbs.ts` and the rule for choosing and rotating a verb.
- Elapsed time formatting, and the threshold after which token counts appear.
- Which token numbers are shown and how they are formatted.
- The shimmer animation and the stalled-state color interpolation, with their timings.
- **The render tick decision.** picopilot only redraws when an event arrives, so nothing
  animates today. State what introducing a frame timer costs — CPU when idle, interaction with
  the live region from
  [Settle the scrollback rendering mechanism](02-scrollback-mechanism.md), behaviour when the
  terminal is not focused — and recommend an interval.
- Which of picopilot's current status bar fields belong on this line and which do not.

## Resolution

Settled from source. Reference read at `C:\dev\git\claude-code`. picopilot facts read from
`src/tui.rs` at the current HEAD. Nothing was executed — no `bun`, no `node_modules`, no
`claude` binary on this machine. Anything I could not confirm is flagged **Unverified**.

The spinner line is `SpinnerAnimationRow`, a single row rendered by `SpinnerWithVerb`. It is
the *only* thing in the reference driven by an animation clock, and it carries five
independent time-derived effects on one shared clock.

### 1. Row anatomy

`src/components/Spinner/SpinnerAnimationRow.tsx:226`

```tsx
<Box ref={viewportRef} flexDirection="row" flexWrap="wrap" marginTop={1} width="100%">
  <SpinnerGlyph … />
  <GlimmerMessage … />
  {status}
</Box>
```

Left to right, at column 0:

| Cols | Content | Source |
| --- | --- | --- |
| 0–1 | one spinner glyph in a `Box width={2}` — glyph at col 0, col 1 blank | `SpinnerGlyph.tsx:70` |
| 2… | the verb message, then a hard trailing space in `messageColor` | `GlimmerMessage.tsx:323-329` |
| … | the status group, `dimColor`, in parentheses | `SpinnerAnimationRow.tsx:203-225` |

`marginTop={1}` gives **one blank line above the spinner row**, matching the blank-line rule in
[03-user-assistant-messages](03-user-assistant-messages.md).

Rendered shape, at rest and after the 30 s threshold:

```
✻ Herding…
✻ Herding… (12s · ↓ 1.3k tokens)
✻ Herding… (thinking with high effort · 1m 5s · ↓ 4.2k tokens)
```

**"esc to interrupt" is not on this line in this version.** It moved to the prompt footer
(`src/components/PromptInput/PromptInputFooterLeftSide.tsx:389,417`). The only place the
spinner still prints it is the teammate case, `SpinnerAnimationRow.tsx:216`. picopilot should
not put it here.

### 2. Animation frames — verbatim, per platform

`src/components/Spinner/utils.ts:4-11`

```ts
export function getDefaultCharacters(): string[] {
  if (process.env.TERM === 'xterm-ghostty') {
    return ['·', '✢', '✳', '✶', '✻', '*'] // Use * instead of ✽ for Ghostty because the latter renders in a way that's slightly offset
  }
  return process.platform === 'darwin'
    ? ['·', '✢', '✳', '✶', '✻', '✽']
    : ['·', '✢', '*', '✶', '✻', '✽']
}
```

So three variants, all six characters long:

| Platform | Frames |
| --- | --- |
| macOS | `· ✢ ✳ ✶ ✻ ✽` |
| Ghostty (`TERM=xterm-ghostty`, any OS — checked first) | `· ✢ ✳ ✶ ✻ *` |
| **Everything else, including Windows and Linux** | `· ✢ * ✶ ✻ ✽` |

Note the substitution differs: Ghostty replaces the *last* glyph `✽`, non-darwin replaces the
*third* glyph `✳`. Windows Terminal gets `· ✢ * ✶ ✻ ✽`. That is the variant picopilot uses.

**Forward-then-reverse cycling.** `SpinnerGlyph.tsx:7` (and identically `Spinner.tsx:41`):

```ts
const SPINNER_FRAMES = [...DEFAULT_CHARACTERS, ...[...DEFAULT_CHARACTERS].reverse()];
```

The spread copies before reversing, so the source array is untouched. The result is **12
frames with the endpoints doubled**, on Windows:

```
· ✢ * ✶ ✻ ✽ ✽ ✻ ✶ * ✢ ·
```

It is not a smooth palindrome — `✽` and `·` each hold for two frames. Copy that; it is the
actual behaviour.

**Interval.** `SpinnerAnimationRow.tsx:131` — `const frame = reducedMotion ? 0 : Math.floor(time / 120)`,
indexed `SPINNER_FRAMES[frame % SPINNER_FRAMES.length]` at `SpinnerGlyph.tsx:49`.
**120 ms per frame, 1440 ms per full cycle.**

**Reduced motion.** `SpinnerGlyph.tsx:8-9,37-40`

```ts
const REDUCED_MOTION_DOT = '●'
const REDUCED_MOTION_CYCLE_MS = 2000 // 2-second cycle: 1s visible, 1s dim
…
const isDim = Math.floor(time / (REDUCED_MOTION_CYCLE_MS / 2)) % 2 === 1;
```

`●` in `messageColor`, alternating normal/dim every 1000 ms.

> **Finding, marked inferred.** In *this* call path that pulse never runs.
> `SpinnerAnimationRow.tsx:103` is `useAnimationFrame(reducedMotion ? null : 50)`, and
> `use-animation-frame.ts:36-39` unsubscribes on `null`, so `time` is frozen. The
> reduced-motion dot is therefore **static**, not pulsing. The 1 s dim cycle only fires if some
> caller feeds a live `time` while `reducedMotion` is true, which no caller does. Unverified
> against a running binary; recommend picopilot implement it as **a static `●`**, which is both
> simpler and what users actually see.

The standalone small `Spinner()` export (`Spinner.tsx:511-540`) uses the same 12 frames at
`useAnimationFrame(120)`, colour `text`, and renders a static `●` under reduced motion. That is
a different component from the status line; not in scope here.

### 3. The verb list — 187 entries, verbatim

`src/constants/spinnerVerbs.ts:16-204`. Branding is being copied, so this is the spec source of
truth. Note the non-ASCII entries (`Flambéing`, `Sautéing`), the apostrophe entry
(`Beboppin'`, the one double-quoted line in the file), both spellings of *Channeling* /
*Channelling*, and the British *Philosophising* / *Unravelling*.

```
Accomplishing, Actioning, Actualizing, Architecting, Baking, Beaming, Beboppin',
Befuddling, Billowing, Blanching, Bloviating, Boogieing, Boondoggling, Booping,
Bootstrapping, Brewing, Bunning, Burrowing, Calculating, Canoodling, Caramelizing,
Cascading, Catapulting, Cerebrating, Channeling, Channelling, Choreographing, Churning,
Clauding, Coalescing, Cogitating, Combobulating, Composing, Computing, Concocting,
Considering, Contemplating, Cooking, Crafting, Creating, Crunching, Crystallizing,
Cultivating, Deciphering, Deliberating, Determining, Dilly-dallying, Discombobulating,
Doing, Doodling, Drizzling, Ebbing, Effecting, Elucidating, Embellishing, Enchanting,
Envisioning, Evaporating, Fermenting, Fiddle-faddling, Finagling, Flambéing,
Flibbertigibbeting, Flowing, Flummoxing, Fluttering, Forging, Forming, Frolicking,
Frosting, Gallivanting, Galloping, Garnishing, Generating, Gesticulating, Germinating,
Gitifying, Grooving, Gusting, Harmonizing, Hashing, Hatching, Herding, Honking,
Hullaballooing, Hyperspacing, Ideating, Imagining, Improvising, Incubating, Inferring,
Infusing, Ionizing, Jitterbugging, Julienning, Kneading, Leavening, Levitating,
Lollygagging, Manifesting, Marinating, Meandering, Metamorphosing, Misting, Moonwalking,
Moseying, Mulling, Mustering, Musing, Nebulizing, Nesting, Newspapering, Noodling,
Nucleating, Orbiting, Orchestrating, Osmosing, Perambulating, Percolating, Perusing,
Philosophising, Photosynthesizing, Pollinating, Pondering, Pontificating, Pouncing,
Precipitating, Prestidigitating, Processing, Proofing, Propagating, Puttering, Puzzling,
Quantumizing, Razzle-dazzling, Razzmatazzing, Recombobulating, Reticulating, Roosting,
Ruminating, Sautéing, Scampering, Schlepping, Scurrying, Seasoning, Shenaniganing,
Shimmying, Simmering, Skedaddling, Sketching, Slithering, Smooshing, Sock-hopping,
Spelunking, Spinning, Sprouting, Stewing, Sublimating, Swirling, Swooping, Symbioting,
Synthesizing, Tempering, Thinking, Thundering, Tinkering, Tomfoolering, Topsy-turvying,
Transfiguring, Transmuting, Twisting, Undulating, Unfurling, Unravelling, Vibing,
Waddling, Wandering, Warping, Whatchamacalliting, Whirlpooling, Whirring, Whisking,
Wibbling, Working, Wrangling, Zesting, Zigzagging
```

`Clauding` and `Gitifying` are Claude-specific; picopilot may want to swap `Clauding` for a
picopilot equivalent. That is a branding call, not a rendering one.

**Choosing and rotating.** `Spinner.tsx:166-171`

```tsx
const [randomVerb] = useState(() => sample(getSpinnerVerbs()));
const leaderVerb = overrideMessage ?? currentTodo?.activeForm ?? currentTodo?.subject ?? randomVerb;
const message = effectiveVerb + '…';
```

- **The verb does not rotate.** `useState` with an initialiser picks it **once on mount**,
  uniformly at random (lodash `sample`). It is stable for the whole life of the component.
- Because the REPL renders the spinner behind `showSpinner &&` (`REPL.tsx:4587`), the component
  unmounts between turns, so in practice **one verb per turn**. *Inferred from the conditional
  render; not verified against a running binary.*
- Precedence: explicit `overrideMessage` → the in-progress todo's `activeForm`, else its
  `subject` → the random verb.
- The displayed message is always `verb + '…'` (U+2026 HORIZONTAL ELLIPSIS, one character —
  not three dots).
- Users can extend or replace the list via the `spinnerVerbs` setting, mode `'replace'` or
  append (`spinnerVerbs.ts:3-14`). Optional for picopilot.

### 4. The shimmer (glimmer) sweep

`SpinnerAnimationRow.tsx:132-138`

```ts
const glimmerSpeed = mode === 'requesting' ? 50 : 200;
const cycleLength = glimmerMessageWidth + 20;
const cyclePosition = Math.floor(time / glimmerSpeed);
const glimmerIndex = reducedMotion ? -100 : isStalled ? -100
  : mode === 'requesting' ? cyclePosition % cycleLength - 10
  : glimmerMessageWidth + 10 - cyclePosition % cycleLength;
```

- One column step every **200 ms**, or **50 ms** in `requesting` mode (4× faster while waiting
  on the API).
- Cycle length is `messageWidth + 20` steps, so the band runs from 10 columns before the
  message to 10 columns after it — roughly 2 s of visible sweep plus dead time, per cycle.
- **Direction reverses by mode.** `requesting` sweeps **left → right**; every other mode sweeps
  **right → left**.
- `reducedMotion` or a stall parks the index at `-100`, i.e. permanently off the message, which
  is how "no shimmer" is expressed.

`GlimmerMessage.tsx:241-329` renders it. The band is `[glimmerIndex-1, glimmerIndex+1]` —
**three display columns wide**. Text is split by grapheme cluster with per-cluster display
width, so the split is column-correct, not byte-correct:

```ts
const shimmerStart = glimmerIndex - 1
const shimmerEnd = glimmerIndex + 1
```

- Inside the band: palette key `claudeShimmer`.
- Outside: `messageColor` (default `claude`).
- Band entirely off the message → whole message in `messageColor`, single span.
- A trailing space in `messageColor` is emitted in **every** branch.

### 5. The tool-use flash

`SpinnerAnimationRow.tsx:139`

```ts
const flashOpacity = reducedMotion ? 0 : mode === 'tool-use' ? (Math.sin(time / 1000 * Math.PI) + 1) / 2 : 0;
```

Sine, **period 2000 ms**, range 0–1. In `mode === 'tool-use'` the *whole* message is a single
colour interpolated `claude → claudeShimmer` by `flashOpacity`; the per-column sweep is
bypassed entirely (`GlimmerMessage.tsx:128-190`). Non-truecolor fallback is a hard switch at
`flashOpacity > 0.5`. This is the run-time interpolation already documented in
[01-dark-palette](01-dark-palette.md) §4 — no new colours.

### 6. The stall fade

`src/components/Spinner/useStalledAnimation.ts:41-49,53-67`

```ts
const isStalled = timeSinceLastToken > 3000 && !hasActiveTools
const intensity = isStalled ? Math.min((timeSinceLastToken - 3000) / 2000, 1) : 0
```

Smoothing, also on the animation clock, `useStalledAnimation.ts:53-67`: every 50 ms of elapsed
clock time, `current += (intensity - current) * 0.1`, snapping when `|diff| < 0.01`. Under
`reducedMotion` the intensity is applied instantly with no smoothing.

- **Threshold 3000 ms** with no new output characters and no active tools.
- **Ramp 2000 ms** to full intensity, then exponentially smoothed at 10 % per 50 ms step.
- Reset conditions: any growth in `responseLengthRef`; `hasActiveTools`; `leaderIsIdle`.
- Both the glyph (`SpinnerGlyph.tsx:50-57`) and the message (`GlimmerMessage.tsx:88-104`)
  interpolate `messageColor → ERROR_RED { r:171, g:43, b:63 }`, the off-palette hardcoded value
  already recorded in [01-dark-palette](01-dark-palette.md).
- A stall also **kills the shimmer** (`glimmerIndex = -100`), so the message goes flat and red
  together.
- Non-truecolor fallback: `stalledIntensity > 0.5 ? 'error' : messageColor`.

### 7. Elapsed time

`SpinnerAnimationRow.tsx:105-107,162`

```ts
const elapsedTimeMs = pauseStartTimeRef.current !== null
  ? pauseStartTimeRef.current - loadingStartTimeRef.current - totalPausedMsRef.current
  : now - loadingStartTimeRef.current - totalPausedMsRef.current;
const timerText = formatDuration(effectiveElapsedMs);
```

Wall clock from turn start, minus accumulated paused time, frozen while paused.

`src/utils/format.ts:33-93` — `formatDuration` with no options:

| Input | Output |
| --- | --- |
| `0` | `0s` |
| `< 1 ms` | `0.0s` (one decimal) |
| `< 60000 ms` | `Math.floor(ms/1000)` + `s` → `12s`, `59s` |
| `< 1 h` | `1m 5s` |
| `< 1 d` | `1h 2m 3s` |
| `≥ 1 d` | `1d 2h 3m` |

Seconds are **floored** under a minute and **rounded** above it, with carry (`59.5s → 1m 0s`).
No zero-padding, no leading zeros, space-separated units.

### 8. Token display and its threshold

`SpinnerAnimationRow.tsx:179` — the gate for **both** the timer and the token count:

```ts
const wantsTimerAndTokens = verbose || hasRunningTeammates || effectiveElapsedMs > SHOW_TOKENS_AFTER_MS;
```

with `SHOW_TOKENS_AFTER_MS = 30_000` (`:19`). So:

- **Nothing but the verb for the first 30 seconds.** The timer and the token count appear
  together at 30 s, not separately.
- `--verbose` shows them immediately.
- Tokens additionally require `totalTokens > 0` (`:192`).

**Which number.** `SpinnerAnimationRow.tsx:159-167`

```ts
const leaderTokens = Math.round(displayedResponseLength / 4);
const totalTokens = … leaderTokens + teammateTokens;
const tokenCount = formatNumber(totalTokens);
```

`responseLengthRef` is the accumulated **character** count of the streamed response this turn
(`REPL.tsx:1443,1574`), so the displayed token count is an **estimate: characters ÷ 4**. This
is **output tokens for the current turn** — *not* context-window usage, *not* a total, and
there is no `/limit` denominator anywhere on this line.

The counter is **animated, not snapped** (`:142-158`):

```ts
const gap = currentResponseLength - tokenCounterRef.current;
if (gap > 0) {
  if (gap < 70) increment = 3;
  else if (gap < 200) increment = Math.max(8, Math.ceil(gap * 0.15));
  else increment = 50;
  tokenCounterRef.current = Math.min(tokenCounterRef.current + increment, currentResponseLength);
}
```

One increment per 50 ms tick, so the number visibly climbs instead of jumping. Under
`reducedMotion` it snaps.

**Formatting.** `format.ts:124-131` — `Intl.NumberFormat('en-US', { notation: 'compact' })`,
lowercased:

- `< 1000` → plain integer, no separator: `900`
- `≥ 1000` → compact with **exactly one forced decimal**: `1.3k`, `1.0k`, `12.4k`, `1.2m`

The `1.0k` case is deliberate (`minimumFractionDigits: 1` for the ≥1000 formatter). picopilot's
`format_count` (`src/tui.rs:3465`) does comma grouping instead and must be replaced for this
field.

**Rendered as** (`SpinnerAnimationRow.tsx:210-215`): a `Box width={2}` holding a dim direction
arrow, then dim `` `${tokenCount} tokens` ``. The arrow comes from `SpinnerModeGlyph`
(`:232-262`):

- `requesting` → `figures.arrowUp`
- `tool-input` / `tool-use` / `responding` / `thinking` → `figures.arrowDown`
- teammates running → **no arrow at all**

**Unverified:** `figures.arrowUp` / `arrowDown` resolve to `↑` / `↓` — `node_modules` is absent
so the fallback table could not be checked. Neither arrow appears in the `figures` Windows
fallback map as far as I know, so `↑`/`↓` on all platforms is the likely answer.

### 9. Thinking status and its shimmer

State machine, `Spinner.tsx:124-159`. `thinkingStatus` is `'thinking' | number | null`:

- Enters `'thinking'` when `mode === 'thinking'`.
- On leaving, holds `'thinking'` for the remainder of a **2000 ms** minimum, then shows the
  numeric duration for a further **2000 ms**, then clears to `null`.

Text, `SpinnerAnimationRow.tsx:172`:

- while thinking → `` `thinking${effortSuffix}` ``, where `effortSuffix` is
  `` ` with ${level} effort` `` or `''` (`src/utils/effort.ts:188-196`)
- after → `` `thought for ${Math.max(1, Math.round(ms / 1000))}s` ``

Shimmer, `SpinnerAnimationRow.tsx:24-35,198-200`:

```ts
const THINKING_INACTIVE = { r: 153, g: 153, b: 153 }
const THINKING_INACTIVE_SHIMMER = { r: 185, g: 185, b: 185 }
const THINKING_DELAY_MS = 3000
const THINKING_GLOW_PERIOD_S = 2
…
const thinkingOpacity = time < THINKING_DELAY_MS ? 0
  : (Math.sin(thinkingElapsedSec * Math.PI * 2 / THINKING_GLOW_PERIOD_S) + 1) / 2;
```

Sine, **period 2000 ms**, interpolating between two hardcoded greys — already recorded in
[01-dark-palette](01-dark-palette.md) §4. **Note the delay is measured against the global clock
`time`, which starts when the app's clock starts, not when thinking starts** — so in any real
session `time` is far past 3000 ms and the delay never has an effect. Treat
`THINKING_DELAY_MS` as vestigial; picopilot should not implement it.

Rendered in the shimmer colour when `thinkingStatus === 'thinking' && !reducedMotion`,
otherwise `dimColor`.

**The `thinkingOnly` special case** (`:193,222`): when thinking is the *only* status part, the
parentheses are printed as part of the shimmering text (`(thinking with high effort)`, all in
the shimmer colour) rather than as separate dim parens.

### 10. Width gating — the shrink order

`SpinnerAnimationRow.tsx:176-192`

```ts
const messageWidth = glimmerMessageWidth + 2;
const availableSpace = columns - messageWidth - 5;
```

`SEP_WIDTH = stringWidth(' · ') = 3`. Parts are admitted in this order, each only if it plus
its separator still fits in the running total:

1. **thinking** — if it does not fit and it has an effort suffix, retry with the bare word
   `thinking` (`THINKING_BARE_WIDTH`); if that fits, use it.
2. **timer**
3. **tokens**

`spinnerSuffix` (used only for a Stop-hook message, `REPL.tsx:4142`) is unconditional and comes
first in the rendered order. Parts are joined by the `Byline` component with `" · "`
(`src/components/design-system/Byline.tsx:10`), all `dimColor`, wrapped in dim `(` `)`.

**Extra rows below the spinner** (`Spinner.tsx:257-258,288-302`) are rendered through
`MessageResponse`, i.e. behind the same dim `  ⎿  ` gutter as tool results
([05-tool-call-rendering](05-tool-call-rendering.md)):

- `Next: <task subject>` if a pending todo exists
- `Tip: …` — the `/btw` tip after **30 000 ms**, the `/clear` tip after **1 800 000 ms** (30 min)
- an ant-only token-budget line

These are optional for picopilot and belong to a tips ticket if wanted, not to this one.

### 11. The render tick — correcting the premise, and the recommendation

**The ticket's premise is wrong, and this is the most important finding here.** picopilot
already redraws on a 50 ms timer. `src/tui.rs:1568-1612`:

```rust
while !app.should_quit() {
    terminal.draw(|frame| draw(frame, &app))?;
    let tick = tokio::time::sleep(Duration::from_millis(50));
    tokio::pin!(tick);
    tokio::select! {
        …
        _ = &mut tick => {
            process_terminal_events(&mut app, &mut runtime, &mut events).await?;
            …
        },
    }
}
```

`terminal.draw` runs unconditionally at the top of every iteration, and the `tick` branch
guarantees an iteration at least every 50 ms. There is a second `tokio::time::interval` at
2 s for usage/cost refresh (`:1565`). So the frame timer **already exists and already costs
what it costs**. Nothing animates today because no widget derives anything from elapsed time —
not because there is no tick.

**Reference cadence, for reconciliation.** The reference runs one shared clock at
`FRAME_INTERVAL_MS = 16` (`src/ink/constants.ts:2`) created in `ClockProvider`
(`src/ink/components/ClockContext.tsx:10,68-104`); each subscriber self-throttles to its own
interval (`src/ink/hooks/use-animation-frame.ts:30-56`). The spinner row subscribes at **50 ms**
(`SpinnerAnimationRow.tsx:103`); the tool-dot blink subscribes at **600 ms**
(`src/hooks/useBlink.ts:3,26`). Every effect is a pure function of one shared `time`, which is
why all dots blink in phase and the glyph never tears.

**Recommendation: one 50 ms master clock, all phases derived. Do not add a second timer.**

50 ms is a common divisor of every interval in the spec — 120 (glyph), 200 and 50 (glimmer),
600 (the tool blink from [05-tool-call-rendering](05-tool-call-rendering.md)), 1000 and 2000
(the sine effects), 30 000 (the token threshold). Reconciling with ticket 05 needs **no second
clock**: keep `t` in milliseconds since the live region became busy and derive

```
glyph_frame  = (t / 120) % 12
blink_on     = (t / 600) % 2 == 0
glimmer_step = t / (if requesting { 50 } else { 200 })
flash        = (sin(t as f64 / 1000.0 * PI) + 1.0) / 2.0
```

Ticket 05's 600 ms blink is exactly 12 ticks of this clock, and the glyph's 120 ms is exactly
2 ticks, so both stay perfectly in phase — the property the reference works hard to preserve.

**What it costs when idle.** With ticket 02's `Viewport::Inline`, the spinner is entirely inside
the live region, so a tick only ever causes `terminal.draw` — never `insert_before`, never a
scrollback write. ratatui double-buffers and diffs; an unchanged frame emits **zero bytes**.
The per-tick cost is therefore one wake-up plus a cell diff over `viewport_height × columns`
(order 5 × 120 = 600 cells), 20 times a second. That is negligible, and picopilot pays it
already today over the *whole screen*, which is larger.

Actual terminal output while animating: the glyph changes every 120 ms (~8 writes/s) and the
shimmer band moves every 200 ms (~5 writes/s, 50 ms in `requesting` mode → 20/s), each a
handful of bytes on one row. Under about 25 small writes per second, worst case.

Concrete recommendation:

1. **Keep the existing 50 ms tick as the master clock.** No new timer.
2. **Make `terminal.draw` conditional**, per step 11 of [02-scrollback-mechanism](02-scrollback-mechanism.md):
   redraw when the live region is dirty **or** any animation is active (spinner up, or any
   unresolved tool dot). When idle with no spinner, skip the draw entirely — then the idle cost
   is one `tokio` wake-up per 50 ms and nothing else, which is strictly cheaper than today.
3. **Handle unfocused terminals.** The reference halves its clock rate on blur
   (`ClockContext.tsx:70` — `BLURRED_TICK_INTERVAL_MS = FRAME_INTERVAL_MS * 2`) and pauses
   animations that scroll out of view (`use-animation-frame.ts:36`). picopilot can get the focus
   signal from crossterm's `EnableFocusChange` / `Event::FocusGained` / `Event::FocusLost`;
   recommend dropping to **250 ms** on `FocusLost` and back to 50 ms on `FocusGained`. **Unverified:**
   whether Windows Terminal actually reports focus events to crossterm was not tested. Because
   this is only an optimisation, gate it defensively — if no focus event is ever seen, stay at
   50 ms, which is the current behaviour anyway.
4. **Do not tie animation phase to wall-clock `Instant::now()` per widget.** Compute one `t`
   once per frame and pass it down, exactly as the reference does, or the dots and the glyph
   will drift apart.
5. **Reduced motion:** one setting that freezes `t`, giving a static `●` glyph, no shimmer, no
   flash, no counter animation, and instant stall colour. Everything else still renders.

### 12. picopilot's status bar fields — where each one goes

Current bar, `src/tui.rs:2065-2094`:

```rust
" {project}  ·  {model}  ·  {reasoning} reasoning  ·  autopilot {mode}  ·  tools {}/{}  ·  skills {}/{}  ·  {context} tokens  ·  {cost} "
```

| Field | Verdict | Why |
| --- | --- | --- |
| **elapsed time** *(new)* | **on the line** | `SpinnerAnimationRow.tsx:162`. Not in picopilot today; add it. After 30 s. |
| **tokens** | **on the line, but a different number** | The reference shows *output tokens this turn* (chars ÷ 4), compact-formatted, with a direction arrow. picopilot's `usage.current_tokens / token_limit` is context usage — a different quantity with a denominator the reference never prints. See the deviation note below. |
| **reasoning effort** | **on the line, conditionally** | Only as `effortSuffix` inside `thinking with high effort`, only while thinking (`Spinner.tsx:180`, `effort.ts:195`). Never as a standalone always-on field. |
| **autopilot mode** (`ready`/`working`) | **delete, do not move** | The spinner line's *existence* is the busy indicator. `busy → working` is exactly "the spinner is up". Redundant in both places. |
| **project name** | **off — `/status`** | Never on the reference's spinner line. |
| **model** | **off — `/status`** | Never on the reference's spinner line. See the map contradiction below. |
| **tools n/n** | **off — `/status`** | No equivalent anywhere in the reference spinner. |
| **skills n/n** | **off — `/status`** | Same. |
| **cost** | **off — `/status`** | The reference has only an Anthropic-internal token-budget line (`Spinner.tsx:262-278`), rendered *below* the spinner, not on it. Not cost, and not shipped. |

**Two things the human should know.**

1. **The map contradicts the reference on the model.** `map.md` says *"model and token counts
   ride on the spinner line"*. Token counts do. **The model does not** — it appears nowhere in
   `SpinnerAnimationRow`. The reasoning effort does, but only inside the thinking text. If
   picopilot wants the model always visible, that is a deliberate addition, not parity. My
   recommendation is to follow the reference and send the model to `/status`.
2. **Deviation to decide: which token number.** The reference's per-turn output-token estimate
   needs picopilot to accumulate streamed characters per turn, which it does not do today; it
   has context `current/limit` from `usage` instead. Options: (a) implement the per-turn counter
   and match exactly; (b) show `{context}` used/limit in the same slot and accept a documented
   deviation. Both are defensible; (b) is arguably more useful and cheaper. Either way it is a
   one-field decision, and I am flagging it rather than deciding it.

**Leftovers for the `/status` ticket:** project name, model, tools n/n, skills n/n, cost, and —
if option (b) above is not taken — context tokens vs limit. `autopilot mode` goes nowhere; it is
deleted.

### 13. Unverified

- Nothing was executed. No `bun`, no `node_modules`, no `claude` binary — same standing risk as
  the rest of this map.
- The `SpinnerMode` union is a stub in the leaked `src/components/Spinner/types.ts:3`. The five
  members used here — `requesting`, `tool-input`, `tool-use`, `responding`, `thinking` — were
  recovered from the `switch` at `SpinnerAnimationRow.tsx:232-262` and the comparisons at
  `:132,139`. The union may have more members that render identically.
- `figures.arrowUp` / `figures.arrowDown` glyph values (§8).
- Whether the reduced-motion glyph pulse is truly dead in this call path (§2) — the reading is
  clear, but only a running binary settles it.
- One verb per turn (§3) depends on the spinner unmounting between turns, inferred from
  `showSpinner &&` at `REPL.tsx:4587`.
- Whether crossterm focus events arrive on Windows Terminal (§11 step 3).
