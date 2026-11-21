# Entregas de escritorio · Sprints 02 a 12

**Plan de producto.** Linux x86-64 es el primer escritorio. macOS arm64 usa la misma interfaz Slint. El punto de partida es la galería de demostración del Sprint 01. La primera entrega con fotos propias se detalla en el [plan del Sprint 02](SPRINT_02.md). Los diez siguientes son propuestas ordenadas para que el fotógrafo pueda repetir lo ya aprendido y hacer una cosa más en cada versión. Las fechas se fijan al elegir cada sprint.

## Diez entregas después de la primera importación

Cada fila parte del último paquete aceptado. La columna de demostración describe lo que tiene que conseguir una persona desde el escritorio para dar por entregada la función.

| Sprint | Al terminar, el fotógrafo puede… | Trabajo sobre el flujo anterior | Demostración de cierre |
|---|---|---|---|
| 03 · Primera selección | Marcar fotos para conservar o rechazar y retomar su revisión. | Añadir selección por teclado y clic, indicadores, contadores y persistencia a la biblioteca importada. | Marcar fotos en dos páginas, corregir una decisión, filtrar por conservadas y reabrir. Coinciden fotos, marcas y recuentos. |
| 04 · Inspección de foco | Ampliar una foto para decidir si está enfocada. | Abrir el inspector desde la selección, mostrar preview, zoom y encuadre. Volver a la misma tarjeta. | Comparar una toma nítida y otra desenfocada al 100 %, decidir y volver. Se conserva posición, filtro y decisión. |
| 05 · Sesión RAW+JPEG | Revisar un par RAW+JPEG como una sola toma. | Extender la importación y la vista previa a un formato RAW acordado con muestras reales. Indicar el origen de la preview. | Importar un par y un RAW suelto del formato documentado. El par cuenta una vez, la decisión pertenece a la toma y el RAW suelto muestra preview o una explicación si carece de ella. Verificar hashes de originales. |
| 06 · Comparar ráfagas | Escoger la mejor foto de una serie corta. | Reutilizar agrupación, zoom y decisiones para abrir una ráfaga y comparar sus tomas contiguas. | Abrir una serie conocida y distinguirla de una foto aislada. Comparar dos tomas, marcar la favorita y encontrarla al regresar a Library. |
| 07 · Análisis comprensible | Saber qué fotos merecen otra mirada y por qué. | Conectar el trabajo de análisis a progreso y resultado por foto en el inspector. | Analizar una serie, pausar y continuar. Abrir un resultado y ver la métrica que explica la sugerencia. Las decisiones manuales previas se conservan. |
| 08 · Preselección revisable | Aceptar o descartar sugerencias para una sesión. | Usar el análisis y las ráfagas para una cola de revisión con confirmación explícita. | Aceptar una sugerencia y descartar otra. Reabrir y distinguir sugerencia, decisión final y pendientes. |
| 09 · Hoja de contacto | Compartir una prueba visual de las fotos elegidas. | Generar una hoja de contacto local a partir del filtro de conservadas, con nombres y orden visibles. | Guardar y abrir la hoja fuera de la app. Sus miniaturas y nombres corresponden exactamente a la selección y su orden. |
| 10 · Ajuste visible | Corregir exposición y comparar antes/después en una foto elegida. | Conectar la receta existente con el render de preview, deshacer y persistencia por foto. | Aplicar un ajuste visible, alternar antes/después, deshacer, rehacer y reabrir. La receta se conserva y el original mantiene su hash. |
| 11 · Entrega de JPEG | Exportar copias de las fotos conservadas con sus ajustes. | Usar la misma receta del inspector, elección de destino, progreso y protección de colisiones. | Exportar una selección con una foto ajustada y otra sin ajustar. Abrir las copias fuera de la app, repetir con destino ocupado y comprobar que ningún original ni archivo previo fue sobrescrito. |
| 12 · Volver a una sesión | Recuperar una sesión cuyo disco se desconectó y terminar su entrega. | Conectar el estado de fuente ausente con reconexión, reescaneo y reintento de exportación. | Desmontar una fuente de prueba, reabrir, volver a conectarla y completar la exportación pendiente. Comparar identidades, decisiones, recetas y archivos con el estado anterior, sin duplicados. |

## Cómo se entrega cada versión

1. Elegir la siguiente fila tras cerrar la anterior. Escribir el guion de demostración y acordar el conjunto de fotos antes de programar. Revisar qué piezas existen en el [ledger de integración](../INTEGRATION_CANDIDATE_STATUS.md).
2. Crear la rama de sprint desde master y dividir ese recorrido en ramas de función. En cada rama probar solo el cambio, revisar el PR e integrarlo en la rama del sprint.
3. En la rama del sprint ejecutar la suite completa, ampliar cobertura donde falle el recorrido y revisar el paquete Linux primero. Repetir en macOS con las mismas fotos. Comparar la disposición, los controles, el foco y las respuestas en las pantallas compartidas.
4. Adjuntar al cierre el SHA, paquetes descargables, instrucciones de apertura, el guion «pruébalo tú» y su evidencia. Registrar límites concretos como formatos RAW o tamaños de catálogo probados. Una demo debe incluir un resultado que el fotógrafo pueda reconocer y repetir en su máquina.
5. Fusionar a master cuando pase el recorrido. Verificar SHA y limpiar referencias de función integradas. Si queda incompleta la función, sigue siendo el trabajo activo. Actualizar esta tabla con enlace a la entrega real al cerrarla.
