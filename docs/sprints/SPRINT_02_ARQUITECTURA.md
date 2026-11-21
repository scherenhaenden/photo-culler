# Sprint 02 · Diseño de implementación del cliente de escritorio

**Estado: diseño aplicado; aceptación de paquetes pendiente.** La implementación está integrada en `feature/sprint-02-linux-library` mediante los PR #162–#166. Leer el [recorrido y estado actual del sprint](SPRINT_02.md). El escenario de aceptación sigue empezando con un perfil vacío, una carpeta con 13 JPEG distintos y el paquete nativo con los dos ejecutables contiguos.

## Mapa de responsabilidades

| Pieza existente | Responsabilidad que conserva | Cambio del sprint |
|---|---|---|
| [Ventana Slint](../../rust/apps/photo-culler-slint/ui/demo_window.slint) y [diálogo](../../rust/apps/photo-culler-slint/ui/library/import_dialog.slint) | Presentar estados, recibir acciones de teclado y puntero | Mostrar «Elegir carpeta», nombre, estimación y progreso en la misma composición Linux/macOS. La única superficie propia de cada SO es el selector. |
| [Launcher](../../rust/apps/photo-culler-slint/src/desktop_app.rs) | Crear ventana, transporte y controlador; enlazar callbacks | Resolver modo, configuración y raíz actuales antes de llamar a `start_load`. Los callbacks de reintento deben leer la configuración vigente; hoy capturan copias del arranque. |
| [Selector](../../rust/apps/photo-culler-slint/src/file_selection/mod.rs) | `NativePicker::pick(Intent::Source)` y cancelación | Registrar el módulo nativo en [lib.rs](../../rust/apps/photo-culler-slint/src/lib.rs). Implementar un adaptador de carpetas con diálogo del sistema (propuesta: rfd, portal en Linux y panel nativo en macOS). Declarar dependencia solo para escritorio. Dar a `NativePath` construcción y acceso controlados desde el adaptador. |
| [Proceso local](../../rust/apps/photo-culler-slint/src/desktop/owned_server.rs) | `DemoServer::open`, health, credenciales en memoria y cierre del hijo | Reutilizar el binario hermano y el puerto loopback asignado. Conservar el handle mientras la ventana esté abierta. |
| [Importación](../../rust/apps/photo-culler-slint/src/demo/import_flow.rs), [cliente tipado](../../rust/apps/photo-culler-slint/src/demo/rest.rs) y [controlador](../../rust/apps/photo-culler-slint/src/demo/controller.rs) | Descubrir fuentes, estimar, iniciar, seguir el trabajo, refrescar la galería y paginar | Conectar la fuente confirmada y el catálogo real al flujo existente. El servidor mantiene la identidad de las fotos, las revisiones, el escaneo y SQLite. |

## Arranque y persistencia

1. Mantener las invocaciones explícitas con `--data-dir`, `--authorized-root` y `--gallery-id` y el modo de servicio externo de `PHOTO_CULLER_API_ORIGIN`. Validar sus combinaciones según el parser actual. Al modo externo se le entrega solo el origen configurado; el launcher no termina ese proceso al salir.
2. Para la apertura habitual sin argumentos usar el perfil de escritorio. En Linux, datos en `$XDG_DATA_HOME/photo-culler/desktop` (por defecto `~/.local/share/photo-culler/desktop`). En macOS, `~/Library/Application Support/Photo Culler/Desktop`. Dentro, `catalog.sqlite3` lo administra el servidor. El host guarda `desktop.json` con versión de esquema 1, raíz confirmada y, cuando se conozca, ID de galería. No guardar puerto, token ni pairing en disco. Para la demo anterior, implementar `--demo` y actualizar los lanzamientos de aceptación que dependen de la galería sintética en el mismo PR.
3. Si falta configuración, mostrar la biblioteca inicial con foco en «Elegir carpeta». Si existe y es válida, abrir el mismo catálogo. Si es ilegible o tiene una versión desconocida, mostrar un error recuperable y conservar el archivo. Escribir cambios con temporal en el mismo directorio y renombrado atómico. Validar que la ruta puede representarse sin pérdida antes de almacenarla. No usar conversiones de ruta que sustituyan caracteres silenciosamente.
4. Al confirmar el selector, comprobar que la carpeta sea accesible; pasar la ruta únicamente al hijo local mediante los argumentos de arranque. Ejecutar la espera de readiness fuera del hilo de UI. Entregar endpoint, token y handle como una única conexión válida antes de habilitar «Importar». Persistir la configuración tras resolver la galería y fuente correctas. Si se cierra la ventana durante el arranque, liberar también el hijo que llegue tarde.
5. Si la raíz recordada está desmontada, conservar el catálogo y mostrar «Fuente no disponible» con «Reintentar». Reintentar vuelve a validar esa raíz. Nunca crear un directorio vacío en la ruta del disco ausente. Una biblioteca sin entradas después de un error de apertura no es prueba de catálogo vacío.

