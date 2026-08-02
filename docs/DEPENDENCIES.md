# 📦 Hyperion — Política de dependencias

> **Principio**: menos es más. Cada crate externo es una deuda de mantenimiento.
> Solo se aceptan dependencias con mantenimiento demostrado, licencia permisiva
> y sin `unsafe` innecesario. Lo específico de Minecraft se implementa aquí.

## Reglas

1. **Ninguna dependencia nueva sin PR de auditoría** (justificar: propósito, licencia, mantenimiento, alternativa).
2. **Auditar cada fase** del ROADMAP: si una dependencia lleva 6+ meses sin release y sin commits, se reemplaza o se elimina.
3. **`cargo audit` en CI** (alertas de vulnerabilidades).
4. **Minimizar dependencias transitivas**: preferir crates del mismo ecosistema mantenido.
5. **Preferir zero-dependency** para todo lo específico de Minecraft.

## Lista blanca (verificada 2026-08-02)

| Crate | Propósito | Licencia | Mantenido por | Estado | Se añade en |
|---|---|---|---|---|---|
| `tokio` | Runtime async (red) | MIT | Tokio team (AWS) | ✅ activo | Fase 1 |
| `bytes` | Buffers de red | MIT | Tokio team | ✅ activo | Fase 1 |
| `flate2` / `miniz_oxide` | Compresión zlib | MIT/Apache-2.0 | Alex Crichton | ✅ activo | Fase 1 |
| `sha1`, `sha2`, `aes`, `rsa` | Cripto handshake | Apache-2.0/MIT | RustCrypto | ✅ activo | Fase 1 |
| `serde` + `serde_json` | Datos extraídos | MIT/Apache-2.0 | Serde team | ✅ activo | Fase 1 |
| `uuid` | IDs de jugador/entidad | MIT/Apache-2.0 | uuid-rs | ✅ activo | Fase 1 |
| `thiserror` | Errores ergonómicos | MIT/Apache-2.0 | dtolnay | ✅ activo | Fase 1 |
| `tracing` | Logging estructurado | MIT | Tokio team | ✅ activo | Fase 1 |
| `rayon` | Paralelismo de datos | MIT/Apache-2.0 | Rayon team | ✅ activo | Fase 3 |
| `crossbeam` | Utilidades de concurrencia | MIT/Apache-2.0 | Crossbeam team | ✅ activo | Fase 3 |
| `parking_lot` | Locks más rápidos | MIT/Apache-2.0 | Amanieu | ✅ activo | Fase 3 |
| `dashmap` | Mapas concurrentes | MIT | xacrimon | ✅ activo | Fase 3 |
| `wasmtime` | Runtime WASM de plugins | Apache-2.0 | Bytecode Alliance (18.4k★) | ✅ activo | Fase 4 |
| `mlua` | Scripting Lua | MIT | mlua-rs (2.8k★) | ✅ activo | Fase 4 |
| `cubiomes` (C, FFI) | Referencia worldgen biomas/estructuras | **MIT** | Cubitect | ✅ activo | Fase 2 (opcional) |
| `bevy_ecs` | ECS (decisión abierta) | MIT/Apache-2.0 | Bevy org | ✅ activo | Fase 3 (decisión) |

## Lo que implementamos nosotros (sin dependencia externa)

| Módulo | Por qué |
|---|---|
| **Protocolo / paquetes** | Específico de Minecraft; codegen desde JSON |
| **NBT** | Formato simple y público; control total de rendimiento |
| **Worldgen núcleo** | Paridad 1:1 exige control fino; cubiomes solo como referencia |
| **Formato de mundo HCF** | I/O multihilo ultrarrápida |
| **Simulación / region ticking** | Núcleo del producto; sin delegación |
| **ABI de plugins** | Contrato estable propio (WIT) |

## Excluidos explícitamente

| Crate | Por qué NO |
|---|---|
| `jni-rs` / puentes JVM | Hyperion no embebe Java; la compat con plugins Java (bytecode) está descartada por diseño — ver README |
| `libloading` / `dlopen` | Plugins nativos = código arbitrario sin sandbox; además no se descargan en Windows. WASM resuelve el problema |
| Crates de terceros de "minecraft server" (valence, etc.) | Hyperion es un proyecto desde cero: aprender y controlar todo. Solo se estudian como referencia |

## Fallbacks (si una dependencia muere)

- `tokio`/`bytes` → std + threads (coste: más código, menos abstracción)
- `flate2` → `miniz_oxide` (ya transitiva) o zlib-sys propio
- RustCrypto → implementación propia del handshake (AES/CFB8 + RSA-1024 son algoritmos pequeños)
- `wasmtime` → `wasmer` (misma licencia, ecosistema similar) — decisión con fecha de caducidad
- `mlua` → runtime Lua propio (Lua 5.4 es pequeño) o scripting Rhai

## Decisión registrada: ¿por qué WASM y no cdylib para plugins?

- **Seguridad**: WASM = sandbox por capacidades; cdylib = código arbitrario con acceso total.
- **Portabilidad**: WASM funciona igual en Windows/Linux/macOS; cdylib no se descarga en Windows.
- **Hot-reload**: WASM se descarga limpiamente; los `.so`/`.dll` no.
- **Es la dirección que está tomando el ecosistema nativo** (Pumpkin migra de libloading a WIT/WASM).

*Última auditoría: 2026-08-02 · Próxima: al inicio de cada fase del ROADMAP.*