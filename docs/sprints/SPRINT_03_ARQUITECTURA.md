# Sprint 03 · Arquitectura Linux de selección

## Punto de partida comprobado

- `DemoController::update_decisions` ya escribe por foto con ID de petición y revisión esperada. La ruta SQLite persiste la decisión y el cliente vuelve a leer decisión/revisión al cargar una página.
- Las tarjetas ya se seleccionan con ratón y con Enter/Espacio; Shift/Ctrl/Meta permiten selección múltiple. Badges y marcas de tarjeta ya representan KEEP/ALT/REJECT.
- La barra de selección muestra KEEP/ALT/REJECT/CLEAR, pero `DecisionAction` es una `TouchArea` sin foco ni nombre accesible. Una persona que usa solo teclado puede seleccionar una foto, pero no decidirla.
- `SlintDemoView` calcula contadores con `LibraryInteractionState.items`, que contiene la página cargada. El servidor ya ofrece `GET /api/v1/galleries/{gallery_id}/summary` con recuentos globales.
- `LibraryFilters::matches` filtra el conjunto de la página en memoria. El cliente de catálogo no manda el filtro KEEP al leer la primera página y el controlador no reconstruye el cursor con el filtro activo.

Por tanto, Sprint 03 completa el recorrido de selección; no vuelve a implementar la escritura ni inventa almacenamiento paralelo.

## Contratos y flujo

```text
Tarjeta (ratón / Enter / Espacio)
   └─ selection-requested(photo_id, multi)
       └─ estado de selección del SlintDemoView
           └─ DecisionAction enfocable (KEEP / ALT / REJECT / CLEAR)
               └─ desktop_app callback
                   └─ DemoController::update_decisions
                       └─ PUT /api/v1/photos/{id}/decision con revisión
                           └─ SQLite confirma decisión y nueva revisión
                               ├─ modelo Slint actualiza badge
                               └─ GET summary refresca contadores globales
```

La filtración utiliza la ruta ya disponible `GET /api/v1/galleries/{gallery_id}/photos?decision=KEEP&limit=N`. El cursor devuelto contiene su filtro; navegar conserva el cursor opaco. Al cambiar `all`/`keep`/`alt`/`reject`/`unrated`, el controlador elimina el historial de páginas y carga desde la primera página con el filtro correspondiente. Los filtros de metadatos ya existentes permanecen locales durante este sprint.

El resumen es la fuente de contadores globales. Mapear nombres de decisión por categoría (`KEEP`/`BEST`, `ALT`/`ALTERNATE`/`REVIEW`, `REJECT*`, otros a no valoradas); no sumar una página local como si fuera todo el catálogo. Después de una decisión, refrescar el resumen tras su recibo de servidor. Si falla el recibo o el resumen, conservar la última cifra confirmada y mostrar el estado recuperable ya existente.

## Ramas y propiedad de archivos

### `feature/s03-keyboard-decisions`

Dueña de `rust/apps/photo-culler-slint/ui/library/gallery.slint` y pruebas de presentación asociadas. Convertir `DecisionAction` en un control enfocable con rol/nombre accesible, anillo de foco visible y activación Enter/Espacio. Hacer que “Select All” también se alcance y active con teclado; preservar la selección múltiple actual y no introducir accesos rápidos globales que interfieran con búsqueda.

### `feature/s03-gallery-counts`

Dueña de `rust/apps/photo-culler-slint/src/demo/rest.rs`, `controller.rs`, `slint_view.rs` y tests del modelo/controlador. Añadir lectura tipada del resumen global; suministrarlo a la vista sin calcular totales a partir de la página. Transportar el filtro de decisión en la carga inicial y el cursor; cambiarlo debe restablecer navegación. Mantener las revisiones de decisión y el comportamiento de respuestas tardías.

### `feature/s03-linux-e2e-selection`

Dueña de `migration/demo/smoke/`, `migration/demo/build/ci/` si hace falta y el job Linux de `.github/workflows/rust-slint-demo.yml`. Generar fixture 13 JPEG/manifiesto; arrancar el paquete con XDG/catalog en sandbox temporal; automatizar ratón/teclado bajo Xvfb; verificar SQLite/API, contadores, filtros, capturas, hashes y reapertura. No debe abrir ventanas en `DISPLAY` real.

## Orden de integración

1. Revisar el contrato actual y preservar IDs de foto y revisiones.
2. Integrar acciones de teclado y la carga global de filtro/resumen.
3. Sobre el commit integrado del sprint, ejecutar el recorrido N2N en Xvfb y ajustar los defectos detectados.
4. Ejecutar `cargo fmt --check`, tests del crate Slint, tests completos de workspace que correspondan y workflow de paquete Linux.
5. Revisar artefactos y capturas. Mantener Sprint 03 en su rama hasta cerrar el E2E de Sprint 02 y después evaluar Sprint 03 → `master`.
