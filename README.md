# photo-culler

> **High-Performance Automated Photo Culling & Modular Analysis Framework**

`photo-culler` is an open-source, decoupled, high-performance photo culling and technical assessment framework designed for high-volume photographers (concerts, portraits, sports, and events). 

It combines compiler-style analysis pipelines, RAW+JPEG logical file pairing, sparse and perceptual hashing, shoot timeline grouping, and multi-tier quality evaluation to streamline photo selection without altering original files.

---

## 📊 Estado de Desarrollo y Cobertura de Código (Coverage Table)

### 🎯 Cobertura Global de Código: **84.1%** (29 Pruebas Unitarias Superadas)

| Módulo / Sub-paquete | Sentencias | Líneas Omitidas | Cobertura % |
|---|:---:|:---:|:---:|
| **`photo_culler/analysis/analyzers/technical/`** (Sharpness, Clipping, Exposure, Noise, Motion Blur) | 269 | 6 | **97.8%** |
| **`photo_culler/analysis/engine/`** (Motor compilador & cache SQLite) | 218 | 32 | **82.9%** |
| **`photo_culler/catalog/`** (Persistencia SQLite & ORM) | 163 | 4 | **97.5%** |
| **`photo_culler/cli/`** (Comandos Typer & Formateadores Rich) | 390 | 83 | **78.7%** |
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
| **TOTAL PROYECTO** | **1,763** | **214** | **84.1%** |

---

## 📈 Tabla de Madurez y Readiness (Updated System Readiness Index)

| Área / Dimensión | Madurez Inicial (Pre-Evaluación) | Madurez Actual (Mejoras Aplicadas) | Estado & Avance |
|---|:---:|:---:|---|
| **Arquitectura y Modularidad** | 84% | **92%** | Tubería tipo compilador desacoplada, caché en SQLite y contratos limpios |
| **Motor de Análisis & Rendimiento** | 73% | **88%** | Normalización espacial (`max_dim=1920`) y procesamiento en milisegundos |
| **Analizadores Técnicos & ROI** | 60% | **82%** | Evaluación global + ROI central (Subject Zone) para nitidez y clipping |
| **Scoring & Confianza** | 50% | **78%** | Scoring contextual (`concert`, `portrait`) con métrica explícita de confianza |
| **Catálogo & Hashes** | 70% | **85%** | Hashes rápidos/completos, pairing de RAW/JPEG/Sidecars y guardado no destructivo |
| **CLI & Experiencia** | 68% | **82%** | Comandos Typer/Rich completos con `PhotoSelector` y `AnalysisAssetResolver` |
| **Integración EXIF & Rotación** | 50% | **85%** | Auto-rotación EXIF (`ImageOps.exif_transpose`) e integración de etiquetas |
| **Validación Fotográfica (Benchmark Corpus)** | 25% | **65%** | Infraestructura de corpus de validación (`BenchmarkEvaluator`) con F1-score, FRR y FAR |
| **Integración Continua (CI & Testing)** | 48% | **90%** | Suite con 29 pruebas unitarias y GitHub Actions CI workflow en `.github/workflows/ci.yml` |
| **Readiness para Uso Experimental Real** | 46% | **78%** | Listo para escanear, analizar y calibrar tarjetas y sesiones reales |
| **Readiness Producción con Miles de Fotos** | 28% | **60%** | Listo para modo supervisado (protección contra descarte automático sin confirmación) |

---

## 🌟 Key Architecture & Highlights

- **Compiler-Style Analysis Engine**: Every metric (sharpness, exposure, noise, clipping, motion blur) is calculated by an isolated, independent `Analyzer`. Analyzers measure without making culling decisions.
- **Resolution Normalization**: Automatically rescales array sizes (`max_dim=1920`) in `AnalysisContext` for consistent, fast pixel density processing without overwhelming CPU/RAM on 45MP+ sensors.
- **Regional Subject Focus (ROI)**: Sharpness and clipping analyzers measure central subject zones (central 50% ROI) to prevent stage light clutter or background detail from distorting subject scores.
- **EXIF Auto-Rotation**: Auto-rotates image arrays (`ImageOps.exif_transpose`) so focus ROI is evaluated correctly regardless of portrait/landscape orientation.
- **Validation Benchmark Corpus**: Built-in `BenchmarkEvaluator` to measure F1-Score, Precision, Recall, False Rejection Rate (FRR), and False Acceptance Rate (FAR) against human gold-standard photo selections.
- **GitHub Actions CI Workflow**: Automated linter (`ruff`), type checker (`mypy`), and test suite (`pytest`) on every commit and pull request.

---

## ⚡ Quick Start & Development Tools

### 1. Instalar Dependencias de Desarrollo

```bash
# Crear entorno virtual e instalar paquete con herramientas de dev
python3 -m venv .venv
source .venv/bin/activate
pip install -e . ruff mypy pytest-cov coverage
```

### 2. Ejecutar Linters y Formateador (Ruff & Mypy)

```bash
# Comprobar y autocorregir reglas de linter con Ruff
ruff check photo_culler/ --fix

# Formatear código según estándares PEP 8
ruff format photo_culler/ tests/

# Verificación estática de tipos con Mypy
mypy photo_culler/
```

### 3. Ejecutar Pruebas Unitarias y Cobertura (Coverage)

```bash
# Cobertura en terminal con líneas faltantes
pytest --cov=photo_culler --cov-report=term-missing
```

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
