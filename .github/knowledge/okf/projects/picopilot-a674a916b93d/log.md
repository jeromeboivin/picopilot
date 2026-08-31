# Knowledge extraction log

## 2026-08-31T19:03:00Z — Eighth consolidation pass

**Reviewed transcripts:**
- `73ea30cc` (318 events, 172 KB) — Local model/BYOK investigation session.
  User asked to investigate SDK support for Ollama and local models. Assistant
  researched `NamedProviderConfig`, `ProviderModelConfig`, and the additive
  multi-provider registry in SDK 1.0.13-preview.2. A full implementation plan
  was agreed and implementation started, but no code was committed before the
  session ended. Durable design decision and verified SDK API surface extracted.
- `7028bf67` (1729 events, 1057 KB) — Re-reviewed later messages (13–18).
  User requested TUI cleanup to resemble Claude Code CLI: borderless layout,
  `❯` prompt glyph, `●` assistant prefix, `✻` busy indicator, Markdown
  rendering via `pulldown-cmark`, reasoning always visible in dim gray.
  Also requested install scripts (install.ps1, install.sh) with user-local
  installation and PATH management. All committed (3a79fff, a3b3023).
- `f0959735` (18 events, 52 KB) — "Fetch my system info" session. Transient.
- `bc763a17` (18 events, 54 KB) — "Fetch my system info" session. Transient.
- `88e4e368` (10 events, 28 KB) — "Hi" greeting session. Transient.

**Frontmatter audit:** All 11 concept files pass validation (fenced YAML with
non-empty `type` key, plus title, description, tags, status, sources,
generated). index.md and log.md are exempt.

**Changes:**
- Created `local-provider-support.md` — BYOK/Ollama additive registry design
  decision, verified SDK API surface, agreed implementation plan (draft status).
- Updated `architecture.md` — added `pulldown-cmark` dependency, updated
  packaging section with install.ps1/install.sh and user-local directories.
- Updated `tui-conventions.md` — borderless Claude Code-like layout, `❯`/`●`/`✻`
  glyphs, Markdown rendering via pulldown-cmark, reasoning always visible in
  dim gray italics, terminal caret positioning.
- Updated `sdk-api-surface.md` — added local provider registry section with
  NamedProviderConfig, ProviderModelConfig, session vs client model listing.
- Updated `index.md` — added local-provider-support entry.
- Updated `okf-project-index.instructions.md` — corrected all concept names
  and paths, added local-provider-support entry.

## 2026-08-31T18:23:00Z — Seventh consolidation pass

**Reviewed transcripts:**
- `9b61cf62` events — Re-reviewed (495 KB). Transient system-info gathering
  session: user repeatedly attempted `systeminfo`, `wmic`, `rustc --version`,
  `cargo --version`, `git --version`; assistant hit sub-agent depth limits
  throughout. No durable project decisions, user preferences, or verified
  facts.

**Frontmatter audit:** All 10 concept files pass validation (fenced YAML
with non-empty `type` key, plus title, description, tags, status, sources,
generated). index.md and log.md are exempt.

**Changes:** None. No new durable knowledge extracted.

## 2026-08-31T16:23:00Z — Sixth consolidation pass

**Reviewed transcripts:**
- `9b61cf62` events — Re-reviewed. Transient system-info gathering session
  (user repeatedly tried `systeminfo`, `wmic`, `rustc --version` etc.;
  assistant hit sub-agent depth limits throughout). No durable project
  decisions, user preferences, or verified facts.

**Frontmatter audit:** All 10 concept files pass validation (fenced YAML
with non-empty `type` key, plus title, description, tags, status, sources,
generated). index.md and log.md are exempt.

**Changes:** None. No new durable knowledge extracted.

## 2026-08-31T18:22:00Z — Fifth consolidation pass

**Reviewed transcripts:**
- `9b61cf62` events — Transient system-info gathering session. User
  repeatedly attempted to run Windows diagnostic commands (`systeminfo`,
  `wmic`, `rustc --version`, `cargo --version`, `git --version`). Assistant
  hit tool/depth limitations throughout. No durable project decisions, user
  preferences, or verified facts.

**Changes:** None. No new durable knowledge extracted.

**Frontmatter audit:** All 10 concept files pass validation (fenced YAML
with non-empty `type` key). index.md and log.md are exempt.

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
