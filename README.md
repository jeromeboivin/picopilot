# picopilot

A minimalist Rust coding agent built on the GitHub Copilot SDK.

## Local model providers (experimental)

picopilot can add models from one OpenAI-compatible provider alongside the
hosted GitHub Copilot catalog. The provider registry is an experimental SDK
surface. Ollama, vLLM, LiteLLM, and Foundry Local can use the same integration
when their OpenAI-compatible API exposes tool calling.

### Ollama

1. Start Ollama and install a tool-capable model:

   ```text
   ollama serve
   ollama pull qwen2.5-coder:14b
   ```

2. Verify that the OpenAI-compatible model catalog is available:

   ```text
   curl http://localhost:11434/v1/models
   ```

3. Configure the provider for the current shell and start picopilot:

   PowerShell:

   ```powershell
   $env:PICOPILOT_PROVIDER_URL = "http://localhost:11434/v1"
   cargo run
   ```

   Other shells:

   ```sh
   export PICOPILOT_PROVIDER_URL=http://localhost:11434/v1
   cargo run
   ```

The default provider name is `local`, so discovered models appear as
`local/<model-id>`. Select one with `--model local/<model-id>` or open the
model picker with `Ctrl+P`. The provider URL is queried at startup with
`GET {provider-url}/models`; an unreachable endpoint, unsuccessful response,
malformed catalog, empty catalog, or invalid provider option stops startup
before the alternate screen opens.

### Generic OpenAI-compatible endpoints

```text
PICOPILOT_PROVIDER_URL=https://your-endpoint.example/v1
cargo run -- --provider-name team --provider-wire-api completions
```

`--provider-wire-api` accepts `completions` (the default) or `responses`. Use
`PICOPILOT_PROVIDER_API_KEY` for an endpoint that requires bearer
authentication. API keys are never printed by picopilot. The provider name
cannot contain `/`, because it becomes the first part of a qualified model ID.

Local inference is tracked by the provider rather than billed as GitHub
Copilot usage. The model picker therefore shows local inference and leaves
unknown context, pricing, and reasoning capabilities unset. Local models must
support the OpenAI-compatible tool-calling protocol for file and shell work;
a text-only model may be selectable but cannot complete the normal coding-agent
workflow.

Provider settings are flags and environment variables only; picopilot does not
create a configuration file. Provider definitions are session-scoped. To
resume a local-model session in a later picopilot process, provide the same
`PICOPILOT_PROVIDER_URL`, provider name/wire API options, and API key (when
needed) again. The current implementation supports one additive provider
endpoint and does not pull models or manage their lifecycle.

## Development

```text
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --locked
```
