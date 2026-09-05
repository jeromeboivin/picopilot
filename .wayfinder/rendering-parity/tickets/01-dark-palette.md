---
label: wayfinder:research
name: Capture the Claude Code dark palette
status: closed
assignee: research-subagent
blocked_by: []
---

# Capture the Claude Code dark palette

## Question

What are the exact **dark theme** color values in `C:\dev\git\claude-code\src\utils\theme.ts`,
and what is each one used for?

The charting exploration only captured the *light* theme values. picopilot ships dark only, so
the dark variant is the one that matters, and every other ticket in this map refers to these
colors by key.

Resolve by producing:

- The full list of named keys in the `Theme` type.
- The dark-theme value of each key, verbatim.
- For each key, where it is used in the UI — which component, which message type, which state.
  Keys that turn out to be unused by the conversation window should be marked as such.
- The accent (`claude`) color and its shimmer variant, since branding is being copied.
- Any color chosen dynamically rather than from the palette (interpolated stall colors,
  rainbow highlighting, background hover states).
- A note on which keys have no meaningful picopilot counterpart, and which picopilot colors
  have no Claude Code counterpart.

## Resolution

Source of truth: `C:\dev\git\claude-code\src\utils\theme.ts`. The `Theme` type is declared at
lines 4-89. `darkTheme` is declared at line 440 and closes at line 515. `getTheme()` at line 598
returns `darkTheme` from its `default:` branch (line 611), so dark is the fallback theme.

Method: all values below were read verbatim out of `theme.ts`. Usage was established by scanning
the 1976 `.ts`/`.tsx` files under `C:\dev\git\claude-code\src` for `theme.<key>` and for the key
as a string literal (Ink props take theme keys as strings, e.g. `<Text color="error">`). No
binary was run — see the map's "Known risk, accepted" note; nothing here is confirmed against
rendered output.

### 1. Full dark palette, verbatim

Values are copied exactly, including the inconsistent spacing inside some `rgb(...)` strings.

