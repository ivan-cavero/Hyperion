# ADR 0002 — Runtime de plugins: WASM/WIT (wasmtime)

- Estado: **Aceptada**
- Fecha: 2026-08-02

## Contexto

Se necesita una API de plugins que sea **segura** (el claim principal del
proyecto) y que no arrastre el modelo inseguro de `dlopen`/`libloading`
(código arbitrario con acceso total; además, las DLL no se descargan en Windows).

## Decisión

Los plugins de Hyperion se ejecutan como módulos **WebAssembly** con interfaz
**WIT** (WebAssembly Interface Types), usando `wasmtime` (Bytecode Alliance,
18.4k★, Apache-2.0, activo) como runtime.

## Consecuencias

- Sandbox por capacidades: el plugin solo accede a lo que la API concede.
- Hot-reload limpio: WASM se descarga sin `dlopen`.
- Portabilidad idéntica en Windows/Linux/macOS.
- Coste: se añade `wasmtime` como dependencia de la lista blanca en Fase 4
  (auditoría: docs/DEPENDENCIES.md). Fallback: `wasmer`.
- Scripting Lua (MLua) se construye sobre la misma API para simplicidad.