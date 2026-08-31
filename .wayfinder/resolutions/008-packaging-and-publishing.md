## Resolution

Packaging/publishing for v1 stays minimal, matching the "build it yourself"
philosophy rather than adding release infrastructure:

- **No crates.io release for v1.** picopilot is not published; there's no
  `cargo add picopilot` or `cargo install picopilot` from a registry.
- **Install method**: `git clone` + `cargo build` / `cargo install --path .`.
  No prebuilt per-OS/arch binaries, no installer script, no package manager.
- **SDK dependency pinning**: a normal semver range on `github-copilot-sdk`
  in `Cargo.toml` (e.g. `"1.0"`), with `Cargo.lock` committed — the
  idiomatic way to get exact-version reproducibility for a binary crate,
  rather than a hard `"=x.y.z"` requirement that would need manual bumping
  on top of the lock file. Confirmed via research that `github-copilot-sdk`
  is published on crates.io, follows semver, and its own `build.rs` already
  pins/bundles the matching `copilot` CLI version internally (SDK and CLI
  are released in lockstep) — so picopilot never needs to separately track
  a compatible CLI version; that's the SDK's job.
- **No self-update.** picopilot does not check for or apply its own updates;
  the user re-clones/rebuilds manually.

### What this does not decide

- Any future decision to publish is a scope change, not covered here (redraw
  the destination if it comes up).
