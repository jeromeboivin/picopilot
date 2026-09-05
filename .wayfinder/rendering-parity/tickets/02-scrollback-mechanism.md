---
label: wayfinder:research
name: Settle the scrollback rendering mechanism
status: closed
assignee: research-subagent
blocked_by: []
---

# Settle the scrollback rendering mechanism

## Question

How does Claude Code split **committed history** from the **live region**, and what is the
ratatui/crossterm equivalent that picopilot must adopt?

Moving off the alternate screen is settled, but the mechanism is not, and nine other tickets
depend on knowing where the boundary sits. In particular: once a line is committed to the
user's scrollback it can never be redrawn, so "what is still live" is the load-bearing
question for the spinner, the input box, streaming assistant text and inline pickers.

Resolve by producing:

- How the reference actually commits history. The charting exploration reported "a single
  React tree, no Static/dynamic split", which contradicts how scrollback normally works in
  Ink — settle this from the source and say plainly which it is.
- What lives in the live region at each moment, and the exact event that promotes a message
  from live to committed.
- The ratatui/crossterm equivalent: evaluate `Viewport::Inline` and `Terminal::insert_before`,
  and state whether they can express the same model. Include their known limits.
- Behaviour on terminal resize, and on output arriving from a subprocess while the live region
  is drawn.
- Windows Terminal specifics, since that is the primary platform.
- Anything the reference does that ratatui cannot express, with the closest achievable
  alternative.

## Resolution

Settled from source. Reference read at `C:\dev\git\claude-code` (TypeScript, self-hosted Ink
fork under `src/ink/`). ratatui/crossterm facts read from the ratatui 0.29.0 sources published
on docs.rs, which is the version pinned in `Cargo.toml`. Nothing here was run; see
"Unverified" at the end for what that leaves open.

### 1. The contradiction is settled: there is no Static/dynamic split

**The charting exploration was right. Claude Code has one React tree and no `<Static>`.**

Three independent confirmations:

1. `package.json` has **no `ink` dependency**. It depends on `react-reconciler` directly and
   ships its own Ink fork in `src/ink/` (98 files). There is therefore no upstream `<Static>`
   to inherit.
2. There is **no `Static.tsx`** in `src/ink/components/`, and no `Static` export anywhere in
   `src/ink/`. The only hits for the word in the whole repo are prose comments.
3. One of those comments is the trap. `src/components/messages/UserPromptMessage.tsx:25` says
   *"Non-fullscreen avoids this via `<Static>` (print-and-forget to terminal scrollback)"*.
   That comment is stale — it describes upstream Ink's model, not this code. Anyone grepping
   for "Static" lands on it and concludes the split exists. It does not.

**What Claude Code actually does instead.** The React tree renders into a virtual `Screen`
(`src/ink/screen.ts`) whose height is the height of *the entire rendered document*, which is
routinely much taller than the terminal. That screen is written to the **main screen buffer**,
not the alternate screen. Every frame, `LogUpdate.render(prev, next, altScreen, decstbmSafe)`
in `src/ink/log-update.ts` diffs the previous screen against the new one and emits a patch list
of relative cursor moves and writes. New rows at the bottom are emitted by
`renderFrameSlice()`, which advances with `CR`+`LF` rather than `CSI CUD`:

```ts
// Advance cursor to this row using LF (not CSI CUD / cursor-down).
// CSI CUD stops at the viewport bottom margin and cannot scroll,
// but LF scrolls the viewport to create new lines.
```

So growth is written with newlines, the terminal scrolls itself, and the top of the document
falls off the top of the viewport into the user's real scrollback. **Committing to scrollback
is a side effect of the terminal scrolling, not an action the app takes.**

**Main screen is the shipped default.** `src/utils/fullscreen.ts:112`:

```ts
export function isFullscreenEnvEnabled(): boolean {
  if (isEnvDefinedFalsy(process.env.CLAUDE_CODE_NO_FLICKER)) return false
  if (isEnvTruthy(process.env.CLAUDE_CODE_NO_FLICKER)) return true
  if (isTmuxControlMode()) { /* … */ return false }
  return process.env.USER_TYPE === 'ant'
}
```

`USER_TYPE === 'ant'` is Anthropic-internal. For everyone else this is false, `REPL.tsx` does
not wrap the tree in `<AlternateScreen>`, and the app runs on the main screen with native
scrollback. The alt-screen path (`<AlternateScreen>` → `FullscreenLayout` →
`VirtualMessageList` → `ScrollBox` with DECSTBM scroll optimisation) is the internal variant.
`<AlternateScreen>` is otherwise only used for transient overlays (`ctrl+o` transcript) and
external-editor handoff (`Ink.enterAlternateScreen()` in `editor.ts`, `promptEditor.ts`,
`terminalPanel.ts`).

