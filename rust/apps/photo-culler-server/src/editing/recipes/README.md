# Recetas de edición — tarea 38

El módulo mantiene recetas versionadas y su historial, sin acceder a los originales. Importa el JSON legado con `exposure`, `temperature` y `tint`; valida los rangos acordados en el servicio Python y normaliza el documento a `schema_version = 1`. Las actualizaciones requieren la revisión esperada, incrementan la revisión y limpian redo. Undo/redo restauran recetas completas con historial acotado a 50 estados.

`RecipeStore` es el puerto de persistencia. `SqliteRecipeStore` guarda el snapshot y el historial completo en `edit_documents` y aplica compare-and-swap por revisión en la transacción. El `FileStore` de aceptación aislada usa un JSON temporal y rename atómico; el doble de memoria comprueba conflictos.

Prueba aislada (requiere Rust/Cargo y `serde`/`serde_json` disponibles en caché):

```sh
rust/apps/photo-culler-server/src/editing/recipes/test-standalone.sh --offline
```

El candidato registra el módulo y monta `GET/PUT /api/v1/photos/{photo_id}/recipe` y `POST .../undo|redo` cuando el servidor inicia con `--data-dir`. Las escrituras solo modifican parámetros e historial; no abren ni escriben originales. El servidor sigue ligado a loopback y aplica una allowlist de métodos/rutas y validación Host/Origin. El puerto de autenticación por sesión aún no tiene credenciales ni store, así que esto no habilita acceso LAN ni constituye aceptación runtime.

Pendiente: integrar la edición en la UI, probar catálogo heredado y conflictos concurrentes, y conectar el flujo de preview/export separado. No hay aceptación runtime multiplataforma para estas rutas.
