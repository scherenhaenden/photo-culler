# Primera demo de Photo Culler en Rust y Slint

**Estado:** D01–D33 integradas en `feature/sprint-01-rust-foundation`. La rama candidata `feature/sprint-01-completion` ensambla D34 y los ocho work items S01–S08; no se ha integrado en la rama base. El [estado del Sprint 01](SPRINT_01_REMAINING.md) separa implementación comprobada de aceptación pendiente.

**Base actual del sprint:** `153d04920220fb0c396dca571620c0921052ab1b` en `feature/sprint-01-rust-foundation`. El candidato integra el servidor ejecutable, el cliente compartido, el launcher de escritorio, el arranque WASM, los componentes visuales y el workflow. La ejecución real de CI y la revisión de paridad con capturas siguen siendo gates de aceptación.

**Diseño S05:** el PR [#154](https://github.com/scherenhaenden/photo-culler/pull/154) contiene el candidato Library en `afe088242bdcf173eb8e83c9571bf598f52c4e86`. En ese SHA pasaron localmente 33 pruebas del crate Slint (1 ignorada), 8 contratos de paridad, checks nativo/WASM y capturas de browser a 1440×900 y 1100×700. El CI de ese SHA y la ventana nativa siguen pendientes; las capturas pareadas, hashes y límites están en [evidencia visual S05](evidence/design/README.md).

## Resultado que se va a entregar

Desde un checkout limpio, abrir la misma galería sintética con un JPEG y su miniatura en Slint nativo para macOS arm64, Linux x86-64 y Windows x86-64, y en Slint WASM servido junto con REST en Chromium y Firefox. El servidor y la lógica de aplicación de ambos clientes son Rust. En web, HTML y un cargador inicializan WASM. Los artefactos funcionan sin Python en ejecución.

La demo trabaja con una galería incluida, por loopback y en modo de lectura. La primera versión para fotos propias requiere el siguiente hito: catálogo SQLite, fuente local, importación, persistencia de decisiones y validación con fotos reales. Es el primer recorrido de la [hoja de ruta de 64 tareas](../RUST_SLINT_ROADMAP.md#ensamblaje-y-entrega).

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
