# Photo Culler Rust/Slint demo e importación v2: ejecutar sin Python

La aceptación S01 documentada en las secciones de paquete, arranque web y CI
cubre la galería sintética de solo lectura. Un smoke API limitado del catálogo
persistente, importación v2, preview, decisión y análisis con dos JPEG sintéticos
está registrado en [runtime evidence](../runtime-evidence/2026-11-21-synthetic-persistent-smoke.md).
No demuestra el diálogo de importación, la UI de edición ni aceptación de
producto. Más abajo hay comandos reproducibles para ampliar la aceptación del
candidato; registra cada resultado contra el hash de fuente indicado. No hace
falta instalar Python: el launcher, el servidor y los clientes son Rust/Slint.

## Archivos del paquete

Para escritorio, copia `photo-culler-desktop` y `photo-culler-server` juntos a
un directorio temporal. En Windows usa ambos archivos `.exe`. El launcher
resuelve el servidor hermano, inicia el hijo sin shell, lee una única línea
JSON `ready` por stdout y valida `GET /api/v1/health` antes de mostrar la UI.
El evento debe incluir `protocol: 1` y un `origin` HTTP loopback con el puerto
asignado. Logs de diagnóstico se escriben a stderr.

Para web, copia `photo-culler-server` junto a `web-dist/`. Ese directorio debe
contener `bundle.json`, `index.html`, `bootstrap.js`,
`photo_culler_slint.js`, `photo_culler_slint_bg.wasm` y los recursos declarados
en el manifiesto. El servidor sirve la página y `/api/v1` desde el mismo
origen; el navegador no se conecta a un segundo host ni usa CORS.

## Arranque web

En macOS/Linux, desde el directorio temporal:

```sh
./photo-culler-server --port 0 --web-root ./web-dist
```

En Windows PowerShell:

```powershell
./photo-culler-server.exe --port 0 --web-root ./web-dist
```

El servidor informa en stdout la URL loopback asignada, por ejemplo
`http://127.0.0.1:49152`. Abre esa URL en Chromium o Firefox. Comprueba salud
con `curl -i http://127.0.0.1:<puerto>/api/v1/health`; la respuesta esperada
es HTTP 200, `Content-Type: application/json` y `status: "ready"`. La página,
los módulos JS y el WASM deben provenir de ese mismo origen. En Network revisa
`text/html`, `text/javascript` y `application/wasm`; la miniatura debe responder
`image/jpeg` y ser visible. La página muestra un diagnóstico accesible si
WebGL o la inicialización WASM falla.

## Catálogo SQLite local

Sin `--data-dir`, el servidor conserva el modo de aceptación S01 y sirve la
galería de fixture incluida. Para seleccionar el catálogo persistente, pasa un
directorio escribible; el servidor crea o abre `catalog.sqlite3` en esa ruta,
prepara el esquema y conserva allí los datos entre ejecuciones:

```sh
./photo-culler-server --port 0 --data-dir ./photo-culler-data --web-root ./web-dist
```

El arranque falla si no puede crear, abrir o preparar la base. No cambia
silenciosamente al fixture, porque eso podría hacer parecer que los datos del
catálogo están disponibles cuando la base no pudo abrirse.

`--data-dir` selecciona exclusivamente `catalog.sqlite3`; no detecta ni abre el
catálogo de la app Python (`$XDG_DATA_HOME/photo-culler/catalog.db`, normalmente
`~/.local/share/photo-culler/catalog.db`). Para preparar una copia desde ese
catálogo, detén sus escritores y usa el procedimiento de copia consistente de
[CATALOG_RECOVERY.md](../CATALOG_RECOVERY.md), incluida la API de backup SQLite
si WAL está activo. Pon la copia en un directorio nuevo como `catalog.sqlite3`,
conserva el original y guarda aparte los perfiles JSON: el backup de SQLite no
los incluye. El servidor migra esa copia al abrirla; el catálogo Python
original queda intacto.

### Importación local desde el servidor CLI

La API de importación solo se monta al configurar, además del catálogo, una o
varias raíces locales autorizadas. Por ejemplo:

```sh
./photo-culler-server --port 0 \
  --web-root ./web-dist \
  --data-dir ./photo-culler-data \
  --authorized-root "$HOME/Pictures"
```

Para seguir el procedimiento HTTP de abajo, usa un puerto fijo libre y una
carpeta de prueba disponible. La raíz debe contener fotos de prueba y estar
montada; no configures raíces padre e hija simultáneamente:

