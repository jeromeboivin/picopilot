# picopilot

A minimalist Rust coding agent built on the GitHub Copilot SDK.

```bash
┌───────────────────────────────────────────────────────────────────┐
│ PICOPILOT.EXE                                                     │
│                                                                   │
│ > INITIALIZING SYSTEM... Version: v0.1.0                          │
│ > LOADING NEURAL MODULES... Project: dev                          │
│ > BYPASSING SECURITY... Tools: 17                                 │
│ > ACCESS GRANTED.                                                 │
│                                                                   │
│  ██████╗ ██╗ ██████╗ ██████╗ ██████╗ ██╗██╗      ██████╗ ████████╗│
│  ██╔══██╗██║██╔════╝██╔═══██╗██╔══██╗██║██║     ██╔═══██╗╚══██╔══╝│
│  ██████╔╝██║██║     ██║   ██║██████╔╝██║██║     ██║   ██║   ██║   │
│  ██╔═══╝ ██║██║     ██║   ██║██╔═══╝ ██║██║     ██║   ██║   ██║   │
│  ██║     ██║╚██████╗╚██████╔╝██║     ██║███████╗╚██████╔╝   ██║   │
│  ╚═╝     ╚═╝ ╚═════╝ ╚═════╝ ╚═╝     ╚═╝╚══════╝ ╚═════╝    ╚═╝   │
└───────────────────────────────────────────────────────────────────┘

────────────────────────────────────────────────────────────────────────────────
❯  
────────────────────────────────────────────────────────────────────────────────
  / for commands
```

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

## In-app slash commands

In the prompt, type `/` to open completion. It shows two different kinds of
entries:

- **Built-in commands** run locally in picopilot. They are always available
   and do not send a prompt to the model.
- **User-invocable skills** come from discovered `SKILL.md` files. Selecting
   one enables that skill for the current conversation when necessary, then
   sends the literal slash prompt to the model.

Built-in commands are:

| Command | What it does |
| --- | --- |
| `/status` | Shows the active session, model, reasoning level, context tier, tools, and skills. Takes no arguments. |
| `/context` | Shows live used/free context, measured category snapshots when available, and separate Session usage metrics. Ctrl+U also opens it. PageUp/PageDown scroll; Esc closes. |
| `/context all` | Adds available source totals, discrepancies, stale-state explanations, and explicit unavailable per-item costs. No estimates or invented savings. |
| `/resume` | Opens the previous-conversation picker. Use `Up`/`Down` or `j`/`k` to select, `Enter` to load, or `Esc` to cancel. Takes no arguments. |
| `/fleet PROMPT` | Starts a Fleet run for `PROMPT`. A non-empty prompt is required. |

Use `Up`/`Down` to choose a completion, `Tab` to place it in the prompt while
keeping any trailing arguments, `Enter` to run or send it, and `Esc` to close
completion. Unknown slash commands and skills that are not user-invocable are
sent as ordinary prompts, except the removed `/usage`, which reports an unknown command.

The context screen uses a 10x10 grid below a 1M-token limit and 20x10 at
1M or above. Below 80 columns, it uses 5x5 or 5x10 and stacks the legend.
Valid live usage is authoritative. Category amounts are never scaled: remaining
usage is neutral, unmeasured Unattributed usage. When categories exceed live usage,
the grid is neutral pending refresh; the legend preserves the measured snapshot.
Missing or invalid categories are unavailable, not zero. Without valid live usage,
a valid attribution total/limit can supply a clearly labeled snapshot instead.
Refresh failures retain successful values and mark them stale; idle alone does not.
Session switches clear all usage; model/limit changes clear the old breakdown.
Fresh attribution is accepted after a reset. Selected Auto/provider model IDs and
live context-window versus attribution prompt limits need not match. Expanded mode
shows these source values, and the header shows the attribution's resolved model.
Cells use one rounded used total and largest fractional remainders, with palette
order breaking exact ties. Neutral cells have a distinct undimmed text-colored legend
key, not the System instructions color. Tiny values remain accurate in the legend. Over-limit usage
fills at most 100% of cells but keeps the real counts and percentage.
Per-item costs, cache-aware totals, compaction reserves, and savings are not measured.
No analyzer suggestions are supported by the available data. The live footer meter
remains CT-03 work; its absence is not replaced by a placeholder.
Expanded mode explains refresh failures even when no successful data exists.
The current implementation keeps the context view open until Esc. Submitting a
prompt or invoking `/status` or `/resume` while it is open may leave live chat
output hidden until Esc. This lifecycle still needs real-terminal release-gate
verification; whether the view should auto-close is undecided.
The visual reference is Claude source revision
`6f6f12b37f529488b10e53928dd5508bb93535c7`, not a current Claude binary
comparison. Pixel-perfect parity is not claimed.

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
platform shell plus its list/read/stop/write background-process tools, `view`,
`edit`, `create`, `apply_patch`, `grep`, `glob`, `task` plus its
list/read/write agent tools, `ask_user`, and `skill`; web search and web fetch
are not enabled.

