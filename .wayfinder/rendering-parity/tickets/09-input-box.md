---
label: wayfinder:research
name: Specify the input box and hint line
status: closed
assignee: research-subagent
blocked_by: [02-scrollback-mechanism]
---

# Specify the input box and hint line

## Question

What does the prompt area look like, including its border, its mode indicators and the hint
line beneath it?

Resolve by producing:

- The border: style and glyphs quoted verbatim, the palette key, and any shimmer or focus
  animation.
- The prompt character or its absence, and how the cursor is presented.
- Mode indicators — vim, plan, fast, brief and any others — with their glyphs, wording and
  placement.
- Placeholder text when the input is empty.
- The hint line beneath the border: its exact content, separators, and when each hint appears
  or disappears.
- Multi-line input: how the box grows, whether it caps, and what happens past the cap.
- How the completion or slash-command list is presented relative to the box.
- What picopilot has today — top and bottom borders only, a `"  ❯ "` prompt, an eight-line
  cap, an orange-bordered completion dropdown, and a bottom shortcut bar that is being deleted
  — and the specific gaps.

## Resolution

Settled from source. Reference read at `C:\dev\git\claude-code` (TypeScript, self-hosted Ink
fork under `src/ink/`). The shipped public build is the **non-fullscreen / scrollback** path
(`isFullscreenEnvEnabled()` is false for `USER_TYPE !== 'ant'` — see
[02-scrollback-mechanism](02-scrollback-mechanism.md) §1), so every "shipped" claim below is
the `isFullscreenEnvEnabled() === false` branch. Where the two branches differ it is called
out, because the fullscreen branch is the one that has already solved our fixed-height problem
and is the branch picopilot should copy. Nothing was executed; see §11.

Reference line numbers are for the files as they exist on disk at `C:\dev\git\claude-code`.

---

### 1. The prompt area, top to bottom

The whole area is one React `Box flexDirection="column" marginTop={1}`
(`src/components/PromptInput/PromptInput.tsx:2261`). `marginTop={1}` is **one blank row above
the top rule** — always, except in brief mode where `briefOwnsGap` moves the gap into the
spinner. Rows, in order:

```
                                    ← blank gap row (marginTop=1)
────────────────────────────────────  top rule, full terminal width
❯ what should we build               ← prompt char + input, grows downward
────────────────────────────────────  bottom rule, full terminal width
  ⏸ plan mode on (shift+tab to cycle) · esc to interrupt
  ↑ two-space indent (paddingX={2})
```

There is no left or right border and no padding inside the box, so the prompt character sits
at **column 0** and input text starts at **column 2**. The hint line below is indented **2
columns**, and so is the completion list.

### 2. The border

`PromptInput.tsx:2268` (idle/normal case) and `:2237` (external-editor case):

```tsx
<Box flexDirection="row" alignItems="flex-start" justifyContent="flex-start"
     borderColor={getBorderColor()} borderStyle="round"
     borderLeft={false} borderRight={false} borderBottom width="100%"
     borderText={buildBorderText(...)}>
```

- **Style:** `"round"` from the `cli-boxes` package (`package.json:42`, `cli-boxes@^3.0.0`).
- **Glyphs actually drawn:** only `box.top` and `box.bottom`, both `─` (U+2500 BOX DRAWINGS
  LIGHT HORIZONTAL). The corners `╭ ╮ ╰ ╯` and the sides `│` are **never emitted**, because
  `src/ink/render-border.ts:118-130` gates them on `showLeftBorder`/`showRightBorder`:
  `topBorderLine = (showLeftBorder ? box.topLeft : '') + box.top.repeat(contentWidth) + (showRightBorder ? box.topRight : '')`,
  and `contentWidth = width - (showLeftBorder?1:0) - (showRightBorder?1:0)` (`:120`) is
  therefore the full width. So each rule is exactly `'─'.repeat(terminal_columns)`.
- **Palette key:** `promptBorder`. Dark value `rgb(136,136,136)` (`src/utils/theme.ts:451`,
  matching [01-dark-palette](01-dark-palette.md) row 11). Chosen by `getBorderColor()`
  (`PromptInput.tsx:2214-2233`): `bash` mode → `bashBorder` `rgb(253,93,177)`
  (`theme.ts:442`); an agent-swarm teammate → that teammate's color; otherwise `promptBorder`.