The map's settled choice — scrollback, not alternate screen — therefore matches what the
public product does. Good.

### 2. What is live, and the exact promotion event

**There is no named live region.** The live region is *the last `terminal.rows` lines of the
rendered document*, whatever they happen to contain. Everything above that is committed.

The boundary is computed, not declared. `log-update.ts` derives it every frame:

```ts
const viewportY = growing
  ? Math.max(0, prev.screen.height - prev.viewport.height + cursorRestoreScroll)
  : Math.max(prev.screen.height, next.screen.height) - next.viewport.height + cursorRestoreScroll;
```

`viewportY` is "how many rows are already in scrollback". The diff loop then enforces the rule:

```ts
// If the cell outside the viewport range has changed, we need to reset
// because we can't move the cursor there to draw.
if (y < viewportY) { needsFullReset = true; resetTriggerY = y; return true }
```

**The promotion event is: the document grew taller than the screen, so the LFs that drew the
new rows scrolled old rows off the top.** Nothing else. A message is not "finished" or
"sealed"; it stops being addressable the moment it leaves the viewport. Consequences:

- A tool call still on screen can be repainted freely from `running` to `done` — it is live.
- The same tool call three screens up cannot be repainted. It is committed.
- Whether a given message is live depends only on how much has been printed since, and on the
  terminal height. It is not a property of the message.

**The escape hatch, and its price.** When a change does land above `viewportY`, Claude Code
falls back to `fullResetSequence_CAUSES_FLICKER(frame, reason, stylePool, debug)` — note the
deliberately alarming name. It emits a `clearTerminal` patch plus a from-scratch render of the
whole frame. `clearTerminal` is defined in `src/ink/clearTerminal.ts` and on a modern terminal
is `ERASE_SCREEN + ERASE_SCROLLBACK + CURSOR_HOME` — **it wipes the user's scrollback and
reprints the (capped) document**. `FlickerReason` is `'resize' | 'offscreen' | 'clear'`, and
these are counted per frame in `FrameEvent.flickers`, i.e. they are treated as a defect to be
measured and driven down, not a normal path.

**The memory-level commit is separate and deliberately sticky.** `src/components/Messages.tsx`
caps the non-virtualised tree at 200 messages, and the comment is the clearest statement of the
whole model:

```
// Content dropped from this slice has already been printed to terminal scrollback — users
// can still scroll up natively.
```

The slice start is anchored by **UUID**, not by count, and only advances when the count exceeds
`cap + step` (200 + 50):

```
// Count-based slicing (slice(-200)) drops one message from the front on every append,
// shifting scrollback content and forcing a full terminal reset per turn (CC-941).
```

That is the load-bearing lesson for picopilot: **any change to what occupies row 0 of your
document forces a scrollback-destroying repaint.** The window must be quantised and anchored so
it almost never moves.

### 3. Subprocess output, resize, and Windows

**Subprocess output never reaches the terminal directly.** Grepping `process.stdout.write`
across `src/` excluding `src/ink/` finds only CLI subcommands (`auth`, `mcp`, `print`), the
suspend notice, and instrumentation wrappers (`asciicast.ts`, `streamJsonStdoutGuard.ts`).
Bash/tool output is captured, turned into messages, and rendered inside the frame. The only
things that write around the renderer are full handoffs (`enterAlternateScreen()` for an
external editor), which pause Ink entirely and repaint on return.

**Resize.** `Ink.handleResize` is explicitly *not* debounced (comment: a debounce creates a
window where `stdout.columns` is new but Yoga is old, producing a double clear). It updates
dimensions synchronously and re-renders. `log-update.ts` then takes the blunt path:

```ts
if (next.viewport.height < prev.viewport.height ||
    (prev.viewport.width !== 0 && next.viewport.width !== prev.viewport.width)) {
  return fullResetSequence_CAUSES_FLICKER(next, 'resize', stylePool)
}
```

Any width change, or any height shrink, is a full clear + repaint. The reasoning is in the
comment: predicting the post-resize wrap would mean re-wrapping everything, and resize is rare
enough not to be worth it. **Reflowing already-committed scrollback is not attempted by anyone
— the terminal owns those rows.**

**SIGCONT.** `handleResume` resets both frame buffers to `emptyFrame` and clears
`displayCursor`, so the next frame starts fresh rather than emitting relative moves from a
cursor position the shell has since moved.

**Windows Terminal — the important finding.** `src/ink/terminal.ts:177`:

