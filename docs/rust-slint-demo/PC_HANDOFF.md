# Reproducir la demo en otra PC

Esta guía reproduce la integración local del Sprint 01 en otra máquina. El
flujo de código se integró en `846fb58`; el handoff y roadmap visual se
publicaron en `5b5449b`, y la versión subida con el snapshot incluido fue
`6846558`. Usa la punta más reciente de la rama
`feature/sprint-01-rust-foundation` y conserva el SHA que imprima `git rev-parse`.

## Qué entrega este commit

- Un servidor Rust de loopback, solo lectura, con salud, una galería sintética
  y su miniatura JPEG incorporada.
- Una ventana Slint nativa que inicia el servidor hermano, valida su mensaje
  `ready`, consulta la API y muestra la miniatura.
- Una entrada WASM que utiliza la misma vista y consulta salud, galería y
  miniatura desde Rust.
- Respuestas `no-store`, cabeceras de seguridad para HTML/WASM y `nosniff`
  para JavaScript.
- Reintentos con estado de carga e invalidación de respuestas atrasadas, y un
  límite de diez segundos para recibir `ready` al arrancar el servidor nativo.

Es una demo de una foto sintética. Todavía no importa fotos del usuario, no
persiste un catálogo, no pagina miles de fotos y no representa la aplicación
completa. El bundle generado para navegador y la aceptación visual en cinco
plataformas siguen pendientes.

## Preparar un checkout limpio

```sh
git clone <URL_DEL_REPOSITORIO> photo-culler
cd photo-culler
git fetch origin
git switch --track origin/feature/sprint-01-rust-foundation
git rev-parse HEAD
```

El último comando debe coincidir con la punta remota obtenida en el `fetch`.
Para un checkout existente, confirma
primero que `git status --short` no contiene trabajo que quieras conservar; no
uses `reset --hard` para cambiar de versión.

## Requisitos

- Rust/Cargo compatibles con `rust/Cargo.lock` y el toolchain instalado en la
  máquina. Los comandos usan `--locked` para no cambiar las dependencias.
- Acceso local a loopback (`127.0.0.1`) para el servidor y el cliente nativo.
- macOS necesita un entorno gráfico disponible para abrir Slint. Linux necesita
  una sesión gráfica compatible con Slint; Windows requiere el toolchain MSVC.
- El primer build necesita las dependencias de Cargo disponibles localmente o
  acceso a crates.io. `--offline` solo funciona si ya están en la caché.

## Construir y ejecutar en escritorio

Desde la raíz del repositorio:

```sh
cargo build --locked --manifest-path rust/Cargo.toml \
  -p photo-culler-server -p photo-culler-slint
```

Cargo deja ambos ejecutables juntos en `rust/target/debug/` (o en el directorio
configurado mediante `CARGO_TARGET_DIR`). Ejecuta `photo-culler-desktop` desde
esa carpeta para que pueda localizar `photo-culler-server` como binario hermano:

```sh
./rust/target/debug/photo-culler-desktop
```

En Windows usa `rust\target\debug\photo-culler-desktop.exe`. La ventana debe
mostrar `demo-001.jpg` y una miniatura. Cierra la ventana para pedir el cierre
ordenado del servidor hijo. El lanzador espera como máximo diez segundos por
el mensaje `ready`; si el hijo no responde, la interfaz muestra un error con
reintento.

Para ejecutar solo el servidor:

```sh
cargo run --locked --manifest-path rust/Cargo.toml \
  -p photo-culler-server -- --port 0
```

La primera línea de stdout es el evento JSON `ready`. El servicio solo escucha
en loopback. `--data-dir` se rechaza porque este ejecutable de demo no tiene
catálogo persistente.

## Comprobar la compilación WASM

Instala el target si hace falta y compila la entrada Slint para el navegador:

```sh
rustup target add wasm32-unknown-unknown
cargo check --locked --manifest-path rust/Cargo.toml \
  -p photo-culler-slint --target wasm32-unknown-unknown
```

Este check confirma que el código compila para WASM. Por sí solo no genera el
bundle desplegable. Para la aceptación web también hacen falta el paso de
`wasm-bindgen`, el staging de `index.html`, `bootstrap.js`, JS y WASM, y servir
ese bundle mediante `photo-culler-server --static-dir <directorio>`. Esa receta
y el recorrido visual Chromium/Firefox aún no están cerrados; no se debe tomar
un check WASM como prueba de que el navegador muestra la galería.

## Verificaciones incluidas

Desde la raíz:

```sh
cargo test --locked --manifest-path rust/Cargo.toml -p photo-culler-server
```

Los 26 tests del servidor cubren el router, las rutas, tipos MIME y cabeceras,
política de origen y cierre del listener. Los tests que abren puertos loopback
deben poder crear sockets locales en la máquina donde se ejecutan.

En el entorno de desarrollo también se verificó:

```sh
cargo check --offline --locked --manifest-path rust/Cargo.toml \
  -p photo-culler-server -p photo-culler-slint
cargo check --offline --locked --manifest-path rust/Cargo.toml \
  -p photo-culler-slint --target wasm32-unknown-unknown
cargo build --offline --locked --manifest-path rust/Cargo.toml \
  -p photo-culler-server -p photo-culler-slint
```

La aceptación nativa visible y la aceptación web completa se deben registrar
por separado de estos resultados de compilación y tests.