| # | Key | Dark value | Source comment | theme.ts line |
|---|-----|-----------|----------------|---------------|
| 1 | `autoAccept` | `rgb(175,135,255)` | Electric violet | 441 |
| 2 | `bashBorder` | `rgb(253,93,177)` | Bright pink | 442 |
| 3 | `claude` | `rgb(215,119,87)` | Claude orange | 443 |
| 4 | `claudeShimmer` | `rgb(235,159,127)` | Lighter claude orange for shimmer effect | 444 |
| 5 | `claudeBlue_FOR_SYSTEM_SPINNER` | `rgb(147,165,255)` | Blue for system spinner | 445 |
| 6 | `claudeBlueShimmer_FOR_SYSTEM_SPINNER` | `rgb(177,195,255)` | Lighter blue for system spinner shimmer | 446 |
| 7 | `permission` | `rgb(177,185,249)` | Light blue-purple | 447 |
| 8 | `permissionShimmer` | `rgb(207,215,255)` | Lighter blue-purple for shimmer | 448 |
| 9 | `planMode` | `rgb(72,150,140)` | Muted sage green | 449 |
| 10 | `ide` | `rgb(71,130,200)` | Muted blue | 450 |
| 11 | `promptBorder` | `rgb(136,136,136)` | Medium gray | 451 |
| 12 | `promptBorderShimmer` | `rgb(166,166,166)` | Lighter gray for shimmer | 452 |
| 13 | `text` | `rgb(255,255,255)` | White | 453 |
| 14 | `inverseText` | `rgb(0,0,0)` | Black | 454 |
| 15 | `inactive` | `rgb(153,153,153)` | Light gray | 455 |
| 16 | `inactiveShimmer` | `rgb(193,193,193)` | Lighter gray for shimmer effect | 456 |
| 17 | `subtle` | `rgb(80,80,80)` | Dark gray | 457 |
| 18 | `suggestion` | `rgb(177,185,249)` | Light blue-purple | 458 |
| 19 | `remember` | `rgb(177,185,249)` | Light blue-purple | 459 |
| 20 | `background` | `rgb(0,204,204)` | Bright cyan | 460 |
| 21 | `success` | `rgb(78,186,101)` | Bright green | 461 |
| 22 | `error` | `rgb(255,107,128)` | Bright red | 462 |
| 23 | `warning` | `rgb(255,193,7)` | Bright amber | 463 |
| 24 | `merged` | `rgb(175,135,255)` | Electric violet (matches autoAccept) | 464 |
| 25 | `warningShimmer` | `rgb(255,223,57)` | Lighter amber for shimmer | 465 |
| 26 | `diffAdded` | `rgb(34,92,43)` | Dark green | 466 |
| 27 | `diffRemoved` | `rgb(122,41,54)` | Dark red | 467 |
| 28 | `diffAddedDimmed` | `rgb(71,88,74)` | Very dark green | 468 |
| 29 | `diffRemovedDimmed` | `rgb(105,72,77)` | Very dark red | 469 |
| 30 | `diffAddedWord` | `rgb(56,166,96)` | Medium green | 470 |
| 31 | `diffRemovedWord` | `rgb(179,89,107)` | Softer red (less intense than bright red) | 471 |
| 32 | `red_FOR_SUBAGENTS_ONLY` | `rgb(220,38,38)` | Red 600 | 473 |
| 33 | `blue_FOR_SUBAGENTS_ONLY` | `rgb(37,99,235)` | Blue 600 | 474 |
| 34 | `green_FOR_SUBAGENTS_ONLY` | `rgb(22,163,74)` | Green 600 | 475 |
| 35 | `yellow_FOR_SUBAGENTS_ONLY` | `rgb(202,138,4)` | Yellow 600 | 476 |
| 36 | `purple_FOR_SUBAGENTS_ONLY` | `rgb(147,51,234)` | Purple 600 | 477 |
| 37 | `orange_FOR_SUBAGENTS_ONLY` | `rgb(234,88,12)` | Orange 600 | 478 |
| 38 | `pink_FOR_SUBAGENTS_ONLY` | `rgb(219,39,119)` | Pink 600 | 479 |
| 39 | `cyan_FOR_SUBAGENTS_ONLY` | `rgb(8,145,178)` | Cyan 600 | 480 |
| 40 | `professionalBlue` | `rgb(106,155,204)` | (none) | 482 |
| 41 | `chromeYellow` | `rgb(251,188,4)` | Chrome yellow | 484 |
| 42 | `clawd_body` | `rgb(215,119,87)` | (none) | 486 |
| 43 | `clawd_background` | `rgb(0,0,0)` | (none) | 487 |
| 44 | `userMessageBackground` | `rgb(55, 55, 55)` | Lighter grey for better visual contrast | 488 |
| 45 | `userMessageBackgroundHover` | `rgb(70, 70, 70)` | (none) | 489 |
| 46 | `messageActionsBackground` | `rgb(44, 50, 62)` | cool gray, slight blue | 490 |
| 47 | `selectionBg` | `rgb(38, 79, 120)` | classic dark-mode selection blue (VS Code dark default); light fgs stay readable | 491 |
| 48 | `bashMessageBackgroundColor` | `rgb(65, 60, 65)` | (none) | 492 |
| 49 | `memoryBackgroundColor` | `rgb(55, 65, 70)` | (none) | 494 |
| 50 | `rate_limit_fill` | `rgb(177,185,249)` | Light blue-purple | 495 |
| 51 | `rate_limit_empty` | `rgb(80,83,112)` | Medium blue-purple | 496 |
| 52 | `fastMode` | `rgb(255,120,20)` | Electric orange for dark bg | 497 |
| 53 | `fastModeShimmer` | `rgb(255,165,70)` | Lighter orange for shimmer | 498 |
| 54 | `briefLabelYou` | `rgb(122,180,232)` | Light blue | 499 |
| 55 | `briefLabelClaude` | `rgb(215,119,87)` | Brand orange | 500 |
| 56 | `rainbow_red` | `rgb(235,95,87)` | (none) | 501 |
| 57 | `rainbow_orange` | `rgb(245,139,87)` | (none) | 502 |
| 58 | `rainbow_yellow` | `rgb(250,195,95)` | (none) | 503 |
| 59 | `rainbow_green` | `rgb(145,200,130)` | (none) | 504 |
| 60 | `rainbow_blue` | `rgb(130,170,220)` | (none) | 505 |
| 61 | `rainbow_indigo` | `rgb(155,130,200)` | (none) | 506 |
| 62 | `rainbow_violet` | `rgb(200,130,180)` | (none) | 507 |
| 63 | `rainbow_red_shimmer` | `rgb(250,155,147)` | (none) | 508 |
| 64 | `rainbow_orange_shimmer` | `rgb(255,185,137)` | (none) | 509 |
| 65 | `rainbow_yellow_shimmer` | `rgb(255,225,155)` | (none) | 510 |
| 66 | `rainbow_green_shimmer` | `rgb(185,230,180)` | (none) | 511 |
| 67 | `rainbow_blue_shimmer` | `rgb(180,205,240)` | (none) | 512 |
| 68 | `rainbow_indigo_shimmer` | `rgb(195,180,230)` | (none) | 513 |
| 69 | `rainbow_violet_shimmer` | `rgb(230,180,210)` | (none) | 514 |