```sh
DATA_DIR="$PWD/photo-culler-data"
PHOTO_ROOT="$HOME/Pictures/Photo-Culler-Import-Test"
./photo-culler-server --bind 127.0.0.1 --port 8765 \
  --data-dir "$DATA_DIR" --authorized-root "$PHOTO_ROOT"
```

Si ya sabes que hay varias galerías, inicia primero el servidor en modo
catálogo, sin `--authorized-root`, y consulta `GET /api/v1/galleries`; luego
deténlo y copia el `id` elegido al arranque con importación. Sin
`--gallery-id`, el servidor crea una galería si no hay ninguna, selecciona la
única existente y falla si la selección es ambigua. El server CLI crea el
directorio de datos si falta. El ejecutable desktop acepta los mismos flags de
catálogo y fuente; omite el flag de galería si no hace falta seleccionar una:

```sh
./photo-culler-server --bind 127.0.0.1 --port 8765 --data-dir "$DATA_DIR"
```

Consulta la lista de galerías en otra terminal y detén este proceso antes de
iniciarlo con la raíz autorizada y el `--gallery-id` escogido.

```sh
./photo-culler-desktop --data-dir "$DATA_DIR" \
  --authorized-root "$PHOTO_ROOT"
```

Para una galería existente concreta, agrega `--gallery-id "ID-copiado"` al
comando de server o desktop.

En Windows PowerShell, usa los ejecutables `.exe` y rutas locales, por ejemplo:

```powershell
$dataDir = Join-Path $PWD "photo-culler-data"
$photoRoot = Join-Path $HOME "Pictures\Photo-Culler-Import-Test"
./photo-culler-server.exe --bind 127.0.0.1 --port 8765 --data-dir $dataDir --authorized-root $photoRoot
```

Para desktop, sustituye `photo-culler-server.exe` por
`photo-culler-desktop.exe` y quita `--bind`/`--port`; el launcher administra el
puerto loopback de su servidor hijo. Añade `--gallery-id <ID>` solo cuando el
catálogo ya tenga varias galerías y quieras seleccionar una concreta. El
launcher aún no incluye picker nativo.

El comando con `--port 8765` sirve para API y desktop. Para revisar la UI en
browser, inicia el servidor con el bundle preparado descrito al principio de
esta guía:

```sh
./photo-culler-server --bind 127.0.0.1 --port 8765 \
  --web-root ./web-dist --data-dir "$DATA_DIR" \
  --authorized-root "$PHOTO_ROOT"
```

El servidor registra esas carpetas antes de aceptar peticiones. La API REST
v2 permite seleccionar fuentes mediante `source_id` opacos; las peticiones no
pueden enviar rutas del sistema. La interfaz Slint incluye un diálogo para
estimar, iniciar, pausar, reanudar y cancelar importaciones, además de mostrar
el progreso de los trabajos persistidos. Sin `--authorized-root`, el servidor
sigue ofreciendo el catálogo persistente, pero no monta la API de importación.
El launcher reenvía raíces configuradas al servidor hijo, pero aún no ofrece
selector nativo de carpetas. Todas las raíces indicadas deben estar montadas y
no solaparse entre sí al arrancar; una raíz ausente o anidada dentro de otra
impide el inicio con importación. Omite las raíces para iniciar el modo catálogo
si necesitas abrirlo mientras una unidad está desconectada.

## Cierre y errores

Para servidor independiente, usa Ctrl+C. Para el launcher de escritorio, cierra
la ventana; el launcher cierra stdin del hijo, espera hasta tres segundos y
después termina y recoge el proceso si no salió. Los errores de carga se
presentan fuera del canvas y los detalles operativos están en stderr. Conserva
stdout, stderr, la versión del browser y la URL al registrar una ejecución.

## Comprobación manual del importador v2 candidato

Este procedimiento es una propuesta de aceptación local, no un resultado ya
pasado. Requiere `curl`, un servidor candidato arrancado como arriba y un
directorio de prueba que contenga archivos compatibles. La lista implementada
incluye RAW (`3fr`, `arw`, `cr2`, `cr3`, `dng`, `fff`, `iiq`, `kdc`, `mef`,
`mos`, `mrw`, `nef`, `nrw`, `orf`, `pef`, `raf`, `raw`, `rw2`, `rwl`, `srw`),
JPEG (`jpg`, `jpeg`, `jpe`), otras
imágenes (`bmp`, `gif`, `heic`, `heif`, `png`, `tif`, `tiff`, `webp`) y
sidecars (`aae`, `dop`, `pp3`, `xmp`). Usa una carpeta de prueba con suficientes
archivos para que puedas solicitar una pausa antes de que finalice el escaneo.

