# System Message Trim Candidates — GitHub Copilot CLI SDK (`SystemMessageTransform`)

Research date: 2026-08-31. Scope: `github/copilot-sdk` monorepo (Rust SDK primary, Node/TS SDK cross-checked for parity since section IDs are shared across SDKs), plus `docs.github.com` SDK feature docs.

Primary sources consulted:
- [rust/README.md](https://raw.githubusercontent.com/github/copilot-sdk/main/rust/README.md) — "System Message Transforms" section
- [rust/src/transforms.rs](https://raw.githubusercontent.com/github/copilot-sdk/main/rust/src/transforms.rs) — `SystemMessageTransform` trait source
- [nodejs/README.md](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/README.md) — "System Message Customization" section (customize/replace modes, full section-ID list)
- [nodejs/src/types.ts](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/src/types.ts) — `SystemMessageSection`, `SYSTEM_MESSAGE_SECTIONS`, `SectionOverrideAction`, `SystemMessageCustomizeConfig`
- [docs.github.com/.../features](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features) — feature index (no standalone "System Message Transforms" doc page exists yet; the feature is documented only in the per-language SDK READMEs)
- [docs.github.com/.../features/usage-and-billing](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/usage-and-billing) — `session.metadata.contextInfo` token breakdown
- [docs.github.com/.../features/streaming-events](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/streaming-events) — `system.message` session event
- `docs.github.com/en/copilot/concepts/prompting` — general prompting concepts page; does not enumerate system-message sections
- `docs.github.com/en/copilot/reference/copilot-cli` — returned HTTP 404 at fetch time; could not confirm a CLI reference page enumerating sections or a verbose/debug flag

Note: no dedicated docs.github.com page titled "System Message Transforms" was found via the Features index. The mechanism is documented only in the SDK READMEs (`rust/README.md`, `nodejs/README.md`) and inline Rust doc-comments in `transforms.rs`. If GitHub publishes a dedicated docs page later, re-check it.

---

## 1. What section IDs exist in the default Copilot CLI system message?

The canonical list (12 section IDs, shared across SDKs per the parity note in `rust/README.md`) is documented in [nodejs/README.md](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/README.md) and given descriptions in the `SYSTEM_MESSAGE_SECTIONS` constant in [nodejs/src/types.ts](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/src/types.ts) (mirrored in `python/copilot/session.py` per the `github_text_search` results):

| Section ID | Description (verbatim from `SYSTEM_MESSAGE_SECTIONS`) |
|---|---|
| `preamble` | Agent identity preamble and mode statement |
| `identity` | **Group.** Covers the identity preamble and its sibling sub-sections (tone, tool efficiency, etc.) |
| `tone` | Response style, conciseness rules, output formatting preferences |
| `tool_efficiency` | Tool usage patterns, parallel calling, batching guidelines |
| `environment_context` | CWD, OS, git root, directory listing, available tools |
| `code_change_rules` | Coding rules, linting/testing, ecosystem tools, style |
| `guidelines` | Tips, behavioral best practices, behavioral guidelines |
| `safety` | Environment limitations, prohibited actions, security policies |
| `tool_instructions` | **Group.** Per-tool usage instructions |
| `custom_instructions` | Repository and organization custom instructions |
| `runtime_instructions` | Runtime-provided context and instructions (system notifications, memories, workspace context, mode-specific instructions, content-exclusion policy) |
| `last_instructions` | End-of-prompt instructions: parallel tool calling, persistence, task completion |

Source: [nodejs/src/types.ts](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/src/types.ts) (`SYSTEM_MESSAGE_SECTIONS` const), cross-confirmed by the section-ID list in [nodejs/README.md](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/README.md) ("Available section IDs: `preamble`, `identity`, `tone`, ...").

Important nuance for this taxonomy: `identity` and `tool_instructions` are **groups**, not standalone content — targeting the group with `remove` removes the group and its sub-sections together, unless a sub-section is marked `preserve` to opt out of the group-level removal ([nodejs/src/types.ts](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/src/types.ts), `SectionOverrideAction` doc comment).

Also important: **there is no dedicated section ID for "skills," "MCP servers," or "custom agents."** The taxonomy is organized by prompt *position/purpose* (identity, tone, safety, tool docs, runtime context, etc.), not by *feature*. Any skill/MCP/custom-agent-related content that exists in a real rendered prompt would live inside one of the broader sections above (most likely `tool_instructions` for MCP tool docs, or `runtime_instructions` for mode/workspace context) — not as its own removable unit. This is a load-bearing finding for sub-question 3 below.

The Rust SDK's transform mechanism itself does not define or enumerate section IDs — it is section-agnostic; `section_ids()` just declares which of these (Node-documented) IDs a given `SystemMessageTransform` implementation wants to intercept. Source: [rust/src/transforms.rs](https://raw.githubusercontent.com/github/copilot-sdk/main/rust/src/transforms.rs) — the trait doc-comment example uses `"instructions"` as a placeholder ID, and the unit tests use `"instructions"`/`"context"` as arbitrary example IDs, none of which appear in the canonical 12-ID list above. **This is worth flagging explicitly**: the Rust README/source example section ID (`"instructions"`) does not match any of the 12 documented `SystemMessageSection` values from the Node SDK. Either the example is illustrative/non-canonical, or the Rust SDK's transform dispatch is genuinely ID-agnostic and simply forwards whatever section IDs the CLI sends at runtime (which would be the 12-ID list in practice). Treat the Node-sourced 12-ID list as authoritative for real section IDs; treat the Rust README's `"instructions"` example as illustrative naming only.

---

## 2. Size/token information per section

**Not documented.** No source consulted (README, source comments, or docs.github.com pages) gives per-section byte counts, token counts, or relative proportions of the default system message.

The closest available data is *aggregate*, not per-section:
- `session.metadata.contextInfo` RPC returns `systemTokens`, `conversationTokens`, and `toolDefinitionsTokens` as three lump sums — `systemTokens` covers the entire system message as one number, not broken down by section. Source: [docs.github.com/.../features/usage-and-billing](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/usage-and-billing) ("Context-window utilization" → "On-demand breakdown with `session.metadata.contextInfo`").
- `session.usage_info` / `assistant.usage` events give whole-context or whole-call token counts, again not per-section. Same source.

Conclusion: any per-section size ranking must be done empirically (see Q5) rather than from documentation.

---

## 3. Which sections are plausibly boilerplate for picopilot vs. load-bearing?

picopilot has no skills, no MCP, and no custom agents. Given the finding in Q1 that **no section is dedicated to skills/MCP/custom agents**, the framing of "strip the skills/MCP section" doesn't map cleanly onto this taxonomy — there is no single switch for that. Instead, here's a per-section assessment:

**Plausibly safe to trim/shrink (not core tool-use or safety guidance):**
- `custom_instructions` — "Repository and organization custom instructions." If picopilot's consuming repos have no `AGENTS.md`/`.github/copilot-instructions.md`/org-level instructions, this section may render empty or minimal already; if picopilot doesn't need this feature at all it can also be disabled entirely via `skipCustomInstructions: true` on `SessionConfigBase` (a different, non-transform mechanism — see [nodejs/src/types.ts](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/src/types.ts)) rather than trimming via transform.
- `guidelines` — "Tips, behavioral best practices, behavioral guidelines." Generic advice not tied to a specific tool surface; a reasonable shrink candidate if picopilot's target model/use-case doesn't need general-purpose behavioral tips.
- `tone` — response style/conciseness/formatting. Safe to *customize* (replace with a shorter house style) rather than strictly "boilerplate to delete," but low risk either way.
- `runtime_instructions` — "system notifications, memories, workspace context, mode-specific instructions, content-exclusion policy." Since picopilot doesn't use memory, multi-mode (plan/autopilot) UI, or content-exclusion policy features (per the stated scope), sub-parts of this section that correspond to those unused features are plausible no-ops already, or shrink candidates if not.

**Ambiguous / needs empirical check before touching:**
- `environment_context` — CWD/OS/git root/directory listing/**available tools**. This partially reflects the actual live environment (useful, keep) but the "available tools" enumeration could balloon if picopilot exposes many built-in tools; whether it's safe to shrink depends on picopilot's actual tool count, not its skills/MCP status.
- `tool_instructions` (group) — "Per-tool usage instructions." Since this is data-driven by the tools actually configured, and picopilot has no MCP tools, MCP-specific per-tool instructions plausibly don't render here at all — nothing to strip. Any content that *does* render here (for whatever built-in tools picopilot exposes) is core tool-use guidance and should not be blanket-removed.

**Load-bearing — must not touch:**
- `safety` — "Environment limitations, prohibited actions, security policies." Explicitly a guardrail section; the Node README's `mode: "replace"` doc even warns that full replacement "removes all guardrails including security restrictions." Do not remove or shrink.
- `tool_efficiency` — "Tool usage patterns, parallel calling, batching guidelines." Core tool-use instructions.
- `last_instructions` — "End-of-prompt instructions: parallel tool calling, persistence, task completion." Core agent-loop behavior; end-of-prompt instructions are typically high-leverage for compliance and should be preserved.
- `preamble` / `identity` (group) — Agent identity and mode statement; removing this risks the model losing its operating persona/mode context entirely.

Sources for this section: [nodejs/src/types.ts](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/src/types.ts) (`SYSTEM_MESSAGE_SECTIONS` descriptions), [nodejs/README.md](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/README.md) ("Replace Mode" warning about removing guardrails), and [nodejs/src/types.ts](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/src/types.ts) (`skipCustomInstructions`, `enableSkills`, `mcpServers` config fields showing these features are opt-in and likely already absent from the prompt when unconfigured, rather than present-but-strippable boilerplate).

**Overall conclusion for Q3:** because picopilot's unused features (skills, MCP, custom agents) are opt-in and don't have dedicated sections, the most defensible token-savings targets are `guidelines`, `custom_instructions` (or disable via `skipCustomInstructions`), and possibly `tone`/`runtime_instructions` sub-content — not a "remove the skills/MCP section" action, because no such section exists to remove.

---

## 4. Does `SystemMessageTransform::transform_section` support removing a section, or only rewriting content?

**Only rewriting — the transform trait cannot remove a section.** Exact documented semantics, from [rust/src/transforms.rs](https://raw.githubusercontent.com/github/copilot-sdk/main/rust/src/transforms.rs):

```rust
async fn transform_section(
    &self,
    section_id: &str,
    content: &str,
    ctx: TransformContext,
) -> Option<String>;
```

Doc comment: *"Transform a section's content. Return `Some(new_content)` to modify the section, or `None` to pass through unchanged."*

The dispatch implementation (`dispatch_transform`) confirms this precisely: for each section, if the transform's `transform_section` call returns `Some(transformed)`, that content is used; if it returns `None`, **the original, unmodified content (`data.content`) is inserted into the response** — `None` is "leave as-is," not "delete." There is no third return value or sentinel for removal, and the unit tests (`dispatch_passes_through_unhandled_section`, `dispatch_unknown_section_passes_through`) explicitly verify pass-through-unchanged behavior for `None`/unhandled sections, not omission.

**Section removal is a separate, non-transform mechanism.** It exists only in the static `SystemMessageConfig` "customize" mode (`SystemMessageCustomizeConfig.sections[id].action = "remove"`), documented in [nodejs/README.md](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/README.md) and typed in [nodejs/src/types.ts](https://raw.githubusercontent.com/github/copilot-sdk/main/nodejs/src/types.ts) (`SectionOverrideAction = "replace" | "remove" | "append" | "prepend" | "preserve" | SectionTransformFn`). That "customize" config is set once at session-creation time as static per-section actions — it is a different code path from the async `SystemMessageTransform` trait, which only ever receives/returns section *content strings*, never an action enum. In other words: to actually delete a section for token savings, use `SystemMessageConfig::Customize` with `action: "remove"` (or `"preserve"` to keep a sub-section when its parent group is removed) — not `SystemMessageTransform`.

---

## 5. Can the actual rendered system message be inspected for a real session (to verify token savings empirically later)?

**Yes, via at least two documented mechanisms:**

1. **`system.message` session event.** Documented in [docs.github.com/.../features/streaming-events](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/streaming-events) ("Other events" → `system.message`): *"A system or developer prompt was injected into the conversation."* Payload fields: `content` (string, the full prompt text), `role` (`"system"` | `"developer"`), optional `name` and `metadata.promptVersion`/`metadata.variables`. This event is **not** listed as ephemeral in the "All event types at a glance" table, implying it is persisted and would be replayed on session resume / returned by `session.get_events()` (per the Rust README's `session.get_events()` / TS SDK's `getEvents()` method for "message history"). This is the most direct way to capture the literal rendered system-message text for a real session and diff its length before/after a transform.

2. **`session.metadata.contextInfo` RPC.** Documented in [docs.github.com/.../features/usage-and-billing](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/usage-and-billing): returns `systemTokens` (aggregate token count for the system message), `conversationTokens`, `toolDefinitionsTokens`, and `promptTokenLimit`. This gives an empirical *aggregate* system-message token count you could sample before/after applying a `SystemMessageTransform`, though (per Q2) it does not break the count down by section — you'd need to diff whole-session totals across two configurations (with vs. without a given transform) to attribute savings to a specific section.

**Not found:** a documented CLI debug/verbose flag for dumping the system prompt. The `docs.github.com/en/copilot/reference/copilot-cli` page returned HTTP 404 when fetched, so a dedicated CLI reference enumerating such a flag could not be confirmed from primary sources at this time — this sub-question is only partially resolved; only the two SDK-level session-event/RPC mechanisms above are verified.
