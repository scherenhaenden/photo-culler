# Sprint 05 · Sesión RAW+JPEG

**Estado: implementación integrada en la rama de sprint; aceptación con muestras reales pendiente.** La rama `feature/sprint-05-raw-jpeg` y el PR draft #170 contienen la asociación RAW+JPEG, el origen de preview y las respuestas explícitas cuando una preview RAW no está disponible. El formato RAW soportado aún debe acordarse con muestras reales autorizadas. No se debe declarar cerrado el sprint hasta completar esa aceptación.

## Resultado para el fotógrafo

Revisar un RAW y su JPEG asociado como una toma: la pareja cuenta una vez, comparte una decisión y muestra de dónde procede la preview. Un RAW sin preview utilizable debe explicar por qué no puede mostrarse. Los originales deben conservar sus hashes.

## Criterios de aceptación

- Acordar y documentar al menos un formato RAW con muestras autorizadas; guardar fixtures de prueba reproducibles sin datos personales.
- Importar una pareja RAW+JPEG y comprobar una sola toma en la biblioteca, una sola decisión y el origen `paired-jpeg` de la preview.
- Importar un RAW suelto. Si el formato tiene preview extraíble, comprobar su render; en caso contrario, comprobar la respuesta tipada y la explicación visible al usuario.
- Comprobar que variantes de mayúsculas, directorios distintos, nombres con puntos, sidecars y candidatos duplicados no asocian tomas incorrectamente.
- Calcular hashes de los originales antes y después; deben coincidir.
- Ejecutar tests unitarios de la rama de feature y, al terminar la integración del sprint, la suite Rust completa y los gates de paquete/browser correspondientes.

## Implementación ya presente

- El escáner reconoce extensiones RAW y la asociación lógica agrupa RAW/JPEG por ruta relativa y nombre, sin resolver candidatos duplicados arbitrariamente.
- El endpoint de previews identifica el origen de la imagen y permite usar el JPEG asociado cuando la preview RAW no está disponible.
- Los RAW sin preview generan una respuesta `RawPreviewUnavailable` en vez de una imagen inventada o un fallo opaco.
- Hay tests sintéticos para asociación, fallback JPEG y respuestas tipadas. No sustituyen la comprobación con un archivo RAW producido por una cámara.

## Evidencia y límites actuales

- PR #170 (`feature/sprint-05-raw-jpeg` → `feature/sprint-04-photo-inspector`) sigue abierto como draft; no está integrado en `master`.
- El PR registra 225 tests Rust correctos en la ejecución previa y deja pendiente la validación del decoder con un RAW real autorizado. Este checkout no contiene fixtures RAW.
- En la rama de feature, `cargo test --locked --manifest-path rust/Cargo.toml -p photo-culler-server --lib` pasó: **91 tests**. En el checkout de integración macOS arm64, `cargo test --locked --manifest-path rust/Cargo.toml --workspace` no completó el build: el custom build de `photo-culler-tauri` requiere `rust/apps/photo-culler-tauri/binaries/photo-culler-backend-aarch64-apple-darwin`, que no está presente. Confirmar también los gates hosted del PR.
- No hay evidencia de aceptación de paquete, hashes de muestra ni formato RAW acordado. Mantener estos puntos como gates explícitos en la revisión final.

## Secuencia de cierre

1. Elegir una muestra RAW autorizada y el formato a soportar; registrar procedencia y hashes sin incluir datos personales en el repositorio.
2. Ejecutar la demostración de pareja y RAW suelto, revisar preview/origen/error y validar los hashes.
3. Resolver discrepancias entre la asociación compartida y el finalizador SQLite de importación con pruebas de regresión.
4. Ejecutar tests unitarios de cada feature branch; después, en `feature/sprint-05-raw-jpeg`, ejecutar la suite completa y los gates de empaquetado/browser.
5. Registrar SHA y evidencia en este documento y en el PR. Cerrar el sprint solo cuando el formato y la aceptación real estén confirmados.
