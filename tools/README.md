# tools/

| Tool | Status | Use |
|---|---|---|
| [`mc-ref/`](mc-ref/README.md) | ready | Generates registries/tags/NBT for join from the 26.2 server.jar |
| `extract` | pending (Phase 2) | Generic Mojang data → versioned JSON pipeline |
| `codegen` | pending (Phase 2) | Rust structs/coders from JSON |
| `packet_inspector` | pending | Debug real traffic vs vanilla client |
| `stresser` | pending | Simulate players for benchmarks |

See `mc-ref/README.md` to regenerate `join_data/generated.rs` and `registry_nbt.bin`.
