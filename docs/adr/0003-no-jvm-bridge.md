# ADR 0003 — Sin puente JVM (no embeder Java)

- Estado: **Aceptada**
- Fecha: 2026-08-02

## Contexto

Se consideró la compatibilidad con plugins Bukkit/Spigot/Paper (bytecode Java)
embebiendo una JVM en Rust vía `jni-rs` y reimplementando la API Bukkit.

## Decisión

**No.** Hyperion no embebe Java. `jni-rs` y cualquier puente JVM quedan
excluidos de la lista blanca. La compat con plugins Java está descartada por
diseño (ver README → "No objetivos").

## Justificación (evidencia)

- Los plugins Bukkit son bytecode Java: exigen JVM + la API completa (1000+ clases).
- Los plugins avanzados usan reflection/NMS (internals de Mojang), que no
  existen en un servidor no-Java.
- El puente JNI añade latencia y contradice el objetivo de rendimiento.
- Folia (Java) ya documenta que el multihilo rompe casi todos los plugins
  ("expect compatibility at 0"); un servidor nativo multihilo los rompería más.
- GraalVM Native Image no resuelve el problema (JNI exige metadata estática,
  incompatible con jars arbitrarios en runtime).

## Consecuencias

- El ecosistema de plugins de Hyperion es nativo: WASM/WIT + scripting (ADR 0002).
- La compatibilidad con plugins Java legacy se reevalúa en ADR 0006 (Vía C: TeaVM → WASM; plan B: JVM embebida).
- Se libera presupuesto de desarrollo para el producto real (API WASM).
---

## Actualización (2026-08-02)

Reevaluado por **ADR 0006**. El núcleo sigue **sin JVM** (este ADR permanece
para el núcleo). La compatibilidad con plugins Java pasa a ser una capa
opt-in de menor rendimiento: Vía C (TeaVM → WASM sobre wasmtime) como objetivo,
Vía B (JVM embebida en `hyperion_compat`) como plan B, sin vía de proxy (descartada)
`jni-rs` queda excluido del núcleo pero **opcional** dentro
de `hyperion_compat`.