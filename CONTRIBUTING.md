# Contribuyendo a Hyperion

¡Gracias por querer aportar! Hyperion es un proyecto ambicioso que solo avanza con comunidad. Aquí está cómo participar.

## Cómo empezar

1. **Fork** el repositorio y clónalo.
2. Instala Rust (stable) — `rustup.rs`.
3. `cargo build` desde la raíz del workspace.
4. `cargo test` y `cargo clippy` deben pasar.
5. Elige un issue etiquetado `good-first-issue` o propón uno.

## Convenciones

- **Rust**: `rustfmt` + `clippy` limpio (sin warnings). El CI lo verifica.
- **Sin `unsafe` sin justificación**: si necesitas `unsafe`, debe ir en un módulo aislado con un comentario `// SAFETY:` explicando por qué es correcto.
- **Tests**: todo código de red/protocolo lleva unit tests de round-trip. El fuzzing es obligatorio para cualquier parser de entrada de red.
- **Commits**: mensajes claros en inglés, prefijo de crate cuando aplica (ej. `protocol: fix keep-alive timeout`).
- **Docs**: API pública documentada (`///`).

## Flujo de PR

1. Crea una rama desde `main`.
2. Haz cambios pequeños y revisables.
3. Ejecuta localmente: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`.
4. Abre el PR describiendo *qué* y *por qué* (y cómo lo probaste).
5. Un maintainer revisa; discute en el hilo hasta aprobar.

## Áreas donde siempre se necesita ayuda

- **Fuzzing** del protocolo (encontrar crashes antes que los atacantes).
- **Datos**: extraer y validar registries de cada versión de Minecraft.
- **Benchmarks** comparativos contra Paper/Folia.
- **Documentación** y tutoriales de plugins.
- **Código**: worldgen, simulación, red, API de plugins.

## Código de conducta

Sé respetuoso. Todo el mundo está aprendiendo. Los ataques personales, el spam y el acoso no se toleran — los maintainers pueden expulsar a quien los cometa.

## Licencia

Al contribuir aceptas que tu código queda bajo MIT (igual que el proyecto).