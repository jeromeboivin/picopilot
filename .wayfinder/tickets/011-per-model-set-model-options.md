---
title: Should picopilot expose per-model SetModelOptions?
status: closed
type: grilling
assignee: GitHub Copilot
blocked_by: []
resolution: ../resolutions/011-per-model-set-model-options.md
---

## Question

Ticket 006 settled model *selection* (switching which model, unconstrained
list, modal picker). Decide whether to also expose per-model *options*:

- Does the SDK's `SetModelOptions` (or equivalent) let picopilot override
  things like reasoning effort or context tier per model, distinct from just
  picking a model id? (Needs a research dispatch to confirm the actual
  surface before deciding.)
- If such options exist, is any of it in scope for v1, or does "each model's
  own default applies" (already settled) fully cover it?
- If in scope, how does the user set it — extra fields in the model picker
  modal (ticket 003/006), a separate settings surface, or startup flags only?
