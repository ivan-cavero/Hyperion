# ADR 0007 — Estrategia de versiones: 26.2 primero, multi-versión después

- Estado: **Aceptada**
- Fecha: 2026-08-02

## Contexto

Hyperion nace apuntando a la **última versión estable** (26.2, protocolo 776).
En el futuro queremos dar soporte a versiones más antiguas. Eso condiciona cómo
organizamos código, rama y datos desde el día uno: cada release de Minecraft
cambia layout de paquetes, registries, NBT y límites.

Pregunta clave: ¿un proyecto/rama **por versión de Minecraft**, o un único
codebase que cambia de versión?

## Decisión

### Ahora — solo 26.2, en `main`

- **Una sola rama `main`** y un solo codebase, que apunta **exclusivamente a 26.2**.
- Se implementa **todo lo moderno y nada deprecated**: intents de handshake
  Status/Login/**Transfer**, cifrado RSA-1024 + AES-128/CFB8, compresión zlib,
  y los campos actuales de Login Success (incluido el Session ID de 776).
  Fuera deliberadamente: legacy server list ping, Strict Error Handling
  (eliminado en 1.21.2) y cualquier camino antiguo.
- La versión es **un único parámetro centralizado** (`SUPPORTED_PROTOCOL_VERSION`
  y los límites del codec en `hyperion_protocol`), no literales dispersos.
- **No hay ramas por versión.**

### Cuando salga 26.3+

- Se actualiza `main` a la nueva versión: bump del parámetro de versión, regenerar
  los datos (ver ADR 0005) y ajustar lo cambiado. Un release = un commit de datos
  + regenerar, **no** una rama nueva.

### Multi-versión (futuro, no antes de Fase 2)

- **No** se crean ramas por versión. Evidencia de que ese modelo es caro incluso
  para quien lo paga: Paper mantiene una rama `ver/<versión>` por minor soportado
  (un fork con sus propios patches por rama), documenta el flujo de rebase y sus
  conflictos, evita explícitamente los backports a versiones antiguas y solo
  mantiene 3–5 versiones recientes antes de abandonarlas.
- El patrón dominante en servidores serios es **un codebase para la última
  versión** (Pumpkin, Valence, Feather). Si se quiere aceptar un rango amplio de
  clientes, la traducción de protocolo va en una **capa separada** tipo ViaVersion
  (un único codebase que cubre 1.7.2 → última).
- En Hyperion el multi-versión se apoyará en el **codegen versionado (ADR 0005)**:
  cada versión = un set de datos JSON + parsers generados; el núcleo (mundo,
  simulación, plugins) permanece agnóstico de versión.
- Si más adelante se quiere servir varias versiones simultáneamente, se evalúa
  una capa de traducción propia o un proxy delante (el ROADMAP Fase 5 ya contempla
  ambas opciones).

## Consecuencias

- Durante Fase 1 el codec se escribe a mano, pero con la vista puesta en
  ADR 0005: los layout de paquetes deben terminar en **datos versionados**, no en
  constantes dispersas. El fuzzing y los tests ya cubren los parsers.
- El coste de "cambiar de versión" hoy es pequeño porque el codec es compacto y
  de estructura estable (IDs y límites por constante).
- **No se introduce infraestructura de multi-versión antes de Fase 2**: sería
  prematuro. Esta decisión queda registrada para no construir ramas ni
  feature-flags innecesarios.

## Referencias

- Pumpkin (solo última versión, codebase único):
  https://github.com/Pumpkin-MC/Pumpkin
- Valence ("targets the most recent stable version... multi-version not planned"):
  https://github.com/valence-rs/valence
- Feather (solo 1.16.5, multi-versión no planeado):
  https://github.com/feather-rs/feather
- Paper (ramas por versión; coste de rebase y política de versiones):
  https://github.com/PaperMC/Paper · https://github.com/PaperMC/Paper/blob/main/CONTRIBUTING.md
- Paper #9790 (no se backportea a ramas antiguas):
  https://github.com/PaperMC/Paper/issues/9790
- ViaVersion (traductor de protocolo, un codebase, 1.7.2 → última):
  https://github.com/ViaVersion/ViaVersion
- Velocity (un binario; solo handshake/login traducidos, play sin tocar):
  https://github.com/PaperMC/Velocity · https://docs.papermc.io/velocity/server-compatibility
