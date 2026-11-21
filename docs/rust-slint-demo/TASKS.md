# Fichas de implementación y aceptación

Cada ficha se puede copiar en un mensaje con el [encabezado común de despacho](PIPELINE.md#despacho-y-revisión). Los contratos y ejemplos en [Arquitectura](ARCHITECTURE.md) aplican a todas las tareas. Las rutas de propiedad son relativas al repositorio. Si una prueba aún necesita un doble, identificarlo en la entrega junto con el comando y su resultado. D34 repite el recorrido con artefactos integrados.

## D01

**Contrato REST y puntos de conexión.** Crear `photo-culler-api` con DTOs de salud, galería y error, constantes, constructor de ruta de miniaturas y límites de respuesta. Publicar las respuestas de ejemplo y las firmas de integración para los consumidores.

- **Propiedad:** `rust/crates/photo-culler-api/**`, `migration/demo/contract/**`.
- **Aceptación:** ida y vuelta Serde con JSON válido, galería vacía y error. Ruta de miniatura idéntica en servidor y cliente. El paquete compila para nativo y WASM cuando D33 y D20 estén disponibles. Cualquier diferencia con los adaptadores anteriores queda documentada.

## D02

**Fixture de galería.** Generar un JPEG sintético propio, una miniatura distinta y un manifiesto con ID `demo-001`, procedencia, dimensiones y SHA-256 de cada archivo. El JPEG debe ser visualmente reconocible en una captura pequeña, con contenido determinista.

- **Propiedad:** `migration/demo/fixture/**`.
- **Aceptación:** los hashes coinciden, ambos JPEG se pueden decodificar y la miniatura cabe en el límite del contrato. Documentar cómo regenerar o verificar los archivos. La demo usa los bytes ya incluidos al compilar.

## D03

**Paquete servidor.** Añadir el manifiesto, el punto de entrada y el módulo raíz de `photo-culler-server`. Declarar Axum, Tokio y el contrato compartido. Reservar el punto de entrada que D34 conectará al router y declarar los flags previstos en arquitectura.

- **Propiedad:** `rust/apps/photo-culler-server/Cargo.toml`, `rust/apps/photo-culler-server/src/{lib,main}.rs` y `src/demo_args.rs` del mismo paquete.
- **Aceptación:** con el manifiesto temporal o desde el workspace D33, el paquete compila. `--help` enumera las opciones. Argumentos inválidos se rechazan. Si aún falta el router, el scaffold devuelve un error explícito sin anunciar disponibilidad.

## D04

**Ciclo HTTP.** Construir el listener loopback, incorporar la señal de cierre y emitir el mensaje `ready` según el contrato. Recibir un constructor de router que conozca la dirección efectiva para aplicar la política de origen.

- **Propiedad:** `rust/apps/photo-culler-server/src/http/runtime/lifecycle.rs` y pruebas dedicadas bajo `src/http/runtime/tests/` con prefijo `lifecycle`.
- **Aceptación:** con router simulado y puerto `0`, `ready` contiene un puerto asignado que recibe peticiones. EOF en modo propio y una señal de cierre inyectada en modo independiente liberan el puerto. Un fallo de preparación no emite `ready`.

## D05

**Salud y correlación.** Adaptar el módulo de diagnósticos existente a la respuesta HTTP de la demo. Mantener el identificador por petición y proyectar el fallo de disponibilidad al error acordado.

- **Propiedad:** `rust/apps/photo-culler-server/src/demo/health.rs`.
- **Aceptación:** una sonda inyectada devuelve `200/ready` o `503/service_unavailable`; el JSON y el ID coinciden con D01. No requiere telemetría del sistema para estar listo.

## D06

**Lectura de galería.** Exponer `GET /api/v1/galleries/{id}/photos` sobre el fixture. Encapsular la proyección de campos de catálogo en el borde HTTP con los DTO de D01.

- **Propiedad:** `rust/apps/photo-culler-server/src/demo/gallery.rs`.
- **Aceptación:** `demo` devuelve la fila y su URL de miniatura con los campos pactados. Galería desconocida devuelve `404`, consulta no admitida devuelve `400` y una galería vacía de prueba produce la respuesta pactada.

## D07

**Miniatura JPEG.** Exponer la ruta de miniatura con los bytes incorporados al binario y tamaño máximo definido. Resolver por ID conocido y variante admitida.

- **Propiedad:** `rust/apps/photo-culler-server/src/demo/thumbnail.rs`.
- **Aceptación:** respuesta `200 image/jpeg` y bytes iguales al fixture. Otro ID o tamaño devuelve `404`. Funciona con un directorio actual distinto del repositorio.

## D08

**Política de entrada.** Adaptar `OriginPolicy` a middleware Axum y permitir solo lectura. Aplicar la política al origen efectivo de D04 y conservar respuestas de error comprensibles para REST.

- **Propiedad:** `rust/apps/photo-culler-server/src/demo/ingress.rs`.
- **Aceptación:** `Host` válido y GET nativo sin `Origin` funcionan. Origen presente ajeno o autoridad inesperada se rechazan. `POST` no ejecuta ningún handler. No se emiten cabeceras CORS.

## D09

**Recursos web desde Axum.** Preparar un router de estáticos que lea el bundle explícito y sirva solo los recursos declarados. Validar la raíz antes del arranque y conservar el espacio `/api/v1` para el router API.

- **Propiedad:** `rust/apps/photo-culler-server/src/demo/web_assets.rs`.
- **Aceptación:** HTML, JS y WASM tienen su MIME correcto. Recurso desconocido y ruta API desconocida obtienen `404`; no se responde con el HTML de inicio para ambos. Raíz web inválida falla antes de `ready`. Probar con un bundle simulado.

## D10

**Cliente REST compartido.** Registrar el paquete y añadir un módulo `demo` asíncrono con JSON, binarios, límites de respuesta y errores tipados. Conservar el contrato anterior para sus consumidores.

- **Propiedad:** `rust/crates/photo-culler-client/Cargo.toml`, `rust/crates/photo-culler-client/src/lib.rs`, `rust/crates/photo-culler-client/src/demo/**`.
- **Aceptación:** un transporte ficticio reproduce salud, galería y JPEG. El cliente distingue error de transporte, estado HTTP, MIME y cuerpo inválido. No sigue referencias a otro origen ni acepta redirecciones. El módulo es compilable en WASM sin dependencias nativas.

## D11

**Paquete Slint y preparación del build.** Crear el manifiesto, el build script y una ventana provisional que importe los componentes existentes. Fijar versiones coherentes de Slint y `slint-build`, con features de plataforma que D20 y D33 puedan resolver.

- **Propiedad:** `rust/apps/photo-culler-slint/{Cargo.toml,build.rs,src/lib.rs,ui/demo_scaffold.slint}`.
- **Aceptación:** Slint compila el catálogo de componentes en el paquete nativo con una ventana provisional. D20 puede compilar el mismo paquete para WASM sin resolver librerías nativas de ejecución. El build script tiene un punto claro para que D34 seleccione `demo_window.slint`.

## D12

**Ventana principal.** Crear `DemoWindow` con cuadrícula, nombre de foto, miniatura, error legible y reintento. Reutilizar tema, botones y foco de los componentes existentes.

- **Propiedad:** `rust/apps/photo-culler-slint/ui/demo_window.slint` y auxiliares nuevos en `ui/demo/`.
- **Aceptación:** la vista compila con un runner provisional y se puede mostrar con una fila estática. Carga, vacío, listo y error se distinguen por texto y el botón de reintento es accesible con teclado.

## D13

**Controlador Rust.** Mapear el cliente a las propiedades de `DemoWindow`. Coordinar carga de galería y miniatura, invalidación de resultados atrasados y reintento. Usar un adaptador de vista inyectable durante el trabajo aislado.

- **Propiedad:** `rust/apps/photo-culler-slint/src/demo/controller.rs`.
- **Aceptación:** respuestas simuladas provocan los cuatro estados. Una respuesta vieja no reemplaza una carga nueva y un reintento recupera un fallo. El controlador publica solo en el event loop de Slint.

## D14

**Adaptador JPEG.** Implementar decodificación y entrega de imagen a Slint a partir de bytes, compartida entre escritorio y WASM.

- **Propiedad:** `rust/apps/photo-culler-slint/src/demo/thumbnail_image.rs`.
- **Aceptación:** conservar dimensiones y mostrar el fixture. Un JPEG truncado, un MIME inválido o una imagen fuera de límites producen estado de miniatura no disponible sin vaciar la galería.

## D15

**Transporte HTTP nativo.** Implementar el puerto del cliente para loopback, con timeout y lectura acotada de bytes. Ejecutar I/O en un hilo de trabajo si se usa `ureq`.

- **Propiedad:** `rust/apps/photo-culler-slint/src/desktop/demo_http.rs`.
- **Aceptación:** un servidor de prueba recibe salud, galería y miniatura. El timeout y una redirección se convierten en errores controlados. No se bloquea el event loop con una respuesta lenta.

## D16

**Launcher nativo.** Crear la entrada de escritorio e implementar arranque local y conexión local explícita. Consumir el protocolo `ready` y verificar salud antes de dar el servicio por listo. Usar D28 para resolver el ejecutable y crear el proceso.

- **Propiedad:** `rust/apps/photo-culler-slint/src/bin/photo-culler-desktop.rs`, `rust/apps/photo-culler-slint/src/desktop/demo_launch.rs`.
- **Aceptación:** con hijo simulado, un origen válido abre la UI, un mensaje mal formado o un timeout muestra un fallo y un reintento puede recuperar la conexión. Distinguir desde la API del launcher una instancia propia de una externa.

## D17

**Cierre del servidor propio.** Implementar el cierre en la ventana y la salida del proceso, incluso si falla el arranque. El controlador de vida útil posee el hijo y D16 le entrega ese recurso.

- **Propiedad:** `rust/apps/photo-culler-slint/src/desktop/demo_shutdown.rs`.
- **Aceptación:** cerrar stdin permite la salida ordenada. El hijo que se atasca se termina y recoge. Una instancia externa sigue viva. Repetir el cierre es seguro.

## D18

**Artefacto macOS arm64.** Definir la receta para compilar ambos paquetes y preparar una carpeta portátil con ejecutables contiguos y permisos adecuados.

- **Propiedad:** `migration/demo/build/macos/**`.
- **Aceptación:** el staging con binarios de prueba conserva ejecutabilidad y estructura. El comando puede recibir un directorio de salida con espacios. La receta definitiva se ejecuta con D34 en arm64.

## D19

**Smoke macOS.** Preparar el procedimiento mínimo de arranque, comprobación de galería, cierre, reapertura y preservación de servidor externo.

- **Propiedad:** `migration/demo/smoke/macos/**`.
- **Aceptación:** el guion distingue proceso propio y externo, verifica liberación del puerto y especifica la evidencia visual. La ejecución en el Mac con Slint real se registra con D34.

## D20

**Toolchain WASM.** Documentar el target y preparar el comando de build `--locked`. Instalar o validar la versión de la CLI `wasm-bindgen` que coincide con Cargo. Mantener separadas las dependencias de plataforma.

- **Propiedad:** `migration/demo/build/wasm/**`.
- **Aceptación:** la receta detecta target ausente y versión de herramienta incompatible con mensajes claros. Compila el scaffold de D11 para `wasm32-unknown-unknown`. Indica el archivo de entrada que consumirá D23.

## D21

**Fetch en Rust.** Implementar el transporte WASM con bindings del navegador. Mover al módulo Rust las peticiones de aplicación, incluida salud, que la página provisional hacía desde JS.

- **Propiedad:** `rust/apps/photo-culler-slint/src/browser/demo_fetch.rs`.
- **Aceptación:** Fetch resuelve las rutas relativas al origen, devuelve estado y bytes y distingue fallos HTTP y de red. El transporte rechaza redirecciones y aplica los límites de D01. Coordinar con D22 la retirada del transporte JS.

## D22

**Canvas y cargador.** Conectar la entrada WASM a Slint y al controlador compartido. Adaptar el HTML y el JS existentes para inicializar el módulo enlazado. Mantener el diagnóstico de WebGL y carga fuera del canvas.

- **Propiedad:** `rust/apps/photo-culler-slint/src/browser/{demo_start.rs,index.html,bootstrap.js}`.
- **Aceptación:** el cargador arranca el módulo cuando existe y muestra texto accesible cuando falta o WebGL no está disponible. El JS no consulta la API. Rust usa el transporte de D21 para la galería.

## D23

**Distribución web.** Preparar staging de HTML, bootstrap, JS generado, WASM y recursos requeridos. Emitir el manifiesto de archivos esperado por D09.

- **Propiedad:** `migration/demo/build/web-dist/**`.
- **Aceptación:** un bundle de muestra valida completo, y una entrada ausente o fuera de la lista falla antes de publicarse. La receta documenta `photo-culler-server --port 0 --web-root <directorio>` y su URL de salida.

## D24

**Smoke Chromium.** Preparar un recorrido reproducible con el bundle servido por Axum. Registrar versión del navegador, peticiones y resultado visual de la miniatura.

- **Propiedad:** `migration/demo/smoke/chromium/**`.
- **Aceptación:** los checks se pueden ejercitar con una página de muestra y detectan errores API o WASM. En D34 se ejecutan con HTML, JS, WASM y JPEG desde un solo origen.

## D25

**Smoke Firefox.** Preparar el recorrido equivalente en Firefox, con verificación de arranque del canvas y diagnóstico si falla la inicialización gráfica o del módulo.

- **Propiedad:** `migration/demo/smoke/firefox/**`.
- **Aceptación:** registrar versión, solicitudes y resultado visual. El procedimiento detecta una inicialización fallida y comprueba el texto de error. D34 aporta el resultado sobre el bundle real.

## D26

**Matriz CI de la demo.** Invocar las recetas de macOS, Linux, Windows y WASM en jobs separados. Usar `--locked` y adjuntar binarios, bundle, checksums y manifiestos. Ejecutar ante cambios de la demo en PRs hacia el sprint y en pushes a la rama del sprint.

- **Propiedad:** `.github/workflows/rust-slint-demo.yml`.
- **Aceptación:** los jobs declaran target y runner, los nombres de artefacto son únicos y la matriz conserva fallos por plataforma. El manifiesto identifica versión visible, SHA y toolchain. Una etapa con runner sin UI puede reportar compilación, y D34 registra la prueba gráfica por separado.

## D27

**Guía y runtime sin Python.** Documentar cómo abrir el paquete nativo y web desde un directorio temporal con los artefactos. Preparar protocolo y registro de procesos para la prueba en un entorno sin Python disponible para la aplicación.

- **Propiedad:** `docs/rust-slint-demo/RUN.md`, `migration/demo/smoke/no-python/**`.
- **Aceptación:** la guía identifica qué binarios y recursos copiar, cómo obtener la URL local, cómo cerrar y dónde encontrar errores. El procedimiento permite comprobar el árbol de procesos y la ausencia de Python en ejecución. D34 publica los resultados reales.

## D28

**Portabilidad del launcher.** Implementar la resolución del binario hermano y la parte de creación de proceso común a macOS, Linux y Windows. Definir la entrega del handle y pipes a D16/D17.

- **Propiedad:** `rust/apps/photo-culler-slint/src/desktop/demo_platform.rs`.
- **Aceptación:** ruta absoluta, directorios con espacios y nombre de ejecutable `.exe` en Windows. La invocación usa argumentos estructurados y no shell. Reportar si el binario hermano falta. Probar la resolución de nombres en las tres configuraciones.

## D29

**Artefacto Linux x86-64.** Proveer la receta de compilación y staging, dependencias gráficas necesarias y comandos para ejecutar el paquete en el runner previsto.

- **Propiedad:** `migration/demo/build/linux/**`.
- **Aceptación:** la receta prepara los dos binarios ejecutables en el mismo directorio. Documentar el entorno gráfico usado y distinguir requisitos de compilación de los de ejecución. D34 ejecuta el build con Slint real.

## D30

**Smoke Linux.** Preparar el recorrido de arranque en sesión gráfica Linux, con detección de readiness, miniatura visible y limpieza del proceso/puerto.

- **Propiedad:** `migration/demo/smoke/linux/**`.
- **Aceptación:** el guion registra si el entorno es X11 o Wayland y puede detectar cierre incompleto. D34 registra qué backend de ventana se recorrió con el build real.

## D31

**Artefacto Windows x86-64.** Preparar la receta PowerShell para `x86_64-pc-windows-msvc` y el staging de ambos `.exe` en la misma carpeta.

- **Propiedad:** `migration/demo/build/windows/**`.
- **Aceptación:** el staging con archivos ficticios funciona con rutas que contienen espacios. El build real compila cliente y servidor en runner Windows y el paquete contiene ambos `.exe`.

## D32

**Smoke Windows.** Preparar el recorrido en sesión de Windows con escritorio, desde una carpeta con espacios. Comprobar la miniatura y el cierre de la instancia propia y conservar una instancia externa.

- **Propiedad:** `migration/demo/smoke/windows/**`.
- **Aceptación:** el procedimiento registra versiones, ejecutables y PID, y comprueba que se libera el puerto. Una sesión de CI sin escritorio solo respalda el build. D34 registra ejecución en escritorio real o deja esa casilla pendiente.

## D33

**Ensamblaje común de Cargo.** Integrar los cuatro manifiestos, registrar paquetes y fijar el lockfile en el sprint. Proveer el arnés común de verificación de paquetes y señalar los módulos cuyo registro espera a D34.

- **Propiedad:** `rust/Cargo.toml`, `rust/Cargo.lock`, `migration/demo/harness/**`.
- **Aceptación:** `cargo metadata --locked --manifest-path rust/Cargo.toml` lista API, cliente, servidor y Slint. Los scaffolds compilan con comandos por paquete y la vía de verificación conserva las rutas previas del workspace. Publicar el SHA de referencia para los builds de feature.

## D34

**Ensamblaje final y aceptación del sprint.** Registrar los módulos, montar el router y conectar ambas entradas Slint al controlador. Construir los artefactos desde un checkout limpio, ejecutar la matriz acordada y publicar la evidencia.

- **Propiedad:** `rust/apps/photo-culler-server/src/demo/mod.rs`, `rust/apps/photo-culler-slint/src/demo/mod.rs`, `docs/BUILDS.md` y, una vez entregados por sus autores, los registros de módulos y puntos de entrada de servidor y Slint. Si se requiere un cambio de dependencias, coordinarlo con el propietario de D33.
- **Aceptación:** salud, galería y JPEG funcionan en un servidor real. Chromium y Firefox muestran la miniatura. Los clientes de macOS, Linux y Windows abren y se cierran con los artefactos, con comprobación del proceso y puerto. El arranque usa solo los artefactos Rust/Slint definidos. El registro incluye SHA, comandos, plataforma, versión, salida y capturas o logs relevantes. Si falta una ejecución gráfica, queda pendiente de aceptación y no se declara completada la plataforma. La decisión de promover el sprint se toma después de esta prueba.