69 keys. Note the rainbow values are **identical across light and dark** (compare
`theme.ts:174-188` with `theme.ts:501-514`), as are the eight `*_FOR_SUBAGENTS_ONLY` keys
(`theme.ts:142-149` vs `theme.ts:473-480`), `professionalBlue`, `chromeYellow` and `ide`.

Duplicate values inside dark: `permission` = `suggestion` = `remember` = `rate_limit_fill` =
`rgb(177,185,249)`; `autoAccept` = `merged` = `rgb(175,135,255)`; `claude` = `clawd_body` =
`briefLabelClaude` = `rgb(215,119,87)`.

### 2. Accent (branding) and its shimmer

- Accent: `claude` = `rgb(215,119,87)`, commented "Claude orange" (`theme.ts:443`). It is the
  **same value in light and dark** (`theme.ts:118`), so it is a fixed brand color, not a
  theme-derived one.
- Shimmer variant: `claudeShimmer` = `rgb(235,159,127)` (`theme.ts:444`). Light uses a different
  lighter value, `rgb(245,149,117)` (`theme.ts:119`), so the shimmer *is* theme-dependent.
- Where the pair is bound: `src/components/Spinner.tsx:212-215`
  — `const defaultColor: keyof Theme = 'claude'`, `const defaultShimmerColor = 'claudeShimmer'`,
  then `messageColor = overrideColor ?? defaultColor` and
  `shimmerColor = overrideShimmerColor ?? defaultShimmerColor`. So the spinner verb line is the
  primary place the brand orange appears in the conversation window.
- `claude` is also the brief/assistant label color (`briefLabelClaude`, `theme.ts:500`, used in
  `src/components/messages/Brief/UI.tsx`) and the ASCII mascot body (`clawd_body`, used only by
  `Clawd.tsx` and `WelcomeV2.tsx`).

### 3. Per-key usage

Legend for **Conv?**: **Y** = appears in the conversation window (messages, tool blocks,
spinner, diffs, permission prompts); **N** = other surface (dialogs, onboarding, status, IDE,
usage screens) or unused.

