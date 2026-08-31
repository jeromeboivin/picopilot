---
title: How should picopilot be packaged and published beyond the bundled binary?
status: closed
type: grilling
assignee: GitHub Copilot
blocked_by: []
resolution: ../resolutions/008-packaging-and-publishing.md
---

## Question

Decide packaging/publishing for v1, beyond "ships as a single self-contained
binary" (already settled):

- Is a crates.io release in scope for v1, or is "build it yourself" (`cargo
  install --path .` / a bundled-binary artifact off CI) enough?
- If distributed as a built artifact, how is it versioned against the
  `github-copilot-sdk` crate and the bundled `copilot` CLI release it wraps —
  pinned, ranged, or independent?
- Any install-method expectations (a single downloadable binary per OS/arch,
  a shell installer script, a package manager), or is that out of scope for
  this map (no benchmarking/evaluation scorecard, per the Destination)?