- **No shimmer, no focus animation.** `promptBorderShimmer` is confirmed dead — the only hits
  in the whole repo are the five `theme.ts` declarations/values (lines 16, 127, 209, 290, 371,
  452, 533). This confirms [01-dark-palette](01-dark-palette.md)'s "5 dead keys" finding, and
  **corrects one claim in it**: that ticket lists `promptBorder` as used by `PromptInput.tsx`,
  but `PromptInput.tsx` never names the key — it returns the string from `getBorderColor()`.
  The literal `'promptBorder'` appears only in `src/components/FastIcon.tsx:20,42`,
  `src/utils/analyzeContext.ts:1015,1155,1270`, and `theme.ts`. The border still resolves to
  `promptBorder`; the citation in ticket 01 is just imprecise.
- **The border is not dim.** No `borderDimColor` is passed, so `styleBorderLine`
  (`render-border.ts:67-76`) applies the color only.

**Text embedded in the top rule.** `borderText` (`render-border.ts:16-21, 129-146`) splices a
pre-rendered string into the rule, replacing rule characters — it does not add a row. Only one
producer exists, `buildBorderText` (`PromptInput.tsx:2328-2338`):

```tsx
if (!showFastIcon) return undefined;
const fastSeg = showFastIconHint
  ? `${getFastIconString(true, fastModeCooldown)} ${chalk.dim('/fast')}`
  : getFastIconString(true, fastModeCooldown);
return { content: ` ${fastSeg} `, position: 'top', align: 'end', offset: 0 };
```

So in fast mode the top rule ends `─── ↯ ───` (or `─── ↯ /fast ───` for the first 5 s of a
session). The glyph is `↯` (U+21AF, `LIGHTNING_BOLT` in `src/constants/figures.ts:9`), colored
`fastMode` `rgb(255,120,20)` (`theme.ts:497`), or dim `promptBorder` during cooldown
(`FastIcon.tsx:35-43`). The `/fast` label shows **once per process**, for
`HINT_DISPLAY_DURATION_MS = 5000` (`useShowFastIconHint.ts:3, 10-27`).

**One alternate skin.** When an agent-swarm banner is active (`PromptInput.tsx:2251-2266`) the
two rules are replaced by hand-drawn `'─'.repeat(columns)` in the swarm's color, with an
inverse-video label spliced into the top one. Out of scope for picopilot.

### 3. Prompt character and cursor

`src/components/PromptInput/PromptInputModeIndicator.tsx:51` renders the prompt char:

```tsx
<Text color={color} dimColor={isLoading}>{figures.pointer} </Text>
```

- Glyph: `figures.pointer`, i.e. **`❯`** (U+276F) — `figures@^6.1.0` (`package.json:50`)
  substitutes `>` on terminals without Unicode support. Followed by **one space**; total width
  2. Column 0.
- **No color of its own** — `color` is `undefined` unless a teammate color applies
  (`:50-51`), so it inherits the default foreground. It is **dim while the agent is working**
  (`dimColor={isLoading}`). That dimming is the only "focus" signal the box has.
- **Bash mode replaces it**: `<Text color="bashBorder" dimColor={isLoading}>! </Text>`
  (`PromptInputModeIndicator.tsx:77`) — a `!` in the same pink as the rules.
- The indicator `Box` is `alignSelf="flex-start"`, so on multi-line input the `❯ ` appears
  **only on the first row**; continuation rows are blank in columns 0-1 and text still starts
  at column 2 (it is inside the sibling `flexGrow={1}` box).

**Cursor.** Software-drawn, as reverse video on the character under it — not a real terminal
cursor. `src/components/TextInput.tsx:90` sets `invert = chalk.inverse`, `:106` sets
`cursorChar: props.showCursor ? ' ' : ''` (a space, so end-of-line shows an inverted blank),
and `Cursor.render(cursorChar, mask, invert, ...)` (`src/utils/Cursor.ts:202-208`) applies it.
There is no blink. `showCursor` is false while a footer item is selected, while ctrl-r history
search is open, or when the cursor sits on an image chip (`PromptInput.tsx:2201`).

