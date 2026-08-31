## Resolution

Confirmed via research the SDK's actual per-model options surface:
`session.set_model(model, Some(SetModelOptions { reasoning_effort,
reasoning_summary, context_tier, model_capabilities }))` (RPC
`session.model.switchTo`), with `models.list` exposing each model's
`supportedReasoningEfforts` and `defaultReasoningEffort` to validate
against.

- **Reasoning effort**: in scope for v1. The user can override a model's
  default reasoning effort — the SDK's primary cost/latency lever, directly
  serving the standing "cost/token efficiency is the north star" preference.
- **Context tier** (`default` / `long_context`): in scope for v1, shown only
  when the selected model actually supports it.
- **Reasoning summary** (`none`/`concise`/`detailed`) and the
  `modelCapabilities` override (vision/reasoning feature toggles): **out of
  scope for v1** — no clear need, adds surface without serving an
  already-settled goal.
- **Where to configure**: extra fields in the same model-picker modal
  already settled in tickets 003/006 (no new settings surface), plus
  startup flags for the initial session (mirroring `--model`) — e.g.
  `--reasoning-effort`, `--context-tier`. Invalid values follow the same
  fail-fast convention already settled for `--model` (ticket 006) and
  general startup validation (ticket 009): a stderr message and non-zero
  exit before the TUI launches, naming the model's valid
  `supportedReasoningEfforts`.

### What this does not decide

- Model *selection* itself (which model, unconstrained list, modal picker)
  — already settled in ticket 006.
- Exact flag names/keybindings — implementation detail, same as the
  session picker's placeholder binding in ticket 003.
