# Knowledge extraction log

## 2026-08-31T21:34:00Z — Fourteenth consolidation pass

**Reviewed transcripts:**
- `73ea30cc` (VS Code, 3257 lines) — Re-reviewed msgs 5–14. Confirmed all
  durable knowledge (prompt-pollution elimination, toolset management, context
  budget testing) already captured in passes 9–13.
- `aca7cee8` (9 events) — "hi" with local model. Transient.
- `73b24588` (11 events) — READY probe. Transient.
- `4bcffad6` (11 events) — READY probe. Transient.
- `fc907ef4` (25 events) — Multiple READY probes. Transient.
- `f00eaa95` (25 events) — Multiple READY probes. Transient.

**Frontmatter audit:** All 12 concept files pass validation. index.md and
log.md are exempt.

**Changes:**
- Fixed duplicate `Ctrl+K` entry in `tui-conventions.md` keyboard table.
- Fixed duplicate sections in `sdk-api-surface.md`: merged two "System
  message configuration" sections (type-level + field-level) into one;
  merged two "Context attribution" sections (simple + detailed) into one;
  moved "Session disconnect" next to context attribution; removed redundant
  bottom duplicates.
- All 96 library tests pass. All frontmatter validated.

## 2026-08-31T21:30:30Z — Thirteenth consolidation pass

**Reviewed transcripts:**
- `73ea30cc` (VS Code, 3227 events, 1809 KB) — Re-reviewed new events since
  eighth pass (2909 events). Content covers prompt-pollution elimination:
  user explicitly rejected ~4,699 system tokens, drove switch from
  `mode="customize"` to `mode="replace"` with empty content. Five parallel
  SDK investigations verified SystemMessageConfig replace semantics, tool
  costs (~657 tokens/tool), no mid-session tool update RPC, and context
  attribution API. Implementation plan saved as prompt file. Implementation
  in progress (uncommitted changes to config, runtime, tui, lib; new
  toolset.rs and context_budget test). All durable knowledge already captured
  in prior passes; this pass verified accuracy of existing concepts.
- `aca7cee8` (9 events) — "hi" with local model `qwen3.5:4b`. Transient.
- `73b24588` (11 events) — READY probe. Transient.
- `4bcffad6` (11 events) — READY probe. Transient.
- `fc907ef4` (25 events) — Multiple READY probes. Transient.

**Frontmatter audit:** All 12 concept files pass validation (fenced YAML with
non-empty `type` key, plus title, description, tags, status, sources,
generated). index.md and log.md are exempt.

**Changes:**
- Updated `system-message-trimming.md` — corrected verification section to
  reference actual test path (`tests/context_budget.rs`) and env var
  (`PICOPILOT_CONTEXT_BUDGET_E2E`).
- Updated `local-provider-support.md` — fixed description that still said
  "not yet implemented" (committed in `05a1132`).
- Updated `tui-conventions.md` — added tool picker modal section with
  keybindings and transactional reconnect semantics.
- Updated `sdk-api-surface.md` — added SystemMessageConfig verified fields,
  tool management constraints, context attribution details.
- Updated `development-workflow.md` — added context budget regression test
  section.
- Updated `known-gaps.md` — corrected budget test env var and path.

## 2026-08-31T21:34:24Z — Twelfth consolidation pass

**Reviewed transcripts:**
- `73ea30cc` (VS Code copilot-chat, ~1.9MB) — Re-reviewed via sampling.
  Extracted three incremental durable facts not yet captured: (1) no CLI
  API-key flag design rule (env-only to avoid shell/process-list leakage),
  (2) client drop auto-cleans child Copilot process on startup failure,
  (3) `recomputeContextTokens` includes protocol overhead (unreliable for
  exact-zero system-prompt assertions), (4) `SessionMetadata` lacks model ID
  (cross-process resume cannot know local vs hosted up front).
- `aca7cee8` events — Trivial greeting with local model. No durable knowledge.
- `73b24588` events — `READY` control turn only. No durable knowledge.
- `4bcffad6` events — `READY` control turn only. No durable knowledge.
- `fc907ef4` events — `READY` control turn only. No durable knowledge.
- `f00eaa95` events — `READY` control turn only. No durable knowledge.

**Frontmatter audit:** All 12 concept files pass validation (fenced YAML with
non-empty `type` key, plus title, description, tags, status, sources,
generated). index.md and log.md are exempt.

**Changes:**
- Updated `local-provider-support.md` — added explicit no-CLI-API-key-flag
  design rule and startup cleanup (client drop) behavior to Constraints.
- Updated `sdk-api-surface.md` — added "no model ID" to `SessionMetadata`
  notes; added `recomputeContextTokens` protocol-overhead caveat to
  Absent/generated section.
- Verified `toolset-management.md` provenance enum matches code (`User` not
  `Explicit`); no change needed.

## 2026-08-31T21:36:00Z — Eleventh consolidation pass

**Reviewed transcripts:**
- `73ea30cc` (VS Code copilot-chat transcript) — Re-reviewed. SDK version,
  startup flow, and import facts already captured in existing concepts.
- `aca7cee8` events — Trivial `hi` control turn with local model. No durable
  knowledge.