**Width reserved for text:** `textInputColumns = columns - 3` (`PromptInput.tsx:1991`), and
`Cursor.fromText` wraps at `columns - 1` on top of that (`Cursor.ts:169`) to leave room for
the cursor cell.

### 4. Mode indicators

Two places. **Inside the box** (column 0) only `❯` vs `!` — §3. Everything else is on the
**hint line**, from `src/components/PromptInput/PromptInputFooterLeftSide.tsx`.

**Permission mode** (`PromptInputFooterLeftSide.tsx:355-363`):

```tsx
<Text color={getModeColor(currentMode)} key="mode">
  {permissionModeSymbol(currentMode)}{' '}
  {permissionModeTitle(currentMode).toLowerCase()} on
  {shouldShowModeHint && <Text dimColor> <KeyboardShortcutHint shortcut={modeCycleShortcut} action="cycle" parens /></Text>}
</Text>
```

From `src/utils/permissions/PermissionMode.ts:41-88`, verbatim:

| mode | symbol | rendered text | palette key | dark value |
|---|---|---|---|---|
| `default` | `''` | *(hidden — `isDefaultMode` suppresses the whole part)* | `text` | — |
| `plan` | `⏸` (U+23F8, `PAUSE_ICON`, `figures.ts:17`) | `⏸ plan mode on` | `planMode` | `rgb(72,150,140)` |
| `acceptEdits` | `⏵⏵` (U+23F5 ×2) | `⏵⏵ accept edits on` | `autoAccept` | `rgb(175,135,255)` |
| `bypassPermissions` | `⏵⏵` | `⏵⏵ bypass permissions on` | `error` | — |
| `dontAsk` | `⏵⏵` | `⏵⏵ don't ask on` | `error` | — |
| `auto` (ant-only) | `⏵⏵` | `⏵⏵ auto mode on` | `warning` | — |

The trailing hint is `(shift+tab to cycle)` in dim, and is **dropped when two or more
"primary" items are already on the line** (`shouldShowModeHint = primaryItemCount < 2`,
`:337`). The mode part is hidden entirely in remote mode (`:355`).

**Vim** (`PromptInputFooterLeftSide.tsx:189-191`):

```tsx
showVim ? <Text dimColor key="vim-insert">-- INSERT --</Text> : null
```

- Exact text `-- INSERT --`, dim, and it **replaces the rest of the left side** — when it
  shows, `showHint` is forced false (`:194`: `const t4 = !suppressHint && !showVim`).
- Shown only for `vimMode === 'INSERT'` and only when vim mode is enabled and ctrl-r search is
  not active. **There is no `-- NORMAL --`**; NORMAL mode shows the ordinary hint line. This is
  the only `-- INSERT --` occurrence in the repo.

**Bash** (`PromptInputFooterLeftSide.tsx:319-321`): the entire left side becomes
`<Text color="bashBorder">! for bash mode</Text>` — an early return, so no other hint appears.

**Fast** — in the top rule, not the hint line. §2.

**Brief** — no glyph. `briefOwnsGap` only moves the blank gap row from the prompt to the
spinner (`PromptInput.tsx:2261`); rendering of brief mode itself belongs to the spinner ticket.

### 5. Placeholder

`src/hooks/renderPlaceholder.ts:26-42`, driven by
`src/components/PromptInput/usePromptInputPlaceholder.ts`:

- Shown only when `value.length === 0` **and** a placeholder string exists (`:45`).
- Rendered `chalk.dim(placeholder)` — dim, no color. With the cursor on, the **first character
  is inverse and the rest dim**: `invert(placeholder[0]) + chalk.dim(placeholder.slice(1))`
  (`:36-38`). Empty placeholder → `invert(' ')`.
- The string, in priority order (`usePromptInputPlaceholder.ts:32-66`):
  1. viewing a teammate → `` `Message @${name}…` `` (name truncated to 20 chars with `...`);
  2. queued commands exist and the user has seen the hint fewer than 3 times →
     `Press up to edit queued messages`;
  3. first prompt of the session (`submitCount < 1`) and suggestions enabled →
     `getExampleCommandFromCache()`, a rotating example task;
  4. otherwise **no placeholder at all** — the box is simply empty.

