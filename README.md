# ⚡ Hyperion

> Servidor de Minecraft **nativo en Rust**: seguro, multinúcleo, imparable.
> *Estado: pre-alpha / planificación activa — ver [ROADMAP.md](ROADMAP.md)*

---

## ¿Qué es Hyperion?

Hyperion es un servidor de Minecraft **Java Edition** escrito **desde cero en Rust**, sin una sola línea de Java en el servidor. No es un fork de Paper ni un wrapper: es una implementación independiente del protocolo y de la simulación del juego, diseñada desde el primer día para:

- 🚀 **Rendimiento extremo** — aprovecha TODOS los núcleos del servidor con *ticking por regiones* (sin hilo principal único, sin GC pauses).
- 🛡️ **Seguridad por construcción** — memoria segura garantizada por el compilador + sandboxing real de plugins (WebAssembly).
- 🎯 **Paridad vanilla 1:1** — la misma seed produce el mismo mundo, bloque a bloque, como en el servidor oficial.
- 🔄 **Actualización de versiones asistida** — pipeline de extracción de datos + generación de código para adaptarse a cada release de Mojang en días, no semanas.
- 🧩 **Ecosistema de plugins propio** — una API más potente que Bukkit y a la vez más simple, con plugins en WASM (seguros) y scripting en Lua.

---

## Objetivos (v1)

| Área | Objetivo |
|---|---|
| **Protocolo** | Java Edition, versión actual de Mojang + rango reciente multi-versión |
| **Mundo** | Worldgen vanilla 1:1 (terreno, biomas, estructuras) con la misma seed |
| **Simulación** | Comportamiento vanilla: física, líquidos, redstone, mobs, chunks |
| **Rendimiento** | Ticking por regiones multihilo; 500–1000+ jugadores en un mundo |
| **Seguridad** | Fuzzing del protocolo, zero `unsafe` salvo FFI justificado, plugins sandboxed |
| **Plugins** | API nativa WASM/WIT + scripting Lua (MLua) |
| **Versiones** | Pipeline datos→codegen; adaptación rápida a cada release |

## No objetivos (v1) — decisiones deliberadas

- **Compatibilidad con plugins Bukkit/Spigot/Paper**: no en v1; en investigación como capa OPT-IN de menor rendimiento (Vía C: TeaVM→WASM; plan B: JVM embebida). Ver [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md)
- **Mods Forge/Fabric/NeoForge**: client-side gratuito (los carga el cliente); server-side solo vía portes manuales a la API nativa. Expectativa realista: los mods pesados no corren en el núcleo nativo. Ver [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md)
- ❌ **"Miles de jugadores en un solo proceso con simulación completa"**: ningún servidor (ni Java ni nativo) lo ha sostenido en producción. El escalado horizontal (proxy + múltiples regiones/procesos) se abordará en fases posteriores.

---

## Arquitectura (resumen)

```
┌──────────────────────────────────────────────────────────┐
│  RED (tokio, async)         │  cifrado · compresión · fuzz │
├──────────────────────────────────────────────────────────┤
│  PROTOCOLO (codegen)        │  paquetes Java (+Bedrock)   │
│  · multi-versión con remapping de block-states            │
├──────────────────────────────────────────────────────────┤
│  MUNDO                      │  worldgen 1:1 · chunks      │
│  · Anvil (compat) + formato propio ultra-rápido          │
├──────────────────────────────────────────────────────────┤
│  SIMULACIÓN — TICKING POR REGIONES (multihilo)           │
│  · ECS · entidades · líquidos · redstone · mobs          │
├──────────────────────────────────────────────────────────┤
│  PLUGINS — WASM/WIT sandboxed + Lua scripting            │
└──────────────────────────────────────────────────────────┘
```

Detalle completo: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)

---

## Política de dependencias

**Menos es más.** Solo dependemos de crates con mantenimiento demostrado (listado completo con licencias y justificación en [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md)):

- `tokio`, `bytes` (runtime async y buffers — mantenidos por el equipo Tokio)
- `rayon` (paralelismo de datos)
- `serde` + `serde_json` (serialización de datos extraídos)
- `uuid`, `thiserror`, `tracing` (utilidades estándar)
- `flate2`/`miniz_oxide` (compresión zlib del protocolo y mundo)
- RustCrypto (`sha1`, `sha2`, `aes`, `rsa`) para el handshake y cifrado
- `wasmtime` (runtime WASM para plugins — Bytecode Alliance) y `mlua` (scripting) — **fase 4**

**Lo que implementamos nosotros** (cero dependencia externa, control total):
- Protocolo completo (generado por codegen desde datos extraídos)
- Formato **NBT** (especificación pública, simple)
- Worldgen (núcleo propio + referencia opcional a `cubiomes`, **MIT**)
- Formatos de mundo, simulación, API de plugins

**Excluido explícitamente**: `jni-rs` y cualquier puente JVM — el proyecto no embebe Java.

---

## Estructura del repositorio

```
Hyperion/
├── crates/
│   ├── hyperion_core/         # fundamentos: tipos, matemáticas, registries
│   ├── hyperion_protocol/     # codec de paquetes, NBT, multi-versión
│   ├── hyperion_world/        # chunks, worldgen 1:1, formatos de mundo
│   ├── hyperion_simulation/   # ticking por regiones, ECS, entidades
│   ├── hyperion_plugin_api/   # ABI WASM/WIT, eventos, comandos, scripting
│   └── hyperion_server/       # binario: red, arranque, wiring
├── docs/                      # arquitectura, dependencias y ADRs (decisiones)
├── tools/                     # extractores de datos y codegen
└── README.md · ROADMAP.md · CONTRIBUTING.md · LICENSE
```

---

## Seguridad

- Memoria segura por el compilador (sin `unsafe` salvo FFI puntual y auditado).
- **Fuzzing del protocolo** desde el día 1 (`cargo-fuzz`) — ninguna entrada de red sin fuzz.
- Plugins en **WebAssembly** ejecutados en `wasmtime`: aislados por capacidades, sin acceso al sistema salvo lo concedido.
- Cifrado oficial (AES/CFB8 + RSA), rate-limiting, timeouts y mitigación de DoS.

## Rendimiento

- **Ticking por regiones**: cada región del mundo en su propio hilo (patrón validado por Folia y MCHPRS).
- **ECS** para entidades (cache-friendly, sin punteros dispersos).
- **Chunk-gen en pools de workers** + formato de mundo propio para carga/save ultrarrápida.
- Pregeneración de mundo soportada (elimina el cuello de botella principal).

---

## Contribuir

Lee [CONTRIBUTING.md](CONTRIBUTING.md). Bienvenidos issues, PRs, fuzzing, benchmarks y documentación.

## Licencia

**MIT** — Hyperion contributors. Proyecto independiente, sin afiliación con Mojang/Microsoft. El uso del protocolo y formato de mundo sigue las reglas del EULA de Minecraft.