# 04: Verify the complete context experience

**What to build:** Context tracking 04 verifies the integrated screen, honest data/details, and live footer against the complete approved contract. Release requires independently reviewed cross-state visual fixtures, the full non-ignored suite, and an actual real-terminal smoke test with usable provider access. Each earlier slice already needs its own passing focused tests; this ticket is not an excuse for deferred coverage.

**Blocked by:** [02: Show honest context data and expanded details](02-show-honest-context-data-and-expanded-details.md), [03: Track context live in the footer](03-track-context-live-in-the-footer.md)

**Status:** blocked

- [x] Test public behavior at command input, rendered terminal output, and visible updates after usage events. Use test-first red-green slices, minimal mocking, and reuse the existing screen-model/gallery helpers; do not make private helper calls the acceptance boundary. Write and confirm failing integration tests for uncovered acceptance conditions before making the smallest required correction; earlier slices retain their own focused-test obligations.
- [x] Cover `/context` and `/context all`, removed `/usage`, Ctrl+U, retained session metrics, live updates, missing/unavailable/zero/stale/mismatched/conflicting snapshots, rounding for tiny categories, over-limit and invalid usage, session/model resets, autocomplete/picker/busy coexistence, cursor stability, and no transcript append. Together these tests cover the approved contract. Verify default and expanded views independently, including evidence-only details/suggestions and no fabricated costs or savings.
- [x] Verify the shared five-category names/order/colors, exact reference glyphs and legend symbols, free/unattributed neutral usage, measured zero, absence of unmeasured compaction reserve, context-limit-dependent grids, 80-column breakpoint, source diagnostics, refresh success/failure versus idle, real over-limit percentage, and context-limit resets across screen and footer. Verify the current hint footer only, adaptive cells/percent, control priorities, warning triggers/retention, emergency hide/restore, and in-place live redraw.
- [x] Reuse and extend checked-in fixtures for widths 20, 40, 80, and 120, add the 79/80-column breakpoint checks, and cover constrained-height input and picker cases. Independently review expected styles, text, colors, proportions, wrapping, and non-overlap across normal, expanded, missing, stale, conflicting, over-limit, and transient states. Do not regenerate expected output solely to make a failing test pass.
- [x] Run focused checks during test-first work, then run the full non-ignored Cargo test suite before accepting implementation. The focused commands are: `cargo test --test screen_model --quiet`, `cargo test --lib tui::tests --quiet`, and `cargo test --lib tui::rendering_fixtures --quiet`; the full-suite command is `cargo test`. Ignored authenticated tests are separate and are not claimed as run.
- [ ] Before release, run a real-terminal smoke test with `cargo run -- --project $project` and a usable SDK/provider session. Verify `/context`, `/context all`, Ctrl+U, resizing, and the live footer. Missing authentication or provider access blocks this gate; it is not a pass. Record explicit blocked status if the session is unavailable. Automated fixtures or the non-ignored suite do not substitute for this actual terminal test.
- [x] Use the locally verified Claude source and supplied screenshot as the visual reference. Disclose that Claude's current binary is unavailable locally, so a local binary comparison and pixel-perfect parity claim are not possible.
- [x] Keep the full validation contract explicit: behavior-boundary tests, reviewed style fixtures, the full non-ignored Cargo suite, the real-terminal release gate, and disclosure of Claude-reference limitations. No automated E2E command is specified because none was found; do not invent one. Record actual results and any blocked gate, not an unsupported completion claim.
- [x] Verify completion, help, README, and current rendering documentation accurately expose the replaced command, expanded view, honest data semantics, and current-footer behavior as updated by the applicable slices; correct only gaps in this approved experience, with no unrelated cleanup. See Documentation Maintenance below; lifecycle behavior is characterized without inventing a product decision.

**Source/Decision provenance:** [Destination and interaction contract](../../../.wayfinder/context-tracking/tickets/01-lock-context-tracking-destination.md), [reference evidence and limits](../../../.wayfinder/context-tracking/tickets/02-research-context-visualization-evidence.md), [palette and grid](../../../.wayfinder/context-tracking/tickets/03-decide-category-palette-and-grid.md), [snapshot reconciliation](../../../.wayfinder/context-tracking/tickets/04-decide-snapshot-reconciliation.md), [responsive footer](../../../.wayfinder/context-tracking/tickets/05-decide-responsive-footer-states.md), [binding validation resolution](../../../.wayfinder/context-tracking/tickets/06-decide-validation-fixtures.md).

