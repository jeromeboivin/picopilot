---
label: wayfinder:research
name: Specify inline pickers
status: closed
assignee: research-subagent
blocked_by: [02-scrollback-mechanism]
---

# Specify inline pickers

## Question

How are selection lists rendered in the flow, now that picopilot's centered modals cannot
survive the move to scrollback?

picopilot currently has six centered overlay modals — sessions, models, tools, skills, usage
and todos — plus a completion dropdown. All of them need a new home.

Resolve by producing:

- How the reference renders a picker: its position relative to the input, any border, the
  selected-row styling, the scroll indicators, and the item and description layout.
- The keyboard model, and what the picker leaves behind in the transcript once a choice is
  made or the picker is dismissed.
- Whether multi-select checklists exist in the reference, since picopilot's tool and skill
  modals are checklists.
- A mapping from each of picopilot's six modals to an inline form, calling out any that do not
  fit the pattern.
- Whether any of them are better served by a slash command that prints into the transcript
  instead of an interactive picker.

## Resolution

Settled from source. Reference read at `C:\dev\git\claude-code`. All files there are compiled
output with an inline base64 sourcemap appended; line numbers below are lines of the **compiled
file as it sits on disk**, which is what `rg`/`read_file` report. Nothing was executed — there is
no `bun`, no `node_modules`, no `claude` binary (map "Known risk, accepted"). See "Unverified" at
the end.

### 1. The headline finding: the reference has no overlay to copy

There is no centered overlay in the shipped product, so there is nothing to translate. A picker
is a **normal block in the document flow that takes the input box's place.**

- `REPL.tsx:4540-4541` — `const toolJsxCentered = isFullscreenEnvEnabled() && toolJSX?.isLocalJSXCommand === true;`
  `const centeredModal = toolJsxCentered ? toolJSX!.jsx : null;`. Ticket 02 established that
  `isFullscreenEnvEnabled()` is false for everyone outside Anthropic. **In the public scrollback
  build `centeredModal` is always `null`.**
- `REPL.tsx:4894` — the prompt renders only when
  `!toolJSX?.shouldHidePromptInput && !focusedInputDialog && !isExiting && !disabled && !cursor`.
  A dialog and the input box are mutually exclusive siblings in the same column. The picker is
  **where the input box was**, at the same left edge, with the transcript above it unchanged.
- Non-interactive slash-command screens (`/status`, `/theme`, `/diff`, ~40 others) render inside
  the scrollable transcript column instead; immediate ones (`/btw`, `/sandbox`) render in the
  fixed bottom block (`REPL.tsx:4600-4612` and the comment above it).

So the map's "modals become inline pickers" is not an adaptation — it is what the reference does.

### 2. The row — exact layout, verbatim glyphs

One shared primitive: `design-system/ListItem.tsx`. Every picker row goes through it
(`CustomSelect/select-option.tsx` is a thin pass-through: `SelectOption` → `ListItem`).

The row container, `ListItem.tsx:216`:

```jsx
<Box flexDirection="row" gap={1}>{indicator}{label}{tick}</Box>
```

`gap={1}` is exactly one space between each child. The optional description hangs below,
`ListItem.tsx:226`:

```jsx
description && <Box paddingLeft={2}><Text color="inactive">{description}</Text></Box>
```

**Indicator column — one cell, always present**, `ListItem.tsx:127-133`, in priority order:

| state | glyph | style |
| --- | --- | --- |
| disabled | `" "` (single space) | — |
| focused | `figures.pointer` = `❯` | color `suggestion` |
| more items below, and this is the last visible row | `figures.arrowDown` = `↓` | `dimColor` |
| more items above, and this is the first visible row | `figures.arrowUp` = `↑` | `dimColor` |
| otherwise | `" "` (single space) | — |

Note the scroll arrows **occupy the pointer column** — they are not a separate gutter, and they
never coexist with the pointer on the same row.

**Label color**, `ListItem.tsx:150-159`: `inactive` if disabled → `success` if selected →
`suggestion` if focused → inherited otherwise. **There is no background fill and no inverse
video anywhere in the reference's selection model.** Focus is a glyph plus a foreground color.

**Selected marker**, `ListItem.tsx:207`: `figures.tick` = `✓`/`✔` in color `success`, appended
after the label with the `gap={1}` space.

So a single-select row is literally:

```
❯ 1. Sonnet 4.5 ✓
```

and the two rows around it:

