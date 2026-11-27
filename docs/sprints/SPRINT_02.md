# Sprint 02 · Mi primera carpeta de fotos en Linux

**Estado: aceptación funcional completada en Linux y macOS; revisión visual, foco y teclado de macOS pendiente.** La rama `feature/sprint-02-linux-library` contiene los PR revisados #162–#166. El flujo permite elegir una carpeta, arrancar el catálogo local, abrir el diálogo de importación y recordar la carpeta y galería. CI recorre el paquete Linux con interacción de escritorio y el paquete macOS arm64 con importación vía API, persistencia del perfil y capturas del desktop antes y después de reabrirlo. La automatización macOS no opera el selector nativo ni valida controles, foco o teclado.

## Qué verá el fotógrafo

1. Al abrir por primera vez aparece «Elegir carpeta». Puede cancelar y seguir en la biblioteca vacía. El paquete gestiona el proceso local y el catálogo sin pedir comandos, un puerto ni un código.
2. Elige una carpeta y ve su nombre y la estimación. Confirma «Importar». Mientras trabaja, la ventana responde y muestra el progreso. Al terminar, muestra el recuento y abre la biblioteca.
3. Reconoce las fotos por sus nombres y miniaturas, incluida una de la segunda página. Cierra la ventana y la vuelve a abrir desde el mismo paquete. Recupera la misma biblioteca. Repetir la importación de la misma carpeta conserva el recuento.
4. Una carpeta vacía muestra «No hay fotos compatibles». Si la fuente deja de estar disponible, la app identifica la carpeta y permite reintentar. El catálogo conserva las entradas existentes.

## Diseño aplicado
La [guía de arquitectura e implementación del cliente](SPRINT_02_ARQUITECTURA.md) fijó el arranque, los contratos y los estados de error antes de editar. Las entregas implementadas mantienen el selector nativo como única diferencia de plataforma: Linux usa el portal XDG y macOS el diálogo del sistema; la ventana Slint, el orden del recorrido y el catálogo son compartidos.

El launcher lee el perfil local por usuario, permite elegir la carpeta desde la pantalla inicial, abre su servidor hermano en loopback y muestra el diálogo existente de importación. Al guardar, asocia la ruta con el ID de galería de la carga vigente; cancela sin cambiar el perfil y conserva perfiles ilegibles para recuperación. `--demo` mantiene disponible la fixture de Sprint 01. La importación usa IDs de fuente y el catálogo SQLite sigue bajo control del servicio. Esta entrega se acota a JPEG; RAW+JPEG permanece en Sprint 05.

Linux y macOS comparten componentes Slint, textos, orden de acciones, estados y navegación por teclado. Los adaptadores nativos resuelven el selector y el directorio de datos del usuario. El selector GTK Linux se abre mediante el portal XDG; la prueba empaquetada configura un runtime XDG privado para el portal en el runner hospedado. La app de escritorio inicia a 1440×900 en Linux; macOS CI captura la ventana a 1212×840. Por eso, las capturas de CI sirven para revisar el flujo de cada plataforma, no como comparación píxel a píxel de tamaños iguales. Usar la [referencia visual de Library](../rust-slint-demo/PHOTO_CULLER_PRO_DESIGN_SPEC.md) al revisar ambos paquetes.

## Entregas de implementación

Rama de integración: `feature/sprint-02-linux-library`. Los cambios están integrados en orden de dependencia y la aceptación funcional automatizada pasó en las dos plataformas. La revisión de UI macOS descrita abajo sigue pendiente.

