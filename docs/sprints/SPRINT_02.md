# Sprint 02 · Mi primera carpeta de fotos en Linux

**Estado: planificado.** Partimos de `master` en `24e3455`. El [Sprint 01](https://github.com/scherenhaenden/photo-culler/pull/159) entregó una ventana Slint que abre una galería de muestra. Este sprint termina cuando un fotógrafo abre el paquete Linux desde su escritorio, elige una carpeta JPEG propia, reconoce sus fotos y vuelve a encontrarlas al día siguiente. En macOS debe poder repetir los mismos pasos.

## Qué verá el fotógrafo

1. Al abrir por primera vez aparece «Elegir carpeta». Puede cancelar y seguir en la biblioteca vacía. El paquete gestiona el proceso local y el catálogo sin pedir comandos, un puerto ni un código.
2. Elige una carpeta y ve su nombre y la estimación. Confirma «Importar». Mientras trabaja, la ventana responde y muestra el progreso. Al terminar, muestra el recuento y abre la biblioteca.
3. Reconoce las fotos por sus nombres y miniaturas, incluida una de la segunda página. Cierra la ventana y la vuelve a abrir desde el mismo paquete. Recupera la misma biblioteca. Repetir la importación de la misma carpeta conserva el recuento.
4. Una carpeta vacía muestra «No hay fotos compatibles». Si la fuente deja de estar disponible, la app identifica la carpeta y permite reintentar. El catálogo conserva las entradas existentes.

## Diseño antes de implementar

El [launcher](../../rust/apps/photo-culler-slint/src/desktop_app.rs) ya acepta catálogo y raíces mediante argumentos. El [contrato del selector](../../rust/apps/photo-culler-slint/src/file_selection/README.md), el [diálogo de importación](../../rust/apps/photo-culler-slint/ui/library/import_dialog.slint), el cliente REST y la [biblioteca](../../rust/apps/photo-culler-slint/ui/library/gallery.slint) son los puntos de integración. El [registro técnico](../INTEGRATION_CANDIDATE_STATUS.md) recoge pruebas de API y pendientes de escritorio. Antes del primer cambio, reproducir el recorrido con el paquete de referencia y anotar el primer paso que exige una terminal.

**Decisión propuesta para este sprint.** Usar el selector del sistema desde el host Rust. En el primer arranque el launcher puede mostrar el estado inicial antes de iniciar el servicio de importación. Tras confirmar la carpeta, guarda la ruta autorizada en la configuración local del usuario y arranca el proceso hijo con el catálogo persistente y esa raíz. En la reapertura usa el mismo catálogo y configuración. La UI intercambia IDs de fuente con el servidor. Conservar el modo demo como opción explícita para las pruebas existentes. Acotar la primera entrega a una carpeta JPEG por catálogo. RAW+JPEG se acepta en el Sprint 05.

Linux y macOS comparten componentes Slint, textos, orden de acciones, estados y navegación por teclado. Los adaptadores nativos resuelven el selector y el directorio de datos del usuario. Usar la [referencia visual de Library](../rust-slint-demo/PHOTO_CULLER_PRO_DESIGN_SPEC.md) al revisar ambos paquetes.

## Orden de construcción

Rama de integración: `feature/sprint-02-linux-library`. Cada fila empieza en la rama del sprint actualizada. Integrar cada PR revisado antes de iniciar el siguiente.

| Orden y rama prevista | Resultado y zona de cambio | Comprobación acotada para el PR |
|---|---|---|
| 1 · `feature/s02-abrir-catalogo` | Arranque desde el paquete, catálogo por usuario y estado inicial compartido. Launcher y ciclo de vida del servidor propio. | Abrir, cerrar y reabrir con un directorio de datos nuevo y otro existente. Confirmar misma base y cierre del hijo. |
| 2 · `feature/s02-elegir-carpeta` | Conectar el selector de Linux/macOS y recordar la raíz confirmada. Pasar el ID al diálogo existente. | Cancelar conserva la pantalla. Una ruta con espacios funciona. Una ruta inaccesible muestra recuperación. La API sigue rechazando rutas fuera de la raíz autorizada. |
| 3 · `feature/s02-importar-biblioteca` | Conectar estimación, confirmación, progreso y finalización con la actualización de la biblioteca paginada. Reutilizar el importador y los cursores existentes. | Verificar vacío, éxito, segundo intento, reapertura y foto situada después de la primera página. Comprobar nombres, miniaturas, recuento y ausencia de duplicados. |

## Aceptación sobre la rama del sprint

- Preparar una carpeta con JPEG distintos y autorizados, con al menos una foto más que el tamaño de página que use el cliente. Registrar nombres, cantidad y hashes iniciales. Preparar también una carpeta vacía.
- En Linux x86-64, ejecutar los cuatro pasos de usuario desde el paquete. Registrar pantalla inicial, progreso, biblioteca, foto de segunda página y reapertura. Comparar los hashes de los originales antes y después.
- En macOS arm64, repetir con los mismos datos y tamaño de ventana. Comparar capturas lado a lado, controles, orden de foco y resultado de teclado. Corregir diferencias en la pantalla compartida antes de cerrar.
- Sobre el SHA integrado, ejecutar la suite completa del workspace y la aceptación de paquetes vigente, incluida regresión de la demo. Adaptar los disparadores de CI al Sprint 02: los workflows heredados enumeran ramas del Sprint 01. Los PR de función ejecutan solo sus pruebas acotadas.
- Publicar en la entrega el enlace al paquete Linux y al de macOS, el SHA, los resultados y un guion de reapertura. Revisar y fusionar sprint → master, comprobar que master contiene ese SHA y retirar las ramas de función integradas. Si falla el recorrido, corregirlo en este sprint antes de empezar la selección del Sprint 03.

[Entregas posteriores y criterio común de cierre](ROADMAP.md)