```
  2. Opus 4.1
↓ 3. Haiku 4.5
```

**Index prefix.** `select.tsx:575` (compact) renders `` `${i}.`.padEnd(maxIndexWidth + 2) `` in
`dimColor`, where `maxIndexWidth = String(options.length).length`. So for a 12-item list every
label starts at the same column and `1.` is padded to `"1.  "`. Indexes are 1-based, are the
number key that jumps straight to that option, and can be switched off with `hideIndexes`
(`select.tsx:212`).

**Four layouts**, `select.tsx:219` (`layout` defaults to `'compact'`):

| layout | description placement | rows per item |
| --- | --- | --- |
| `compact` (default) | same row, `marginLeft={2}`, `wrap="wrap-trim"` (`select.tsx:575`) | 1 |
| `compact` + `inlineDescriptions` | same row, single space after the label, dim | 1 |
| `compact` + descriptions on ≥1 option | two aligned columns; labels padded to the widest, description column `marginLeft={2}` (`select.tsx:500-522`) | 1 |
| `compact-vertical` | line below, `paddingLeft={maxIndexWidth + 4}` (or `4` when `hideIndexes`) | 2 |
| `expanded` | line below at `paddingLeft={2}`, **plus a trailing blank line per item** (`select.tsx:403`) | 3 |

`compact` is the one that matters for us: it keeps one terminal row per item, which is what makes
a picker fit a fixed-height live region.

**Container.** `Select` itself is `<Box flexDirection="column">` and nothing else — **no border,
no padding, no title, no `Clear`** (`select.tsx`, `styles.container` = `{ flexDirection: 'column' }`).
Framing is the caller's job, and there are exactly two conventions:

- **Bare.** Just the rows, at the same left edge as the transcript.
- **Framed by `Pane`** (`design-system/Pane.tsx:52-68`): one blank line (`paddingTop={1}`), then a
  `Divider` — `char` defaults to `'\u2500'` = `─` (`Divider.tsx:75`) repeated to the full terminal
  width in the pane color — then the content at `paddingX={2}`. **A top rule only. There is no
  box, no left/right/bottom border anywhere.**
- `Dialog` (`design-system/Dialog.tsx`) wraps `Pane` and adds: title `bold` in the dialog color
  (default `permission`, `Dialog.tsx:43,70`), optional dim subtitle, a blank line (`gap={1}`),
  the children, then `marginTop={1}` and a `dimColor italic` hint line (`Dialog.tsx:105`).

### 3. Scroll indicators and the max visible item count

`visibleOptionCount` defaults to **5** (`select.tsx:218`, `use-select-state.ts`,
`use-multi-select-state.ts`). Concrete overrides found:

| picker | count | source |
| --- | --- | --- |
| `/model` | `Math.min(10, selectOptions.length)` | `ModelPicker.tsx:135` |
| resume-session, embedded in the flow | `Math.max(1, Math.min(sessions.length, 5, rows - 6 - 7))` | `ResumeTask.tsx:178` |
| resume-session, own screen | `Math.max(1, Math.min(sessions.length, rows - 1 - 7))` | `ResumeTask.tsx:178` |
| slash/file autocomplete | `Math.min(6, Math.max(1, rows - 3))` | `PromptInputFooterSuggestions.tsx:224` |
| autocomplete when overlaid | `OVERLAY_MAX_ITEMS = 5` | `PromptInputFooterSuggestions.tsx:18` |

There are **three** scroll affordances, and no "+N more" line anywhere:

1. `↑` / `↓` dim, in the pointer column of the first / last visible row (§2).
2. A dim counter in the title: `ResumeTask.tsx:185-188` renders
   `Select a session to resume (7 of 42):` where `7` is the 1-based focused index.
3. Nothing else. No scrollbar, no ellipsis row, no percentage.

**Window movement**, `use-select-navigation.ts`: the window scrolls by exactly one row when focus
crosses an edge (`nextVisibleToIndex = min(size, visibleToIndex + 1)`, line 113;
`nextVisibleFromIndex = max(0, visibleFromIndex - 1)`, line 170). Navigation **wraps**, and
wrapping snaps the window to the far end (`visibleFromIndex: 0` on wrap-to-first, line 99;
`visibleToIndex = size` on wrap-to-last, line 148). PageUp/PageDown move a full page.
The autocomplete menu uses a different rule — it **centers** the selection:
`startIndex = max(0, min(selected - floor(maxVisible / 2), n - maxVisible))`
(`PromptInputFooterSuggestions.tsx:238`).