```ts
/** True if the terminal scrolls the viewport when it receives cursor-up
 *  sequences that reach above the visible area. On Windows, conhost's
 *  SetConsoleCursorPosition follows the cursor into scrollback
 *  (microsoft/terminal#14774), yanking users to the top of their buffer
 *  mid-stream. WT_SESSION catches WSL-in-Windows-Terminal where platform
 *  is linux but output still routes through conhost. */
export function hasCursorUpViewportYankBug(): boolean {
  return process.platform === 'win32' || !!process.env.WT_SESSION
}
```

And in `REPL.tsx:1463`:

```ts
const showStreamingText = !reducedMotion && !hasCursorUpViewportYankBug();
```

**On Windows and Windows Terminal, Claude Code turns live token-by-token streaming off
entirely.** The feature that most needs frequent in-place repaint of the bottom region is the
one they disabled on our primary platform, because upward cursor motion near the top of the
buffer teleports the user's view. Any picopilot design that repaints upward on Windows inherits
this bug.

Other Windows facts, all from `src/ink/`:

- `ERASE_SCROLLBACK` (`CSI 3J`) is only sent when Windows Terminal (`WT_SESSION`), VS Code's
  integrated terminal, or mintty is detected; legacy conhost gets `ERASE_SCREEN` + `CSI 0f`
  only, so on legacy conhost a "full reset" leaves duplicated history behind.
- DEC 2026 synchronized output (BSU/ESU) **is** supported under `WT_SESSION`, so atomic frame
  updates are available on Windows Terminal.
- Windows Terminal is excluded from OSC 9;4 progress reporting (it treats it as notifications).
- `windows-terminal` is on the extended-keys allowlist (Kitty protocol / modifyOtherKeys).

### 4. The ratatui 0.29 equivalent — what it can and cannot express

Real signatures, from `ratatui-0.29.0/src/terminal/terminal.rs` and `viewport.rs`:

```rust
pub enum Viewport {
    Fullscreen,
    Inline(u16),
    Fixed(Rect),
}

pub struct TerminalOptions {
    pub viewport: Viewport,
}

impl<B: Backend> Terminal<B> {
    pub fn with_options(backend: B, options: TerminalOptions) -> io::Result<Self>;
    pub fn insert_before<F>(&mut self, height: u16, draw_fn: F) -> io::Result<()>
    where
        F: FnOnce(&mut Buffer);
    pub fn draw<F>(&mut self, render_callback: F) -> io::Result<CompletedFrame>
    where
        F: FnOnce(&mut Frame);
    pub fn autoresize(&mut self) -> io::Result<()>;
    pub fn resize(&mut self, area: Rect) -> io::Result<()>;
}
```

Docs for `Viewport::Inline(u16)`: *"The viewport's height is fixed and specified in number of
lines. The width is the same as the terminal's width. The viewport is drawn below the cursor
position."*

Docs for `insert_before`: *"Insert some content before the current inline viewport. This has no
effect when the viewport is not inline. … If more lines are inserted than there is space on the
screen, then the top lines will go directly into the terminal's scrollback buffer. At the limit,
if the viewport takes up the whole screen, all lines will be inserted directly into the
scrollback buffer."*

**Can it express the model? Yes — but it expresses a stricter, better-behaved version of it.**

ratatui gives an *explicit* commit call where Claude Code has an implicit one. `insert_before`
is exactly "promote these N lines to history, forever". That is a real improvement: the
boundary becomes an API call instead of an emergent property of terminal height, and the
"changed a row above `viewportY`" failure mode is structurally impossible because ratatui never
lets you address those rows at all.

**What it can express**

- Committed history that ratatui never touches again, and that the terminal owns for
  selection, native scroll and copy.
- A live region of fixed height at the bottom, drawn with normal `terminal.draw` and normal
  widgets, diffed as usual.
- Arbitrarily tall commits. The non-scrolling-regions implementation loops
  (`while buffer_height + viewport_height > screen_height`), drawing a screenful at a time and
  scrolling. Committing 500 lines in one call is supported.

**What it cannot express — the five real limits**

1. **The inline viewport height is fixed for the life of the `Terminal`.** `viewport: Viewport`
   is a private field set only in `with_options`; `resize()` reads `Viewport::Inline(height)`
   back out of it. **There is no public API in 0.29 to change it.** picopilot's live region is
   not fixed height — the input box grows with multi-line input, permission prompts and inline
   pickers are taller than the prompt. Options: (a) size the viewport for the worst case and
   bottom-align, wasting rows; (b) drop and rebuild the `Terminal` on height change, which is
   observable; (c) reserve a generous height and accept blank rows. See "recommended
   architecture".
