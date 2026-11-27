# Sprint 03 · Primera selección en Linux

**Estado: implementación integrada; aceptación Linux aprobada en el head anterior; integración de S02 actualizada y pendiente de repetir CI.** La rama es `feature/sprint-03-linux-selection`, derivada de Sprint 02. No fusionar a `master` antes de cerrar las aceptaciones de ambos sprints.

## Resultado para el fotógrafo

Desde la biblioteca local podrá marcar fotos como KEEP o REJECT con ratón o teclado, corregir una marca, ver contadores del catálogo completo, filtrar KEEP aunque las fotos estén en distintas páginas, cerrar la aplicación y volver a la misma selección.

## Demostración reproducible

Usar un catálogo Linux con 13 JPEG sintéticos distintos y autorizados, límite de página 12, perfil XDG y hashes iniciales registrados.

1. En la página 1, marcar `photo-01` como KEEP con ratón y `photo-02` como REJECT usando solo teclado.
2. Ir a la página 2 y marcar `photo-13` como KEEP.
3. Volver a la página 1 y corregir `photo-01` a REJECT.
4. Comprobar en el servidor y en la pantalla: KEEP=1, REJECT=2, UNRATED=10, total=13. El filtro KEEP, activo desde cualquiera de las páginas, debe mostrar únicamente `photo-13`.
5. Cerrar normalmente y reabrir el paquete con el mismo perfil XDG. Los IDs, decisiones y contadores deben coincidir; volver a aplicar KEEP debe encontrar `photo-13`.
6. Comprobar que los trece originales conservan sus hashes.

Capturar una imagen de la primera página, otra de la segunda y otra del filtro KEEP en la misma resolución. El recorrido se ejecuta bajo Xvfb en CI; no automatizará el escritorio del usuario.

## Criterio de aceptación

- Las acciones KEEP, ALT, REJECT y CLEAR tienen foco visible, rol y nombre accesibles, y responden a Enter/Espacio. La selección de tarjetas sigue funcionando con ratón, Enter y Espacio.
- Cambiar el filtro de decisión reinicia el cursor y consulta el catálogo completo. Los cursores conservan el mismo filtro al navegar.
- Los contadores representan el catálogo completo, no solo la página visible. Se refrescan después de confirmar una decisión en el servidor.
- Una escritura solo se refleja como confirmada después de recibir la respuesta revisionada. Un error no aumenta contadores ni deja un badge falso.
- Tras reiniciar, SQLite devuelve las mismas decisiones e identidades. Los hashes de los originales no cambian.

## Alcance

Se reutilizan las rutas y la persistencia de decisión que ya existen. La guía de implementación define tres ramas aisladas: acciones accesibles de selección, filtro/cuentas globales, y aceptación E2E Linux de paquete. No se amplía a RAW, análisis, exportación, IA, ni aceptación macOS en este sprint.

El selector de carpetas y primer importado pertenecen a Sprint 02. Sus runners Linux y macOS están incluidos en la misma rama y sus pasos de CI corren por separado de la aceptación de selección persistente de Sprint 03. Sprint 03 usa un catálogo de prueba sembrado para probar selección.

## Pruebas por nivel

- **Rama UI:** pruebas de componentes/modelo para selección y acciones de teclado; `cargo fmt` y tests acotados del crate Slint.
- **Rama de catálogo:** tests de controlador para filtro/cursor y refresco de resumen; tests de API existentes para contrato y revisión de decisiones.
- **Rama Linux E2E:** paquete real Linux bajo Xvfb, diálogo no requerido porque se inicia desde un perfil/catálogo sembrado; simular ratón y teclado sobre la ventana, comprobar estado SQLite, capturas, hashes y reapertura.
- **Rama Sprint 03:** suite Rust completa relevante, workflow de paquete Linux, regresión del smoke demo y N2N anterior. Publicar SHA, paquete y manifiesto de evidencia.

## Evidencia de integración

- El head `79c2439a70fdc430d2bc6f75fca3c73397efcd30` pasó el workflow de compilación e integración `45862827930` y la matriz de paquete Slint `45869568546`; dentro de esta última, `Run persistent library decision selection acceptance` pasó en Linux. La ejecución nativa Linux registrada es el artefacto `15142017983`.
- Sprint 02 avanzó después a `0fc7d3c2f4806d80b7e511794c6907a434c3cc47`. Esta rama integra ese head y conserva pasos independientes para las aceptaciones Linux/macOS de S02 y la aceptación Linux de S03.
- La integración crea un SHA nuevo. Repetir los workflows de paquete y la suite pertinente sobre ese SHA antes de considerar la aceptación final de S03 cerrada.

## Secuencia de ramas

Base: `feature/sprint-03-linux-selection`, derivada de Sprint 02. Ramas de trabajo:

- `feature/s03-keyboard-decisions`
- `feature/s03-gallery-counts`
- `feature/s03-linux-e2e-selection`

Cada rama ejecuta sus pruebas acotadas, recibe revisión y se integra en Sprint 03. El E2E y las pruebas amplias corren después en la rama Sprint 03. No hacer merge a `master` hasta que Sprint 02 esté aceptado y luego se cierre esta demostración de Sprint 03.
