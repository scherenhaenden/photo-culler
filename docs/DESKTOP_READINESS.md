# Estado de las versiones de escritorio

## Decisión de esta entrega

Se terminó como camino recomendado **Linux fácil (ejecutable + ventana aislada de
Chrome/Chromium)**. “Terminado” aquí significa que el flujo de instalación y uso
experimental es completo: el ejecutable encuentra el navegador, crea el catálogo
fuera del directorio reemplazable del programa, levanta el servidor privado en un
puerto aleatorio, abre una ventana dedicada con token de sesión y apaga el servidor
al cerrar la ventana. No significa todavía un instalador firmado ni soporte para
Windows/macOS.

La decisión favorece el camino que ya reutiliza toda la interfaz web y que no obliga
a distribuir Qt/WebKit. Se añadió `PHOTO_CULLER_CHROME` para instalaciones donde el
navegador no está en `PATH`; un valor inválido produce ahora un error explícito. El
perfil del navegador es temporal, aislado y tiene la sincronización desactivada. Un
cierre anormal del navegador o un servidor que no se detiene ya no se reportan como
éxito.

## Análisis verificable de los dos caminos Python

Los porcentajes son una estimación de **readiness de entrega desktop**, no cobertura
de código. Se calculan con diez criterios de igual peso: UI utilizable, catálogo
persistente, importación, análisis, edición no destructiva, aislamiento de sesión,
cierre limpio, build reproducible, prueba unitaria del launcher y prueba end-to-end
del launcher. Cada criterio vale 10 puntos y sólo se acredita cuando existe código y
una comprobación automatizable en este repositorio.

| Criterio | Linux fácil (Chrome) | pywebview |
|---|:---:|:---:|
| UI web completa reutilizada | ✅ | ✅ |
| Catálogo persistente en datos de usuario | ✅ | ✅ |
| Importación desde carpeta | ✅ | ✅ |
| Análisis y decisiones | ✅ | ✅ |
| Edición no destructiva | ✅ | ✅ |
| Puerto aleatorio + token + perfil aislado | ✅ | Parcial: no controla perfil del motor |
| Cierre del servidor comprobado | ✅ | Parcial: cierre implementado, no verificado E2E |
| Build reproducible en CI | ✅ | ❌ |
| Tests unitarios del launcher | ✅ | Parcial |
| Test de proceso end-to-end | ✅ | ❌ |
| **Resultado** | **100% (10/10) para el alcance Linux experimental** | **65% (6 completos + 1 parcial)** |

Los “parciales” cuentan 2.5 puntos. El resultado del camino Linux no incluye tareas
fuera del alcance declarado: paquete `.deb`/AppImage, firma, actualizaciones,
asociación de archivos, sandbox nativo ni builds Windows/macOS. Por eso no debe
interpretarse como 100% de readiness comercial multiplataforma.

### Evidencia y comandos

- Unitarias del escritorio: `pytest -m 'not e2e' tests/test_desktop.py tests/test_linux_launcher.py`.
- End-to-end del launcher: `pytest -m e2e tests/e2e/test_linux_desktop_launcher.py`.
- Suite completa: `pytest`.
- Calidad estática: `ruff check photo_culler tests` y `ruff format --check photo_culler tests`.
- Build: `./scripts/build_linux.sh`; CI comprueba que `builds/linux/chromium/photo-culler` sea ejecutable.

El E2E inicia el launcher como proceso real. Un navegador controlado de prueba abre
la URL autenticada generada por el launcher, solicita el dashboard FastAPI real,
comprueba su contenido, sale y permite verificar el cierre limpio, el catálogo y el
log persistentes. No reemplaza el E2E existente en Chrome real, que cubre el flujo
completo de importar, generar miniatura, analizar y editar en el navegador.

## Uso del camino terminado

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -e '.[build]'
./scripts/build_linux.sh
./builds/linux/chromium/photo-culler
```

Si Chrome/Chromium no está en `PATH`:

```bash
PHOTO_CULLER_CHROME=/ruta/al/chromium ./builds/linux/chromium/photo-culler
```

El catálogo queda en `${XDG_DATA_HOME:-~/.local/share}/photo-culler/catalog.db` y el
log rotativo en `${XDG_STATE_HOME:-~/.local/state}/photo-culler/photo-culler.log`.

## Qué falta en pywebview

1. E2E de apertura y cierre de la ventana real en al menos Linux.
2. Build reproducible por plataforma y matriz CI.
3. Propagación explícita de fallos del motor gráfico y del cierre del servidor.
4. Pruebas del bridge nativo (seleccionar carpeta, guardar, fullscreen y revelar).
5. Decidir si el coste de Qt/WebKit mejora realmente la experiencia frente al build
   Linux fácil ya terminado.

## Estado de los caminos Rust

Tauri/WebGL (7% declarado) sigue siendo un bootstrap: sólo dibuja un canvas WebGL y
no abre el catálogo ni ejecuta importación/análisis.

egui/wgpu se recalculó a **100% de readiness funcional para el alcance Linux
experimental** con diez criterios de igual peso. Usa el API local versionado, sin
leer SQLite directamente, y su launcher empaquetado inicia/cierra el servicio local.

| Criterio egui/wgpu | Estado |
|---|:---:|
| Ventana nativa con backend wgpu | ✅ |
| Consulta de catálogo y galerías | ✅ |
| Importación persistente | ✅ |
| Vista de miniatura | ✅ |
| Inicio y control de análisis | ✅ |
| Decisiones no destructivas | ✅ |
| Sesiones y grupos | ✅ |
| Edición no destructiva | ✅ |
| Packaging independiente | ✅ |
| E2E de launcher nativo | ✅ |
| **Resultado** | **100% (10/10)** |

Tauri parece el candidato más corto para una futura entrega que reutilice la UI web;
egui requiere completar la paridad de interacción y el empaquetado antes de competir
como entrega de escritorio.

## Siguiente corte recomendado

Crear un artefacto AppImage o `.deb`, probarlo en una instalación Linux limpia y
añadir una prueba de ventana con Chrome/Chromium real. Después conviene congelar el
camino pywebview o justificarlo con una ventaja medida; mantener dos launchers Python
sin evidencia duplica packaging y soporte.