### 4. Keyboard model

From `keybindings/defaultBindings.ts` and `CustomSelect/use-select-input.ts`.

Context `Select` (`defaultBindings.ts:318-329`) — used by `/model`, `/resume`, permission prompts:

| key | action |
| --- | --- |
| `↑`, `k`, `ctrl+p` | `select:previous` |
| `↓`, `j`, `ctrl+n` | `select:next` |
| `Enter` | `select:accept` |
| `Esc` | `select:cancel` |

Handled outside the keybinding table, in `use-select-input.ts`:

- `PageUp` / `PageDown` — move a whole page.
- **`1`–`9` select that option immediately** (not just focus it), which is why the index prefix is
  rendered; suppressed by `disableSelection: 'numeric'` and by `hideIndexes`.
- `Space` toggles, in multi-select only.
- `Tab` toggles input-mode on an option that embeds a text field.
- While a text field is focused, digits type literally and only the arrows still navigate.
- `onUpFromFirstItem` / `onDownFromLastItem`, when supplied, disable wrapping and hand focus out of
  the picker instead — this is how a picker composes with a neighbouring widget.

Context `ModelPicker` (`defaultBindings.ts:310-315`) adds a second axis on the focused row:
`←` `modelPicker:decreaseEffort`, `→` `modelPicker:increaseEffort`.

Standard hint line, assembled from `KeyboardShortcutHint` (renders `"{shortcut} to {action}"`)
joined by `Byline` with the separator `" · "` (`design-system/Byline.tsx`), the whole line `dimColor`
and, inside a `Dialog`, `italic`:

```
↑/↓ to select · Enter to confirm · Esc to cancel
```

verbatim from `ResumeTask.tsx:215-217`. The `Dialog` default is the shorter
`Enter to confirm · Esc to cancel` (`Dialog.tsx:60`). Multi-selects add a toggle hint:
`Space to select · Enter to confirm · …` (`MCPServerMultiselectDialog.tsx:112`) or
`↑↓ to navigate · Space to toggle · …` (`WorkflowMultiselectDialog.tsx:31-32`).

### 5. What the picker leaves behind

**Nothing of the picker survives.** It unmounts completely; what stays is a one-line message the
command writes on its way out, via `LocalJSXCommandOnDone`
(`types/command.ts:107-125`: `onDone(result?, { display?: 'skip' | 'system' | 'user' })`).

`/model` is the clean example, `commands/model/model.tsx`:

- confirm → `onDone(\`Set model to ${chalk.bold(displayModel)}\`)`, with ` · Fast mode ON` /
  ` · Billed as extra usage` appended conditionally.
- cancel → `onDone(\`Kept model as ${chalk.bold(displayModel)}\`, { display: 'system' })`.

So the rule is: **a picker always commits exactly one line to the transcript, on both paths, and
the cancel line states the unchanged value rather than saying "cancelled".** `display: 'skip'`
exists for pickers that should leave nothing at all.

This is a perfect fit for `insert_before` from ticket 02: the picker lives entirely in the live
region, is never committed, and on exit commits one short line whose height is trivially 1.

### 6. Multi-select checklists exist, and are a distinct component

`CustomSelect/SelectMulti.tsx` + `use-multi-select-state.ts`. Real users:
`MCPServerMultiselectDialog.tsx:91`, `MCPServerDesktopImportDialog.tsx`,
`WorkflowMultiselectDialog.tsx`, `permissions/.../QuestionView.tsx`.

The row, `SelectMulti.tsx:156`:

```jsx
<Box gap={1}><SelectOption isFocused={isOptionFocused} isSelected={false} ...
  description={option.description}>
  {!hideIndexes && <Text dimColor>{`${i}.`.padEnd(maxIndexWidth)}</Text>}
  <Text color={isSelected ? 'success' : undefined}>[{isSelected ? figures.tick : ' '}]</Text>
  <Text color={isOptionFocused ? 'suggestion' : undefined}>{option.label}</Text>
</SelectOption></Box>
```

Read off it:

- The checkbox is **`[✓]` when checked, `[ ]` (bracket, space, bracket) when not** — three cells
  either way, so nothing shifts on toggle. Checked is color `success`, unchecked inherits.
- `isSelected={false}` is passed to `SelectOption` deliberately: the trailing `✓` of §2 is
  suppressed, because the checkbox already carries that meaning. No double tick.