## Contrato entre la ventana y el servicio

| Evento visible | Operación y resultado | Ante cancelación, error o repetición |
|---|---|---|
| Elegir carpeta | `desktop_pick` devuelve una ruta local. Abrir el hijo y consultar `import_sources(gallery_id)`; seleccionar por el ID de la fuente configurada. | Cancelar regresa al estado anterior. Si hay ambigüedad, no iniciar un trabajo sobre la primera fuente. |
| Previsualizar importación | Usar `estimate_import(source_id, recursive, exclude_patterns)`. Mostrar el resultado de ese DTO junto al nombre de la carpeta. | Cero fotos compatibles deja la biblioteca accesible. Un error permite repetir la estimación. Descartar resultados tardíos de una selección anterior. |
| Confirmar | Enviar una vez `start_import(gallery_id, source_id, recursive, exclude_patterns)` con los mismos parámetros de la estimación. Guardar el ID del recibo en el estado de la ventana. | Bloquear el doble clic mientras se resuelve. Si se pierde la respuesta, consultar trabajos de esa galería con `list_active_import_jobs` y el historial antes de ofrecer otro inicio. |
| Ver progreso o reabrir | Consultar `import_job(job_id)` mediante el sondeo de `import_flow`. Al reabrir, recuperar el trabajo conocido del servidor. Usar los contadores que devuelve el DTO. | Distinguir pausado, completado, cancelado y fallido. Un error temporal de transporte conserva el ID. Un trabajo pausado se reanuda mediante su acción existente. |
| Ver las fotos | Al completarse el trabajo, refrescar una vez por galería/trabajo mediante el controlador existente. Obtener la página siguiente con el cursor devuelto por el servidor. | Un fallo de miniatura afecta a esa tarjeta. Un fallo de página conserva la página visible para reintentar. Reabrir recupera el catálogo y sus IDs. |

## Implementación integrada y cierre pendiente

Las fichas de implementación se trabajaron en paralelo desde el SHA de diseño y se integraron en el sprint mediante PR revisados #162–#166. No se crean nuevas fichas de producto antes de cerrar la aceptación.

1. **Primera apertura y perfil** (`#163`, `#164`, integrado por `#166`): estado vacío accesible, perfil versionado y recuperación de archivo inválido. Falta demostrar abrir/cerrar/reabrir con los paquetes nativos.
2. **Elegir carpeta** (`#162`, integrado por `#166`): portal XDG en Linux, diálogo nativo en macOS, cancelación, ruta sin pérdida y asociación con la galería vigente. Falta probar carpeta real con espacios y volumen no disponible en ambos sistemas.
3. **Importar y revisar fotos** (`#166` conecta el diálogo existente): reutiliza descubrimiento de fuentes, estimación, confirmación, sondeo, refresco y cursores del import flow. Falta correr el caso de 13 JPEG, segunda página, reimportación sin duplicados y reapertura; registrar hashes y capturas comparables.

Los PR de función tuvieron sus checks acotados. Los gates completos se habilitan para la rama Sprint 02 en el workflow actualizado del PR de CI/status; revisar resultados Linux, macOS y regresión WASM antes de declarar cerrado el sprint.

**Comandos de referencia.** En una rama de función, seleccionar el módulo modificado; por ejemplo, una vez registrado el selector, `cargo test --locked --manifest-path rust/Cargo.toml -p photo-culler-slint --lib file_selection`. Comprobar que ejecutó casos. Para comportamientos nativos, adjuntar además el resultado de la interacción en la ventana real. En la rama de integración ejecutar `cargo test --locked --manifest-path rust/Cargo.toml --workspace` y la matriz de paquetes. Antes de la aceptación, adaptar los filtros de rama y condiciones internas de [integración](../../.github/workflows/rust-integration-candidate-build.yml) y [artefactos](../../.github/workflows/rust-slint-demo.yml) para que los trabajos completos corran sobre el sprint.

## Guion de entrega reproducible

Con 13 JPEG distintos, `CATALOG_PAGE_LIMIT` vale 12 en la base revisada. Importar desde la ventana, comprobar 12 entradas en la primera página y la restante en la segunda. Identificar las 13 por nombre, comprobar que las miniaturas corresponden a las imágenes y reabrir. Repetir la importación y comparar IDs y recuento antes/después. Si cambia el límite, usar límite + 1 imágenes. Verificar los hashes de originales antes y después. Repetir cancelación, carpeta vacía y fuente desconectada. Registrar SO, SHA, paquete, resultado y capturas del mismo tamaño en Linux y macOS. Estos resultados alimentan el [cierre de Sprint 02](SPRINT_02.md).
