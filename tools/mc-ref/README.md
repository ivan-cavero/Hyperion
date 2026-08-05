# mc-ref — vanilla 26.2 join data generators

Rust tools that rebuild the embedded Configuration / world data used by Hyperion.
**No Python required.**

## Prerequisites

- Java 21+ (for the Mojang data generator only)
- Vanilla `server.jar` for **26.2**
- Rust toolchain (workspace)

## Setup (once per machine)

```powershell
# 1) Download server.jar 26.2 into this folder as server-26.2.jar
# 2) Extract the inner jar from the bundler:
#    META-INF/versions/26.2/server-26.2.jar → server-inner-26.2.jar

java "-DbundlerMainClass=net.minecraft.data.Main" `
  -jar server-26.2.jar `
  --reports --output datagen/generated
```

Mojang’s data generator writes JSON reports (`blocks.json`, `registries.json`, …).
JSON as intermediate format is intentional and fine; Hyperion then turns it into
committed Rust tables / binary blobs via `hyperion_tools`.

## Regenerate embedded assets

```powershell
# All three generators:
cargo run -p hyperion_tools --bin gen-mc-ref

# Or individually:
cargo run -p hyperion_tools --bin gen-join-data
cargo run -p hyperion_tools --bin gen-registry-nbt
cargo run -p hyperion_tools --bin gen-block-states
```

| Output | Purpose |
|---|---|
| `crates/hyperion_server/src/network/join_data/generated.rs` | Known pack id, registry entry lists, full Update Tags |
| `crates/hyperion_server/src/network/join_data/registry_nbt.bin` | Network NBT for every synchronized registry entry |
| `crates/hyperion_world/src/generated/block_states.rs` | Default block-state ids (binary-search table + constants) |

Generated files are **committed** so CI does not need jars or reports.

Jars, `datagen/`, `classes/` and extracts are gitignored.
