# Mapa tecnológico y rendimiento de escritorio

## Qué corre dónde

| Capa | Python | Rust |
|---|---|---|
| Dominio y catálogo | `core`, `catalog`, `identity`, `pairing`, `metadata` | Contratos iniciales en `rust/crates/photo-culler-core` |
| Importación y organización | `importing`, `scanner`, `grouping`, `bursts`, `sessions`, `volumes` | No hay implementación de producto aún |
| Análisis y decisión | `analysis`, `scoring`, `selection`, `learning`, `validation` | Sólo `AnalysisRequest`, `MetricResult` y el trait `AnalysisEngine` |
| Imagen y edición | `previews`, `editing`, `restoration`, `generative` | No hay procesadores de imagen aún |
| Aplicación y entrega | `cli`, `web`, `desktop`, `reports`, `config`, `logging` | CLI experimental y shells `tauri`/`egui` |

Python es hoy el motor de producto: catálogo SQLite, análisis, importación,
miniaturas, edición, sesiones, grupos y la API FastAPI. Rust no reemplaza esos
módulos; establece contratos compartidos y contiene prototipos de shells de UI.
Mover un analizador a Rust sólo está justificado cuando conserve el contrato y
demuestre una mejora medible de tiempo o memoria con el mismo corpus.

## Comparación de recursos de las superficies Linux

Estas cifras son **rangos orientativos**, no benchmarks del proyecto. Varían con
el driver, WebKit/Chromium instalado, tamaño de catálogo, resolución y número de
miniaturas. El análisis de fotos se ejecuta hoy en Python, por lo que su coste es
prácticamente común a todas las interfaces.

| Superficie | Memoria base esperada | Arranque/UI | Estado para uso |
|---|---:|---|---|
| Rust + egui + wgpu | ~80–250 MB | Menor overhead y render nativo | No tiene paridad ni entrega standalone |
| Rust + Tauri + WebKitGTK | ~200–500 MB | Menor que Chromium, motor del sistema | Prototipo; no recomendado todavía |
| Chromium aislado | ~400–900 MB | Más procesos y mayor huella | Entrega funcional recomendada |

La alternativa más ligera a largo plazo es egui/wgpu; la alternativa con mejor
equilibrio previsto entre paridad web y uso de recursos es Tauri; y Chromium es
la opción completa y verificable disponible hoy. Estas decisiones deben revisarse
con un benchmark que registre RAM pico, CPU, GPU, tiempo de arranque, latencia de
galería y corpus/catálogo utilizados.
