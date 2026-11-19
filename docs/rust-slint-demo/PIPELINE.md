# Pipeline de las 34 microtareas

Las 34 fichas quedan en estado **lista para revisión**. Una vez aprobadas, el coordinador las envía individualmente en el orden de esta tabla. Puede dejar solicitudes en cola para toda la lista. La columna «Integrar tras» indica cuándo hay base suficiente para conectar una entrega al sprint. Una ficha con dependencias puede empezar antes con el contrato escrito y los dobles de su ficha.

## Orden de envío

Los enlaces abren la ficha completa. Los IDs de cada fila son requisitos directos. Sus dependencias se heredan.

| Envío | Ficha | Frente | Preparación en paralelo | Integrar tras |
|---:|---|---|---|---|
| 01 | [D01](TASKS.md#d01) Contrato REST | Fundaciones | Fijar DTO, rutas, fixtures de respuesta y firmas. | — |
| 02 | [D02](TASKS.md#d02) Galería incluida | Datos | Generar JPEG sintético, miniatura y manifiesto. | D01 |
| 03 | [D03](TASKS.md#d03) Paquete servidor | Servidor | Preparar paquete y entrada con estado de scaffolding explícito. | D01 |
| 04 | [D10](TASKS.md#d10) Paquete cliente | Cliente | Preparar Cargo y transporte síncrono simulado mediante la interfaz asíncrona. | D01 |
| 05 | [D11](TASKS.md#d11) Paquete Slint | UI | Compilar el catálogo de componentes existentes con ventana provisional. | D10 |
| 06 | [D33](TASKS.md#d33) Workspace | Integración | Preparar registro y reproducibilidad. Generar el lockfile al tener los cuatro manifiestos. | D01, D03, D10, D11 |
| 07 | [D04](TASKS.md#d04) Socket y ciclo HTTP | Servidor | Probar con router local de una sola ruta. | D33 |
| 08 | [D05](TASKS.md#d05) Salud | Servidor | Probar el adaptador con sonda inyectada. | D33 |
| 09 | [D06](TASKS.md#d06) Lectura de galería | Servidor | Usar las respuestas normales y vacías del contrato. | D02, D33 |
| 10 | [D07](TASKS.md#d07) Miniaturas | Servidor | Probar con bytes JPEG incorporados. | D02, D33 |
| 11 | [D08](TASKS.md#d08) Política de entrada | Servidor | Probar con router y puerto falsos. | D04 |
| 12 | [D09](TASKS.md#d09) Recursos web | Servidor | Probar con distribución mínima simulada. | D04 |
| 13 | [D12](TASKS.md#d12) Ventana principal | UI | Compilar con filas estáticas y estados precargados. | D33 |
| 14 | [D13](TASKS.md#d13) Controlador | UI | Probar con respuestas simuladas y desordenadas. | D12 |
| 15 | [D14](TASKS.md#d14) Adaptador de JPEG | UI | Probar con bytes válidos, truncados o fuera de límites. | D02, D33 |
| 16 | [D15](TASKS.md#d15) Transporte nativo | Escritorio | Usar servidor HTTP de prueba. | D33 |
| 17 | [D28](TASKS.md#d28) Portabilidad del launcher | Escritorio | Probar rutas, procesos y nombres de binario con dobles. | D33 |
| 18 | [D16](TASKS.md#d16) Arranque nativo | Escritorio | Usar un hijo ficticio que publique el protocolo acordado. | D04, D13, D15, D28 |
| 19 | [D17](TASKS.md#d17) Cierre del hijo | Escritorio | Usar procesos controlados, incluido uno que no responde. | D16 |
| 20 | [D18](TASKS.md#d18) Artefacto macOS | Escritorio | Preparar receta de staging con binarios de prueba. | D17 |
| 21 | [D19](TASKS.md#d19) Smoke macOS | Aceptación nativa | Escribir guion y practicar con un paquete temporal. | D18 |
| 22 | [D20](TASKS.md#d20) Toolchain WASM | Web | Preparar receta y comprobación de versiones. | D33 |
| 23 | [D21](TASKS.md#d21) Fetch en Rust | Web | Usar las respuestas JSON/JPEG del contrato. | D20 |
| 24 | [D22](TASKS.md#d22) Canvas y cargador | Web | Probar el cargador con un módulo WASM mínimo. | D13, D14, D20, D21 |
| 25 | [D23](TASKS.md#d23) Distribución web | Web | Preparar staging con un bundle de prueba. | D09, D22 |
| 26 | [D24](TASKS.md#d24) Smoke Chromium | Aceptación web | Preparar checks con una página y API simuladas. | D23 |
| 27 | [D25](TASKS.md#d25) Smoke Firefox | Aceptación web | Preparar la misma cobertura en Firefox. | D23 |
| 28 | [D29](TASKS.md#d29) Artefacto Linux | Linux | Preparar receta y dependencias del runner. | D17 |
| 29 | [D30](TASKS.md#d30) Smoke Linux | Linux | Preparar guion y entorno gráfico de prueba. | D29 |
| 30 | [D31](TASKS.md#d31) Artefacto Windows | Windows | Preparar receta PowerShell y runner x86-64. | D17 |
| 31 | [D32](TASKS.md#d32) Smoke Windows | Windows | Preparar guion, verificación de proceso y repetición. | D31 |
| 32 | [D26](TASKS.md#d26) Matriz CI | Entrega | Preparar el workflow con entradas y salidas ya fijadas. | D18, D23, D29, D31 |
| 33 | [D27](TASKS.md#d27) Arranque sin Python | Entrega | Escribir comandos y procedimiento de verificación. | D19, D24, D25, D30, D32 |
| 34 | [D34](TASKS.md#d34) Ensamblaje y aceptación | Integración | Preparar matriz de evidencias y lista de cambios de registro. | D05–D08, D17, D26, D27 |

## En paralelo con cuatro responsables

1. **Fundación e integración:** D01, revisión de los cuatro manifiestos y D33. Reserva D34 para cerrar el sprint.
2. **Servidor:** D03 y D04–D09. Puede empezar con router y repositorio simulados mientras se registra el workspace.
3. **Cliente y escritorio:** D10–D17 y D28. UI puede empezar con respuestas de muestra; launcher y transporte se prueban por separado.
4. **Web y automatización:** D20–D25. Apenas estén definidos los comandos de staging, repartir D18/D19, D29/D30 y D31/D32 entre responsables con acceso a cada plataforma. D26/D27 pueden prepararse en paralelo con los guiones de prueba.

D02 se puede asignar al primer responsable disponible desde el comienzo. Se puede despachar toda la cola desde el primer día bajo la capacidad disponible. D33 desbloquea el registro común, y D34 une las líneas de servidor, escritorio y web. Las fichas de smoke pueden entregar guiones comprobados con dobles antes de D34. Solo D34 les da estado de «verificada en la aplicación real». Las pruebas unitarias y de contrato se hacen por feature. La comprobación integral se hace en el sprint antes de promoverlo a `master`.

## Propiedad de archivos

- Un worktree y un escritor por ficha. Aplicar solo los cambios de la ficha. Adjuntar rutas a registrar y dependencias adicionales si aparecen.
- D01, D03, D10 y D11 crean los manifiestos locales de sus paquetes. D33 controla `rust/Cargo.toml` y `rust/Cargo.lock`, incluidos los ajustes de dependencias que se acuerden durante el sprint. D11 fija una versión de Slint compatible en coordinación con D20. D20 deriva la versión de `wasm-bindgen` del lockfile.
- Durante las entregas aisladas, los módulos pueden ejercitarse con un arnés temporal. D34 registra y cablea esos módulos en los puntos de entrada definitivos. Es el único responsable de editar otra vez los archivos raíz de paquetes que ya haya entregado su propietario. Si un archivo todavía tiene un escritor activo, el integrador espera su entrega.
- D26 posee el workflow de la demo. Los builds por plataforma aportan recetas por separado y D26 las invoca. D27 aporta la guía de arranque. D34 escribe las evidencias reales en `docs/BUILDS.md`.

## Despacho y revisión

Mensaje para cada tarea, una vez aprobado:

> Implementa solo la ficha **DXX** del plan Rust y Slint de Photo Culler. Usa la rama base del sprint indicada por el coordinador y un worktree propio. Sigue el contrato y la lista de rutas de la ficha. Usa dobles cuando un vecino siga en cola. Ejecuta las pruebas de la ficha y entrega rama, commit, archivos, comandos, salidas relevantes y límites. El PR apunta a `feature/sprint-01-rust-foundation`. Registra cualquier dependencia pendiente antes de pedir revisión.

Usar una rama `feature/rust-slint-demo-dxx-<tema>` por ficha. Si una ficha se implementa antes de que entren sus dependencias, documentar el SHA del arnés o de la base que permitió probarla. El PR puede quedar en borrador hasta tener una base adecuada. Estados sugeridos: **lista para revisión**, **en cola**, **en trabajo**, **PR abierto**, **integrada**, **verificada en artefactos reales**. Si cambia el contrato, comunicar el cambio a los consumidores y registrar qué SHA contiene la revisión. Anotar una plataforma sin runner gráfico como pendiente de aceptación hasta que se ejecute el recorrido, aunque compile.
