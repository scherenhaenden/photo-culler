# Guía de usuario: sesión RAW/JPEG

> Estado: esta guía describe el camino Python disponible en la rama base. El
> reemplazo completo Rust/Slint y sus exportaciones siguen en desarrollo.

## Qué persiste

El catálogo SQLite guarda identidades lógicas, rutas, metadatos, puntuaciones y
decisiones. El análisis no edita originales. El comando `preselect` crea un CSV
separado y los scripts de hojas de contacto crean JPEG derivados. No exportan
originales. El caché de métricas puede quedar en `<catálogo>.metrics.db`.

La importación web indexa las fuentes en su sitio. En el CLI legado, `scan`
empareja por nombre base. El detector de volumen intenta escribir
`.photo-culler-volume.json` en la raíz del volumen; un fallo de escritura se
ignora. La opción `--read-only` no evita ese intento. Para originales, use una
fuente montada en solo lectura y compruebe sus hashes antes/después.

## Ensayo reproducible con imágenes sintéticas

Requiere Python 3.14 y dependencias instaladas (por ejemplo,
`pip install -e '.[test]'`). Los NEF vacíos de este ensayo solo prueban el
emparejamiento por nombre; no son RAW válidos para decodificación o recuperación.

```bash
WORK="$(mktemp -d)"
mkdir -p "$WORK/shoot"
python - "$WORK/shoot" <<'PY'
from pathlib import Path
import sys
from PIL import Image, ImageDraw
root = Path(sys.argv[1])
for n, color in enumerate(("navy", "teal", "gold", "crimson"), start=1):
    image = Image.new("RGB", (640, 420), color)
    ImageDraw.Draw(image).text((30, 30), f"Synthetic frame {n}", fill="white")
    image.save(root / f"FRAME_{n:03}.JPG", quality=92)
    (root / f"FRAME_{n:03}.NEF").write_bytes(b"")
PY

photo-culler --catalog "$WORK/catalog.db" scan "$WORK/shoot"
photo-culler --catalog "$WORK/catalog.db" --json photos
photo-culler --catalog "$WORK/catalog.db" analyze
photo-culler --catalog "$WORK/catalog.db" --json photos
```

Guarde la ruta `WORK` durante el ensayo. El análisis usa el JPEG cuando está
disponible. Confirme los contadores y las entradas antes de interpretar resultados:
los marcadores NEF no prueban que el RAW pueda abrirse.

Para practicar una decisión corregible, copie un `photo_id` completo del listado:

```bash
photo-culler --catalog "$WORK/catalog.db" decisions set \
  --photo-id ID_COMPLETO --decision keep
photo-culler --catalog "$WORK/catalog.db" --json photos --photo-id ID_COMPLETO
```

Repita `set` para cambiar a `best`, `alternate`, `review`,
`reject_technical` o `reject_redundant`. La acción reemplaza la decisión
anterior y queda en el catálogo. En la UI web, abra biblioteca, grupos e inspector
para comparar candidatos; las acciones dependen de la versión cliente.

Los grupos ayudan a revisar similares; no ordenan borrar. Los estados
`REJECT_TECHNICAL` y `REJECT_REDUNDANT` son decisiones del catálogo, no borrado
del archivo. Revise los rechazos y corrija falsos descartes antes de cerrar.

## CSV y hojas de contacto

`preselect` analiza JPEG de una carpeta, no recursivamente. Su selección es una
propuesta algorítmica independiente de las decisiones del catálogo.

```bash
photo-culler preselect "$WORK/shoot" \
  --raw-dir "$WORK/shoot" --output "$WORK/preselection.csv"
python scripts/contact_sheets_from_manifest.py "$WORK/preselection.csv" \
  --output "$WORK/contact-sheets"
```

Columnas: `jpeg_path`, `raw_path`, `group`, `selected`,
`representative_jpeg`, `quality`, `similarity`. El generador usa filas
`selected=yes`, ordena por nombre y produce páginas de hasta 6×8 como JPEG
derivados. Compruebe visualmente las páginas, recuentos y rutas; mantenga la salida
separada de originales.

## Persistencia y evidencia

Vuelva a abrir el mismo catálogo tras cerrar el proceso y confirme IDs/decisiones.
Eso prueba persistencia básica, no recuperación WAL; para copia/restauración siga
[tarea 58 de la hoja de ruta](RUST_SLINT_ROADMAP.md#las-64-tareas).

Registre commit, SO, conteos, IDs (en informes privados), decisiones antes/después,
CSV y hashes, páginas generadas, errores y archivos omitidos. No comparta fotos ni
rutas privadas sin autorización.

## Límites

- Sintéticos prueban el flujo, no la calidad fotográfica.
- Use RAW reales autorizados para validar decodificación; anote cámara, formato y
  procedencia.
- El CSV de preselección no representa el estado humano del catálogo.
- Aún no existe paridad completa de flujo Rust/Slint ni recuperación de UI entre
  todos los clientes.