## CT-04 Verification Evidence (2026-10-05)

ACK - CT-04: Verify the complete approved integrated context experience in
C:\dev\picopilot, reporting to the coordinating Chief through this exact ticket.
All nine verbatim criteria and six closed decisions read, together with the
CT-01/02/03 completion and review reports. Accepted baseline:
CT-01 30f8c03, CT-02 58bd04c8, CT-03
ac4cc28b95215562d2dafd020fdc769712c4ac01; documentation 1698272.
No openwiki exists. Elevated test risk YES: integrated shared snapshot/rendering
contracts and independent fixture quality. Required reviewer Expensive.

### Scope and Test-First Evidence

- Public input/render/visible-event boundaries were pre-agreed. TDD skill read.
	No production change was needed or made; this is a coverage/characterization
	extension, not a claimed production bug fix.
- Added context-busy-completion fixture at 20/40/79/80/120 columns, height 32.
	RED: committed gallery comparison failed because that scenario was absent.
	Independent public boundary assertions preceded recording the new output.
	Initial hand expectations incorrectly required the full narrow completion name
	and five rather than six meter cells; corrected after nearby renderer inspection.
	These were expectation failures, not claimed renderer regressions.
- Literal checks: clipped /conte... (Unicode ellipsis in actual output), selected
	completion below input and above footer, 6 narrow cells (1 used, 5 free), 20
	normal cells (4 used, 16 free), free #888888 DIM, and four measured used colors.
	Gallery inputs give [1,1,1,0,1] used cells; narrow largest remainder goes to
	Messages. Public test inputs tie and narrow first cell goes to System.
- GREEN: guarded writer used only after independent assertions; opt-in setting
	removed afterward. Expensive reviewer confirmed the only gallery insertion is
	the new section: old sections unchanged, 90 -> 91 named sections.
- Added integrated test with actual keys, usage event, and production
	draw_with_screen_for_platform path on one reused 22-row terminal buffer.
	Footer row specifically checked at 40.0% before/after first Esc, visible
	REVERSED input cursor stable, draft/history unchanged. First Esc closes context;
	second dismisses completion. This pins current behavior, NOT a product decision.
- Corrected earlier footer cursor test: replaced untouched TestBackend hardware
	cursor equality with actual REVERSED rendered-cell/row equality.
- Taller gallery frames (32/64/80) are inspection artifacts, not actual production
	geometry or terminal-smoke evidence. Production viewport is 22 rows.

### Acceptance Coverage


1. Public boundary tests, minimal mocking, and existing helpers: verified. The
	new gallery addition and coexistence test follow the recorded RED/GREEN cycle;
	no private helper calls or estimated data define acceptance.
2. The reviewer mapped the 35 context/footer tests plus TUI tests to the listed
	commands, retained metrics, data states, resets, rounding, transients,
	cursor/history, and evidence-only semantics. Verified automatically.
3. Shared category palette/order/glyphs, measured zero, neutral/free cells,
	grids, stale/refresh/idle, over-limit, resets, adaptive footer priorities,
	warnings, and emergency restore were verified through prior tests and the
	integrated cross-state test.
4. Required widths, 79/80 breakpoint, constrained input/pickers, and cross-state
	expected styles were independently reviewed. Old expected output is unchanged.
5. Required focused checks and the exact full `cargo test` passed; counts follow.
6. BLOCKED release gate: the required actual command was attempted, but
	interactive smoke steps and a usable SDK/provider session could not be verified.
7. Claude's current binary is unavailable: source/screenshot-derived reference
	only; no local binary comparison or pixel-perfect parity claim. The supplied
	screenshot is absent from this workspace; CT-01 records article-image review.
8. Contract and actual results are explicit; automated tests do not substitute
	for the blocked real-terminal gate. No automated E2E command was invented.
