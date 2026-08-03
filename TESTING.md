# Testing en Hyperion

Cómo se testea Hyperion, qué ejecuta cada comando y dónde vive cada test.

## Matriz rápida

| Comando | Qué cubre | Uso |
|---|---|---|
| `cargo test --workspace` | Todos los tests (unit + integración + doctests) | Verificación rápida tras un cambio |
| `cargo nextest run --workspace` | Lo mismo, pero paralelo, con mejor aislamiento y salida por test | Desarrollo diario y CI |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lints, con warnings como errores | Antes de cada commit/PR (lo exige el CI) |
| `cargo fmt --all -- --check` | Formato | Antes de cada commit/PR |
| `cargo llvm-cov --workspace` | Cobertura de líneas/regiones | Para medir huecos de cobertura |
| `cargo llvm-cov --workspace --lcov --output-path lcov.info` | Reporte en formato LCOV | CI (sube a Codecov) |
| `cargo +nightly fuzz run <target>` (en `crates/hyperion_protocol/fuzz`) | Fuzzing de parsers de red | Tras tocar cualquier parser (ver abajo) |
| `cargo audit` / `cargo deny check` | Vulnerabilidades / licencias | CI |

Los comandos de la matriz se ejecutan desde la raíz del workspace, salvo que
se indique otro directorio.

## Dónde vive cada test

```
crates/hyperion_protocol/src/*.rs          # unit tests junto al código (round-trips de codificación)
crates/hyperion_protocol/tests/            # integración del protocolo (ej. handshake completo)
crates/hyperion_protocol/fuzz/fuzz_targets/# fuzzing obligatorio de parsers de red
crates/hyperion_server/src/                # unit tests inline donde aplica
crates/hyperion_server/tests/              # integración end-to-end sobre sockets TCP reales
crates/hyperion_server/tests/common/       # infraestructura compartida (MockClient, mock session server)
```

### Convenciones por capa

- **Protocolo** (`hyperion_protocol`): cada función de codificación/decodificación
  lleva tests de round-trip (encode → decode → valor original) y de bordes
  (truncado, var-int inválido, longitudes fuera de rango). Los parsers de
  entrada de red tienen además un target de fuzzing.
- **Servidor** (`hyperion_server`): la lógica de red se testea como *caja
  negra* desde `tests/`: un `MockClient` (en `tests/common/`) habla el
  protocolo real —incluidos cifrado AES/CFB8 y compresión zlib— contra un
  listener real, y cada test ejecuta `handle_connection` en una tarea tokio.
  Los flujos que llaman a Mojang usan `mock_session_server` (HTTP en memoria).
- **Placeholders** (`hyperion_core`, `hyperion_world`, …): tests triviales
  hasta que tengan lógica real.

### Añadir un test nuevo

1. **Unit**: junto al código, `#[cfg(test)] mod tests`.
2. **Integración de red**: nuevo archivo en `crates/hyperion_server/tests/`
   con `mod common;` en la cabecera; reutiliza `MockClient` y los helpers de
   `tests/common/mod.rs` (no los reimplementes).
3. **Parser nuevo en el protocolo**: unit tests de round-trip **y** un target
   en `crates/hyperion_protocol/fuzz/fuzz_targets/` + entrada en la matriz del
   job `fuzz` de CI.

## Cobertura

Baseline actual (medido con `cargo llvm-cov --workspace`):

- **Líneas: ~89 %** · Regiones: ~86 % (sin contar `main.rs`, ver abajo).
- `hyperion_server/src/main.rs` figura al 0 % porque es el entry point del
  binario y los tests ejercitan la librería. Es un wrapper delgado y se acepta
  sin cobertura; si algún día crece, añadir un smoke test con `assert_cmd`.
- Huecos conocidos que merecen tests cuando se toque esa zona:
  - `network/mod.rs` (~32 %): `serve()` (bucle de aceptación) y las ramas de
    logging de errores de conexión no están cubiertas.
  - `network/login.rs` (~76 %): fallos de descifrado RSA del shared secret,
    longitudes de secret inválidas y errores de generación de claves.
  - `protocol/status.rs` (~82 %) y `protocol/compression.rs` (~91 %): bordes
    de payloads malformados.

Para regenerar el baseline localmente:

```sh
cargo llvm-cov --workspace
```

## Fuzzing

Obligatorio para todo parser que reciba bytes de red. Targets actuales:
`frame`, `handshake`, `status`, `login`.

```sh
cd crates/hyperion_protocol/fuzz
cargo +nightly fuzz run frame        # en bucle hasta Ctrl-C
cargo +nightly fuzz run frame -- -max_total_time=60   # sesión acotada
```

El CI ejecuta un smoke test de 30 s por target en cada PR. Los crashes se
suben como artefacto (`fuzz-artifacts-*`).

## Herramientas locales

`cargo-nextest` y `cargo-llvm-cov` se instalan con:

```sh
cargo install cargo-nextest --locked
cargo install cargo-llvm-cov --locked
```

(o vía `taiki-e/install-action` en CI, que es lo que usa el workflow).
