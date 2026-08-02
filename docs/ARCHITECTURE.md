# 🏗️ Hyperion — Arquitectura

> Documento vivo. Esta es la arquitectura objetivo; se refina con cada fase.

## Principios

1. **Un hilo por región, nunca datos compartidos entre regiones.** El tick de una región solo toca su mundo. Las interacciones cross-región (portales, teleports) son mensajes asíncronos.
2. **Cero GC, cero locks globales.** Todo lo que se puede paralelizar se paraleliza; lo que no, se serializa explícitamente.
3. **Los datos generados no se escriben a mano.** Protocolo, registries y NBT salen de codegen desde JSON extraídos de Mojang.
4. **Sin `unsafe` salvo FFI puntual y auditado.** La superficie de red y de plugins es 100% segura.

## Capas

### 1. Red (hyperion_server + hyperion_protocol)
- `tokio` async: cientos de miles de conexiones con un pool pequeño de hilos.
- Cifrado AES/CFB8, compresión zlib, handshake RSA-1024 (formato Mojang).
- Rate-limiting, timeouts por estado, mitigación de flooding.
- **Fuzzing** de cada parser de paquete (`cargo-fuzz`), corpus en el repo.

### 2. Protocolo (hyperion_protocol)
- Paquetes definidos en JSON extraído → `build.rs` genera structs + coders.
- **NBT propio**: lector/escritor streaming, sin dependencias externas.
- Multi-versión: capa de remapping de block-states entre versiones soportadas (rango corto en v1: 1.21 → 26.x, modelo Pumpkin).

### 3. Mundo (hyperion_world)
- **Chunks**: formato Anvil (compat con mundos vanilla, lectura/escritura) + **HCF** (Hyperion Chunk Format) para I/O multihilo ultrarrápida.
- **Worldgen 1:1**: ruido + biomas + superficie + features + estructuras. Referencia: cubiomes (MIT) para biomas/estructuras; núcleo de terreno propio. Verificación por diff contra vanilla (misma seed).
- **Luz**: sky/block light, cálculo paralelo.

### 4. Simulación (hyperion_simulation)
- **Ticking por regiones**: partición del mundo en regiones de ~N×N chunks; cada región tiene su propio loop de tick a 20 TPS.
- **ECS**: entidades como componentes; sistemas por región; acceso cache-friendly.
- Subsistemas: física, líquidos, redstone, IA de mobs, inventarios, daño.
- **Interacción cross-región**: cola de mensajes; un jugador que cruza de región "migra" (transferencia de estado, no de datos compartidos).

### 5. Plugins (hyperion_plugin_api)
- **ABI WASM/WIT**: contrato estable de plugins (WIT = WebAssembly Interface Types).
- **Runtime wasmtime**: sandbox por capacidades; el plugin no toca el sistema salvo lo que la API concede.
- Ciclo de vida: `on_load` / `on_enable` / `on_disable`; eventos; comandos (árbol Brigadier); scheduler por región.
- **Scripting Lua** (MLua) sobre la misma API para plugins en 20 líneas.
- Hot-reload limpio (WASM se descarga sin `dlopen`).

## Flujo de datos (jugador entrando)

```
Cliente ──handshake/login──▶ Red (tokio) ──▶ Protocolo (parse + fuzz)
    │                                               │
    │                                    Auth/cifrado/compresión
    │                                               ▼
    │                                   Simulación (asigna región)
    │                                               │
    ◀────────── chunk data + entidades ─────────────┘
```

## Codegen (la máquina de actualizaciones)

```
Mojang jar / data generators ──▶ tools/extract ──▶ JSON versionado
                                                      │
                        build.rs (codegen) ◀───────────┘
                                                      │
                          structs + coders + registries
```

Cada release de Mojang = un commit de datos nuevos + regenerar. La parte LLM (opcional) asiste en los diffs, siempre verificada por fuzzing y diff testing.

## Estado de conexión (máquina de estados)

`Handshake → Status | Login → (cifrado → compresión) → Configuration → Play`

Cada estado tiene su propio conjunto de paquetes, timeouts y fuzzing.