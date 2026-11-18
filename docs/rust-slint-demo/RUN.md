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

El runner actual no tiene aún los binarios Slint/servidor del sprint. D34 debe
copiar esos artefactos a un directorio temporal, ejecutar este protocolo y
publicar el resultado real. Un build manifestado no cuenta como arranque.