9. Current completion/help/data/footer documentation was checked against prior
	slices. Corrections are recorded in Documentation Maintenance below. Criterion
	9 is verified; condition 6 and overall CT-04 release remain BLOCKED.

### Actual Terminal Attempt and Release Gate


- Executed `$project = 'C:\dev\picopilot'`, then the exact
  `cargo run -- --project $project` through the terminal tool, with no redirected
  stdin, TestBackend, fabricated events, credential collection, or auth changes.
- Output: Finished dev profile and started `target\debug\picopilot.exe --project
  C:\dev\picopilot`. The app reported that the Ollama model catalog was
  unreachable; it reported the same for vLLM.
- The tool returned no interactive terminal session to drive. No key or resize
  automation was available; a subsequent process probe found no running process.
- No proof of usable SDK authentication or successful provider response obtained.
  Provider catalog errors do not prove Copilot authentication failure.
- Release status: BLOCKED, not passed. Need usable interactive terminal control and
  authenticated SDK/provider observation. No secret may pass through the model.

### Automated Validation

- `cargo test --test screen_model --quiet`: 157 passed, including correction rerun.
- `cargo test --lib tui::tests --quiet`: 138 passed.
- cargo test --lib tui::rendering_fixtures --quiet: 2 passed, 1 ignored writer.
- cargo check: passed.
- Exact `cargo test`: 383 library + 9 ANSI + 157 screen-model = 549 passed,
  0 failed; 3 ignored (gallery writer + 2 authenticated tests). Main/doc tests 0.
  Authenticated ignored tests were NOT run or claimed passed. The guarded writer
  separately ran once to record the independently checked new scenario.
- Reviewer independently reran all focused suites, full cargo test (same counts),
  Clippy with warnings denied and diff hygiene: passed.
- Reviewer cargo fmt --check: fails in unrelated baseline files; initial new
  formatting differences were corrected locally, without baseline reformatting.
- Final compiler/full-suite reruns and final review confirmation pending below.

### Help Needed

- Task ID: CT-04
- Status: BLOCKED - release gate and lifecycle decision
- Requested help: coordinating Chief
- Exact decision needed: should the context overlay auto-close when submitting a
	prompt, /status or /resume, and what should Esc/hint priority be while completion
	or busy state coexists? Existing behavior remains Esc-only; no decision invented.
- Local evidence: show_context clears only on Esc; prompt/status/resume do not
	clear it. Context replaces live chat region, so response can remain hidden.
- Busy Esc hint has no interrupt action; confirmed pre-existing by reviewer.
	Do not silently fix unrelated interrupt behavior. Chief must route that decision.
- refresh_status_cost can continue attribution RPC after fatal recovery/quit;
	left unchanged, real transport behavior unverified. Physical-Shift flakiness
	pre-existed; no failure observed in our completed runs.
- Decision log: none. No human/product decision received or accepted.

### Documentation Gaps for Coordinator (reported before maintenance)

- docs/rendering-validation.md says 68 gallery sections, actual now 91 (90 at HEAD).
	Add CT-03 footer inventory and context-busy-completion, CT-04 actual counts and
	blocked smoke status; older 135 TUI/280 library log rows are historical, not final.
- Manual smoke matrix needs explicit /context all, Ctrl+U, resize, live footer and
	coexistence rows. Add current lifecycle characterization and gallery heights
	versus production 22-row viewport disclosure.
- Record existing unrelated baseline cargo fmt --check failure rather than imply
	current repo-wide formatting pass. Do not reformat unrelated code.
- README tools N/17 and skills N/M still refer to a status bar, though current
	counts live in /status. Reviewer routes these older text gaps, not code changes.
- Final overlay lifecycle remains undecided pending Chief decision/live observation.
	Implementing role forbids self-updating wiki/traditional docs; route to
	Documentation Maintainer. Condition 9 remains open until corrected/verified.

### Documentation Maintenance (2026-10-05)

