# 🗺️ Hyperion — ROADMAP

> Plan de desarrollo completo. Las fechas son orientativas para un equipo de 1–3 personas a tiempo parcial.
> Cada fase termina con criterios de salida verificables. El plan se revisa al final de cada fase.

**Leyenda de estado**: ⬜ pendiente · 🔄 en curso · ✅ completado · ⏸️ aplazado

---

## Visión de producto (norte)

> Un servidor de Minecraft nativo en Rust que el ecosistema **elija por rendimiento y seguridad**:
> más rápido que Paper, más seguro que cualquier server actual, con plugins sandboxed
> y actualización de versiones en días. Primero hubs/minijuegos, luego survival de alto rendimiento.

**No-norte** (cosas que NO somos): un reemplazo de Bukkit/Forge, un proceso mágico de "miles de jugadores", un clon de Paper.

---

## FASE 0 — Fundación (mes 0–1) · ✅ (completada 2026-08-02)

Preparar el terreno: decisiones, toolchain, CI, esqueleto del workspace.

- [x] Nombre (Hyperion), licencia MIT y branding básico — **repo público: pendiente de crear en GitHub**
- [x] Workspace Cargo con los 6 crates (`hyperion_core`, `hyperion_protocol`, `hyperion_world`, `hyperion_simulation`, `hyperion_plugin_api`, `hyperion_server`)
- [x] Toolchain Rust estable fijada (`rust-toolchain.toml`) + `rustfmt.toml` + `.editorconfig` + `.gitattributes` (LF)
- [x] CI completo: `cargo build` · `cargo test` · `cargo clippy -D warnings` · `cargo fmt --check` + **cargo-audit** (seguridad) + **cargo-deny** (licencias) + dependabot — fuzz se añade en Fase 1
- [ ] Decisión de worldgen: núcleo propio vs. referencia a cubiomes (MIT) vs. heredar de Pumpkin (GPL) — **ADR 0001 abierto, límite Fase 2**
- [ ] Configuración de extracción de datos (ver Fase 2) — elegir fuente: data generators de Mojang + jar extractor
- [x] README + ROADMAP + CONTRIBUTING + docs (ARCHITECTURE, DEPENDENCIES, ADR 0001–0005) publicados
- [x] Primer commit de fundación creado

**Criterio de salida**: `cargo test` verde en CI, un binario `hyperion-server` que imprime versión y arranca. ✅

---## FASE 1 — Red y protocolo (mes 1–4) · ⬜

El corazón del proyecto: hablar el protocolo de Minecraft con seguridad.

- [ ] **Handshake + status (ping)**: responder al server list del cliente
- [ ] **Login completo**: RSA-1024 handshake, AES/CFB8, compresión zlib
- [ ] **Play**: paquetes básicos (join game, keep-alive, chat, position)
- [ ] **Codec de paquetes generado** por codegen desde JSON extraído (registries + protocolo)
- [ ] **NBT propio** (lectura/escritura, streaming, sin dependencias)
- [ ] **Fuzzing**: corpus de paquetes para cada estado (handshake/login/play) — `cargo-fuzz`
- [ ] **Unit tests** de todos los codecs (round-trip byte→struct→byte)
- [ ] Herramienta `tools/packet_inspector` para depurar tráfico real contra un cliente vanilla

**Criterio de salida**: un cliente vanilla 26.x entra al servidor, ve el mundo vacío, chatea y se mueve, con fuzzing verde en CI.

---

## FASE 2 — Mundo y worldgen 1:1 (mes 4–8) · ⬜

La promesa "misma seed, mismo mundo" se cumple aquí.