| Key | Conv? | Where used |
|-----|:---:|------------|
| `autoAccept` | N | Permission-mode chrome only. `src/utils/permissions/PermissionMode.ts:59-63` maps mode `acceptEdits` → `color: 'autoAccept'`, symbol `⏵⏵`. Also `CompanionSprite.tsx`. |
| `bashBorder` | Y | The `!` bash-input marker: `src/components/messages/UserBashInputMessage.tsx:34` `<Text color="bashBorder">! </Text>`. Also `PromptInput.tsx`, `PromptInputModeIndicator.tsx`, `PromptInputFooterLeftSide.tsx`, `MessageSelector.tsx`. |
| `claude` | Y | Spinner verb default color (`Spinner.tsx:212`). Widely referenced elsewhere; note the literal-string scan for `"claude"` is noisy (48 files) because `'claude'` is also a provider/model token. |
| `claudeShimmer` | Y | Spinner shimmer sweep only (`Spinner.tsx:213`). Single use site. |
| `claudeBlue_FOR_SYSTEM_SPINNER` | Y | Set only during compaction/hook phases: `src/screens/REPL.tsx:2500` `setSpinnerColor('claudeBlue_FOR_SYSTEM_SPINNER')` inside `onCompactProgress` `case 'hooks_start'`. Cleared to `null` on `compact_end` (`REPL.tsx:2508-2510`). |
| `claudeBlueShimmer_FOR_SYSTEM_SPINNER` | Y | Same site, `REPL.tsx:2501`. |
| `permission` | N/Y | Permission dialog chrome; `PermissionMode.ts:27-31` lists it as a valid mode color. 42 files, mostly dialogs (`Dialog.tsx`, `AddPermissionRules.tsx`, `ConsoleOAuthFlow.tsx`). |
| `permissionShimmer` | — | **Unused.** Only appears in `theme.ts` (declaration line 12, values 123/205/286/367/448). No consumer anywhere in the repo. |
| `planMode` | Y | Plan-mode messages: `EnterPlanModePermissionRequest.tsx`, `ExitPlanModePermissionRequest.tsx`, `PlanApprovalMessage.tsx`, `RejectedPlanMessage.tsx`, `UserPlanMessage.tsx`; and `PermissionMode.ts:51-56` (mode `plan`, symbol `PAUSE_ICON`). |
| `ide` | N | IDE connection surfaces only: `IdeStatusIndicator.tsx`, `IdeAutoConnectDialog.tsx`, `IdeOnboardingDialog.tsx`, `ManagePlugins.tsx`. |
| `promptBorder` | N | Input box border (`PromptInput.tsx`), `FastIcon.tsx`, `useSwarmBanner.ts`. Not part of the transcript. |
| `promptBorderShimmer` | — | **Unused.** Declared (line 16) and valued, never read. |
| `text` | Y | Default foreground. Explicitly set on the `⏺` message bullet when not selected: `src/components/messages/AssistantTextMessage.tsx:232` `<Text color={isSelected ? "suggestion" : "text"}>{BLACK_CIRCLE}</Text>`; also `CompactSummary.tsx:34,77`, `Messages.tsx:706`, `Brief/UI.tsx:29`, `UserBashInputMessage.tsx:41`, `UserMemoryInputMessage.tsx:51`. `PermissionMode.ts:44-49` uses it as the `default` mode color. 191 files. |
| `inverseText` | N/Y | Used on inverted chips/badges: `AssistantToolUseMessage.tsx`, `AgentProgressLine.tsx`, `BackgroundTaskStatus.tsx`, `Tabs.tsx`, `PromptInput.tsx`, `QuestionNavigationBar.tsx`, `ColorPicker.tsx`, `AgentDetail.tsx`. |
| `inactive` | Y | **This is what `dimColor` resolves to.** `src/components/design-system/ThemedText.tsx:104`: `dimColor ? theme.inactive as Color : …`. Since almost every secondary line in the transcript is `<Text dimColor>`, `rgb(153,153,153)` is the de-facto secondary text color. Also explicit in `ListItem.tsx:226` (`description`) and as `messageColor` for the "requesting…" shimmer in `BashPermissionRequest.tsx:54`. |
| `inactiveShimmer` | — | **Unused as a key.** Note: the thinking shimmer hardcodes the same idea numerically instead — see §4. |
| `subtle` | Y | Structural/very-dim chrome. Spinner right-hand status text: `Spinner.tsx:419,483` `<Text color="subtle">{rightText}</Text>`. Also `FileEditToolDiff.tsx`, `FileWriteToolDiff.tsx`, `FileEditToolUseRejectedMessage.tsx`, `HighlightedThinkingText.tsx`, `ExitPlanModePermissionRequest.tsx`, `BashPermissionRequest.tsx:54` (`shimmerColor="subtle"`), `FullscreenLayout.tsx`. |
| `suggestion` | Y | Selection / info accent. `AssistantTextMessage.tsx:232` (selected bullet), `design-system/ListItem.tsx:127` (`figures.pointer` cursor), `design-system/StatusIcon.tsx:38-41` (`info` status icon). 59 files. |
| `remember` | Y | The `#` memory marker: `src/components/messages/UserMemoryInputMessage.tsx:44` `<Text color="remember" backgroundColor="memoryBackgroundColor">#</Text>`. Also `memory.tsx`, `ThinkingToggle.tsx`, `AskUserQuestionTool.tsx`. |
| `background` | N/Y | Background-task surfaces (`BackgroundTaskStatus.tsx`, `BackgroundTasksDialog.tsx`, `AgentTool.tsx`), diff dialogs. **Despite the name it is a foreground accent** (`rgb(0,204,204)`, bright cyan) — it is not the terminal background. |
| `success` | Y | Resolved tool bullet: `src/components/ToolUseLoader.tsx:19` `const color = isUnresolved ? undefined : isError ? "error" : "success"`. Also `design-system/StatusIcon.tsx:28-31` (`figures.tick`), `ListItem.tsx:207`. 117 files. |
| `error` | Y | Failed tool bullet (`ToolUseLoader.tsx:19`), every assistant error line (`AssistantTextMessage.tsx:40,92,103,125,137,149,160,171,203`), system error bullet (`SystemTextMessage.tsx:103`), stalled spinner (`Spinner.tsx:406,475`), `StatusIcon.tsx:32-35` (`figures.cross`), and `PermissionMode.ts:65-77` for `bypassPermissions`/`dontAsk`. 264 files — the most used key. |
| `warning` | Y | `StatusIcon.tsx:36-39` (`figures.warning`), plus 132 files of warnings/banners. `PermissionMode.ts:78-88` uses it for ant-only `auto` mode. |
| `warningShimmer` | — | **Unused.** |
| `merged` | N | Git/PR status only: `PrBadge.tsx`, `ghPrStatus.ts`, `gitOperationTracking.ts`, `CollapsedReadSearchContent.tsx`, `hooks.ts`. |
| `diffAdded` | Y | Line background for an added diff line: `src/components/StructuredDiff/Fallback.tsx:329,403`. |
| `diffRemoved` | Y | Line background for a removed diff line: same lines. |
| `diffAddedDimmed` | Y | Same, when the diff is rendered `dim` (`Fallback.tsx:329,403`). |
| `diffRemovedDimmed` | Y | Same. |
| `diffAddedWord` | Y | Word-level background inside an added line: `Fallback.tsx:275,279`. Also `DiffDialog.tsx`, `DiffFileList.tsx`, `MessageSelector.tsx`, `IdeOnboardingDialog.tsx`. |
| `diffRemovedWord` | Y | `Fallback.tsx:275,286` + same four files. |
| `red/blue/green/yellow/purple/orange/pink/cyan_FOR_SUBAGENTS_ONLY` | Y | Sub-agent identity colors. Single mapping table: `src/tools/AgentTool/agentColorManager.ts:24-33` `AGENT_COLOR_TO_THEME_COLOR`. `getAgentColor()` (line 35) returns `undefined` for `general-purpose`, so the default agent is **not** colored. `cyan_FOR_SUBAGENTS_ONLY` additionally appears in `TaskAssignmentMessage.tsx`, `TeammateSpinnerTree.tsx`, `useSwarmBanner.ts`. |
| `professionalBlue` | N | `Grove.tsx`, `HelpV2.tsx` only. |
| `chromeYellow` | N | Claude-in-Chrome surfaces: `chrome.tsx`, `ClaudeInChromeOnboarding.tsx`. |
| `clawd_body` | N | ASCII mascot: `Clawd.tsx`, `WelcomeV2.tsx` (36 references, 2 files). |
| `clawd_background` | N | Same two files. |
| `userMessageBackground` | Y | Background of a user prompt block: `src/components/messages/UserPromptMessage.tsx:76`, `UserCommandMessage.tsx:62,99`, `FullscreenLayout.tsx:509,558`. |
| `userMessageBackgroundHover` | Y | Mouse-hover variant of the above, alt-screen only: `FullscreenLayout.tsx:509,558`, `VirtualMessageList.tsx`. |
| `messageActionsBackground` | Y | A message that is selected in the message-actions UI: `UserPromptMessage.tsx:76` (`isSelected ? 'messageActionsBackground' : …`), `messageActions.tsx:213`, `AssistantTextMessage.tsx`. |
| `selectionBg` | Y | Mouse text selection. Not read as `theme.selectionBg` but via `getTheme(themeName).selectionBg` in `src/hooks/useCopyOnSelect.ts:96`, pushed into the Ink style pool (`ink.tsx:1131-1145`, `screen.ts:239-256`, `selection.ts:914`). Replaces the cell background while preserving the foreground. |
| `bashMessageBackgroundColor` | Y | Background of the `! …` bash input block: `UserBashInputMessage.tsx:49`. |
| `memoryBackgroundColor` | Y | Background of the `# …` memory input block: `UserMemoryInputMessage.tsx:44,51`. |
| `rate_limit_fill` / `rate_limit_empty` | N | Usage bar only: `Usage.tsx`. |
| `fastMode` | N | `FastIcon.tsx`, `fast.tsx`, `Config.tsx`, `useFastModeNotification.tsx`. |
| `fastModeShimmer` | — | **Unused.** |
| `briefLabelYou` | N/Y | Brief layout only: `HighlightedThinkingText.tsx`. |
| `briefLabelClaude` | N/Y | Brief layout only: `src/components/messages/Brief/UI.tsx`. |
| `rainbow_*` (7) | Y | Ultrathink keyword highlighting in the prompt: `src/utils/thinking.ts:59-84` — `RAINBOW_COLORS` array and `getRainbowColor(charIndex, shimmer)` returning `colors[charIndex % colors.length]`. Consumed at `PromptInput.tsx:693,708,722,735`. |
| `rainbow_*_shimmer` (7) | Y | Same function with `shimmer = true` (`thinking.ts:69-77,80-84`); every `PromptInput.tsx` call site passes `true`. |

