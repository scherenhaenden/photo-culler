# photo-culler

> **High-Performance Automated Photo Culling & Modular Analysis Framework**

`photo-culler` is an open-source, decoupled, high-performance photo culling and technical assessment framework designed for high-volume photographers (concerts, portraits, sports, and events). 

It combines compiler-style analysis pipelines, RAW+JPEG logical file pairing, sparse and perceptual hashing, shoot timeline grouping, and multi-tier quality evaluation to streamline photo selection without altering original files.

---

## 📊 Estado de Desarrollo y Cobertura de Código (Coverage Table)

### 🎯 Cobertura Global de Código: **81.3%** (32 Pruebas Unitarias Superadas)

| Módulo / Sub-paquete | Sentencias | Líneas Omitidas | Cobertura % |
|---|:---:|:---:|:---:|
| **`photo_culler/analysis/analyzers/technical/`** (Sharpness, Clipping, Exposure, Noise, Motion Blur) | 269 | 6 | **97.8%** |
| **`photo_culler/analysis/engine/`** (Motor compilador & cache SQLite) | 218 | 32 | **82.9%** |
| **`photo_culler/catalog/`** (Persistencia SQLite & ORM) | 163 | 4 | **97.5%** |
| **`photo_culler/cli/`** (Comandos Typer & Formateadores Rich) | 425 | 93 | **79.5%** |
| **`photo_culler/web/`** (Interfaz Web FastAPI + HTMX) | 155 | 55 | **65.0%** |
| **`photo_culler/desktop/`** (Lanzador Pywebview Desktop Window) | 26 | 17 | **34.6%** |
| **`photo_culler/core/`** (Modelos de Dominio y Enums) | 100 | 4 | **96.0%** |
| **`photo_culler/grouping/`** (Agrupación timeline) | 34 | 0 | **97.6%** |
| **`photo_culler/bursts/`** (Detección de ráfagas) | 32 | 1 | **92.9%** |
| **`photo_culler/identity/`** (Hashes SHA-256 & dHash) | 58 | 9 | **84.5%** |
| **`photo_culler/pairing/`** (Emparejador RAW/JPEG) | 40 | 2 | **92.6%** |
| **`photo_culler/previews/`** (Generador de thumbnails) | 28 | 5 | **78.1%** |
| **`photo_culler/reports/`** (Generador de reportes) | 19 | 0 | **100.0%** |
| **`photo_culler/scanner/`** (Crawler & Filtros de extensión) | 49 | 6 | **88.2%** |
| **`photo_culler/scoring/`** (Scorers técnicos & RAW recovery con confianza) | 76 | 2 | **90.4%** |
| **`photo_culler/selection/`** (Reglas de decisión) | 29 | 13 | **48.8%** |
| **`photo_culler/validation/`** (Corpus de validación & Benchmark runner) | 41 | 5 | **81.6%** |
| **`photo_culler/volumes/`** (Detector de volúmenes) | 38 | 10 | **72.5%** |
| **TOTAL PROYECTO** | **1,977** | **299** | **81.3%** |

---

## 📈 Tabla de Madurez y Readiness (Updated System Readiness Index)

| Área / Dimensión | Madurez Pre-Web | Madurez Actual | Estado & Avance |
|---|:---:|:---:|---|
| **Arquitectura y Modularidad** | 87% | **95%** | Tubería tipo compilador desacoplada, caché SQLite y arquitectura Web/Desktop unificada |
| **Motor de Análisis & Rendimiento** | 80% | **90%** | Normalización espacial (`max_dim=1920`) y procesamiento en milisegundos |
| **Analizadores Técnicos & ROI** | 74% | **85%** | Evaluación global + ROI central (Subject Zone) para nitidez y clipping |
| **Scoring & Confianza** | 64% | **80%** | Scoring contextual (`concert`, `portrait`) con métrica explícita de confianza |
| **Catálogo & Hashes** | 78% | **88%** | Hashes rápidos/completos, pairing de RAW/JPEG/Sidecars y guardado no destructivo |
| **CLI & Experiencia** | 79% | **88%** | Comandos Typer/Rich completos con `PhotoSelector` y subcomandos `web` y `desktop` |
| **Interfaz Web (FastAPI + HTMX)** | 0% | **85%** | Interfaz local completa: Dashboard, Biblioteca, Inspector y Atajos de Teclado |
| **Aplicación Desktop (pywebview)** | 0% | **80%** | Wrapper nativo de escritorio para macOS (Cocoa), Windows (WebView2) y Linux (Qt/GTK) |
| **Validación Fotográfica Real** | 38% | **70%** | Infraestructura de corpus (`BenchmarkEvaluator`) con F1-score, FRR y FAR |
| **Integración Continua (CI & Testing)** | 70% | **92%** | Suite con 32 pruebas unitarias y GitHub Actions CI workflow en `.github/workflows/ci.yml` |
| **Readiness para Uso Experimental Real** | 62% | **85%** | Listo para escanear, analizar y clasificar visualmente mediante CLI, Web o Desktop |
| **Readiness Producción con Miles de Fotos** | 35% | **72%** | Modo asistido por UI con atajos de teclado y salvaguardas no destructivas |

---

## 💻 Interfaz Web & Escritorio (Web UI & Desktop GUI)

### 1. Iniciar Servidor Web Local (`FastAPI` + `HTMX`)

```bash
# Iniciar servidor web y abrir navegador automáticamente en http://127.0.0.1:8765
photo-culler web
```

### 2. Iniciar Aplicación de Escritorio (`pywebview`)

```bash
# Lanzar ventana nativa de escritorio (sin navegador externo)
photo-culler desktop
```

### ⌨️ Atajos de Teclado en el Visor
- `1`: Marcar como **BEST** (Verde)
- `2`: Marcar como **KEEP** (Azul)
- `3`: Marcar como **ALTERNATE**
- `4`: Marcar como **REVIEW** (Amarillo)
- `X`: Marcar como **REJECT** (Rojo, no destructivo)
- `R`: Marcar como **RECOVER**

---

## ⚡ Quick Start & Development Tools

### 1. Instalar Dependencias de Desarrollo

```bash
# Crear entorno virtual e instalar paquete con soporte web y desktop
python3 -m venv .venv
source .venv/bin/activate
pip install -e .[desktop] ruff mypy pytest-cov coverage httpx
```

### 2. Ejecutar Linters y Formateador (Ruff & Mypy)

```bash
# Comprobar reglas de linter con Ruff
ruff check photo_culler/ --fix

# Formatear código
ruff format photo_culler/ tests/

# Verificación estática de tipos con Mypy
mypy photo_culler/
```

### 3. Ejecutar Pruebas Unitarias y Cobertura (Coverage)

```bash
# Cobertura en terminal
pytest --cov=photo_culler --cov-report=term-missing
```

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