So in steady state the empty box shows nothing but `❯ ` and an inverted blank cursor.

### 6. The hint line

One row (sometimes two), directly under the bottom rule, `paddingX={2}`, laid out
`space-between` — left group and right group — collapsing to a stacked column below 80
columns (`src/components/PromptInput/PromptInputFooter.tsx:135-150`, `isNarrow = columns < 80`
at `:104`).

**Separator: `" · "` (space, U+00B7 MIDDLE DOT, space), always dim.** Two implementations,
identical output:
- Between the parts inside the byline: `src/components/design-system/Byline.tsx:74`
  `{index > 0 && <Text dimColor> · </Text>}`.
- Between the three fixed groups: `PromptInputFooterLeftSide.tsx:473` and `:477`
  `<Text dimColor> · </Text>`.

**Hint phrasing: `"{shortcut} to {action}"`**, optionally parenthesised.
`src/components/design-system/KeyboardShortcutHint.tsx:64` → `<Text>({shortcut} to {action})</Text>`,
`:73` → `<Text>{shortcut} to {action}</Text>`. Shortcut strings come from the keybinding
registry (`useShortcutDisplay`) with the defaults given below, so a rebound key changes the
text automatically.

**Left group order** (`PromptInputFooterLeftSide.tsx:465-483`):
`[permission mode] · [background-tasks pill] · [byline of remaining parts]`.

**Every hint, with its rule:**

| text | when it shows |
|---|---|
| `Press {key} again to exit` | `exitMessage.show` — first ctrl-c / ctrl-d press. Early return, hides everything else (`:145-155`) |
| `Pasting text…` | during a bracketed paste. Early return (`:157-165`) |
| `-- INSERT --` | vim INSERT. Suppresses all hints (`:191`) |
| `! for bash mode` | `mode === 'bash'`. Early return (`:319-321`) |
| `⏸ plan mode on` etc. | non-default permission mode, not remote (`:355-363`) |
| `(shift+tab to cycle)` | appended to the above when fewer than 2 primary items (`:337`) |
| `esc to interrupt` | `isLoading` (`:506-508`) |
| `ctrl+x ctrl+k to stop agents` | not loading, agent tasks running, kill-confirm not up (`:509-511`) |
| `ctrl+t to show tasks` / `show teammates` / `hide tasks` / `hide` | task items or teammates exist; label cycles with `expandedView` (`:487-500, 512-514`) |
| `↓ to manage` / `Enter to view tasks` | tasks pill present, no teams (`:451-455`) |
| `esc to return to team lead` | viewing a finished teammate (`:387-390`) |
| `ctrl+c to copy`, `shift+click to native select` | fullscreen only, text selected (`:419-441`) |
| `hold {key} to speak` | voice enabled, idle, fewer than 3 shows (`:442-446`) |
| **`? for shortcuts`** | **fallback only** — pushed when the parts array, the tasks pill and the mode part are all empty (`:411-415`) |

**Global suppression.** `showHint = !suppressHint && !showVim` (`:194`), where
`suppressHint = (input.length > 0) || statusLineShouldDisplay(settings) || isSearching`
(`PromptInput.tsx:2274` passes `suppressHint={input.length > 0}`;
`PromptInputFooter.tsx:120` ORs in the other two). **The practical rule: the hint line is
there when the box is empty and disappears the moment the user types a character.** The mode
indicator and the tasks pill are *not* hints and survive typing.

**Height.** In the shipped non-fullscreen path the footer returns `null` when there is nothing
to say, so the row disappears (`:463`). It can also become **two rows**: a custom status line
renders above the left side in the same column (`PromptInputFooter.tsx:139`), and teammate
pills force their own row (`:392-401`). The main row is `Box height={1} overflow="hidden"`
(`:465`) with the trailing byline `wrap="truncate"`, so overflow is cut at the tail, never
wrapped.

**Right group** (`PromptInputFooter.tsx:141-147`): notifications (auto-update status, IDE
connection, MCP errors) and the bridge status pill. Not part of the hint model.

