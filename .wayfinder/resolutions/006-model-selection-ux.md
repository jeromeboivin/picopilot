## Resolution

Grilled model-selection UX over two rounds plus one inline fact-finding
dispatch (`models.list`/`session.set_model` mechanics).

### Scope

v1 exposes **mid-session model switching**, not just a startup default/flag.
Justified by the research: `session.set_model` is a clean swap — it
preserves conversation history, system message, and compaction state — and
`models.list` already carries cost/capability metadata (billing multiplier,
token prices, context window, reasoning-effort support), which is directly
useful given "cost/token efficiency is the north star."

- **Unconstrained, not a curated allow-list**: the picker shows every model
  `models.list` returns for the account. picopilot hardcodes no model
  names of its own, so there's no allow-list to keep in sync as new models
  ship.
- **Surface**: a full-screen modal, the same convention
  [tui-shape](003-tui-shape.md) settled for the session-history picker
  (plain list, arrow-key navigate, Enter to select — no fuzzy search, since
  that belonged to the TUI variant that wasn't chosen). Each row shows
  id/name plus cost tier and context-window size from `models.list`'s
  metadata.
- **Default model**: picopilot **never sets `model`** at session creation
  unless the user passed `--model` or picked one mid-session. Whatever the
  CLI/account defaults to (undocumented, CLI-owned) is picopilot's default
  too — one less thing to hardcode or keep current.
- **`--model` flag validation**: picopilot checks the given value against
  `models.list` itself **before** creating the session, and exits with a
  clear error listing valid ids if it doesn't match. This doesn't rely on
  the CLI's own undocumented behavior for unknown model ids (research
  found no documented validation/error behavior there).

### What this does not decide

- Per-model `SetModelOptions` (reasoning effort, context tier) — the
  picker switches the model id only; whatever reasoning-effort default
  each model reports (`defaultReasoningEffort`) applies unless a later
  ticket wants finer control. Left as fog.
