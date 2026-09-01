# picopilot

A minimalist Rust coding agent built on the GitHub Copilot SDK.

## Command-line usage

picopilot uses the process's current directory as the project directory by
default. Specify a project directory either as the positional `PROJECT`
argument:

```text
picopilot PROJECT
cargo run -- PROJECT
```

or with the explicit `--project` option:

```text
picopilot --project PROJECT
cargo run -- --project PROJECT
```

Relative project paths are resolved from the directory where picopilot is
started. The positional argument and `--project` cannot be used together.

The available startup options are:

| Option | Description |
| --- | --- |
| `PROJECT` | Project directory, positional form |
| `--project PROJECT` | Project directory, explicit form |
| `--model MODEL` | Select the initial model |
| `--reasoning-effort EFFORT` | Set the initial reasoning effort |
| `--context-tier TIER` | Set the initial context tier |
| `--provider-url URL` | Add an OpenAI-compatible model provider |
| `--provider-name NAME` | Name the provider (requires `--provider-url`) |
| `--provider-wire-api API` | Select `completions` or `responses` (requires `--provider-url`) |

Use `picopilot --help` for the generated command reference.

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

## Prompt and tool budget

picopilot sends an explicitly empty system message to both hosted and local
models. The SDK's default system instructions are not merged into the session.
Built-in tools are also sent as an explicit allowlist so an empty selection
cannot accidentally restore SDK defaults. The selectable set contains the
platform shell, `view`, `edit`, `create`, `grep`, `glob`, and `task`; web search
and web fetch are not enabled.

New local-model conversations start with the shell only. New hosted-model
conversations start with all seven tools. Before the first message, changing
models recomputes that default unless tools were selected manually. After a
conversation has history, model changes preserve the current tool selection.

Press `Ctrl+K` to open the full-height tool picker. Use `Space` to toggle the
highlighted tool, `s` for shell only, `a` for all tools, `Enter` to apply, and
`Esc` to cancel. Applying a selection reconnects the same session; it is
available only while idle and failed changes are rolled back. The status bar
shows the active count as `tools N/7`. The picker can still be opened during an
approval or reconnect, but applying a change waits until that work is finished.

Press `Ctrl+N` while idle to start a new conversation immediately. The current
model, reasoning/context choices, and tool selection are retained; the
transcript, usage details, fleet state, and pending conversation input are
cleared. Previous conversations remain available through `Ctrl+O`.

When resuming a historical session, picopilot first reconnects with shell-only
tools, then detects the stored model from usage metrics or model-change history.
Known hosted models are expanded to all seven tools; local and unknown models
remain shell-only. Custom tool selections are not persisted across processes.
Automatic transport recovery preserves the exact active selection.

The live context-budget regression is opt-in because it requires a running
Copilot CLI and authentication. With a configured local provider it also
requires a tool-capable local model:

```text
PICOPILOT_CONTEXT_BUDGET_E2E=1 cargo test --test context_budget -- --ignored --nocapture
```

## Development

```text
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --locked
```