**`?` opens a full help menu** which replaces the footer entirely
(`PromptInputFooter.tsx:131-133`, `PromptInputHelpMenu` with `paddingX={2}`).

### 7. Multi-line input — the growth question

**What the reference does.** `PromptInput.tsx:1999`:

```ts
const maxVisibleLines = isFullscreenEnvEnabled()
  ? Math.max(MIN_INPUT_VIEWPORT_LINES, Math.floor(rows / 2) - PROMPT_FOOTER_LINES)
  : undefined;
```

with `PROMPT_FOOTER_LINES = 5` and `MIN_INPUT_VIEWPORT_LINES = 3` (`:192-193`).

- **Shipped scrollback path: `undefined` — no cap. The box grows without limit** and the
  terminal scrolls to make room. `Cursor.render` (`Cursor.ts:202-218`) slices
  `allLines.slice(startLine, endLine)` where `endLine = allLines.length` when
  `maxVisibleLines` is undefined. A 200-line paste renders 200 rows. This works only because
  the document may be taller than the screen — see [02](02-scrollback-mechanism.md) §1.
- **Fullscreen path: capped and internally scrolled.** `Cursor.getViewportStartLine`
  (`Cursor.ts:172-184`) centres the window on the cursor:
  `startLine = max(0, cursorLine - floor(max/2))`, then pulled back so the window is always
  full (`:180-182`). **No indicator of any kind is rendered for the hidden rows** — no
  `+N more`, no ellipsis, no fade. Lines above and below simply are not drawn.

**What picopilot must do — recompute is not available, so cap and scroll.**

[02](02-scrollback-mechanism.md) §4 limit 1 settles this: in ratatui 0.29 `Viewport::Inline(h)`
is a private field set once in `with_options`, with no public setter, so **growing the box by
growing the viewport is not an option** short of dropping and rebuilding the `Terminal`, which
repaints visibly. Recommendation, in order of preference:

1. **Cap the input, scroll inside the box. Copy the reference's own fullscreen answer.** This
   is not a compromise picopilot invents — it is what Claude Code already ships for exactly
   this constraint (a fixed-height bottom slot), and it is the same shape as ticket 02 §5
   step 5 ("scroll them inside the live region rather than growing it").
2. **Compute the cap per frame from the live-region budget, not from a constant.** Mirror
   `Math.max(MIN, available)`:

   ```
   chrome        = 1 gap + 1 top rule + 1 bottom rule            = 3
   footer_rows   = 6 when a completion list is open, else 0 or 1
   spinner_rows  = rows the spinner/streaming tail wants
   max_input     = max(3, viewport_height - chrome - footer_rows - spinner_rows)
   ```

   `MIN_INPUT_LINES = 3` is the reference's `MIN_INPUT_VIEWPORT_LINES` (`:193`). When the
   budget cannot cover the minimum, **the input box wins** — drop optional rows above it
   first, as the reference drops its StatusLine first when `rows < 24`
   (`PromptInputFooter.tsx:108`).
3. **Choose `Inline(h)` so the common case never hits the cap.** Ticket 02 §6 leaves the number
   open and asks that it be prototyped. From this ticket's budget, **`h = 14` is the
   recommendation**: 3 chrome + 1 hint + 1 spinner leaves 9 input rows idle, and 3 chrome + 6
   completion rows + 1 spinner still leaves 4 — above the minimum of 3. `h = 12` also works but
   drops to exactly 3 with the completion list open. This number is a recommendation from
   arithmetic, not from measurement.
