# Sprint 02 · Mi primera carpeta de fotos en Linux

**Estado: aceptación funcional de catálogo integrada; revisión visual macOS pendiente.** La rama `feature/sprint-02-linux-library` ya contiene los PR revisados #162–#166. El flujo permite elegir una carpeta, arrancar el catálogo local, abrir el diálogo de importación y recordar la carpeta y galería. CI verifica el recorrido de catálogo empaquetado en Linux con interacción de escritorio y en macOS arm64 mediante las APIs del servidor y el perfil persistido, además de guardar capturas del desktop macOS antes y después de reabrirlo. La automatización macOS no opera el selector nativo ni valida controles, foco o teclado; esos puntos requieren revisión visual manual antes del cierre completo.

## Qué verá el fotógrafo

1. Al abrir por primera vez aparece «Elegir carpeta». Puede cancelar y seguir en la biblioteca vacía. El paquete gestiona el proceso local y el catálogo sin pedir comandos, un puerto ni un código.
2. Elige una carpeta y ve su nombre y la estimación. Confirma «Importar». Mientras trabaja, la ventana responde y muestra el progreso. Al terminar, muestra el recuento y abre la biblioteca.
3. Reconoce las fotos por sus nombres y miniaturas, incluida una de la segunda página. Cierra la ventana y la vuelve a abrir desde el mismo paquete. Recupera la misma biblioteca. Repetir la importación de la misma carpeta conserva el recuento.
4. Una carpeta vacía muestra «No hay fotos compatibles». Si la fuente deja de estar disponible, la app identifica la carpeta y permite reintentar. El catálogo conserva las entradas existentes.

## Diseño aplicado
La [guía de arquitectura e implementación del cliente](SPRINT_02_ARQUITECTURA.md) fijó el arranque, los contratos y los estados de error antes de editar. Las entregas implementadas mantienen el selector nativo como única diferencia de plataforma: Linux usa el portal XDG y macOS el diálogo del sistema; la ventana Slint, el orden del recorrido y el catálogo son compartidos.

El launcher lee el perfil local por usuario, permite elegir la carpeta desde la pantalla inicial, abre su servidor hermano en loopback y muestra el diálogo existente de importación. Al guardar, asocia la ruta con el ID de galería de la carga vigente; cancela sin cambiar el perfil y conserva perfiles ilegibles para recuperación. `--demo` mantiene disponible la fixture de Sprint 01. La importación usa IDs de fuente y el catálogo SQLite sigue bajo control del servicio. Esta entrega se acota a JPEG; RAW+JPEG permanece en Sprint 05.

Linux y macOS comparten componentes Slint, textos, orden de acciones, estados y navegación por teclado. Los adaptadores nativos resuelven el selector y el directorio de datos del usuario. Usar la [referencia visual de Library](../rust-slint-demo/PHOTO_CULLER_PRO_DESIGN_SPEC.md) al revisar ambos paquetes.

## Entregas de implementación

Rama de integración: `feature/sprint-02-linux-library`. Los cambios están integrados en orden de dependencia; la aceptación completa sigue pendiente.

| PR | Entrega integrada | Evidencia de revisión |
|---|---|---|
| [#163](https://github.com/scherenhaenden/photo-culler/pull/163) | Estado inicial nativo con botón accesible para elegir carpeta; mantiene el estado web. | Review aprobado; `git diff --check`. |
| [#164](https://github.com/scherenhaenden/photo-culler/pull/164) | Perfil local versionado XDG/macOS y guardado atómico sin secretos. | Review aprobado; harness aislado 4/4; formato y diff limpios. |
| [#162](https://github.com/scherenhaenden/photo-culler/pull/162) | Selector Linux/macOS, cancelación y rechazo de rutas no UTF-8 con pérdida. | Review aprobado; 7 pruebas de módulo con stub de rfd; formato y diff limpios. |
| [#165](https://github.com/scherenhaenden/photo-culler/pull/165) | Smoke nativo selecciona explícitamente `--demo` para el fixture. | Review aprobado; sintaxis Python y diff limpios. |
| [#166](https://github.com/scherenhaenden/photo-culler/pull/166) | Launcher integra selección, servidor local, perfil, ID de galería y apertura del diálogo de importación; conserva el modo externo. | Review aprobado; checks de formato y metadatos. Se corrigieron cargas obsoletas y persistencia prematura del perfil. |

El modo de estimación, confirmación, progreso, paginación y refresco usa el flujo de importación ya existente. La prueba nativa aún debe confirmar que toda la secuencia queda conectada de extremo a extremo en el paquete.

## Aceptación sobre la rama del sprint

- Preparar una carpeta con JPEG distintos y autorizados, con al menos una foto más que el tamaño de página que use el cliente. Registrar nombres, cantidad y hashes iniciales. Preparar también una carpeta vacía.
- En Linux x86-64, ejecutar los cuatro pasos de usuario desde el paquete. Registrar pantalla inicial, progreso, biblioteca, foto de segunda página y reapertura. Comparar los hashes de los originales antes y después.
- En macOS arm64, repetir con los mismos datos y tamaño de ventana. Comparar capturas lado a lado, controles, orden de foco y resultado de teclado. Corregir diferencias en la pantalla compartida antes de cerrar.
- La aceptación automatizada macOS genera la misma fixture de 13 JPEG, importa por la API del servidor empaquetado, verifica hash y página de catálogo 2, reinicia el servidor, reimporta sin duplicar IDs y abre/cierra/reabre el desktop desde un `HOME` aislado con su perfil `desktop.json`. El artefacto contiene las capturas `desktop-first.png` y `desktop-reopen.png`; estas capturas requieren revisión humana y no acreditan por sí solas el selector nativo ni foco/teclado.
- Sobre el SHA integrado, ejecutar la suite completa del workspace y la aceptación de paquetes vigente, incluida regresión de la demo. Los workflows ahora incluyen la rama Sprint 02 y ejecutan sus gates sobre este PR; confirmar los resultados antes del cierre. Los PR de función ejecutan solo sus pruebas acotadas.
- Publicar en la entrega el enlace al paquete Linux y al de macOS, el SHA, los resultados y un guion de reapertura. Revisar y fusionar sprint → master, comprobar que master contiene ese SHA y retirar las ramas de función integradas. Si falla el recorrido, corregirlo en este sprint antes de empezar la selección del Sprint 03.

### Evidencia hasta ahora

- En el checkout limpio del sprint, `photo-culler-api` (4), `photo-culler-client` (9) y `photo-culler-server` (87 unitarios + 4 de runtime) pasaron: **104 pruebas**. Los tests del servidor que enlazan loopback se ejecutaron fuera del sandbox.
- `cargo fmt --manifest-path rust/Cargo.toml --all -- --check`, `cargo metadata --locked --no-deps`, `git diff --check` y la sintaxis del smoke Python pasaron.
- La suite completa del workspace no llegó a compilar Slint: el entorno local no tiene `pkg-config` para `glib-sys`/Fontconfig. La aceptación interactiva Linux/macOS y la reabrir/cerrar con paquetes aún no tienen evidencia. Los nuevos disparadores de CI se entregan en el PR de cierre de gates.

[Entregas posteriores y criterio común de cierre](ROADMAP.md)
