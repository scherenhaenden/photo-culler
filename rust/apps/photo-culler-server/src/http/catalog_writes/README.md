# Tarea 13: adaptador de escrituras

Este módulo es autónomo porque en `a68f360` no existen el crate
`photo-culler-server`, el arnés común, los DTO ni las rutas congeladas. `handle`
es un adaptador tipado, no un servidor HTTP ni una implementación Axum.

El integrador debe conectar las rutas y métodos acordados bajo `/api/v1` a
`handle`, convertir los DTO Serde a `Request` y convertir `Response` a HTTP.
Los nombres de rutas, JSON, los códigos de estado y mensajes aquí son
provisionales hasta acordar el contrato común. En particular, el único endpoint
REST anterior identificado para estas escrituras es `PUT /v1/photos/{photo_id}/decision`;
las rutas de sesiones y perfiles anteriores pertenecen a la interfaz web y no
son prueba de un contrato REST nuevo.

El servicio de dominio debe validar y persistir, comprobar la revisión de forma
atómica y persistir la deduplicación de `request_id` con la operación. La
repetición del mismo ID y contenido devuelve el resultado original; el uso del
mismo ID con otro contenido produce un error de estado. El adaptador no
implementa persistencia ni garantiza idempotencia por sí mismo. Deben definirse
el alcance, retención e identidad de la clave de reintento en integración. El
campo `document` es un marcador opaco, no un DTO de receta o perfil definitivo.

Prueba independiente desde la raíz del worktree:

```sh
rustc --edition 2021 --test rust/apps/photo-culler-server/src/http/catalog_writes/mod.rs -o /tmp/catalog-writes-tests
/tmp/catalog-writes-tests
```