New local-model conversations start with the shell only. New hosted-model
conversations start with all tools. Before the first message, changing
models recomputes that default unless tools were selected manually. After a
conversation has history, model changes preserve the current tool selection.

Press `Ctrl+K` to open the full-height tool picker. Use `Space` to toggle the
highlighted tool, `s` for shell only, `a` for all tools, `Enter` to apply, and
`Esc` to cancel. Applying a selection reconnects the same session; it is
available only while idle and failed changes are rolled back. The status bar
shows the active count as `tools N/17`. The picker can still be opened during an
approval or reconnect, but applying a change waits until that work is finished.

Press `Ctrl+N` while idle to start a new conversation immediately. The current
model, reasoning/context choices, and tool selection are retained; the
transcript, usage details, fleet state, and pending conversation input are
cleared. Previous conversations remain available through the `/resume` command.

When resuming a historical session, picopilot first reconnects with shell-only
tools, then detects the stored model from usage metrics or model-change history.
Known hosted models are expanded to all tools; local and unknown models
remain shell-only. Custom tool selections are not persisted across processes.
Automatic transport recovery preserves the exact active selection.

### Skills

picopilot discovers Agent Skills from these standard roots, in this order:

```text
PROJECT/.agents/skills
PROJECT/.github/skills
PROJECT/.claude/skills
~/.agents/skills
~/.copilot/skills
~/.claude/skills
```

It also merges enabled entries from VS Code's `chat.agentSkillsLocations`
setting in the user settings file and the project's `.vscode/settings.json`.
Settings are read as JSONC, so comments and trailing commas are accepted.
Relative entries are resolved from the project; `~` entries are resolved from
the user home directory. Duplicate roots are scanned once. A skill must be a
directory containing `SKILL.md` with valid `name` and `description`
frontmatter, and its name must match the directory name. Invalid or unreadable
skills are skipped with a diagnostic instead of preventing startup.

Skills are never injected into the system message and are disabled explicitly
when a session starts. Press `Ctrl+S` to open the skill picker. `Space` toggles
the highlighted skill, `a` selects all discovered skills, `n` clears the
selection, `Enter` applies it, and `Esc` cancels. The selection lasts for the
current conversation only; `Ctrl+N` and historical-session resume clear it.
The status bar shows the active count as `skills N/M`. Applying a selection is
available while idle and reconnects the current session when it already has
history.

See [In-app slash commands](#in-app-slash-commands) for the distinction between
native commands and user-invocable skills, their behavior, and completion
controls.

### Input and terminal controls

The prompt is a multiline editor. Press `Enter` to send the current prompt and
`Shift+Enter` to insert a newline. Pasting text, including text with line
breaks, inserts it as one editable prompt. Use the arrow keys, `Home`, `End`,
`Backspace`, and `Delete` to correct the prompt before sending. `Ctrl+Alt`
characters are accepted for keyboard layouts that use AltGr.

The transcript uses the terminal's native scrollback. The terminal keeps mouse
selection and clipboard handling, so drag-select and copy using the normal
behavior of the VS Code or Windows Terminal window.

Press `Ctrl+O` to expand transcript details such as reasoning and truncated
tool output. Press `Esc` to return to the compact transcript view. Diagnostics
remain hidden unless `Ctrl+I` is enabled.

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

## Agents

Type `/agent` to select an agent for the current conversation. The Copilot
runtime discovers user agents in `~/.copilot/agents/` (on Windows,
`%USERPROFILE%\.copilot\agents\`) and project agents in `.github/agents/`
under the project root. Picopilot also loads Markdown agent definitions from
`~/.agents/agents/` (or `%USERPROFILE%\.agents\agents\` on Windows). Restart
Picopilot after adding an agent so the session picks it up.

Running custom agents as subagents (for example, an orchestrator agent that
delegates to other agents with the `task` tool) requires the full Copilot CLI.
Picopilot uses the `copilot` found on `PATH`, or the one named by
`COPILOT_CLI_PATH`. Without it, Picopilot falls back to the SDK's bundled
runtime, which can list and select agents but cannot delegate to custom agents,
and shows a warning at startup.
