# Bitácora de integración

## 21 de noviembre de 2026 — D34, flujo vertical sintético

### Trabajo realizado

- Se conectó el ejecutable Axum al ciclo loopback; anuncia el puerto asignado y
  sirve salud, la galería `demo-001`, su JPEG y recursos web declarados.
- Se conectó `DemoWindow` al flujo Rust compartido en escritorio y WASM. La
  carga valida salud, DTO de galería y miniatura JPEG; muestra vacío, error y
  reintento.
- El escritorio inicia el servidor hermano, valida `ready` con límite de diez
  segundos, hace las peticiones desde un worker y cierra el proceso propio al
  salir.
- Los reintentos descartan resultados anteriores. Las respuestas usan
  `Cache-Control: no-store`; HTML/WASM reciben CSP, `nosniff` y
  `no-referrer`; JavaScript conserva su MIME y recibe `nosniff`.
- Se actualizó el estado de S01–S04 y se añadió la guía de reproducción en
  [PC_HANDOFF.md](PC_HANDOFF.md).

### Evidencia

- `cargo test --offline --locked --manifest-path rust/Cargo.toml -p photo-culler-server`:
  **26 tests pasan**, incluidos lifecycle loopback, router integrado, asset
  headers, API `no-store` y política de origen.
- `cargo check --offline --locked --manifest-path rust/Cargo.toml -p photo-culler-server -p photo-culler-slint`: pasa en macOS nativo.
- `cargo check --offline --locked --manifest-path rust/Cargo.toml -p photo-culler-slint --target wasm32-unknown-unknown`: pasa.
- `cargo build --offline --locked --manifest-path rust/Cargo.toml -p photo-culler-server -p photo-culler-slint`: pasa en macOS nativo.
- `rustfmt --check` para los archivos Rust nuevos o modificados (excepto
  `desktop/mod.rs`, que conserva formato histórico fuera de rustfmt) y
  `git diff --check`: pasan.
- El smoke loopback anterior confirmó `ready`, salud, fila `demo-001` y JPEG de
  8.304 bytes. El nuevo test de router integrado verifica que HTML/API
  reciben las cabeceras esperadas.

### Límites pendientes

- No se logró aceptación visual nativa en esta máquina: el entorno de ejecución
  bloqueó el socket loopback con `Operation not permitted` al iniciar la demo.
- `wasm-bindgen` CLI no está instalado aquí; no se generó ni sirvió el bundle
  web real, ni se validó visualmente en Chromium/Firefox.
- La galería sigue siendo una fixture sintética de una foto. La importación de
  originales, SQLite, paginación y el flujo completo del producto no están en
  esta entrega.

### Limpieza de temporales

- Se eliminó `/private/tmp/photo-culler-s01-final-target` después de verificar
  los builds. Ocupaba 4,7 GB; el espacio libre de `/private/tmp` pasó de 55 GiB
  a 59 GiB.
- Se verificó que `/private/tmp/photo-culler-pro-design-artifacts-final-align`
  solo contenía dos capturas idénticas, por SHA-256, a las de
  `/private/tmp/photo-culler-pro-design-artifacts-final`; se eliminó la copia
  duplicada (aprox. 196 KiB).
- Después de comparar todos los archivos, también se borraron tres copias
  idénticas de `photo-culler-s02-state` (`photo-culler-baseline`,
  `photo-culler-patch-check`, `photo-culler-verify-v2`) y la copia duplicada
  `photo-culler-native-macos-debug` de `photo-culler-native-macos-artifact`.
  Se liberaron unos 14.6 MiB; se conservaron `s02-state` y `native-macos-artifact`.
- No se borraron cachés globales, worktrees existentes ni archivos previos del
  usuario. Se conservaron los demás temporales que contienen artefactos de
  aceptación, referencias visuales o ramas/cambios locales sin publicar.

### Entrega publicada

- `846fb58` — conecta el flujo sintético de servidor y Slint, corrige las
  cabeceras/cache, los reintentos y el timeout de inicio; incluye el handoff.
- `5b5449b` — publica el roadmap de paridad visual referenciado por el handoff.
- `bd1a608` — registra la primera limpieza y publicación.
- `6846558` — incorpora el ZIP de Photo Culler Pro para reproducir la referencia
  en la otra PC. El archivo solo contiene variables de ejemplo.
- Todos estos commits están en `origin/feature/sprint-01-rust-foundation`.
