# ADR 0006 — Compatibilidad con plugins Java: estrategia (TeaVM → JVM)

- Estado: **Aceptada**
- Fecha: 2026-08-02
- Relación: reevalúa y amplía ADR 0003.

## Contexto

Se quiere retrocompatibilidad futura con plugins Bukkit/Spigot/Paper y mods.
El ADR 0003 descartó la JVM para el **núcleo**. Esta ADR decide la estrategia
para una **capa opt-in** de compatibilidad, sin comprometer el producto.

## Decisión

Dos vías, en orden de prioridad (detalle: docs/COMPATIBILITY.md):

1. **Vía C (objetivo)**: compilar plugins Java a WebAssembly con TeaVM y
   ejecutarlos en el runtime WASM de Hyperion (`wasmtime`). La API Bukkit se
   reimplementa como host imports del WASM. Investigación con PoC y criterio
   go/no-go.
2. **Vía B (plan B)**: módulo opt-in `hyperion_compat` con JVM embebida
   (`jni-rs`) + subconjunto de la API Bukkit en Rust. Solo plugins "API-only".

**Descartado por decisión del equipo**: la vía de coexistencia por proxy
(servidores Java detrás de un proxy). No aporta valor al producto y no se
contempla.

Mods: client-side gratis; server-side solo portes manuales a la API nativa.

## Consecuencias

- `jni-rs` pasa a **opcional** en la lista blanca: solo dentro de
  `hyperion_compat` (Vía B), nunca en el núcleo (ADR 0003 se mantiene para el núcleo).
- TeaVM es una **herramienta externa** (compilador Java), no un crate runtime;
  se audita aparte (ver docs/DEPENDENCIES.md).
- La compatibilidad es **no prioritaria y opt-in**; el rendimiento nativo no se sacrifica.
- Los plugins Java correrán peor que en Paper (sin JVM nativa, modelo síncrono
  roto por el multihilo — evidencia: Folia "expect compatibility at 0").
- Licencia: la API de Paper es MIT; implementarla en clean-room desde Rust no
  copia la implementación GPL. Se documenta si se incorpora código de Cardboard.