- Status: documentation corrections complete; CT-04 completion and release remain BLOCKED.
- Resolved: rendering gallery count corrected to 91, with CT-03 footer/constrained-layout inventory and `context-busy-completion`; CT-04 test counts and blocked real-terminal result recorded; smoke rows added for `/context all`, `Ctrl+U`, resizing, live footer, and coexistence; gallery fixture heights distinguished from the production 22-row test buffer; current Esc-only overlay behavior and unresolved lifecycle documented; unrelated baseline `cargo fmt --check` failure recorded; README tool/skill counts now point to `/status`.
- Remaining human blocker: the actual terminal gate has no usable interactive session; auth status is unknown. Unreachable Ollama/vLLM catalogs do not establish Copilot auth failure. Condition 6 and release remain BLOCKED. Lifecycle/Esc decisions await the Chief and live observation. Existing busy `Esc` does not interrupt and remains a known limitation.

### Review Status

- Code Reviewer (Expensive): PASS for automated three-file diff; CT-04 completion
	and release BLOCKED on smoke, lifecycle and documentation gaps.
- Low recommendations for footer-specific percentage assertions, true visible
	cursor checks, same production-path terminal and local formatting addressed.
	Final re-review PASS for automated scope before authorized commit; exact below.

## Final Handoff
- Task ID: CT-04
- Status: blocked; automated validation changes complete, CT-04 completion/release NOT accepted.
- Summary: Extended independently reviewed integrated fixture and public redraw/Esc coverage; corrected ineffective rendered-cursor assertions. No production behavior changed.
- Changes: src/tui_rendering_fixtures.rs, tests/screen_model.rs, tests/fixtures/rendering/gallery.txt. Assigned ticket updated outside commit; all other planning preserved.
- Validation: Final cargo check passed; exact final cargo test 549 passed, 0 failed, 3 ignored. Required focused checks 157 screen-model, 138 TUI, 2 gallery/1 ignored. Local rustfmt and editor diagnostics clean. Reviewer clippy/diff hygiene passed. Existing repo-wide fmt failure not fixed.
- Code review: Code Reviewer (Expensive), final PASS automated changes; BLOCKED overall completion/release. Exact report below includes Unit-Test Quality.
- Risks/Assumptions: Authenticated smoke unavailable; auth unknown, local provider catalogs unreachable. Taller fixtures are inspection only. Claude binary unavailable, source/screenshot-only. Existing overlay, busy Esc, recovery continuation and physical-Shift observations remain routed.
- Commit: 704036409c437614fa69e5750cea5b74eabf3d40, Verify integrated context rendering contracts. CT-04 and Co-authored-by: Copilot copilot@github.com verified; current master, no push/new branch.
- Documentation gaps: Resolved in Documentation Maintenance above. Criterion 9 is verified; condition 6 and overall release remain BLOCKED.
- Follow-up: Chief obtains usable interactive terminal smoke and resolves/logs lifecycle and busy-Esc decisions before accepting CT-04/release.
- Coordination: This assigned local CT-04 ticket; 8/9 criteria checked, condition 6 remains open. No unsupported completion claim.
- Decision log: none; no human decision received and no new product rule invented.

## Exact Final Reviewer Report

## Code Review
- Task ID: CT-04, [.scratch/context-tracking/issues/04-verify-the-complete-context-experience.md](.scratch/context-tracking/issues/04-verify-the-complete-context-experience.md)
- Reviewer tier: Expensive
- Status:
	- **PASS** for the automated validation changes. The scoped commit can go ahead.
	- **BLOCKED** for CT-04 completion and release.
- Review baseline: HEAD `ac4cc28b95215562d2dafd020fdc769712c4ac01` plus the worktree. Exactly three files changed:
	- [src/tui_rendering_fixtures.rs](src/tui_rendering_fixtures.rs) +19/-0
	- [tests/screen_model.rs](tests/screen_model.rs) +86/-4
	- [tests/fixtures/rendering/gallery.txt](tests/fixtures/rendering/gallery.txt) +578/-0, one hunk appended at line 20281
- Production code is unchanged. Only `.scratch/` and `.wayfinder/` are untracked. There is no `openwiki/`.
- Scope reviewed: the new `context-busy-completion` gallery scenario, the new public coexistence test, and the corrected CT-03 footer cursor test.

