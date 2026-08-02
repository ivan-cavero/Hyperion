# ADR 0004 — Ticking por regiones multihilo

- Estado: **Aceptada**
- Fecha: 2026-08-02

## Contexto

El objetivo de rendimiento exige aprovechar todos los núcleos. El modelo
vanilla (un hilo de tick único) no escala. Folia (Java) y MCHPRS (Rust)
validan el patrón: particionar el mundo y tickear regiones independientes en
paralelo.

## Decisión

La simulación usa **ticking por regiones**: el mundo se particiona en regiones
de chunks; cada región tiene su propio loop de tick (20 TPS); los loops corren
en un pool de hilos (rayon / threads dedicadas). **No hay datos compartidos
entre regiones**: las interacciones cross-región (portales, teleports) se
implementan por mensajes y migración de estado.

## Consecuencias

- Cero locks globales en el camino caliente del tick.
- Rust (ownership + Send/Sync) convierte errores de concurrencia en errores de
  compilación.
- El API de plugins (ADR 0002) debe exponer "contexto de región" (como
  RegionScheduler en Folia).
- Redstone y entidades que cruzan fronteras de región son un caso de diseño
  delicado — se documenta en Fase 3.