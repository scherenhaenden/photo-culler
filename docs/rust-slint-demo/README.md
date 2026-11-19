# Primera demo de Photo Culler en Rust y Slint

**Estado:** borrador para aprobación. 34 fichas preparadas. El envío y la implementación empiezan tras la aprobación del usuario.

**Base revisada:** `db9a00723da400c479c09f8381657874d620e845` en `feature/sprint-01-rust-foundation`. Cargo incorpora core, motor de imágenes, CLI, egui y Tauri. El código de servidor, cliente REST y Slint existe por módulos. Todavía requiere registro en Cargo, ensamblaje y validación de arranque.

## Resultado que se va a entregar

Desde un checkout limpio, abrir la misma galería sintética con un JPEG y su miniatura en Slint nativo para macOS arm64, Linux x86-64 y Windows x86-64, y en Slint WASM servido junto con REST en Chromium y Firefox. El servidor y la lógica de aplicación de ambos clientes son Rust. En web, HTML y un cargador inicializan WASM. Los artefactos funcionan sin Python en ejecución.

La demo trabaja con una galería incluida, por loopback y en modo de lectura. La primera versión para fotos propias requiere el siguiente hito: catálogo SQLite, fuente local, importación, persistencia de decisiones y validación con fotos reales. Es el primer recorrido de la [hoja de ruta de 64 tareas](../RUST_SLINT_ROADMAP.md#ensamblaje-y-entrega).

## Paquete para revisar

| Documento | Decisión que permite revisar |
|---|---|
| [Arquitectura y contratos](ARCHITECTURE.md) | Límites de la demo, API, ciclo del servidor, interfaz compartida, build, artefactos y condiciones de aceptación. |
| [Orden de envío y pipeline](PIPELINE.md) | Las 34 fichas en orden de despacho, cuándo se puede empezar, requisitos para integrar y quién escribe los archivos compartidos. |
| [34 fichas listas para enviar](TASKS.md) | Cada ficha tiene alcance, rutas bajo su propiedad y pruebas que debe aportar. |

## Alcance y origen del conteo

- **D01–D27:** mantienen la numeración de las 27 tareas del estimado inicial para macOS y web.
- **D28–D32:** mantienen las cinco tareas para Linux y Windows.
- **D33:** registra los paquetes en el workspace y fija el lockfile para las ramas de integración.
- **D34:** ensambla los módulos que se trabajaron de forma aislada y ejecuta la matriz de aceptación con artefactos reales.

El **≈35%** se estimó para la demo original de macOS y web. Estas 34 fichas incluyen además Linux, Windows y dos entregas explícitas de integración. El número de fichas mide el trabajo dividido, no el porcentaje completado.
