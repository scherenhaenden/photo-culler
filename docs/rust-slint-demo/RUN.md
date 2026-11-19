# Photo Culler Rust/Slint demo: ejecutar sin Python

Esta guía prueba el paquete preparado, no un checkout de desarrollo. No hace
falta instalar Python: el launcher, el servidor, el HTML, JavaScript y WASM
proceden de los artefactos Rust/Slint.

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

## Cierre y errores

Para servidor independiente, usa Ctrl+C. Para el launcher de escritorio, cierra
la ventana; el launcher cierra stdin del hijo, espera hasta tres segundos y
después termina y recoge el proceso si no salió. Los errores de carga se
presentan fuera del canvas y los detalles operativos están en stderr. Conserva
stdout, stderr, la versión del browser y la URL al registrar una ejecución.

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

La aceptación final [#44546749693](https://github.com/scherenhaenden/photo-culler/actions/runs/44546749693)
pasó sus seis jobs en el head `68959bf702c554839b720d80fd51bfbad6824317`, basado
en diseño `afe088242bdcf173eb8e83c9571bf598f52c4e86` y docs `51deca2`. Verificó
contratos y staging, Linux x86-64, macOS arm64, Windows x86-64, WASM y
Chromium/Firefox. En cada desktop runner ejecutó el flujo live de una foto, una
captura separada del fixture de diez tarjetas, cierre/reapertura con liberación
del puerto propio y supervivencia del servidor externo. Los hashes de las
capturas nativas y los digests de artifacts están en
[`evidence/design`](evidence/design/README.md) y [BUILDS.md](../BUILDS.md).

Descarga local de toda la evidencia:

```sh
gh run download 44546749693 --repo scherenhaenden/photo-culler --dir ./artifacts/run-44546749693
```

El runner Windows captura el marco DWM visible a 1442×932; el tamaño real de la
ventana se mantiene en 1456×939 y el recorte excluye taskbar y escritorio. El
workflow usa `winit-software` en Windows alojado porque el runner no dispone de
OpenGL 2.0 fiable; los paquetes normales conservan la selección estándar del
renderizador.

La corrida anterior [#44508069950](https://github.com/scherenhaenden/photo-culler/actions/runs/44508069950)
validó una revisión anterior del diseño (`14a4a6fc3371ee5d7ebc8fc3c303e2d7bfa93ceb`).
Sigue como evidencia histórica del build y browser flow; la evidencia nativa
vigente es #44546749693.

El código final del PR de diseño #154 es `afe088242bdcf173eb8e83c9571bf598f52c4e86`.
En ese SHA pasaron localmente 33 pruebas Slint (una ignorada), 8 contratos de
paridad, checks nativo/WASM y la captura browser de ambas dimensiones. El CI de
ese SHA todavía no aparecía al cerrar la captura. Revisa el manifest y los
límites exactos de esa evidencia en [`evidence/design`](evidence/design/README.md);
no la presentes como una ventana desktop aceptada.