1. Consulta el catálogo y copia el `id` de destino. El servidor mantiene
   loopback; ejecuta el browser en la misma máquina:

   ```sh
   BASE_URL=http://127.0.0.1:8765
   curl -fsS "$BASE_URL/api/v1/galleries"
   ```

2. Lista las fuentes registradas para esa galería y copia el `id` de la raíz
   autorizada. La respuesta de fuentes contiene IDs opacos, nombre y estado;
   no devuelve la ruta del sistema:

   ```sh
   GALLERY_ID="copia-aqui-el-id-de-la-galeria"
   curl -fsS "$BASE_URL/api/v2/galleries/$GALLERY_ID/sources"
   ```

3. Estima y luego inicia el trabajo. Copia `job_id` de la respuesta HTTP 202:

   ```sh
   SOURCE_ID="copia-aqui-el-id-de-la-fuente"
   curl -fsS -X POST -H 'Content-Type: application/json' \
     -d "{\"source_id\":\"$SOURCE_ID\",\"recursive\":true}" \
     "$BASE_URL/api/v2/import-estimates"
   curl -i -X POST -H 'Content-Type: application/json' \
     -d "{\"source_id\":\"$SOURCE_ID\",\"recursive\":true}" \
     "$BASE_URL/api/v2/galleries/$GALLERY_ID/imports"
   ```

4. Comprueba el trabajo con `GET /api/v2/import-jobs/<JOB_ID>`. Solicita pausa,
   espera a que ese GET muestre `paused`, reanuda y confirma que vuelve a
   `running` o llega a un estado terminal. Después solicita cancelación en un
   trabajo nuevo o todavía activo y consulta hasta que aparezca `cancelled`:

   ```sh
   JOB_ID="copia-aqui-el-id-del-trabajo"
   curl -i -X POST "$BASE_URL/api/v2/import-jobs/$JOB_ID/pause"
   curl -fsS "$BASE_URL/api/v2/import-jobs/$JOB_ID"
   curl -i -X POST "$BASE_URL/api/v2/import-jobs/$JOB_ID/resume"
   curl -fsS "$BASE_URL/api/v2/import-jobs/$JOB_ID"
   ```

   Las acciones son asíncronas: repite el GET hasta que el estado esperado se
   vea en la respuesta. Si el escaneo termina antes de pausar, repite con una
   carpeta de prueba mayor. Para probar cancelación, inicia otro trabajo con el
   paso 3 y antes de que termine ejecuta:

   ```sh
   curl -i -X POST "$BASE_URL/api/v2/import-jobs/$JOB_ID/cancel"
   curl -fsS "$BASE_URL/api/v2/import-jobs/$JOB_ID"
   ```

   Antes de esos comandos, actualiza `JOB_ID` con el ID del nuevo trabajo.
   Repite el GET hasta que indique `cancelled`. Si termina demasiado rápido,
   prueba con una carpeta mayor. Para verificar la frontera autorizada, intenta
   estimate con un `source_id` no registrado; la petición debe fallar. Prueba
   también que el API rechaza un cuerpo con una ruta arbitraria:

   ```sh
   curl -i -X POST -H 'Content-Type: application/json' \
     -d '{"source_id":"not-registered","path":"/outside/photos"}' \
     "$BASE_URL/api/v2/import-estimates"
   ```

   El API no admite rutas del sistema en el cuerpo: los DTO rechazan campos
   extra como `path`.

5. Para verificar persistencia y recuperación, inicia otro trabajo, espera a
   que quede `paused`, detén el server con Ctrl+C y reinícialo con el mismo
   `--data-dir`, las mismas raíces y, si aplica, el mismo `--gallery-id`. La
   consulta `GET /api/v2/import-jobs?active_only=true&limit=100` debe seguir
   mostrando el trabajo pausado. Reanúdalo y comprueba que finaliza; consulta
   `GET /api/v2/scan-revisions?gallery_id=<ID>&limit=10` para revisar la revisión
   persistida. No cambies ni muevas la raíz entre arranques, pues el registro de
   fuente está ligado a su ruta canónica.

Al registrar la aceptación, guarda el SHA, sistema operativo, comandos, estado
HTTP final, respuesta de estimate, estados del trabajo y revisión. Ejecuta el
mismo flujo desde el diálogo Slint en escritorio y browser para dar por
aceptada cada interfaz; la secuencia `curl` por sí sola solo verifica la API.