- [ ] **Pipeline de extracción de datos**: mod Fabric o data generators de Mojang → JSON versionado (registries, biomes, items, protocolo)
- [ ] **Codegen**: `build.rs` genera Rust desde los JSON (structs, coders, registries)
- [ ] **Chunks**: formato Anvil (lectura/escritura, compat con mundos existentes)
- [ ] **Worldgen**: ruido (simplex/octaves), biomas, superficie, cuevas, minerales, árboles — objetivo paridad bloque a bloque
- [ ] **Estructuras**: stronghold, villages, bastions… (fase WIP — declarar nivel de paridad honesto)
- [ ] **Formato propio** "Hyperion chunk format" (HCF) para carga/save multihilo ultrarrápida
- [ ] **Luz**: cálculo de luz (sky/block) multihilo
- [ ] **Testing diferencial**: comparar chunks generados contra vanilla (misma seed) en CI
- [ ] Decision: worldgen heredado vs propio (cierre del punto abierto de Fase 0)

**Criterio de salida**: misma seed → mismo chunk en Hyperion y en vanilla (suite de diff tests), mundos Anvil existentes cargables.

---

## FASE 3 — Simulación multinúcleo (mes 8–12) · ⬜

Donde Hyperion se separa del resto: el hilo único desaparece.

- [ ] **Ticking por regiones**: grupos de chunks independientes, cada uno con su thread (patrón Folia/MCHPRS)
- [ ] **ECS** (`bevy_ecs` o sistema propio): entidades, componentes, sistemas — cache-friendly
- [ ] **Física y movimiento**: gravedad, colisiones con el mundo
- [ ] **Líquidos**: flujo básico (agua/lava) con update por región
- [ ] **Redstone**: circuito básico (wire, torches, repeaters, comparators) — sin locks globales
- [ ] **Mobs básicos**: spawn/despawn, AI simple (zombies, skeletons), daño y muerte
- [ ] **Inventarios y bloques interactivos**: cofres, hornos, crafing básico
- [ ] **Interacción cross-región** SOLO por mensajes (ports, teleports) — sin datos compartidos
- [ ] Profiling (perf/tracy) y benchmarks internos

**Criterio de salida**: 100+ jugadores en un mundo con simulación activa a 20 TPS estables, sin locks globales.

---

## FASE 4 — API de plugins (mes 10–14) · ⬜

El diferenciador: plugins seguros y simples.

- [ ] **ABI WASM/WIT**: contrato estable de plugins (ciclo de vida `on_load`/`on_enable`/`on_disable`)
- [ ] **Runtime**: `wasmtime` embebido, sandbox por capacidades (sin acceso al sistema salvo concedido)
- [ ] **Eventos**: sistema de eventos (player join, block break, chat…) con dispatch por regiones
- [ ] **Comandos**: registro con árbol estilo Brigadier
- [ ] **Scheduler**: tareas async/sync, programadas por región
- [ ] **Scripting Lua** (MLua) como capa de simplicidad: plugin en 20 líneas
- [ ] **SDK de plugins**: plantillas (cargo-generate), docs, ejemplos
- [ ] Hot-reload seguro de plugins (sin `dlopen` — WASM se descarga limpiamente)

**Criterio de salida**: un plugin WASM de ejemplo (comando + evento + scheduler) funciona de extremo a extremo, documentado en la landing.

---

## FASE 5 — Multi-versión y actualización asistida (mes 12–16) · ⬜

Tu idea de "adaptarnos a cada release fácilmente" se vuelve sistema.

- [ ] **Remapping de block-states** entre versiones (modelo Pumpkin: rango 1.21→26.x)
- [ ] **Diff automático**: al salir una versión nueva, comparar JSONs → generar mappings
- [ ] **Codegen versionado**: un release = un commit de datos + regenerar código
- [ ] **LLM como asistente de diffs** (con verificación obligatoria: fuzzing + diff testing contra vanilla)
- [ ] Rango amplio (1.8+): evaluar proxy ViaVersion delante vs. traductor nativo propio (decisión en Fase 6)
- [ ] Documentar el "release playbook": pasos exactos para actualizar a una versión nueva

**Criterio de salida**: actualizar de 26.x a 26.(x+1) tomando ≤3 días-persona con el playbook, clientes 1.21+ conectando.

---

## FASE 6 — Escalado y benchmarks (mes 14–18) · ⬜

Demostrar la promesa de rendimiento con datos.

