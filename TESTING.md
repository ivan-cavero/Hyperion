# Testing in Hyperion

How Hyperion is tested, what each command runs, and where each test lives.

## Quick matrix

| Command | What it covers | Use |
|---|---|---|
| `cargo test --workspace` | All tests (unit + integration + doctests) | Quick check after a change |
| `cargo nextest run --workspace` | Same, but parallel, with better isolation and per-test output | Daily development and CI |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lints, with warnings as errors | Before every commit/PR (required by CI) |
| `cargo fmt --all -- --check` | Formatting | Before every commit/PR |
| `cargo llvm-cov --workspace` | Line/region coverage | To measure coverage gaps |
| `cargo llvm-cov --workspace --lcov --output-path lcov.info` | Report in LCOV format | CI (uploads to Codecov) |
| `cargo +nightly fuzz run <target>` (in `crates/hyperion_protocol/fuzz`) | Fuzzing of network parsers | After touching any parser (see below) |
| `cargo audit` / `cargo deny check` | Vulnerabilities / licenses | CI |

Matrix commands run from the workspace root, unless another directory is indicated.

## Where each test lives

```
crates/hyperion_protocol/src/*.rs          # unit tests next to the code (encoding round-trips)
crates/hyperion_protocol/tests/            # protocol integration (e.g. full handshake)
crates/hyperion_protocol/fuzz/fuzz_targets/# mandatory fuzzing of network parsers
crates/hyperion_server/src/                # inline unit tests where applicable
crates/hyperion_server/tests/              # end-to-end integration over real TCP sockets
crates/hyperion_server/tests/common/       # shared infrastructure (MockClient, mock session server)
```

### Conventions by layer

- **Protocol** (`hyperion_protocol`): every encode/decode function has round-trip
  tests (encode → decode → original value) and edge cases (truncated, invalid
  var-int, out-of-range lengths). Network input parsers also have a fuzzing
  target.
- **Server** (`hyperion_server`): network logic is tested as a *black box* from
  `tests/`: a `MockClient` (in `tests/common/`) speaks the real protocol
  —including AES/CFB8 encryption and zlib compression— against a real listener,
  and each test runs `handle_connection` in a tokio task. Flows that call Mojang
  use `mock_session_server` (in-memory HTTP).
- **Placeholders** (`hyperion_core`, `hyperion_world`, …): trivial tests until
  they have real logic.

### Adding a new test

1. **Unit**: next to the code, `#[cfg(test)] mod tests`.
2. **Network integration**: new file in `crates/hyperion_server/tests/` with
   `mod common;` at the top; reuse `MockClient` and the helpers in
   `tests/common/mod.rs` (do not reimplement them).
3. **New protocol parser**: round-trip unit tests **and** a target in
   `crates/hyperion_protocol/fuzz/fuzz_targets/` + entry in the CI `fuzz` job
   matrix.

## Coverage

Current baseline (measured with `cargo llvm-cov --workspace`):

- **Lines: ~89 %** · Regions: ~86 % (excluding `main.rs`, see below).
- `hyperion_server/src/main.rs` shows 0 % because it is the binary entry point
  and tests exercise the library. It is a thin wrapper and is accepted without
  coverage; if it ever grows, add a smoke test with `assert_cmd`.
- Known gaps that deserve tests when that area is touched:
  - `network/mod.rs` (~32 %): `serve()` (accept loop) and connection error
    logging branches are not covered.
  - `network/login.rs` (~76 %): RSA shared-secret decrypt failures, invalid
    secret lengths, and key generation errors.
  - `protocol/status.rs` (~82 %) and `protocol/compression.rs` (~91 %): edges
    of malformed payloads.

To regenerate the baseline locally:

```sh
cargo llvm-cov --workspace
```

## Fuzzing

Mandatory for every parser that receives network bytes. Current targets:
`frame`, `handshake`, `status`, `login`.

```sh
cd crates/hyperion_protocol/fuzz
cargo +nightly fuzz run frame        # loop until Ctrl-C
cargo +nightly fuzz run frame -- -max_total_time=60   # bounded session
```

CI runs a 30 s smoke test per target on every PR. Crashes are uploaded as
artifacts (`fuzz-artifacts-*`).

## Local tools

`cargo-nextest` and `cargo-llvm-cov` are installed with:

```sh
cargo install cargo-nextest --locked
cargo install cargo-llvm-cov --locked
```

(or via `taiki-e/install-action` in CI, which is what the workflow uses).
