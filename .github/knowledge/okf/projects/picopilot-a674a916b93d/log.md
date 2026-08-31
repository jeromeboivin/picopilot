# Knowledge extraction log

## 2026-08-31T18:05:00Z — Fourth consolidation pass (frontmatter audit)

**Reviewed transcripts:**
- `7028bf67` (re-read) — No new durable knowledge beyond prior extractions.
- `f309c464` events — No durable picopilot knowledge (transient system-info
  session using gpt-5.6-luna).

**Changes:**
- Updated `tui-conventions.md` — fixed `r/R` shortcut to correctly show `r`
  for reasoning and `c` for context tier; added model-picker three-panel
  layout section with detail pane description.
- Repaired `okf-project-index.instructions.md` — replaced stale concept
  names and broken relative paths with actual filenames and correct paths.

**Frontmatter audit:** All 10 concept files pass validation (fenced YAML
with non-empty `type` key).

## 2026-08-31T18:03:00Z — Third consolidation pass (code-verified)

**Reviewed transcripts:**
- `7028bf67` (re-review) — Confirmed Ctrl-key migration rationale, picker
  simplification, and model chooser feedback from user.
- `f309c464` events — Live smoke test of picopilot binary. No durable project
  decisions; confirmed binary runs standalone from `target/debug`.

**Reviewed commits (post-previous-extraction):**
- `46de582` — Require Ctrl for all main-window shortcuts.
- `5d1eafd` — Remove per-selection session preview; local-only navigation.
- `849c061` — Resume notice demoted to debug-only diagnostic.

**Changes:**
- Updated `tui-conventions.md` — full rewrite reflecting Ctrl+key shortcuts,
  shortcut bar, diagnostic entries, session picker simplification,
  blocked-state quit controls.
- Updated `architecture.md` — Fleet dispatch moved from runtime.rs to tui.rs.
- Updated `known-gaps.md` — marked resume transcript replay as fixed.
- Removed duplicate "Known gaps" entry from `index.md`.
- Updated instructions mirror to match current concept set.

**Frontmatter audit:** All 10 concept files verified to have valid YAML
frontmatter with non-empty `type` key. index.md and log.md are exempt.

## 2026-08-31T18:04:00Z — Second consolidation pass

**Reviewed transcripts:**
- `7028bf67` (re-read) — Extracted 4 gotchas and 2 verified facts from audit
  report. Most audit fixes already applied to code; residual gaps captured.
- `f309c464` events — No new durable knowledge.

**Changes:**
- Created `known-gaps.md` — resume transcript replay, status bar cost
  polling, model picker metadata, and missing transport recovery integration
  test.
- Updated `index.md` — added known-gaps entry.

## 2026-08-31T18:04:00Z — Consolidation pass

**Reviewed transcripts:**
- `7028bf67` (re-read) — Shortcut rework (Ctrl+key, nano-style bar), model
  chooser simplification, debug-mode resume notice.
- `f309c464` events — No durable knowledge (transient PowerShell debugging).

**Changes:**
- Updated `tui-conventions.md` — shortcuts now Ctrl+key, added nano-style
  command bar, updated key table to match implemented code (46de582).
- Created `model-selection.md` — unconstrained picker, per-model options
  scope, cost-focused UX, fail-fast validation (from resolutions 006/011
  and commits 1409704, 6d86c8d).
- Updated `index.md` — added model-selection entry, updated TUI description.
- Repaired instructions mirror to match actual filenames.

## 2026-08-31T17:28:00Z — Initial extraction

**Reviewed transcripts:**
- `b8030d13` — Full implementation session: SDK API research, TDD implementation
  of config/events/permissions/runtime/tui, 20+ focused commits.
- `7028bf67` — Audit session: 9-issue audit against wayfinder map, then 6
  regression fixes (Fleet routing, telemetry hiding, user dedup, runtime
  instructions, cost units).
- `bdd88f37` — Trivial greeting ("hi"), no durable knowledge.
- `7117923a` — Trivial greeting ("Hi"), no durable knowledge.

**Concepts created:** 8 (architecture, sdk-api-surface, permission-policy,
fleet-dispatch, transport-recovery, tui-conventions, system-message-trimming,
development-workflow).

**Skipped (transient):** Individual commit SHAs, intermediate compiler errors,
terminal environment issues, formatting-only diffs.