- [ ] Benchmarks públicos vs Paper y Folia (mismos hardware/mundo/población)
- [ ] Pregeneración de mundo a escala
- [ ] 500+ jugadores en un mundo con features subset a 20 TPS
- [ ] Optimización de ancho de banda (view distance dinámico, packet coalescing)
- [ ] Decisión de arquitectura para "miles": sharding horizontal con proxy (Velocity/Bungee) + MultiPaper-style, o traductor nativo multi-versión propio
- [ ] Instrumentación: métricas (tiempo de tick por región, chunk-gen, red) exportables

**Criterio de salida**: benchmark reproducible publicado en la landing con ventaja documentada sobre Paper/Folia en escenarios realistas.

---

## FASE 7 — Bedrock (opcional, mes 16–24) · ⬜

Protocolo RakNet + registries propios. Solo si Java está sólido y hay demanda.

- [ ] RakNet base + cifrado ECDH/AES
- [ ] Mapeo de entidades/bloques Java↔Bedrock
- [ ] Skin system de Bedrock

**Criterio de salida**: clientes Bedrock y Java en el mismo mundo (opcional de producto).

---

## FASE 8 — Lanzamiento (mes 18–24) · ⬜

Convertir el proyecto en producto con comunidad.

- [ ] Landing page (Astro + Tailwind): hero, benchmarks, roadmap público, "primer plugin en 5 min"
- [ ] Docs completas (Starlight/Docusaurus + rustdoc embebido)
- [ ] Beta pública: servidor de demostración (hub/minijuegos)
- [ ] Discord + directrices de contribución activas
- [ ] v1.0.0: protocolo actual, worldgen 1:1 declarado, API estable de plugins

**Criterio de salida**: v1.0.0 publicada, 100+ plugins de ejemplo creados por la comunidad, benchmarks en la portada.

---

## Riesgos y mitigaciones

| Riesgo | Severidad | Mitigación |
|---|---|---|
| **Alcance** (el mayor): paridad vanilla completa es un proyecto de equipo de años (Pumpkin lleva 2 años sin 1.0) | 🔴 Alta | MVP = hub/minijuegos; declarar paridad por capas; features subset para v1 |
| Mortandad de proyectos Rust MC (muchos mueren por alcance) | 🔴 Alta | Roadmap con hitos verificables, comunidad desde el día 1 |
| Worldgen 1:1 completo (estructuras jigsaw) | 🟠 Media | Referencia cubiomes (MIT) + diff testing; estructuras en fases |
| Multi-versión amplia (1.8+) | 🟠 Media | v1 rango corto (1.21+); ViaVersion delante como opción |
| GPLv3 de Pumpkin: si se hereda código, el proyecto pasa a GPL | 🟠 Media | Decisión de licencia en Fase 0; código propio siempre que sea posible |
| Rendimiento de WASM en plugins | 🟡 Baja | wasmtime JIT; scripting Lua para caminos calientes; benchmark en Fase 6 |
| Dependencias abandonadas | 🟡 Baja | Lista blanca con mantenimiento verificado (docs/DEPENDENCIES.md); re-auditar cada fase |

---

## Decisiones abiertas (ADR pendientes)

1. **Worldgen**: núcleo propio vs. cubiomes (MIT) vs. heredar de Pumpkin (GPL). *Fecha límite: Fase 2.*
2. **ECS**: usar `bevy_ecs` vs. sistema ECS propio minimalista. *Fase 3.*
3. **Licencia definitiva** si se incorpora código ajeno. *Fase 0.*
4. **Rango de multi-versión** para v1: solo 26.x vs. 1.21+ como Pumpkin. *Fase 5.*
5. **Bedrock**: incluir en v1.0 o posponer. *Fase 7.*

---

## Métricas de éxito (definición de "lo logramos")

- 20 TPS sostenidos con 500+ jugadores en un mundo con features subset (Fase 6)
- Actualización de versión menor en ≤3 días-persona (Fase 5)
- Plugin WASM de ejemplo funcional en <5 minutos para un desarrollador nuevo (Fase 4)
- 100+ contribuidores y 10+ plugins comunitarios antes de v1.0 (Fase 8)

---

*Hyperion · MIT · Proyecto independiente, sin afiliación con Mojang/Microsoft.*