2. **`insert_before` needs the height up front**, as a `u16`, before `draw_fn` runs. You must
   compute the wrapped line count of a message yourself before rendering it. Overshoot leaves
   blank committed rows you can never reclaim; undershoot silently truncates —
   `draw_lines` does `cells.split_at(width * lines_to_draw)` on a buffer sized to `height`, so
   content past `height` is simply never in the buffer.
3. **`insert_before` is a no-op on any viewport other than `Inline`** (`_ => Ok(())`). It fails
   silently. Easy to lose an afternoon to.
4. **The default implementation ends with `self.clear()`**, and for an inline viewport `clear()`
   is `set_cursor_position(viewport top)` + `ClearType::AfterCursor` + reset of the back buffer.
   So **every commit forces a full repaint of the live region on the next `draw`.** With a
   spinner committing lines during streaming this is a lot of redundant output. The
   `scrolling-regions` cargo feature replaces this with DECSTBM (`CSI top;bot r` + `CSI n S/T`)
   and avoids the viewport redraw entirely — the same optimisation Claude Code applies in its
   alt-screen path. It is **off by default** in 0.29.
5. **Resize clears and repaints the live region, and cannot reflow committed history.**
   `autoresize()` → `resize(area)` → `compute_inline_size(...)` → `set_viewport_area` →
   `clear()`. `compute_inline_size` itself calls `backend.append_lines(lines_after_cursor)`,
   which for `CrosstermBackend` is literally `Print("\n")` repeated — so a resize can push more
   rows into scrollback. Committed rows are re-wrapped by the terminal, not by us, exactly as in
   the reference.

**A Windows-relevant detail in ratatui's favour.** The default `scroll_up` path is:

```rust
fn scroll_up(&mut self, lines_to_scroll: u16) -> io::Result<()> {
    if lines_to_scroll > 0 {
        self.set_cursor_position(Position::new(0, self.last_known_area.height.saturating_sub(1)))?;
        self.backend.append_lines(lines_to_scroll)?;
    }
    Ok(())
}
```

and `CrosstermBackend::append_lines` is `for _ in 0..n { queue!(self.writer, Print("\n"))? }`.
It scrolls with newlines from the *bottom* row and never moves the cursor above the viewport —
so it does not trip `microsoft/terminal#14774`. This is the same LF-not-CUD trick the reference
uses, and it is why the default path is the safer one on Windows.

**A Windows-relevant detail against `scrolling-regions`.** `ScrollUpInRegion` /
`ScrollDownInRegion` have:

```rust
#[cfg(windows)]
fn execute_winapi(&self) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported,
        "ScrollUpInRegion command not supported for winapi"))
}
```

On ANSI-capable Windows Terminal / ConPTY crossterm takes the ANSI path and this never fires,
but on a legacy console falling back to the WinAPI path it is a hard error. Enabling
`scrolling-regions` is therefore a Windows risk that must be tested, not assumed.

### 5. Recommended architecture for picopilot

Today `src/tui.rs:1512-1521` does `enable_raw_mode()` → `execute!(stdout, EnterAlternateScreen,
EnableBracketedPaste)` → `Terminal::new(backend)` (which is `Viewport::Fullscreen`), and
`run_loop` at line 1569 calls `terminal.draw(|frame| draw(frame, &app))` unconditionally at the
top of every iteration of a `tokio::select!` loop with a 50 ms tick. Every frame repaints
status bar, transcript `Paragraph`, input box and shortcut bar. `restore_terminal` at line 2052
leaves the alternate screen.

Proposed replacement:

1. **Drop `EnterAlternateScreen` / `LeaveAlternateScreen` entirely.** Keep `enable_raw_mode`
   and `EnableBracketedPaste`.
2. **Build the terminal with an inline viewport:**
   `Terminal::with_options(backend, TerminalOptions { viewport: Viewport::Inline(h) })`.
3. **Introduce an explicit two-part state.** A `history: Vec<Committed>` that is append-only and
   write-once, and a `live: LiveRegion` that is re-rendered every frame. Only `live` may change.
4. **Make the promotion explicit and one-way.** When a message reaches a terminal state, render
   it to `Vec<Line>` against the current width, `insert_before(lines.len() as u16, …)`, and move
   it into `history`. After that it is unreachable — enforce that in the type, not by
   convention. This is the single most important structural change; it turns Claude Code's
   emergent boundary into a compile-time one.
