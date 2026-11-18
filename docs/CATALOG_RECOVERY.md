# Recuperación del catálogo y sus fuentes

> Este procedimiento explica SQLite/WAL y ofrece una prueba aislada. La API de
> backup SQLite se usa para hacer la copia consistente de muestra. La recuperación
> integrada completa —catálogo más perfiles/recetas y configuración— forma parte
> del trabajo de migración; no suponga que existe hoy un botón de backup en el
> cliente. El módulo Rust de recuperación es trabajo en curso y no es el camino
> operativo por defecto.

## Qué debe incluir una copia

Con el escritor detenido, identifique:

- El archivo de catálogo SQLite configurado mediante `--catalog`,
  `PHOTO_CULLER_DATABASE_URL` o la ruta elegida por la aplicación.
- Los archivos de perfil y configuración referenciados por el catálogo o por el
  flujo local.
- Las recetas de edición, si se almacenan fuera del catálogo en esa versión.
- El caché de métricas (`<catálogo>.metrics.db`) como dato regenerable; decida si
  copiarlo por tiempo de recuperación, pero no lo confunda con el registro de
  decisiones.
- Los originales por separado. La copia del catálogo no copia fotos.

No copie solo `catalog.db` mientras hay un escritor activo. En modo WAL pueden
existir `catalog.db-wal` y `catalog.db-shm`; copiar a mano solo el archivo
principal puede omitir transacciones confirmadas. Detenga el servicio y los
clientes, o use la API `sqlite3.Connection.backup` desde una conexión a la base.
Para una copia de varios archivos relacionados, detenga todos los escritores
durante toda la captura. Mantenga copia y restauración en almacenamiento privado
con permisos restringidos.

## Ensayo de copia y restauración en otra ruta

Este ensayo estándar de biblioteca no modifica el catálogo real. Crea una base de
muestra, activa WAL, confirma una tabla de decisiones/recetas, copia mediante la
API de SQLite y restaura en una ubicación distinta.

```bash
WORK="$(mktemp -d)"
python - "$WORK" <<PY
from pathlib import Path
import sqlite3, sys

root = Path(sys.argv[1])
source = root / "source" / "catalog.db"
backup = root / "backup" / "catalog.db"
restored = root / "restored" / "catalog.db"
for path in (source, backup, restored):
    path.parent.mkdir(parents=True, exist_ok=True)

with sqlite3.connect(source) as db:
    db.execute("PRAGMA journal_mode=WAL")
    db.execute("CREATE TABLE photos(photo_id TEXT PRIMARY KEY, decision TEXT NOT NULL)")
    db.execute("CREATE TABLE recipes(photo_id TEXT PRIMARY KEY, revision INTEGER NOT NULL, payload TEXT NOT NULL)")
    db.execute("INSERT INTO photos VALUES (photo-synthetic-001, KEEP)")
    db.execute("INSERT INTO recipes VALUES (photo-synthetic-001, 3, {"exposure":0.25})")
    db.commit()
    check = db.execute("PRAGMA integrity_check").fetchone()[0]
    assert check == "ok", check

with sqlite3.connect(source) as src, sqlite3.connect(backup) as dst:
    src.backup(dst)

with sqlite3.connect(backup) as src, sqlite3.connect(restored) as dst:
    src.backup(dst)

with sqlite3.connect(restored) as db:
    assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    assert db.execute("SELECT photo_id, decision FROM photos").fetchall() == [
        ("photo-synthetic-001", "KEEP")
    ]
    assert db.execute("SELECT photo_id, revision FROM recipes").fetchall() == [
        ("photo-synthetic-001", 3)
    ]
    print("integrity_check=ok; IDs, decision and recipe revision restored")
PY
```

La tabla de muestra no reproduce el esquema completo de Photo Culler; valida el
método SQLite y la lista mínima de datos que debe comprobarse. Para un ensayo de
versión real, inicialice un catálogo temporal con la versión objetivo, cree
decisiones/perfiles/recetas mediante esa versión, cierre sus escritores, aplique el
mismo método de backup y abra la copia restaurada con la misma versión. Compare
conteos e IDs además de `PRAGMA integrity_check`; registre esquema/migraciones,
hash de la copia, origen/destino, hora y commit.

## Restaurar una copia

1. Detenga Photo Culler y confirme que ningún proceso mantiene la base abierta.
2. Preserve la copia original inmutable. Restaure en un directorio nuevo con espacio
   y permisos de usuario de servicio; no sobrescriba la única copia válida.
3. Verifique integridad, conteos, IDs conocidos, decisiones, perfiles y revisiones
   de recetas. Abra primero con la versión compatible que creó la copia.
4. Configure la aplicación explícitamente para usar la ruta restaurada. Mantenga
   las fuentes originales sin cambios y observe el estado de disponibilidad.
5. Archive logs y resultados. Si algo falla, pare el servicio y vuelva a apuntar a
   la ubicación anterior; no mezcle dos escritores sobre copias divergentes.

## Volúmenes reubicados: `offline` frente a `missing`

- **offline:** el volumen o la carpeta fuente no está accesible. Caso reproducible:
  indexe la carpeta sintética, desconecte o renombre temporalmente su directorio
  raíz y reescanee con el flujo de importación que registra revisiones. La fuente y
  sus archivos deben figurar temporalmente como desconectados. No interprete el
  estado como eliminación.
- **missing:** la raíz sigue disponible, pero falta un archivo indexado. Caso:
  con una copia del fixture, elimine un JPEG de esa copia y reescanee la fuente
  accesible. El archivo debe quedar como ausente, no toda la fuente. No haga esto
  con originales.

La ruta física por sí sola no es identidad. Al volver a conectar el NAS en otra
ruta, seleccione/relocalice la fuente mediante el flujo soportado por esa versión y
compare rutas relativas, tamaño/hashes disponibles e IDs antes de aceptar cambios.
No cree una segunda importación a ciegas: una carpeta reubicada podría verse como
otra fuente si no se reconcilia. La detección de movimiento y la preservación de
decisiones deben comprobarse con una copia del catálogo.

## Evidencia para cada simulacro

Registre commit y versión de esquema, rutas origen/destino, si había WAL activo,
método de captura, hashes SHA-256 de los artefactos de backup, salida de
`integrity_check`, IDs/conteos comparados, decisiones y recetas verificadas,
logs/errores y resultados separados para fuente offline y archivo missing. No
incluya rutas privadas o nombres de fotos en evidencia compartida.
