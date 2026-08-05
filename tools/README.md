# tools/

| Tool | Status | Use |
|---|---|---|
| [`mc-ref/`](mc-ref/README.md) | ready | Vanilla 26.2 jars + reports; codegen via `hyperion_tools` |
| `extract` | pending (Phase 2) | Generic Mojang data → versioned JSON pipeline |
| `codegen` | pending (Phase 2) | Rust structs/coders from JSON |
| `packet_inspector` | pending | Debug real traffic vs vanilla client |
| `stresser` | pending | Simulate players for benchmarks |

## Offline codegen (`hyperion_tools`)

```powershell
cargo run -p hyperion_tools --bin gen-mc-ref
```

See `mc-ref/README.md` for jar setup and individual generators.