5. **Solve the variable-height live region by making it fixed and internally laid out.** Pick
   `Inline(h)` where `h` covers the tallest routine live state (spinner line + input box +
   hint line, plus room for a short picker), bottom-align inside it, and for genuinely tall
   surfaces (long permission prompts, big pickers) **scroll them inside the live region** rather
   than growing it. This avoids rebuilding the `Terminal`, and matches the map's "modals become
   inline pickers" decision. If a resizable live region proves necessary later, the fallback is
   to recreate the `Terminal` on height change — expect a visible repaint each time.
6. **Stream by committing whole lines, not by repainting.** Hold the in-flight assistant text in
   the live region only up to the last newline (the reference does exactly this:
   `streamingText.substring(0, streamingText.lastIndexOf('\n') + 1)`), and `insert_before` each
   completed line. Partial trailing text stays live. This gives streaming that only ever moves
   downward.
7. **On Windows, follow the reference and default streaming preview off.** Gate on
   `cfg!(windows) || env::var_os("WT_SESSION").is_some()`, matching `hasCursorUpViewportYankBug`.
   Commit assistant text per line without a live preview there. Make it an opt-in setting.
8. **Handle resize by repainting only the live region.** ratatui does this for you; do not try
   to re-commit or reflow history. Accept that committed rows re-wrap however the terminal
   wraps them. This is what the reference accepts too.
9. **Compute commit height with the same wrapper used to render.** One function that turns a
   message + width into `Vec<Line>`; `insert_before` gets `lines.len()`, `draw_fn` renders those
   exact lines. Never compute the height a second way.
10. **Keep `scrolling-regions` off initially.** Revisit only if the per-commit live-region
    repaint measurably hurts, and only with real Windows Terminal and legacy-conhost testing.
11. **Drive the loop from events, not from a 50 ms unconditional `draw`.** With commits being
    permanent, a redundant `draw` is cheap but a redundant commit is not; the loop should draw
    when the live region is dirty and commit only on state transitions.

**Things that disappear, and are net wins:** picopilot's own scroll offset (the terminal owns
scrolling now), the status bar and shortcut bar (already slated for deletion), and centered
modal overlays (already slated to become inline pickers).

### 6. Risks

- **Commit height miscalculation is unrecoverable.** A wrong `height` leaves permanent blank
  rows or permanently truncated output in the user's scrollback. Highest-severity risk; mitigate
  with the single-wrapper rule (step 9) and property tests over widths.
- **Fixed inline viewport vs. variable live content.** If the chosen `h` turns out too small for
  common surfaces the design degrades into internal scrolling everywhere, which is worse than
  today's modals. This should be prototyped before the spec commits to a number.
- **Windows cursor-up viewport yank (`microsoft/terminal#14774`).** The reference disables live
  streaming on Windows rather than fight it. If picopilot ships streaming preview on Windows by
  default it will likely reproduce the "view jumps to the top of the buffer" bug on our primary
  platform.
- **Legacy conhost.** No `CSI 3J`, and `scrolling-regions` would hard-error on the WinAPI path.
  Any "clear and repaint" recovery behaves differently there.
- **Loss of retroactive correction.** Once a tool result is committed it can never be amended.
  Anything that updates after completion (token counts, durations, late errors) must either stay
  live longer or be re-emitted as a new line. Worth an inventory pass across message types.
- **`insert_before` silently no-ops on the wrong viewport.** A future refactor that switches to
  `Fullscreen` for any reason makes all history vanish with no error.
- **Very long sessions.** Every committed line is bytes written once and never revisited, so the
  reference's GC death spiral does not apply — but a large paste or a huge tool result becomes
  one enormous `insert_before` that loops a screenful at a time. Chunking may be needed.
- **Terminals that reflow scrollback on resize** will re-wrap committed lines with the width
  they were rendered at, so long lines may look wrong after a narrow→wide resize. Unavoidable in
  this model; the reference accepts it.

### 7. Unverified

Flagged honestly, per the map's standing risk:

- **Nothing here was observed rendering.** There is no `bun`, no `node_modules` and no `claude`
  binary on this machine. Every claim about Claude Code is read from source.
- **`insert_before` behaviour on Windows Terminal was not executed.** The LF-based `scroll_up`
  reasoning is from reading `CrosstermBackend::append_lines`, not from a run.
- **Whether ratatui ≥ 0.30 adds a way to change the inline viewport height at runtime was not
  checked.** The "fixed for the life of the `Terminal`" claim is verified for 0.29.0 only.
- **`microsoft/terminal#14774`** is cited because the reference's own comment cites it; the
  issue itself was not opened.
- **The exact chosen inline viewport height** is deliberately left open — it depends on the
  input-box and picker specs, which are other tickets.
- **The `scrolling-regions` feature was not exercised**, on any platform.