| PR | Entrega integrada | Evidencia de revisión |
|---|---|---|
| [#163](https://github.com/scherenhaenden/photo-culler/pull/163) | Estado inicial nativo con botón accesible para elegir carpeta; mantiene el estado web. | Review aprobado; `git diff --check`. |
| [#164](https://github.com/scherenhaenden/photo-culler/pull/164) | Perfil local versionado XDG/macOS y guardado atómico sin secretos. | Review aprobado; harness aislado 4/4; formato y diff limpios. |
| [#162](https://github.com/scherenhaenden/photo-culler/pull/162) | Selector Linux/macOS, cancelación y rechazo de rutas no UTF-8 con pérdida. | Review aprobado; 7 pruebas de módulo con stub de rfd; formato y diff limpios. |
| [#165](https://github.com/scherenhaenden/photo-culler/pull/165) | Smoke nativo selecciona explícitamente `--demo` para el fixture. | Review aprobado; sintaxis Python y diff limpios. |
| [#166](https://github.com/scherenhaenden/photo-culler/pull/166) | Launcher integra selección, servidor local, perfil, ID de galería y apertura del diálogo de importación; conserva el modo externo. | Review aprobado; checks de formato y metadatos. Se corrigieron cargas obsoletas y persistencia prematura del perfil. |

El modo de estimación, confirmación, progreso, paginación y refresco usa el flujo de importación ya existente. La aceptación Linux recorre el paquete de extremo a extremo; macOS cubre importación, catálogo y persistencia con el servidor empaquetado y comprueba que el desktop vuelve a cargar la galería.

## Aceptación sobre la rama del sprint

- Preparar una carpeta con JPEG distintos y autorizados, con al menos una foto más que el tamaño de página que use el cliente. Registrar nombres, cantidad y hashes iniciales. Preparar también una carpeta vacía.
- En Linux x86-64, la aceptación empaquetada pasó sobre `f75fe36`: selector, importación, progreso, catálogo, página 2, cierre/reapertura, IDs estables y hashes de origen. El runner conserva coordenadas OCR por palabra para separar el nombre de carpeta del texto de la barra lateral.
- En macOS arm64, la aceptación empaquetada pasó sobre `f75fe36`: 13 JPEG, hashes, página 2, reinicio/reimportación sin duplicados y reapertura estable del catálogo. El artefacto contiene `desktop-first.png` y `desktop-reopen.png`; la prueba no automatiza el selector nativo ni la interacción visual/teclado.
- Sobre el SHA integrado, ejecutar la suite completa del workspace y la aceptación de paquetes vigente, incluida regresión de la demo. Los workflows ejecutan esos gates al actualizar la rama Sprint 02; conservar el SHA, enlaces de ejecución y artefactos de la última ejecución verde en el informe de cierre. Los PR de función ejecutan sus pruebas acotadas.
- Publicar en la entrega el enlace al paquete Linux y al de macOS, el SHA, los resultados y un guion de reapertura. Revisar y fusionar sprint → master, comprobar que master contiene ese SHA y retirar las ramas de función integradas. Si falla el recorrido, corregirlo en este sprint antes de empezar la selección del Sprint 03.

### Evidencia de cierre

- En `d5258e2`, `7d56f4f`, `9b75cf7`, `55ab879` y `f75fe36`, `cargo test --locked --manifest-path rust/Cargo.toml --workspace`, formato y Clippy pasaron; la última ejecución es [45966667891](https://github.com/scherenhaenden/photo-culler/actions/runs/45966667891).
- La aceptación empaquetada macOS arm64 pasó en [45959953004](https://github.com/scherenhaenden/photo-culler/actions/runs/45959953004). El artefacto [macOS Sprint 02](https://github.com/scherenhaenden/photo-culler/actions/runs/45959953004/artifacts/15180727686) confirma 13 importaciones, hashes correctos, página 2 y persistencia estable. Las dos capturas son 1212×840.
- En el SHA final `f75fe36`, la matriz de paquetes, WASM, navegadores y la aceptación de catálogo Linux/macOS arm64 pasaron en [45966667966](https://github.com/scherenhaenden/photo-culler/actions/runs/45966667966). Artefactos de aceptación: [Linux](https://github.com/scherenhaenden/photo-culler/actions/runs/45966667966/artifacts/15183932003) y [macOS](https://github.com/scherenhaenden/photo-culler/actions/runs/45966667966/artifacts/15183052860).
- Las pruebas focales del runner Linux, runner macOS y fixture pasaron 39/39 con Pillow disponible; `git diff --check` y análisis YAML pasaron. La revisión macOS del selector nativo, controles, foco y teclado sigue pendiente.

[Entregas posteriores y criterio común de cierre](ROADMAP.md)