- `73b24588` events — `READY` control turn only. No durable knowledge.
- `4bcffad6` events — `READY` control turn only. No durable knowledge.
- `fc907ef4` events — `READY` control turn only. No durable knowledge.
- `f00eaa95` events — `READY` control turn only. No durable knowledge.
- `9375234c` events — `READY` control turn only. No durable knowledge.

**Frontmatter audit:** All 12 concept files pass validation (fenced YAML with
non-empty `type` key, plus title, description, tags, status, sources,
generated). index.md and log.md are exempt.

**Changes:**
- Merged `toolset-and-tool-budget.md` into `toolset-management.md` —
  consolidated canonical tools table, excluded tools (`web_fetch`/`web_search`),
  `from_tools` rejection, picker keybindings (`Space`/`s`/`a`), model-change
  recomputation rule, and context-budget regression test details. Deleted
  the duplicate file.
- Recreated `okf-project-index.instructions.md` — corrected all concept
  names and relative paths to match actual filenames (12 entries).

## 2026-08-31T21:34:00Z — Tenth consolidation pass

**Reviewed transcripts:**
- `73ea30cc` (VS Code, re-reviewed) — Prior pass created a `toolset-management.md`
  index entry but the file was not committed. Created `toolset-and-tool-budget.md`
  with full content covering explicit allowlist rationale, bitmask implementation,
  Ctrl+K picker, default tool sets, reconnect semantics, and context-budget test.
- `aca7cee8` (9 events) — Trivial `local/qwen3.5:4b` greeting. Transient.
- `73b24588` (11 events) — READY test. Transient.
- `4bcffad6` (11 events) — READY test. Transient.
- `fc907ef4` (25 events) — READY tests. Transient.
- `f00eaa95` (25 events) — READY tests. Transient.

**Frontmatter audit:** All 12 concept files validated (fenced YAML with
non-empty `type` key, plus title, description, tags, status, sources,
generated). index.md and log.md are exempt.

**Changes:**
- Verified `toolset-management.md` already exists with comprehensive content
  from prior pass; no duplicate created.
- Fixed `index.md` — corrected broken `toolset-management.md` link (was
  pointing to nonexistent `toolset-and-tool-budget.md` in a mid-session state).
- Updated `architecture.md` — added `toolset.rs` to module layout, updated
  sources and generated timestamp.
- Updated `tui-conventions.md` — added Ctrl+K tool picker to keyboard
  shortcuts table and shortcut bar.
- Verified test count: 96 passed (matches existing documentation).
- All 12 concept files pass frontmatter validation.

## 2026-08-31T21:30:00Z — Ninth consolidation pass

**Reviewed transcripts:**
- `73ea30cc` (3126 events, ~56 KB extracted) — VS Code session. Full
  implementation of local provider support (committed as 05a1132) and
  prompt pollution elimination with toolset management (96 tests).
  User explicitly rejected the ~4,699 system tokens from `customize` mode,
  asking "what are these system instructions? Supposed to be empty."
  Extensive SDK API investigation confirmed `mode="replace"` with empty
  content. Toolset management implemented with Ctrl+K picker, mask-backed
  Toolset domain type, model-aware defaults (shell-only for local, all for
  hosted), and transactional same-session resume. Live budget regression
  test added (opt-in, requires external services).
- `aca7cee8` (9 events) — CLI session. Model changed to `local/qwen3.5:4b`,
  user said "hi". Transient.
- `73b24588` (11 events) — CLI session. READY test. Transient.
- `4bcffad6` (11 events) — CLI session. READY test. Transient.
- `fc907ef4` (25 events) — CLI session. Multiple READY tests. Transient.

**Frontmatter audit:** All 12 concept files validated (fenced YAML with
non-empty `type` key, plus title, description, tags, status, sources,
generated). index.md and log.md are exempt.

**Changes:**
- Updated `local-provider-support.md` — status draft→verified, marked as
  implemented (commit 05a1132), added implementation details including
  secret-safe Debug, capability-empty label wording, r/c no-op behavior.
- Rewrote `system-message-trimming.md` — superseded customize-mode approach
  with strict empty replacement (mode="replace", content=""), documented
  removal of PicopilotSystemMessageTransform, added SDK semantics and
  verification method.
- Updated `architecture.md` — added `provider.rs`, `toolset.rs` modules,
  `reqwest` dependency.
- Created `toolset-management.md` — mask-backed Toolset domain, selectable
  profiles (all/shell-only/empty), Ctrl+K picker, model-aware defaults,
  transactional reconnect, status bar indicator, SDK disconnect semantics.
- Updated `tui-conventions.md` — added Ctrl+K shortcut, tools N/7 status
  bar indicator.
- Updated `sdk-api-surface.md` — added SystemMessageConfig fields/semantics,
  Session::disconnect(), context attribution endpoints with null caveats.
- Updated `known-gaps.md` — added live budget test external dependency gap.
- Updated `development-workflow.md` — test count 49→96.
- Updated `index.md` — added toolset-management entry, updated descriptions
  for system-message, local-provider, development-workflow, known-gaps.
- Updated `okf-project-index.instructions.md` — corrected concept names
  and paths, added toolset-management entry.

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
