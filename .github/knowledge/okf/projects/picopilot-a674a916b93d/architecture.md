---
type: project-architecture
title: picopilot architecture
description: >
  Tech stack, module layout, SDK dependency, and packaging decisions for the
  minimalist Rust TUI coding agent built on the GitHub Copilot SDK.
tags: [picopilot, rust, copilot-sdk, architecture, tui]
status: verified
sources:
  - .wayfinder/map.md
  - Cargo.toml
  - src/lib.rs
  - session b8030d13 (implementation)
  - session 7028bf67 (audit + fixes, TUI simplification, install scripts)
  - session 73ea30cc (local provider implementation, prompt elimination, toolset)
generated: "2026-08-31T21:34:00Z"
---

# Architecture

**picopilot** is a single-binary, full-screen TUI coding agent that wraps the
`copilot` CLI process via JSON-RPC using the official Rust SDK.

## Tech stack

| Layer        | Crate                                   | Version           |
|--------------|-----------------------------------------|-------------------|
| SDK          | `github-copilot-sdk`                    | 1.0.13-preview.2  |
| CLI parsing  | `clap` (derive)                         | 4.5               |
| Terminal     | `crossterm`                             | 0.28              |
| TUI          | `ratatui`                               | 0.29              |
| Async        | `tokio` (macros, rt-multi-thread, sync) | 1                 |
| Serialization| `serde` + `serde_json`                  | 1                 |
| Markdown     | `pulldown-cmark`                        | 0.13              |
| HTTP         | `reqwest` (json, rustls-tls)             | 0.12              |
| Traits       | `async-trait`                           | 0.1               |

## Module layout

```
src/
  main.rs         — CLI entry, startup validation, TUI launch
  lib.rs          — pub mod re-exports
  config.rs       — AppConfig (Clap), session/client builders, catalog validation
  events.rs       — SDK SessionEvent → typed EventUpdate adapter
  permissions.rs  — PermissionHandler: auto-approve, workspace confinement, approval queue
  provider.rs     — Local provider discovery, HTTP model listing, registry construction
  runtime.rs      — AppRuntime: client lifecycle, session resume, recovery
  toolset.rs      — Toolset domain: mask-backed tool profiles, provenance, serialization
  tui.rs          — App state, Ratatui renderer, keyboard handler, async event loop,
                    Fleet fallback dispatch, tool picker
```

## Key design invariants

- picopilot **never reimplements** native Copilot tools; it only curates via
  `available_tools`/`excluded_tools` and the permission handler.
- Ships as a **bundled CLI** binary (`bundled-cli` SDK feature) — no separate
  `copilot` install required.
- Default mode is **autopilot** (keeps going until `task_complete` or idle).
- **No config file**; all configuration via CLI flags or environment.
- **No skills, no MCP, no custom tools** in v1.
- Install via `install.ps1` (Windows) or `install.sh` (Unix), which build
  `--release --locked` and copy to user-local directories:
  - Windows: `%LOCALAPPDATA%\Programs\picopilot\bin`
  - Unix: `${XDG_BIN_HOME:-$HOME/.local/bin}`
  Both scripts offer to add picopilot to the persistent user PATH.
  Alternatively: `git clone` + `cargo build` / `cargo install --path .`.
- No crates.io release for v1, no prebuilt binaries, no self-update.
