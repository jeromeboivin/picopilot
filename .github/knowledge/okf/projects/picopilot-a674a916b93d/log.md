# Knowledge extraction log

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