4. **Scroll the window cursor-centred, not bottom-anchored.** picopilot today keeps the cursor
   pinned to the last visible row ([tui.rs#L2341-L2344](src/tui.rs#L2341-L2344)), so editing
   the middle of a long prompt shows no following context. Port `getViewportStartLine`:
   `start = clamp(cursor_row - max/2, 0, total - max)`.
5. **Render no truncation marker.** Verified: the reference draws none.

**Also copy the hard input cap.** `useMaybeTruncateInput.ts:31-42`: any input over
**10 000 characters** is replaced by a `[Pasted text #N]` reference and stored out of band.
That, not the visual cap, is what keeps a giant paste from becoming a giant box.

### 8. Completion / slash-command list

`PromptInputFooter.tsx:126-130`:

```tsx
if (suggestions.length && !isFullscreen) {
  return <Box paddingX={2} paddingY={0}>
      <PromptInputFooterSuggestions ... />
    </Box>;
}
```

- **Below the box, in the flow, replacing the hint line.** Not an overlay, no `Clear`, no
  border, no title. Indented 2 columns, same as the hint line it displaces. (The fullscreen
  path portals it into an absolute overlay — not our model.)
- **At most 6 rows**: `maxVisibleItems = Math.min(6, Math.max(1, rows - 3))`
  (`PromptInputFooterSuggestions.tsx:212`). Scroll window is cursor-centred, same formula as
  the input viewport: `startIndex = max(0, min(sel - floor(max/2), len - max))` (`:222`).
- **Selection is by color only — no bullet, no arrow, no background.** Selected row:
  `color="suggestion"` `rgb(177,185,249)` (`theme.ts:458`) and **not** dim. Unselected:
  default foreground, `dimColor` (`:118-121`, `:152-165`).
- Two row shapes. Commands: `displayText` padded to a column, then optional `[tag] `, then the
  description, all `wrap="truncate"` (`:129-172`). Files / MCP resources / agents:
  `` `${icon} ${displayText} – ${description}` `` where the separator is **` – `** (U+2013 en
  dash, spaces around it) and the icons are `+` for files, `◇` for MCP resources, `*` for
  agents (`:22-28`, `:100-106`).
- Path truncation is **middle** (`truncatePathMiddle`), description truncation is tail, and
  descriptions have whitespace collapsed with `/\s+/g → ' '` (`:70-77`).

### 9. What picopilot has, and the gaps

| aspect | picopilot today | reference | gap |
|---|---|---|---|
| Rules | `Borders::TOP \| BOTTOM`, `Color::DarkGray` ([tui.rs#L2345-L2350](src/tui.rs#L2345-L2350)) | `─` full width, `promptBorder` `rgb(136,136,136)` | Structure already correct. Only the color is wrong — `DarkGray` is a 4-bit ANSI index, not `rgb(136,136,136)` |
| Blank gap above | none | one row (`marginTop={1}`) | Missing |
| Prompt char | `"  ❯ "`, orange `rgb(240,177,94)`, width 4 ([tui.rs#L2152](src/tui.rs#L2152), [tui.rs#L2243](src/tui.rs#L2243)) | `❯ `, width 2, **no color**, dim while working | Wrong indent (2 → 0), wrong width (4 → 2), spurious accent color, no dim-while-loading |
| Continuation indent | 4 spaces ([tui.rs#L2153](src/tui.rs#L2153)) | 2 columns | Off by two — follows from the prompt-width fix |
| Cursor | real terminal cursor positioned by the app ([tui.rs#L1494-L1499](src/tui.rs#L1494-L1499)) | software reverse-video cell | Different mechanism. Keeping the real cursor is arguably better on Windows Terminal, but it cannot render the inverted-first-character placeholder of §5 |
| Placeholder | none | dim text, inverted first char, 4 rules | Entirely missing |
| Mode indicators | none | `!` in box; `⏸ plan mode on`, `⏵⏵ accept edits on`, `-- INSERT --`, `↯` in the rule | Entirely missing. picopilot has no permission modes and no vim mode, so only the *mechanism* is a gap — the hint line must be able to carry a colored mode part left of the separator |
| Hint line | fixed `shortcut_bar()` on the last screen row, always visible ([tui.rs#L1476-L1479](src/tui.rs#L1476-L1479)) | one dim row under the bottom rule, `paddingX=2`, disappears on first keystroke | The deletion of the shortcut bar (map "Notes") leaves nothing. Replace it with `? for shortcuts` under the rule, suppressed when the input is non-empty |
| Separator | n/a | `" · "` dim | Missing |
| Multi-line cap | `MAX_INPUT_CONTENT_LINES = 8`, constant ([tui.rs#L2154](src/tui.rs#L2154), [tui.rs#L2178](src/tui.rs#L2178)) | uncapped on the shipped path; `max(3, rows/2 - 5)` on the fixed-height path | The constant must become a per-frame budget (§7 step 2). 8 is a defensible starting value but must not stay hard-coded |
| Scroll within box | bottom-anchored ([tui.rs#L2341-L2344](src/tui.rs#L2341-L2344)) | cursor-centred | Wrong anchor |
| Truncation marker | none | none | **No gap.** Do not add one |
| Hard input cap | none | 10 000 chars → `[Pasted text #N]` | Missing; matters more than the visual cap |
| Completion list | `Clear` + `Borders::ALL` + orange border + `"commands"` title + `"› "` marker + orange-on-black selected row, drawn **above** the box, max 7 ([tui.rs#L2356-L2419](src/tui.rs#L2356-L2419)) | borderless, titleless, markerless, **below** the box at `paddingX=2`, max 6, selection by color alone | Every dimension differs: position, border, title, marker, highlight style, count |
| Approval / reconnect / blocked | bordered boxes with titles replacing the input ([tui.rs#L2290-L2331](src/tui.rs#L2290-L2331)) | the box keeps its rules and the *content* changes (`PromptInput.tsx:2237-2242` for the editor case) | picopilot's `Borders::ALL` + `.title(...)` here contradicts the two-rule model. These become inline pickers per the map |

Also note picopilot's `input_height` already reserves `area.height - 3`
([tui.rs#L2169-L2179](src/tui.rs#L2169-L2179)) for the status bar and shortcut bar; once both
are deleted that arithmetic becomes the live-region budget of §7 step 2.

### 10. Spec-ready summary

```
[blank row]
'─' × width                                   fg promptBorder rgb(136,136,136)
'❯ ' + input                                  ❯ default fg, dim while working; col 0
  … more input rows, text at col 2 …          cap = max(3, budget); cursor-centred scroll
'─' × width                                   fg promptBorder
'  ' + hint line                              dim, parts joined by ' · ', 1 row, truncate tail
```

- Empty + idle → hint line reads `? for shortcuts`.
- First keystroke → hint line vanishes; mode part (if any) stays.
- Working → `❯` dims, hint line reads `esc to interrupt`.
- Completion open → the hint row is replaced by ≤ 6 suggestion rows at the same indent.
- Bash mode → `❯` becomes `!` in `bashBorder`, rules turn `bashBorder`, hint reads
  `! for bash mode`.
- Fast mode → `─── ↯ ───` at the right end of the top rule.

### 11. Unverified

- **Nothing was observed rendering.** No `bun`, no `node_modules`, no `claude` binary. Every
  claim is read from source, per the map's standing risk.
- **The `cli-boxes` `round` glyph table was not read.** `node_modules` is absent, so
  `box.top === '─'` and `box.bottom === '─'` are asserted from the package's documented
  `round` style, not from a file on this machine. The *structure* — that only `box.top` and
  `box.bottom` are ever emitted, and that corners are suppressed — **is** verified, from
  `render-border.ts:118-130, 176-183`.
- **`figures.pointer === '❯'` and its non-Unicode fallback `'>'` were not read** for the same
  reason. The reference's own `figures.ts` does not redefine `pointer`.
- **`⏵⏵` is a two-character string in the source** (`PermissionMode.ts:61, 67, 73, 82`); it was
  not confirmed that it renders as two cells rather than one wide cell on Windows Terminal.
- **`getExampleCommandFromCache()` returns a cached rotating string**; its contents were not
  enumerated.
- **`h = 14` for `Inline(h)` is arithmetic, not measurement.** Ticket 02 §6 asks for this to be
  prototyped and that request stands.
- **The reference's non-fullscreen box being genuinely uncapped was not stress-tested.** It
  follows from `maxVisibleLines === undefined` flowing into `Cursor.render`'s `endLine =
  allLines.length`, which is a code path, not an observation.
- **Whether ratatui ≥ 0.30 adds a runtime inline-viewport height setter was not re-checked**;
  ticket 02 verified its absence for 0.29.0 only. If it exists, recommendation §7.1 should be
  revisited before the spec freezes.
