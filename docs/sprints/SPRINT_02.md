# Sprint 02 · Mi primera carpeta de fotos en Linux

**Estado: implementación integrada; aceptación de paquetes pendiente.** La rama `feature/sprint-02-linux-library` ya contiene los PR revisados #162–#166. El flujo permite elegir una carpeta, arrancar el catálogo local, abrir el diálogo de importación y recordar la carpeta y galería. Falta comprobarlo desde los paquetes nativos Linux y macOS; hasta registrar esa evidencia, Sprint 02 no está cerrado.

## Qué verá el fotógrafo

1. Al abrir por primera vez aparece «Elegir carpeta». Puede cancelar y seguir en la biblioteca vacía. El paquete gestiona el proceso local y el catálogo sin pedir comandos, un puerto ni un código.
2. Elige una carpeta y ve su nombre y la estimación. Confirma «Importar». Mientras trabaja, la ventana responde y muestra el progreso. Al terminar, muestra el recuento y abre la biblioteca.
3. Reconoce las fotos por sus nombres y miniaturas, incluida una de la segunda página. Cierra la ventana y la vuelve a abrir desde el mismo paquete. Recupera la misma biblioteca. Repetir la importación de la misma carpeta conserva el recuento.
4. Una carpeta vacía muestra «No hay fotos compatibles». Si la fuente deja de estar disponible, la app identifica la carpeta y permite reintentar. El catálogo conserva las entradas existentes.

## Diseño antes de implementar
La [guía de arquitectura e implementación del cliente](SPRINT_02_ARQUITECTURA.md) fija el arranque, los contratos, los estados de error y las fichas de las tres ramas.

El [launcher](../../rust/apps/photo-culler-slint/src/desktop_app.rs) ya acepta catálogo y raíces mediante argumentos. El [contrato del selector](../../rust/apps/photo-culler-slint/src/file_selection/README.md), el [diálogo de importación](../../rust/apps/photo-culler-slint/ui/library/import_dialog.slint), el cliente REST y la [biblioteca](../../rust/apps/photo-culler-slint/ui/library/gallery.slint) son los puntos de integración. El [registro técnico](../INTEGRATION_CANDIDATE_STATUS.md) recoge pruebas de API y pendientes de escritorio. Antes del primer cambio, reproducir el recorrido con el paquete de referencia y anotar el primer paso que exige una terminal.

**Decisión propuesta para este sprint.** Usar el selector del sistema desde el host Rust. En el primer arranque el launcher puede mostrar el estado inicial antes de iniciar el servicio de importación. Tras confirmar la carpeta, guarda la ruta autorizada en la configuración local del usuario y arranca el proceso hijo con el catálogo persistente y esa raíz. En la reapertura usa el mismo catálogo y configuración. La UI intercambia IDs de fuente con el servidor. Conservar el modo demo como opción explícita para las pruebas existentes. Acotar la primera entrega a una carpeta JPEG por catálogo. RAW+JPEG se acepta en el Sprint 05.

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
- Sobre el SHA integrado, ejecutar la suite completa del workspace y la aceptación de paquetes vigente, incluida regresión de la demo. Los workflows ahora incluyen la rama Sprint 02 y ejecutan sus gates sobre este PR; confirmar los resultados antes del cierre. Los PR de función ejecutan solo sus pruebas acotadas.
- Publicar en la entrega el enlace al paquete Linux y al de macOS, el SHA, los resultados y un guion de reapertura. Revisar y fusionar sprint → master, comprobar que master contiene ese SHA y retirar las ramas de función integradas. Si falla el recorrido, corregirlo en este sprint antes de empezar la selección del Sprint 03.

### Evidencia hasta ahora

- En el checkout limpio del sprint, `photo-culler-api` (4), `photo-culler-client` (9) y `photo-culler-server` (87 unitarios + 4 de runtime) pasaron: **104 pruebas**. Los tests del servidor que enlazan loopback se ejecutaron fuera del sandbox.
- `cargo fmt --manifest-path rust/Cargo.toml --all -- --check`, `cargo metadata --locked --no-deps`, `git diff --check` y la sintaxis del smoke Python pasaron.
- La suite completa del workspace no llegó a compilar Slint: el entorno local no tiene `pkg-config` para `glib-sys`/Fontconfig. La aceptación interactiva Linux/macOS y la reabrir/cerrar con paquetes aún no tienen evidencia. Los nuevos disparadores de CI se entregan en el PR de cierre de gates.

[Entregas posteriores y criterio común de cierre](ROADMAP.md)