- The focus pointer `❯` and its `gap={1}` still come from `ListItem`, unchanged.
- Descriptions still hang below at `paddingLeft={2}` in `inactive`.

Keys, `use-multi-select-state.ts`: `↑`/`k`/`ctrl+p`, `↓`/`j`/`ctrl+n`, `PageUp`/`PageDown`,
`Space` toggles, `1`–`9` toggle by index, `Esc` cancels. `Enter` is overloaded and this is the one
genuinely fiddly part:

- **without** `submitButtonText`, `Enter` **submits** the whole selection and only `Space` toggles;
- **with** `submitButtonText`, `Enter` toggles like `Space`, and submitting requires focusing an
  extra row below the list — rendered `SelectMulti.tsx:191` as pointer, `marginLeft={3}`, bold
  label — reached with `Tab` or `↓` past the last option.

Recommendation for picopilot: use the **no-submit-button** form. `Space` toggles, `Enter` applies,
`Esc` cancels — which is exactly what picopilot's tool and skill pickers already do
([tui.rs#L2599](src/tui.rs#L2599), [tui.rs#L2628](src/tui.rs#L2628)), so only the visuals change.

### 7. The completion dropdown

`PromptInputFooter.tsx:131-132` — in the non-fullscreen (scrollback) build:

```jsx
if (suggestions.length && !isFullscreen) {
  return <Box paddingX={2} paddingY={0}>
      <PromptInputFooterSuggestions ... />
```

It is the **footer, below the input box**, indented 2, and it **replaces the hint line** rather
than floating over anything. No border, no `Clear`, no title.

Row rendering, `PromptInputFooterSuggestions.tsx`:

- **No pointer glyph at all.** Selection is: selected row gets color `suggestion`; every other row
  gets `dimColor` (lines 158-176). That is the entire selected-row treatment.
- Slash commands / generic items: `name` padded to a common column width — capped at
  `Math.floor(columns * 0.4)` — then optional `[tag] ` in dim, then the description, all inside
  one `<Text wrap="truncate">`.
- File / MCP / agent items are a single string, `` `${icon} ${displayText} – ${truncatedDesc}` ``
  (line 112) — note the separator is an **en dash `–` surrounded by spaces**, not a hyphen. Icons
  (line 24): `+` for files, `◇` for MCP resources, `*` for agents. File paths are truncated in the
  middle; MCP names are truncated to 30 columns.
- Max 6 rows (`Math.min(6, rows - 3)`), window centered on the selection.

Keys, `defaultBindings.ts:99-106`, context `Autocomplete`: `Tab` accepts, `Esc` dismisses,
`↑`/`↓` move. **`Enter` is not bound here** — it stays with the input box and submits the prompt.

### 8. Fitting a picker into the fixed-height live region (the ticket 02 conflict)

Ticket 02 chose `Viewport::Inline(h)` with `h` **fixed for the life of the `Terminal`** — there is
no public API in ratatui 0.29 to resize it. A 40-session list obviously cannot grow the viewport.
The reference already solves this, and the solution is the `visibleOptionCount` window itself:

**Budget, bottom-aligned inside `Viewport::Inline(12)`:**

| live state | rows |
| --- | --- |
| spinner line | 1 |
| input box (top rule, one text row, bottom rule) | 3 |
| hint line **or** up to 6 completion rows | 1–6 |
| **worst case, normal mode** | **10** |
| blank + `─` rule (`Pane`) | 2 |
| title line (with `(n of m)` counter) | 1 |
| optional column header | 1 |
| 5 option rows | 5 |
| hint line | 1 |
| **worst case, picker mode** | **10** |

`h = 12` covers both with slack for a two-line title. Three rules make this hold:

1. **Always `compact` layout — one terminal row per option, never `expanded`.** Descriptions go on
   the same row (§2). This is the single decision that makes a picker fit.
2. **Cap visible options at 5**, the reference default, and never derive it from terminal height
   the way `ResumeTask.tsx:178` does — our height is fixed, so the cap is a constant.
   picopilot's model picker may use up to 8; the reference's own `/model` uses 10
   (`ModelPicker.tsx:135`) but is not height-constrained.
3. **Length lives in the window, not the layout.** 500 sessions render as 5 rows plus `↑`/`↓` plus
   `(7 of 500)` in the title. Nothing about the picker's height depends on the list's length. This
   is the direct answer to "a long list conflicts with a fixed-height live region": in the
   reference it never does.

The picker is drawn with `terminal.draw` into the live region, never with `insert_before`. Only
the one-line outcome of §5 is committed.

### 9. Mapping picopilot's six modals plus the dropdown

Today, from [tui.rs#L94](src/tui.rs#L94) (`enum ModalKind`) and
[tui.rs#L2422](src/tui.rs#L2422) (`draw_modal`): every surface is a `Clear` + full `Borders::ALL`
box in `Rgb(240,177,94)`, with the selected row in **inverse video** (`fg Black`, `bg
Rgb(240,177,94)`) and a `"› "` highlight symbol. `modal_area` ([tui.rs#L2756](src/tui.rs#L2756))
gives sessions/models/tools/skills the whole terminal and usage/todos a `centered_rect(70, 70)`.
None of that survives.

| picopilot modal | becomes | why |
| --- | --- | --- |
| **Sessions** ([tui.rs#L2504](src/tui.rs#L2504)) | **Inline single-select picker.** Title `Select a session to resume (n of m):`, bold header row at indent 2 (`Updated` padded, two spaces, `Session Title`), option label `${paddedTime}  ${title}`, 5 visible rows. | Exact analog exists: `ResumeTask.tsx:156-220`. Today's `"{modified_time} | {summary}"` already matches once the `|` becomes column padding. |
| **Models** ([tui.rs#L2551](src/tui.rs#L2551)) | **Inline single-select picker.** Rows keep `model_picker_row_for`'s aligned columns ([tui.rs#L2703](src/tui.rs#L2703)) as the label. The 5-row **detail pane is dropped**; the one line worth keeping (context tier / cost) becomes the row's inline description. Reasoning and context tier move from `r`/`c` to `←`/`→` on the focused row. | Direct analog `/model` → `ModelPicker.tsx`, including the `←`/`→` second axis (`defaultBindings.ts:310-315`). The detail pane is a sub-window that has no counterpart and costs 5 of 12 live rows. |
| **Tools** ([tui.rs#L2599](src/tui.rs#L2599)) | **Inline multi-select checklist.** `[x]` → `[✓]`, `[ ]` unchanged, `success` when checked. Keep `Space`/`Enter`/`Esc`; keep `s` (shell only) and `a` (all) as extra bindings — the reference has no equivalent but they are cheap and orthogonal. | `SelectMulti.tsx:156` is a faithful match for what this already is. The only changes are glyph, color and losing the border. |
| **Skills** ([tui.rs#L2628](src/tui.rs#L2628)) | **Inline multi-select checklist**, same as tools. The 5-row detail pane (`skill_picker_detail_lines`, [tui.rs#L2684](src/tui.rs#L2684)) is **dropped**; `skill.description` becomes the row's inline description. `Source:` and `Directory:` are diagnostics, not selection aids — move them to a `/skills` static listing if they are wanted at all. | Same as models: the sub-pane does not fit 12 rows and has no reference counterpart. |
| **Usage** ([tui.rs#L2836](src/tui.rs#L2836)) | **Slash command that prints a static block into the transcript.** Read-only text, no selection, no state to leave behind. | The reference's `/cost` is `type: 'local'` (`commands/cost/index.ts:9`) — it returns a string that becomes a transcript message. `usage_detail_lines` is already exactly that shape: 4–12 flat lines plus an indented attribution breakdown. As a committed block it also becomes scrollable and selectable for free, which a 70%-of-screen modal is not. |
| **Todos** ([tui.rs#L2798](src/tui.rs#L2798)) | **Neither.** A **non-interactive live block above the input**, toggled on and off (keep `Ctrl+T`), never committed. | `REPL.tsx:4606` renders `<TaskListV2 isStandalone />` in the fixed bottom block, gated on `showExpandedTodos && tasksV2.length > 0`. `TaskListV2.tsx` contains **no `useInput`** — it is display-only. Its status glyphs are `figures.tick` done, `figures.squareSmallFilled` `◼` in progress, `figures.squareSmall` `◻` pending (`TaskListV2.tsx:227-237`), replacing today's `[{status}]`. It is live because todo state changes mid-turn, so a printed block would go stale — this is the one surface that is neither a picker nor a print. It must be counted in the live-region budget: cap it — the reference uses `maxDisplay = rows <= 10 ? 0 : Math.min(10, Math.max(3, rows - 14))` (`TaskListV2.tsx:48`) and appends a dim `` ` … +2 pending, 1 in progress` `` summary (`TaskListV2.tsx:185,189`) — or accept that it and a picker are mutually exclusive. |
| **Completion dropdown** ([tui.rs#L2356](src/tui.rs#L2356)) | **Stays, restyled and relocated.** Move from a bordered `Clear`ed box floating *above* the input to an unbordered block **below** it at `paddingX=2`. Drop the border, the `"commands"` title, the `"› "` symbol and the inverse-video highlight; selected row = color `suggestion`, all others `dimColor`. Row = command padded to a common width (cap 40% of terminal width), then description. Cap 7 → 6. `Tab` accepts, `Esc` dismisses, `↑`/`↓` move, `Enter` stays with the input. | `PromptInputFooter.tsx:131` + `PromptInputFooterSuggestions.tsx`. The current dropdown is drawn at `y = input_area.y - height`, which is upward painting near the top of the buffer — the exact pattern ticket 02 flagged as tripping `microsoft/terminal#14774` on Windows. Moving it below the input removes that hazard as well as the styling gap. |

Nothing is dropped outright. Two *sub-panes* are dropped: the model detail pane and the skill
detail pane, both folded into inline descriptions.

**Consequences beyond rendering.** `centered_rect` ([tui.rs#L2893](src/tui.rs#L2893)) and
`modal_area` ([tui.rs#L2756](src/tui.rs#L2756)) both become dead. Every `Clear` in `draw_modal`
becomes dead — there is nothing to clear when the picker owns its rows. The approval-details
overlay at [tui.rs#L2424](src/tui.rs#L2424) is not one of the six but has the same problem and
needs the same treatment; it is not specified here.

### 10. Overlap with ticket 09 (still open)

Ticket 09 owns the input box and hint line, and §7 above lands squarely inside it: the completion
dropdown is rendered *by the input's footer*, and it *replaces* the hint line. Two facts are
shared; neither blocks the other:

- The suggestion list is the footer's alternate content, not a separate widget
  (`PromptInputFooter.tsx:131`). Ticket 09 should treat "hint line" and "completion list" as one
  slot with two states.
- A picker and the input box never coexist (`REPL.tsx:4894`), so ticket 09's box height and this
  ticket's picker height are alternatives in the same budget, not additions. §8's `h = 12` assumes
  a 3-row input box; if ticket 09 lands a taller cap, `h` must be recomputed.

### 11. Unverified

- **The `figures@6.1.0` values themselves.** `package.json:50` pins `figures: ^6.1.0` and there is
  no `node_modules`. `pointer` = `❯` and its `>` fallback were already settled in ticket 03 with
  the same caveat. `tick`, `arrowUp`, `arrowDown`, `squareSmall`, `squareSmallFilled` are read from
  the package's documented Unicode table, **not** from a file on this machine. Specifically:
  `ListItem.tsx`'s own JSDoc (line 68) writes the checkmark as `✓` (U+2713) while `figures.tick`
  ships `✔` (U+2714) — **the doc comment and the runtime value disagree, and only one of them can
  be right.** Resolve before writing the glyph into the spec.
- **Non-Unicode Windows fallbacks** for `tick`, `squareSmall`, `squareSmallFilled`. Same cause.
- **Exact intra-row spacing of the multi-select row.** `SelectMulti.tsx:156` passes three sibling
  `<Text>` nodes as `children` into a `ListItem` whose row `Box` has `gap={1}`. Whether Yoga
  applies that gap between array-flattened children was not confirmed against the Ink fork, so the
  row is either `❯ 1. [✓] Label` or `❯ 1.[✓]Label`. Single-select is unaffected — there the
  children are wrapped in an inner `<Box flexDirection="row">` with no gap, so `1.` and the label
  are adjacent and the padding comes from `padEnd`.
- **Whether a picker draws a blank line above itself in practice.** `Pane` has `paddingTop={1}`
  (`Pane.tsx:68`), but a bare `Select` has none and callers vary. Not settled which convention
  picopilot should use for each surface.
- **`h = 12`.** Derived by adding up the reference's parts; not measured, and it depends on ticket
  09's final input-box height.
- **Whether picopilot needs the reference's `figures` fallback logic at all.** `figures` swaps to
  ASCII when `isUnicodeSupported()` is false. Ticket 03 left this open; the glyphs above assume
  the Unicode branch.
