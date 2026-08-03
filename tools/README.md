# tools/

| Tool | Estado | Uso |
|---|---|---|
| [`mc-ref/`](mc-ref/README.md) | listo | Genera registries/tags/NBT de join desde el server.jar 26.2 |
| `extract` | pendiente (Fase 2) | Pipeline genérico de datos Mojang → JSON versionado |
| `codegen` | pendiente (Fase 2) | structs/coders Rust desde JSON |
| `packet_inspector` | pendiente | Depurar tráfico real vs cliente vanilla |
| `stresser` | pendiente | Simular jugadores para benchmarks |

Ver `mc-ref/README.md` para regenerar `join_data/generated.rs` y `registry_nbt.bin`.
