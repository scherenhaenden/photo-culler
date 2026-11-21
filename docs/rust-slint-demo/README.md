# Primera demo de Photo Culler en Rust y Slint

**Estado:** D01–D33 están integradas en `feature/sprint-01-rust-foundation`. La pila original para D34/S01–S08 se preparó en `feature/sprint-01-completion`; sus PR #153–#157 siguen abiertos como draft según la consulta de GitHub del 2026-11-20. Este worktree de continuación usa `feature/sprint-01-integration-candidate` (HEAD `26991eb0`) y aún no está integrado en la rama base. El [estado del Sprint 01](SPRINT_01_REMAINING.md) separa implementación comprobada de aceptación pendiente.

El ledger del [estado del candidato de integración](../INTEGRATION_CANDIDATE_STATUS.md) separa la aceptación S01 del código incorporado en este worktree y registra las brechas por capa del roadmap.

**Alcance de la evidencia:** los runs de CI y las capturas descritos aquí acreditan
la demo S01 de galería sintética y solo lectura. El worktree candidato actual
añade catálogo SQLite e importación REST v2 desde raíces configuradas localmente,
pero ese flujo todavía no tiene aceptación runtime en escritorio o navegador.
La guía [RUN.md](RUN.md) separa el procedimiento del demo aceptado del protocolo
manual pendiente para import v2. No presentes la evidencia S01 como aceptación
del importador.

**Base actual del sprint:** `153d04920220fb0c396dca571620c0921052ab1b` en `feature/sprint-01-rust-foundation`. El candidato integra el servidor ejecutable, el cliente compartido, el launcher de escritorio, el arranque WASM, los componentes visuales y el workflow. La ejecución real de CI y la revisión de paridad con capturas siguen siendo gates de aceptación.

**Diseño S05:** el PR [#154](https://github.com/scherenhaenden/photo-culler/pull/154) contiene el candidato Library en `afe088242bdcf173eb8e83c9571bf598f52c4e86`; el commit de documentación actual es `51deca2f945bc9892b05fb51fcca9d09b326da3c`. En el SHA de código pasaron 33 pruebas Slint (1 ignorada), 8 contratos de paridad y checks nativo/WASM. El run [#44546749693](https://github.com/scherenhaenden/photo-culler/actions/runs/44546749693) pasó sus seis jobs en la pila actual y capturó la Library nativa en Linux, macOS y Windows. Las comparaciones, capturas, hashes y límites están en [evidencia visual S05](evidence/design/README.md). La iconografía vectorial está visualmente verificada en el PR apilado #155.

## Resultado que se va a entregar

Desde un checkout limpio, abrir la misma galería sintética con un JPEG y su miniatura en Slint nativo para macOS arm64, Linux x86-64 y Windows x86-64, y en Slint WASM servido junto con REST en Chromium y Firefox. El servidor y la lógica de aplicación de ambos clientes son Rust. En web, HTML y un cargador inicializan WASM. Los artefactos funcionan sin Python en ejecución.

La aceptación S01 trabaja con una galería incluida, por loopback y en modo de
lectura. El candidato de import v2 ya conecta un catálogo SQLite con fuentes
locales autorizadas y persistencia; aún falta verificar ese recorrido en runtime
con datos propios y registrar evidencia para el SHA exacto. Esto amplía el
alcance de la demo S01, pero no cierra la [hoja de ruta de 64 tareas](../RUST_SLINT_ROADMAP.md#ensamblaje-y-entrega), que también incluye migración, revisión, edición y release.

## Paquete para revisar

| Documento | Decisión que permite revisar |
|---|---|
| [Arquitectura y contratos](ARCHITECTURE.md) | Límites de la demo, API, ciclo del servidor, interfaz compartida, build, artefactos y condiciones de aceptación. |
| [Trabajo pendiente del Sprint 01](SPRINT_01_REMAINING.md) | Bloqueos actuales, dependencias, criterios de cierre y pruebas por plataforma. |
| [Roadmap de paridad visual](DESIGN_PARITY_ROADMAP.md) | Referencias de escritorio y web, componentes a portar y criterios para aceptar el diseño en Slint. |
| [Especificación visual Photo Culler Pro](PHOTO_CULLER_PRO_DESIGN_SPEC.md) | SHA del ZIP proporcionado, composición exacta de Library y mapa de sus componentes a Slint. |
| [Orden de envío y pipeline](PIPELINE.md) | Las 34 fichas en orden de despacho, cuándo se puede empezar, requisitos para integrar y quién escribe los archivos compartidos. |
| [34 fichas de implementación](TASKS.md) | Cada ficha tiene alcance, rutas bajo su propiedad y pruebas que debe aportar. |

## Alcance y origen del conteo

- **D01–D27:** mantienen la numeración de las 27 tareas del estimado inicial para macOS y web.
- **D28–D32:** mantienen las cinco tareas para Linux y Windows.
- **D33:** registra los paquetes en el workspace y fija el lockfile para las ramas de integración.
- **D34:** ensambla los módulos que se trabajaron de forma aislada y ejecuta la matriz de aceptación con artefactos reales.

El **≈35%** se estimó para la demo original de macOS y web. Estas 34 fichas incluyen además Linux, Windows y dos entregas explícitas de integración. El número de fichas mide el trabajo dividido, no el porcentaje completado.
