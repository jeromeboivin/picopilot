## Plan: Eliminate Prompt Pollution

The 4,699 system tokens are present because [src/config.rs](src/config.rs) uses SDK mode `customize`, which preserves almost the entire default Copilot prompt. It removes only `guidelines` and `custom_instructions` and replaces `tone`. It does **not** create an empty system message.

The fix is to use `mode = "replace"` with explicit empty content everywhere. The separate 4,593 tokens come from the seven eager tool schemas and will be managed through a selectable toolset.

**Implementation**

1. Replace the current system-message customization with a strict empty replacement.
2. Remove `PicopilotSystemMessageTransform`, the tone instruction, section overrides, and transform registration.
3. Apply the empty message consistently during create, historical resume, toolset reconfiguration, and transport recovery.
4. Add [src/toolset.rs](src/toolset.rs) to own the seven canonical tools, platform shell name, ordering, arbitrary subsets, and defaults.
5. Serialize every toolset as an explicit allowlist. An empty selection must disable all tools rather than activate SDK defaults.
6. Store the active toolset and whether it came from defaults or an explicit user choice in `AppRuntime`.
7. Default new/empty conversations as follows:
   - Local model: shell only.
   - Hosted model: all seven.
8. Before the first message, model changes recompute that default unless the user manually selected tools.
9. After conversation history exists, model switches preserve the current toolset.
10. Add a transactional same-session reconnect operation for applying tool changes. On failure, restore the previous toolset.
11. Preserve the active toolset exactly during automatic transport recovery.
12. For historical resume, reconnect provisionally with shell only and without overriding the stored model.
13. Detect the restored model from usage metrics or model-change history.
14. Keep shell only for local or unknown models; reconnect once more with all tools for hosted models.

**Tool Picker**

Add a `Ctrl+K` full-height checkbox picker in [src/tui.rs](src/tui.rs):

- `Space`: toggle the selected tool.
- `s`: Shell only.
- `a`: All tools.
- `Enter`: apply.
- `Esc`: cancel.
- Status bar: compact `tools N/7` indicator.

The picker may be opened at any time, but changes apply only while idle. Streaming, active tool calls, pending approvals, and reconnection will produce a recoverable wait message instead of disconnecting in-flight work.

**Regression Coverage**

Add [tests/context_budget.rs](tests/context_budget.rs) as an ignored, opt-in live test:

- Assert observed system-prompt tokens are exactly zero.
- Measure all-tools and shell-only schema costs.
- Enforce measured ceilings with a small runtime/tokenizer tolerance.
- Verify shell-only costs materially less than all-tools.
- Verify zero system tokens after create, toolset change, resume, and recovery.
- With a configured local provider, complete a real shell call rather than testing chat only.

Run it with:

`PICOPILOT_CONTEXT_BUDGET_E2E=1 cargo test --test context_budget -- --ignored --nocapture`

Any nonzero system prompt, tool-budget regression, failed local shell call, or lifecycle mismatch remains release-blocking.

**Scope Decisions**

- Empty system instructions apply to every hosted and local model.
- All seven tools remain manually selectable.
- Local conversations default to shell only, not permanently shell only.
- Mid-conversation model switches preserve tools.
- Explicit user selection overrides defaults.
- Cross-process custom tool selections are not persisted; resumed sessions derive defaults from their restored model.
- Adding a separate new-conversation command is outside this change.

The comprehensive plan is saved in `/memories/session/plan.md`.
