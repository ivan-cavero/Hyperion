# Architecture Decision Records (ADR)

Registro de decisiones de arquitectura. Cada ADR documenta una decisión,
su contexto y sus consecuencias — incluso las decisiones *abiertas*.

Convención: `NNNN-nombre.md`. Estado: **Aceptada**, **Propuesta** (abierta),
**Sustituida**.

| ADR | Título | Estado |
|-----|--------|--------|
| 0001 | Fuente del worldgen (1:1 vanilla) | **Propuesta** — decisión pendiente, límite Fase 2 |
| 0002 | Runtime de plugins: WASM/WIT (wasmtime) | Aceptada |
| 0003 | Sin puente JVM (no embeder Java) | Aceptada (núcleo) — ampliada por 0006 |
| 0004 | Ticking por regiones multihilo | Aceptada |
| 0005 | Pipeline datos → codegen para el protocolo | Aceptada |
| 0006 | Compatibilidad con plugins Java: TeaVM → JVM | Aceptada (estratégica) |