Para cada interfaz, la comprobación manual pendiente es: iniciar con los flags
de la sección anterior, abrir Import, verificar que solo aparece la fuente
configurada, seleccionar esa fuente, ejecutar estimate y revisar su cantidad de
archivos/fotos, iniciar y comprobar progreso, pausar y reanudar un trabajo que
tarde lo suficiente, y cancelar otro. Cierra y vuelve a abrir la app con el
mismo catálogo y raíz para confirmar que el trabajo persistido aparece. En
browser, sirve `web-dist` desde el mismo proceso loopback y ejecuta la página en
ese mismo equipo. Anota por separado los resultados del API, desktop y browser;
no infieras aceptación de una superficie a partir de otra.

## Capturar Library en dos tamaños

`ui/library/capture_smoke.mjs` abre el servidor Axum local con el bundle WASM
real y activa el fixture solo mediante `?photo-culler-smoke=1`. Requiere Node.js
y Playwright 1.51.1. El comando reproducible y sus precondiciones están en
[`BUILDS.md`](../BUILDS.md#reproduce-the-paired-s05-browser-captures). El script
comprueba el tamaño CSS y backing store del canvas, la miniatura REST, la
selección inicial y las dimensiones/hash de cada PNG.

Las capturas pareadas y su manifest están en
[`evidence/design`](evidence/design/README.md). El fixture tiene diez registros
con metadatos sintéticos y repite un JPEG; no representa datos EXIF reales. La
captura WASM tampoco prueba el renderizador ni el ciclo de vida de una ventana
nativa.

## Verificar procesos y Python

Tras arrancar el paquete, identifica el PID del servidor (`pgrep -f
photo-culler-server` en macOS/Linux; `Get-Process photo-culler-server` en
PowerShell). Verifica todo el subárbol del proceso con:

```sh
migration/demo/smoke/no-python/verify-process-tree.sh <pid>
```

El comando imprime los procesos descendientes y falla si alguno contiene un
intérprete Python. Registra el PID, árbol, binario, versión y resultado. La
prueba no declara aceptación visual: D24/D25 registran Chromium/Firefox y D34
repite ambos sobre el bundle real, con MIME y miniatura.

## Evidencia CI vigente

La ejecución más reciente de la pila [#44566993573](https://github.com/scherenhaenden/photo-culler/actions/runs/44566993573)
pasó sus seis jobs sobre el commit documental `3884dc47feea0d29d691c9f5431343b624537538`.
La ejecución integrada inmediatamente anterior [#44563618333](https://github.com/scherenhaenden/photo-culler/actions/runs/44563618333)
pasó la misma matriz sobre `59cb9f0` y produjo los artefactos de aceptación cuyos
hashes se registran en [`evidence/design`](evidence/design/README.md) y
[BUILDS.md](../BUILDS.md). Ambas cubren build/staging, WASM, Linux x86-64,
macOS arm64, Windows x86-64 y Chromium/Firefox. Los runners de escritorio
abrieron la galería real de una foto y una captura separada de diez tarjetas
sintéticas, verificaron cierre y reapertura, liberación del puerto propio y
supervivencia de un servidor externo. Los navegadores capturaron estados de
carga, vacío, error y reintento a 1440×900 y 1100×700.

La pila de PR #153–#157 sigue abierta y en borrador. D34 requiere integrar la
pila en `feature/sprint-01-rust-foundation` y repetir la aceptación en la rama
resultante antes de promover el sprint.

Descarga local de toda la evidencia:

```sh
gh run download 44546749693 --repo scherenhaenden/photo-culler --dir ./artifacts/run-44546749693
```

El runner Windows captura el marco DWM visible a 1442×932; el tamaño real de la
ventana se mantiene en 1456×939 y el recorte excluye taskbar y escritorio. El
workflow usa `winit-software` en Windows alojado porque el runner no dispone de
OpenGL 2.0 fiable; los paquetes normales conservan la selección estándar del
renderizador.

La corrida [#44546749693](https://github.com/scherenhaenden/photo-culler/actions/runs/44546749693)
es evidencia histórica del build y la aceptación nativa de una versión anterior
del diseño. #44563618333 es la ejecución integrada que produjo los artefactos
registrados actualmente. #44566993573 confirmó después que la revisión
documental conserva el verde de los seis jobs.

El código final del PR de diseño #154 es `afe088242bdcf173eb8e83c9571bf598f52c4e86`.
En ese SHA pasaron localmente 33 pruebas Slint (una ignorada), 8 contratos de
paridad, checks nativo/WASM y la captura browser de ambas dimensiones. El CI de
ese SHA todavía no aparecía al cerrar la captura. Revisa el manifest y los
límites exactos de esa evidencia en [`evidence/design`](evidence/design/README.md);
no la presentes como una ventana desktop aceptada.
