---
type: design-decision
title: Local provider support (BYOK)
description: >
  Additive local model registration via SDK NamedProviderConfig for Ollama and
  OpenAI-compatible endpoints, coexisting with hosted Copilot models in the
  same picker. Implemented and committed (05a1132).
tags: [picopilot, ollama, byok, local-models, providers]
status: verified
sources:
  - session 73ea30cc (investigation, plan, implementation)
  - github/copilot-sdk rust/src/types.rs (NamedProviderConfig, ProviderModelConfig)
  - commit 05a1132 (Add OpenAI-compatible local provider support)
generated: "2026-08-31T21:30:00Z"
---

# Local provider support (BYOK)

## Status

Implemented and committed in session `73ea30cc` (commit `05a1132`).
79 tests pass including 17 provider-focused tests with real async HTTP
fixtures. The `NamedProviderConfig` API is experimental in the SDK.

## Design choice: additive registry

The SDK offers two approaches:

1. **Singular `ProviderConfig`** — replaces the entire session with one local
   provider. Loses access to hosted Copilot models.
2. **Experimental `NamedProviderConfig` + `ProviderModelConfig`** — adds local
   models alongside hosted Copilot models. Models appear with qualified IDs
   like `local/qwen2.5-coder:14b`.

**Decision**: Use the additive registry (option 2) because picopilot's
existing picker and `set_model` flow support mixed model lists, and users
should be able to switch between hosted and local models mid-session.

## SDK API surface (verified for 1.0.13-preview.2)

- `NamedProviderConfig`: name, provider type, base URL, wire API, API key,
  bearer token, transport.
- `ProviderModelConfig`: provider name (references `NamedProviderConfig.name`),
  wire model, model ID, capabilities.
- `SessionConfig` accepts `providers: Vec<NamedProviderConfig>` and
  `models: Vec<ProviderModelConfig>`.
- `Client::list_models()` returns **only** the hosted catalog.
- `session.rpc().model().list()` returns **both** hosted and registered models.
- `session.set_model()` accepts qualified local model IDs.
- Provider definitions are **per-session** and must be resupplied on
  resume and transport recovery.

## Ollama configuration

```
provider_type: "openai"
base_url: "http://localhost:11434/v1"
wire_api: "completions"
api_key: None  (Ollama requires no auth)
```

## Agreed implementation plan

### Flags and environment variables

| Flag                      | Env                         | Default       |
|---------------------------|-----------------------------|---------------|
| `--provider-url`          | `PICOPILOT_PROVIDER_URL`    | (none)        |
| `--provider-name`         | —                           | `local`       |
| `--provider-wire-api`     | —                           | `completions` |
| —                         | `PICOPILOT_PROVIDER_API_KEY`| (none)        |

### Implementation (committed)

1. `src/provider.rs` — HTTP discovery (`GET {url}/models`), bounded `reqwest`
   client, bearer authentication via `PICOPILOT_PROVIDER_API_KEY`, sorted and
   deduplicated model IDs, qualified names (e.g. `local/qwen:7b`),
   secret-safe error output and `Debug` impl (never delegates to SDK's
   secret-printing provider formatter).
2. Startup fail-fast: provider URL must be reachable, authorized, and return
   at least one model before the TUI opens.
3. `--model` validation expanded to accept qualified IDs.
4. Provider registry stored in `AppRuntime`, reapplied to every
   `ResumeSessionConfig` including transport recovery.
5. Picker shows local models honestly: no fabricated pricing, context limits,
   or reasoning controls. Label as "local inference." Capability-missing
   local rows say "unavailable" (not "fixed"). `r`/`c` cycling is a no-op
   for local models.
6. Local models must implement OpenAI-compatible tool calling.

## Constraints

- No config file — all provider configuration via CLI flags and env vars,
  consistent with picopilot's "no config file" policy.
- **No CLI API-key flag** — API keys are accepted only via
  `PICOPILOT_PROVIDER_API_KEY` to prevent exposure in shell history and
  process lists.
- No capability fabrication — unknown local models are registered with
  `capabilities: None`.
- The `NamedProviderConfig` API is marked experimental in the SDK.
- **Startup cleanup** — dropping the SDK `Client` automatically cleans up
  the child Copilot CLI process, so provider-discovery failures during
  startup do not require explicit shutdown logic.
