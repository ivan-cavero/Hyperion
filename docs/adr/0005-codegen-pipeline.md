# ADR 0005 — Pipeline datos → codegen para el protocolo

- Estado: **Aceptada**
- Fecha: 2026-08-02

## Contexto

Mojang publica releases frecuentes (versionado 26.x). Escribir el protocolo,
registries y NBT a mano por versión es insostenible (semanas por release).
Valence y Pumpkin validan el patrón: extraer datos del jar de Mojang a JSON y
generar código Rust con `build.rs`.

## Decisión

El protocolo, los registries y los formatos de datos se definen en **JSON
versionado extraído de Mojang** (data generators + jar extractor) y se genera
código Rust por codegen (`build.rs`). Nada de ese código se escribe a mano.

## Consecuencias

- Cada release de Mojang = un commit de datos nuevos + regenerar (días, no semanas).
- Los LLM pueden asistir en los diffs, **siempre** verificados por fuzzing +
  diff testing contra vanilla (los datos generados no se confían a la IA sin
  verificación).
- `minecraft-data` (comunidad) se usa solo como referencia secundaria, no como
  fuente primaria (va por detrás de Mojang y es parcial).
- El codegen genera parsers → la superficie de fuzzing (Fase 1) cubre todo lo
  generado automáticamente.