# Contributing to Hyperion

Thanks for wanting to help! Hyperion is an ambitious project that only moves forward with community. Here is how to take part.

## Getting started

1. **Fork** the repository and clone it.
2. Install Rust (stable) — `rustup.rs`.
3. `cargo build` from the workspace root.
4. `cargo test` and `cargo clippy` must pass.
5. Pick an issue labeled `good-first-issue` or propose one.

## Conventions

- **Rust**: clean `rustfmt` + `clippy` (no warnings). CI enforces this.
- **No `unsafe` without justification**: if you need `unsafe`, it must live in an isolated module with a `// SAFETY:` comment explaining why it is correct.
- **Tests**: all network/protocol code includes round-trip unit tests. Fuzzing is mandatory for any network input parser.
- **Commits**: clear messages in English, crate prefix when applicable (e.g. `protocol: fix keep-alive timeout`).
- **Docs**: public API documented (`///`).

## PR flow

1. Create a branch from `main`.
2. Make small, reviewable changes.
3. Run locally: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (the full command matrix and what each covers is in [`TESTING.md`](TESTING.md)).
4. Open the PR describing *what* and *why* (and how you tested it).
5. A maintainer reviews; discuss in the thread until approved.

## Areas where help is always needed

- **Fuzzing** the protocol (find crashes before attackers do).
- **Data**: extract and validate registries for each Minecraft version.
- **Benchmarks** comparing against Paper/Folia.
- **Documentation** and plugin tutorials.
- **Code**: worldgen, simulation, network, plugin API.

## Code of conduct

Be respectful. Everyone is learning. Personal attacks, spam, and harassment are not tolerated — maintainers may remove anyone who commits them.

## License

By contributing you agree that your code is licensed under MIT (same as the project).
