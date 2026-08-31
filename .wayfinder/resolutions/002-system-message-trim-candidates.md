# Resolution: Which default Copilot system-message sections are safe to trim for token savings?

Findings: [.wayfinder/research/system-message-trim-candidates.md](../research/system-message-trim-candidates.md)

- 12 documented section IDs exist (from the Node SDK's
  `SYSTEM_MESSAGE_SECTIONS` constant — no dedicated docs.github.com page or
  Rust source enumerates them): `preamble`, `identity` group, `tone`,
  `tool_efficiency`, `environment_context`, `code_change_rules`,
  `guidelines`, `safety`, `tool_instructions` group, `custom_instructions`,
  `runtime_instructions`, `last_instructions`. The Rust `transforms.rs`
  example uses a non-canonical placeholder ID (`"instructions"`) — a real
  implementation must use the actual section IDs above, not that example.
- **No section is dedicated to skills/MCP/custom agents** — picopilot's
  "strip the boilerplate we don't use" premise doesn't map to a single
  strippable section. Best candidates: `guidelines`, `custom_instructions`
  (or disable entirely via `skipCustomInstructions`), and parts of
  `tone`/`runtime_instructions`. Load-bearing, do-not-touch: `safety`,
  `tool_efficiency`, `last_instructions`.
- Per-section token sizes are not documented anywhere.
- `SystemMessageTransform::transform_section` can only **rewrite** content
  (returning `None` passes it through unchanged) — outright removal requires
  the separate static `SystemMessageConfig` "customize" mode with
  `action: "remove"`, a different mechanism than the transform trait.
- Empirical verification of savings is possible via the `system.message`
  session event (full rendered text) and `session.metadata.contextInfo`
  (aggregate system token count); no CLI debug flag could be confirmed (the
  reference doc page 404'd).

Implication: the "trim the system message" architecture decision stands, but
implementation needs both mechanisms — `SystemMessageConfig` removal for
whole sections (`guidelines`, `custom_instructions`) and `SystemMessageTransform`
rewrites for shrinking `tone`/`runtime_instructions` — verified empirically via
`session.metadata.contextInfo`, not assumed from documentation alone.
