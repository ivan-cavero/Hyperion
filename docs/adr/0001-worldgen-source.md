# ADR 0001 — Fuente del worldgen (paridad vanilla 1:1)

- Estado: **Propuesta** (decisión abierta — límite: Fase 2 del ROADMAP)
- Fecha: 2026-08-02

## Contexto

Hyperion promete "misma seed, mismo mundo" (1:1 con el servidor vanilla de Mojang).
Implementar la generación completa a nivel de bloque (ruido, biomas, superficie,
cuevas, minerales, árboles, estructuras/jigsaw) es el componente de mayor riesgo
del proyecto: el mejor servidor nativo (Pumpkin) lleva 2 años y aún no completa
las estructuras.

## Opciones

| Opción | Descripción | Pros | Contras |
|--------|-------------|------|---------|
| **A. Núcleo propio** | Implementar todo el generador desde cero en Rust | Control total, cero deuda ajena, aprendizaje máximo | Años de trabajo; riesgo alto |
| **B. Referencia cubiomes** | Usar `cubiomes` (C, **MIT**) vía FFI para biomas/estructuras + núcleo de terreno propio | Paridad de biomas probada, MIT compatible | FFI `unsafe`; cubiomes no cubre bloque a bloque; capa de terreno sigue siendo propia |
| **C. Heredar de Pumpkin** | Portar/heredar el generador de Pumpkin (Rust, 1:1 reclamado) | Menos trabajo; paridad ya parcial | **GPLv3** — fuerza licencia GPL a todo el proyecto; depende de su roadmap |

## Decisión (pendiente)

Se evaluará A vs B como primeras opciones. C solo si el proyecto pasa a GPL.
Criterio de cierre: decidir al inicio de Fase 2 con un PoC de cada opción
(chunk de referencia vs. vanilla, misma seed).

## Consecuencias

- El ADR 0004 (region ticking) asume generación paralela de chunks → la opción
  elegida debe ser thread-safe y paralelizable.