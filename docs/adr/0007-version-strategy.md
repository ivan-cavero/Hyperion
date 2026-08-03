# ADR 0007 — Version strategy: 26.2 first, multi-version later

- Status: **Accepted**
- Date: 2026-08-02

## Context

Hyperion starts targeting the **latest stable version** (26.2, protocol 776).
In the future we want to support older versions. That conditions how we
organize code, branches, and data from day one: each Minecraft release
changes packet layouts, registries, NBT, and limits.

Key question: **one project/branch per Minecraft version**, or a single
codebase that changes version?

## Decision

### Now — only 26.2, on `main`

- **A single `main` branch** and a single codebase, targeting **exclusively 26.2**.
- Everything modern is implemented and nothing deprecated: handshake intents
  Status/Login/**Transfer**, RSA-1024 + AES-128/CFB8 encryption, zlib compression,
  and the current Login Success fields (including the Session ID of 776).
  Deliberately out: legacy server list ping, Strict Error Handling
  (removed in 1.21.2), and any old path.
- The version is **a single centralized parameter** (`SUPPORTED_PROTOCOL_VERSION`
  and the codec limits in `hyperion_protocol`), not scattered literals.
- **There are no per-version branches.**

### When 26.3+ ships

- `main` is updated to the new version: bump the version parameter, regenerate
  the data (see ADR 0005), and adjust what changed. One release = one data commit
  + regenerate, **not** a new branch.

### Multi-version (future, not before Phase 2)

- Per-version branches are **not** created. Evidence that that model is expensive even
  for those who pay for it: Paper maintains a `ver/<version>` branch per supported minor
  (a fork with its own patches per branch), documents the rebase flow and its
  conflicts, explicitly avoids backports to old versions, and only
  maintains 3–5 recent versions before abandoning them.
- The dominant pattern in serious servers is **one codebase for the latest
  version** (Pumpkin, Valence, Feather). If a wide client range is to be accepted,
  protocol translation lives in a **separate layer** like ViaVersion
  (a single codebase covering 1.7.2 → latest).
- In Hyperion multi-version will rest on **versioned codegen (ADR 0005)**:
  each version = a set of JSON data + generated parsers; the core (world,
  simulation, plugins) remains version-agnostic.
- If later several versions are to be served simultaneously, either an own
  translation layer or a front proxy is evaluated (ROADMAP Phase 5 already contemplates
  both options).

## Consequences

- During Phase 1 the codec is written by hand, but with an eye on
  ADR 0005: packet layouts must end up in **versioned data**, not in
  scattered constants. Fuzzing and tests already cover the parsers.
- The cost of "changing version" today is small because the codec is compact and
  of stable structure (IDs and limits by constant).
- **No multi-version infrastructure is introduced before Phase 2**: it would be
  premature. This decision is recorded so unnecessary branches or
  feature-flags are not built.

## References

- Pumpkin (latest version only, single codebase):
  https://github.com/Pumpkin-MC/Pumpkin
- Valence ("targets the most recent stable version... multi-version not planned"):
  https://github.com/valence-rs/valence
- Feather (1.16.5 only, multi-version not planned):
  https://github.com/feather-rs/feather
- Paper (per-version branches; rebase cost and version policy):
  https://github.com/PaperMC/Paper · https://github.com/PaperMC/Paper/blob/main/CONTRIBUTING.md
- Paper #9790 (no backports to old branches):
  https://github.com/PaperMC/Paper/issues/9790
- ViaVersion (protocol translator, one codebase, 1.7.2 → latest):
  https://github.com/ViaVersion/ViaVersion
- Velocity (one binary; only handshake/login translated, play untouched):
  https://github.com/PaperMC/Velocity · https://docs.papermc.io/velocity/server-compatibility