**Keys with no consumer at all (5):** `permissionShimmer`, `promptBorderShimmer`,
`inactiveShimmer`, `warningShimmer`, `fastModeShimmer`. Verified by scanning every
`.ts`/`.tsx`/`.js` file in `C:\dev\git\claude-code` outside `node_modules`; the only hits are the
declaration in the `Theme` type and the six theme literals.

### 4. Colors chosen dynamically, not from the palette

| Effect | Mechanism | Source |
|--------|-----------|--------|
| Colour interpolation primitive | `interpolateColor(c1, c2, t)` does per-channel `Math.round(c1.x + (c2.x - c1.x) * t)`; `toRGBColor` formats it as `` `rgb(${r},${g},${b})` ``; `parseRGB` reads an `rgb(...)` theme string into `{r,g,b}` with a module-level cache. | `src/components/Spinner/utils.ts:14-29`, `:69-80` |
| **Stall → red fade** (the one the map calls "interpolated stall colors") | When `stalledIntensity > 0`, the spinner verb is `interpolateColor(parseRGB(theme[messageColor]), ERROR_RED, stalledIntensity)`. `ERROR_RED` is **hardcoded** `{ r: 171, g: 43, b: 63 }` — which is the *light* theme's `error` (`theme.ts:137`), **not** dark `error` `rgb(255,107,128)`. So on dark the fade target is off-palette. | `src/components/Spinner/GlimmerMessage.tsx:18-22`, `:86-92` |
| Stall timing | Stall begins after 3000 ms with no new tokens and no active tools; intensity ramps `min((t-3000)/2000, 1)` over 2 s, then is smoothed on the animation clock. | `src/components/Spinner/useStalledAnimation.ts:41-49` |
| Stall fallback (no truecolor parse) | If `parseRGB` fails, it degrades to a hard switch: `stalledIntensity > 0.5 ? "error" : messageColor`. | `GlimmerMessage.tsx:104` |
| **Tool-use flash** | In `mode === "tool-use"` the verb colour is `interpolateColor(messageColor, shimmerColor, flashOpacity)` — i.e. a continuous blend between `claude` and `claudeShimmer`. Fallback: `flashOpacity > 0.5 ? shimmerColor : messageColor`. | `GlimmerMessage.tsx:134-141`, `:162` |
| **Thinking shimmer** | `interpolateColor(THINKING_INACTIVE, THINKING_INACTIVE_SHIMMER, thinkingOpacity)`, both **hardcoded**: `{153,153,153}` and `{185,185,185}`. `{153,153,153}` equals dark `inactive`; `{185,185,185}` is close to but **not equal** to `inactiveShimmer` `rgb(193,193,193)`. Starts after `THINKING_DELAY_MS = 3000`, glow period `THINKING_GLOW_PERIOD_S = 2`. | `src/components/Spinner/SpinnerAnimationRow.tsx:23-33`, `:195-200`, `:210` |
| Shimmer sweep (position, not colour) | `computeShimmerSegments(text, glimmerIndex)` splits the verb into `{before, shimmer, after}` around a 3-column window (`glimmerIndex-1 … glimmerIndex+1`); tick interval `SHIMMER_INTERVAL_MS = 150`. `before`/`after` are `dimColor`, the swept segment uses the default fg. | `src/bridge/bridgeStatusUtil.ts:20-21,60,79-110`; `Spinner.tsx:372-373,406` |
| Per-character shimmer | `ShimmerChar` picks `shimmerColor` when the char is at or adjacent to `glimmerIndex`, else `messageColor` — a discrete switch, no blend. | `src/components/Spinner/ShimmerChar.tsx:19-24` |
| Per-character flash | `FlashingChar` blends `messageColor`→`shimmerColor` by `flashOpacity`, with the same `>0.5` discrete fallback. | `src/components/Spinner/FlashingChar.tsx:22-49` |
| Rainbow highlighting | Not interpolated — `getRainbowColor(i, shimmer)` indexes a fixed 7-entry palette array modulo its length. Genuinely palette-driven. | `src/utils/thinking.ts:59-84` |
| Hover backgrounds | Discrete key swap, not a computed tint: `hover ? "userMessageBackgroundHover" : "userMessageBackground"`. | `src/components/FullscreenLayout.tsx:509,558` |
| Voice-mode waveform | `hueToRgb(hue)` generates HSL(h, 0.7, 0.6) → RGB entirely outside the theme. | `src/components/Spinner/utils.ts:31-66` |
| Syntax highlighting | A completely separate hardcoded Monokai-ish palette, unrelated to `Theme` (`keyword: rgb(249,38,114)`, `string: rgb(230,219,116)`, …), plus separate diff colors for the IDE-style diff (`addLine: rgb(2,40,0)`, `deleteLine: rgb(61,1,0)`, …). | `src/components/HighlightedCode/index.ts:191-214`, `:303-354` |
| Tab status indicator | Hardcoded `rgb(0,215,95)`, `rgb(255,149,0)`, `rgb(95,135,255)`, `rgb(136,136,136)` — off-palette. | `src/hooks/use-tab-status.ts:26-38` |
| 256-color downgrade | On Apple Terminal, chalk is forced to level 2; the comment notes `rgb(215,119,87)` (the accent) degrades to index 174 `rgb(215,135,135)`, "washed-out salmon". | `src/utils/theme.ts:617-621`; `src/ink/colorize.ts:10-11` |