## Findings
No Critical, High or Medium findings. All four Low recommendations from my previous review are resolved:
1. **Footer `40.0%`:** now checked on the footer row while the overlay is open, and again after the first Esc (footer row found again). This is no longer satisfied by the overlay header.
2. **Cursor:**
	 - The new test checks a REVERSED cell at `(6, input_row)` on the first frame. After the update it checks the same cell is identical and the input row is unchanged.
	 - The misleading `/` cell lookup is gone.
	 - The CT-03 test now checks the REVERSED cell at `(7, input_row)` instead of the untouched default `TestBackend` cursor position.
3. **Format:** `rustfmt --edition 2021 --check tests/screen_model.rs` is clean.
4. **Fixture height:** the 32-row fixture is taller than the real 22-row screen. This is now disclosed in the ticket. The new test itself uses the real 22-row height and keeps one `TestBackend` across redraws through `draw_with_screen_for_platform`.

I checked the expected used-cell colours by hand against the test's inputs (four categories at 10,000 tokens each, limit 200,000, MCP at 0):
- **Normal width:** each category gets exactly 1 cell, so the colours are [#888888 System, #D77757 Custom, #999999 Tools, #9333EA Messages].
- **Width 20:** 6 cells are available, about 1.2 would be used, and four categories tie at 0.3. The tie goes to the first category, so the one used cell is [#888888].
- System's colour is the same grey as free cells. The test still tells them apart by symbol (⛁ for used, ⛶ for free) and by DIM, which only free cells have.

The fixture and test still pin current behaviour for things nobody has decided yet: the overlay stays open while busy, and the first Esc closes the overlay. This is recorded as characterization, not a product rule.

Task-contract concerns: none.

## Unit-Test Quality
- **Covered:**
	- Busy turn, `/con` completion and the context overlay together at widths 20, 40, 79, 80 and 120.
	- Completion label clipped at width 20.
	- Row order: input above completion above footer.
	- Free-cell count, colour and DIM style.
	- Used-cell colours, normal and narrow.
	- Live footer update while the overlay is open and after the first Esc.
	- Cursor cell and input row stay the same across redraws.
	- Input text and history entry count are preserved.
	- Esc order: first Esc closes the overlay, second dismisses completion, then `esc to interrupt` appears.
	- Percent is dropped at width 20 after both Esc presses.
	- Earlier context and footer contract coverage is unchanged.
- **Missing or weak:** none that blocks.
	- Residual: the post-update footer check reuses the footer row from the first frame. This is valid because the layout is fixed.
	- The gallery is visual evidence only, not a substitute for the real-terminal check.

## Validation Assessment
- **Evidence I re-ran myself:**
	- `cargo test --test screen_model`: 157 passed, 0 failed.
	- `cargo test --lib tui::`: 140 passed, 1 ignored (the fixture writer).
	- `cargo clippy --all-targets -D warnings`: clean.
	- `git diff --check`: clean, apart from CRLF notices.
- **Reported full run** matches my previous independent run: 383 + 9 + 157 = 549 passed, 0 failed, 3 ignored (the fixture writer and two authenticated tests, not claimed as passed).
- **Remaining risks:**
	- **Real-terminal gate (condition 6): BLOCKED.** `cargo run -- --project $project` launched but reported the Ollama/vLLM model list as unreachable. There was no session to drive and no process left afterwards. Authentication status is unknown. No smoke steps are claimed as passed.
	- **Claude reference:** based on source and screenshot only. No local binary comparison.
	- Repo-wide `cargo fmt --check` already fails at HEAD in unrelated files; not caused by this diff.
- **Documentation gaps** (for the Documentation Maintainer; condition 9 stays open):
	- [docs/rendering-validation.md](docs/rendering-validation.md): the gallery section count says 68 but there are now 91.
	- The same file's inventory leaves out the CT-03 `footer-*` sections and `context-busy-completion`.
	- Its results and real-terminal smoke matrix are stale.
	- README still describes tool and skill counts as appearing in the status bar; they now appear in `/status`.
	- The overlay lifecycle is not documented.
- **Decisions logged:** none. No new human decision was needed. Still waiting on the Chief of Staff:
	- When the overlay should close automatically.
	- What Esc should do and which hint should show while busy.
	- Related risks: the `refresh_status_cost` attribution call can continue after fatal recovery, and the existing physical-Shift flakiness.