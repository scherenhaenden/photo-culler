# Recetas de edición — tarea 38

El módulo mantiene recetas versionadas y su historial, sin acceder a los originales. Importa el JSON legado con `exposure`, `temperature` y `tint`; valida los rangos acordados en el servicio Python y normaliza el documento a `schema_version = 1`. Las actualizaciones requieren la revisión esperada, incrementan la revisión y limpian redo. Undo/redo restauran recetas completas con historial acotado a 50 estados.

`RecipeStore` es el puerto de persistencia. `save` debe reemplazar el snapshot completo de forma atómica; el integrador debe aplicar control de revisión en la transacción SQLite para evitar actualizaciones concurrentes perdidas. La prueba `FileStore` usa un archivo JSON temporal y rename atómico para simular persistencia/reapertura entre instancias del servicio. El doble de memoria comprueba el contrato de revisión y conflictos.

Prueba aislada (requiere Rust/Cargo y `serde`/`serde_json` disponibles en caché):

```sh
rust/apps/photo-culler-server/src/editing/recipes/test-standalone.sh --offline
```

Integración pendiente: registrar el módulo en el crate servidor, conectar `RecipeStore` con el repositorio/tabla SQLite de recetas, adaptar los DTO REST y mapear errores a `code/message/request_id`. Confirmar contra fixtures de catálogo heredado y repetir la prueba de reinicio usando el servicio integrado.
