# 🔌 Compatibilidad con plugins y mods Java — Estrategia

> Documento de estrategia. La compatibilidad es **NO prioritaria** (no bloquea
> el núcleo) y **opt-in** (activada por servidor). El rendimiento nativo nunca
> se sacrifica por compatibilidad.

**Estado**: investigación (Vía C). Decisión de arquitectura: ADR 0006.

---

## Visión

Que un servidor Hyperion pueda, **opcionalmente**, cargar plugins existentes de
Bukkit/Spigot/Paper — aceptando que esos plugins correrán **peor que en Paper**
(es el precio de no tener JVM ni el código de Mojang). El núcleo nativo
(WASM/Lua) sigue siendo el producto.

---

## Las dos vías

| Vía | Descripción | Prioridad | Estado |
|-----|-------------|-----------|--------|
| **C. TeaVM → WASM** | Compilar el plugin Java a WebAssembly y ejecutarlo en nuestro runtime WASM (`wasmtime`). La API Bukkit se reimplementa como *host imports* del WASM | 🥇 Objetivo | Investigación (Fase 4.5) |
| **B. JVM embebida** | JVM embebida (`jni-rs`) + reimplementación de un subconjunto de la API Bukkit en Rust, con shims Java | 🥈 Plan B | Solo si Vía C falla el go/no-go |

### Vía C — TeaVM → WebAssembly (objetivo)

**Arquitectura objetivo**:

```
plugin.jar ──TeaVM──▶ plugin.wasm ──▶ wasmtime (sandbox Hyperion)
                        ▲
   API Bukkit (subconjunto) = host imports implementados en Rust
```

- TeaVM (3k★, activo) compila bytecode Java → WASM/JS/C.
- El plugin "ve" la API Bukkit (JavaPlugin, onEnable, comandos…) pero la
  ejecución es WASM **sandboxed** — encaja con la arquitectura de seguridad
  de Hyperion (ADR 0002) y no introduce JVM ni GC externo.
- La API Bukkit se reimplementa **solo** como funciones import del host.
  Precedente de que la API se puede reimplementar: Cardboard (1.1k★, activo)
  sobre Fabric — aunque Cardboard corre sobre el código de Mojang; nosotros no.

**Riesgos conocidos** (documentados, no ignorados):
- TeaVM no soporta bien: reflection completa, threads, class-loading dinámico
  → una fracción de plugins no compilará.
- "Runtime dentro de runtime": el WASM contendría el GC de TeaVM → rendimiento
  impredecible (debate abierto en Pumpkin #2299, jun-2026).
- Nadie ha hecho el PoC Bukkit→WASM; es terreno nuevo.

**Milestones del PoC (Fase 4.5)**:
1. TeaVM compila un plugin mínimo (JavaPlugin + onEnable + 1 comando) → WASM.
2. Host imports: expone `Player.send_message`, `command` dispatch, permisos.
3. El plugin corre en wasmtime sobre Hyperion con un subconjunto de la API.
4. **Go/no-go**: si >X% de plugins objetivo dependen de features que TeaVM
   no soporta → degradar a Vía B (JVM embebida).

### Vía B — JVM embebida (plan B)

- Módulo opt-in `hyperion_compat`: JVM embebida vía `jni-rs` + subconjunto de
  la API Bukkit implementada en Rust; el plugin Java corre en la JVM y delega
  al núcleo nativo por JNI.
- Precedente de coste: Cardboard lleva años y aún tiene incompatibilidades.
- Expectativa: solo plugins "API-only" (sin NMS/reflection) — y aun así con
  rendimiento inferior a Paper (bridge JNI + modelo síncrono roto por el
  multihilo, como documenta Folia: "expect compatibility at 0").
- Licencia: implementar la *API* (MIT) en clean-room es distinto de copiar la
  implementación (GPL). Documentado en ADR 0006.

---

## Mods (Fabric / Forge / NeoForge)

| Tipo | ¿Funciona en el núcleo nativo? | Vía |
|------|-------------------------------|-----|
| **Client-side** | ✅ Sí (los carga el cliente; el servidor no necesita nada) | — |
| **Server-side** | ❌ No (son mixins sobre el código Java de Mojang) | Portes manuales a la API nativa |

Expectativa honesta: mods pesados (Create, Tinkers…) **no correrán** en el
núcleo nativo salvo portes manuales. El objetivo a futuro es que la API nativa
de Hyperion sea lo bastante buena para *atraer* portes de mods clave.

---

## Prioridad y protección del producto

1. **El núcleo nativo (WASM/Lua) siempre va primero** — la compat es opt-in.
2. La Vía C es investigación; si el PoC no convence → Vía B; si B es inviable
   por coste → decisión documentada de abandonar (sin atajos que degraden el
   núcleo).
3. Todo el código de compat vive aislado en `crates/hyperion_compat` (o
   herramienta externa) — nunca toca los caminos calientes del servidor.