Also worth recording for later tickets: `ansi:cyan` is used **raw**, bypassing the theme, in
`TranscriptSharePrompt.tsx:53` and `SkillImprovementSurvey.tsx:95`.

### 5. Mapping against picopilot today

picopilot has no palette module. Colors are `ratatui::style::Color` literals scattered through
`src/tui.rs`. Observed set (61 `Color::` sites):

| picopilot color | Role in `src/tui.rs` | Nearest Claude Code key |
|---|---|---|
| `Rgb(240, 177, 94)` | The de-facto accent: user prefix `❯ ` ([tui.rs#L2944](src/tui.rs#L2944)), busy spinner glyph `✻` ([tui.rs#L2123](src/tui.rs#L2123)), nearly every modal border, selected-row background, markdown list bullets and task markers ([tui.rs#L3242](src/tui.rs#L3242), [tui.rs#L3279](src/tui.rs#L3279)) | `claude` `rgb(215,119,87)` — but picopilot's is yellower and is also used for borders and selection, which Claude Code splits across `promptBorder` and `suggestion` |
| `Rgb(154, 230, 180)` | Assistant message text ([tui.rs#L2979](src/tui.rs#L2979)) | none — Claude Code renders assistant text as plain `text` white and colors only the `⏺` bullet |
| `Color::DarkGray` | Reasoning text, hints, status-bar label, markdown rule ([tui.rs#L2987](src/tui.rs#L2987), [tui.rs#L2093](src/tui.rs#L2093)) | `inactive` `rgb(153,153,153)` (what `dimColor` resolves to) |
| `Rgb(165, 174, 187)` | `debug` diagnostics label ([tui.rs#L2971](src/tui.rs#L2971)) | no counterpart; closest is `inactive` |
| `Rgb(139, 181, 255)` | Tool blocks, markdown headings and links ([tui.rs#L3025](src/tui.rs#L3025), [tui.rs#L3263](src/tui.rs#L3263)) | `suggestion` `rgb(177,185,249)` for the accent role; tool blocks have no single color in Claude Code (bullet is `success`/`error`/dim) |
| `Rgb(204, 166, 255)` | Sub-agent blocks ([tui.rs#L3055](src/tui.rs#L3055)) | the eight `*_FOR_SUBAGENTS_ONLY` keys — Claude Code assigns a *per-agent* color, picopilot uses one |
| `Rgb(242, 204, 96)` | Warning banner, inline code span ([tui.rs#L3071](src/tui.rs#L3071), [tui.rs#L3227](src/tui.rs#L3227)) | `warning` `rgb(255,193,7)` |
| `Rgb(255, 169, 122)` | `retry` banner ([tui.rs#L3077](src/tui.rs#L3077)) | none — Claude Code has no recoverable-error tier |
| `Rgb(255, 117, 117)` | `error` banner ([tui.rs#L3083](src/tui.rs#L3083)) | `error` `rgb(255,107,128)` |
| `Rgb(255, 219, 129)` | Approval prompts ([tui.rs#L3109](src/tui.rs#L3109)) | `permission` `rgb(177,185,249)` |
| `Rgb(70, 88, 104)` | Inner/secondary borders ([tui.rs#L2586](src/tui.rs#L2586)) | `subtle` `rgb(80,80,80)` |
| `Rgb(132, 147, 160)` | Markdown blockquote `> ` ([tui.rs#L3275](src/tui.rs#L3275)) | none |
| `Rgb(180, 190, 200)` | Markdown code block body ([tui.rs#L3284](src/tui.rs#L3284)) | none in `Theme`; Claude Code uses the separate syntax palette |
| `Color::Black` on `Rgb(240,177,94)` | Selected modal row ([tui.rs#L2414](src/tui.rs#L2414)) | `inverseText` on an accent |
| `Color::White` | Unselected modal row ([tui.rs#L2532](src/tui.rs#L2532)) | `text` `rgb(255,255,255)` |

**Claude Code keys with no meaningful picopilot counterpart** (safe to drop for now, or to adopt
only if the matching feature is built):

- Unused even in Claude Code: `permissionShimmer`, `promptBorderShimmer`, `inactiveShimmer`,
  `warningShimmer`, `fastModeShimmer`.
- Features picopilot does not have: `ide`, `merged`, `chromeYellow`, `professionalBlue`,
  `clawd_body`, `clawd_background`, `fastMode`, `rate_limit_fill`, `rate_limit_empty`,
  `briefLabelYou`, `briefLabelClaude`, `autoAccept`, `planMode`, `background`.
- Depend on features under separate tickets: the 14 `rainbow_*` keys (ultrathink highlighting),
  the six `diff*` keys (structured diff), `selectionBg` / `userMessageBackgroundHover` /
  `messageActionsBackground` (mouse in alt-screen — and the map has already decided to leave
  alt-screen, so these three are likely dead on arrival), `bashMessageBackgroundColor` and
  `remember` / `memoryBackgroundColor` (`!` and `#` input modes).
- Genuinely needed for parity: `claude`, `claudeShimmer`, `text`, `inactive`, `subtle`,
  `success`, `error`, `warning`, `suggestion`, `permission`, `inverseText`,
  `userMessageBackground`, the eight `*_FOR_SUBAGENTS_ONLY` keys, and
  `claudeBlue_FOR_SYSTEM_SPINNER` + its shimmer if a system spinner is kept.

**picopilot colors with no Claude Code counterpart:** `Rgb(154,230,180)` (green assistant body
text — Claude Code assistant text is plain white), `Rgb(255,169,122)` (the `retry` severity tier),
`Rgb(165,174,187)` (the `debug` diagnostics channel behind `Ctrl+I`), `Rgb(132,147,160)`
(blockquote marker) and `Rgb(180,190,200)` (code-block body). The last two exist because picopilot
renders markdown itself; Claude Code delegates code blocks to a separate hardcoded syntax palette
and has no `Theme` key for them.

### Not determined from source

- **Whether `subtle` `rgb(80,80,80)` is legible on the user's terminal background.** The palette
  has no key for the terminal background itself (`background` `rgb(0,204,204)` is a cyan
  *foreground* accent, and `clawd_background` `rgb(0,0,0)` is mascot-specific). Claude Code never
  paints a full-screen background, so contrast depends on the user's terminal. Checked: all 69
  keys, `colorize.ts`, `ThemedText.tsx`.
- **Which exact glyph/indent each color pairs with.** Colors were traced, not layout; that is
  other tickets' work.
- **Whether the `ERROR_RED` mismatch in `GlimmerMessage.tsx:18-22` is deliberate.** The value is
  the light theme's `error`, with no comment explaining it. Checked `GlimmerMessage.tsx`,
  `useStalledAnimation.ts`, `Spinner.tsx`, `SpinnerAnimationRow.tsx` and the git-less source tree;
  nothing states intent.
