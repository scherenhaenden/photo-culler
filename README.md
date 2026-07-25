# photo-culler

> **High-Performance Automated Photo Culling & Modular Analysis Framework**

`photo-culler` is an open-source, decoupled, high-performance photo culling and technical assessment framework designed for high-volume photographers (concerts, portraits, sports, and events). 

It combines compiler-style analysis pipelines, RAW+JPEG logical file pairing, sparse and perceptual hashing, shoot timeline grouping, and multi-tier quality evaluation to streamline photo selection without altering original files.

---

## 📊 Estado de Desarrollo y Cobertura de Código (Coverage Table)

### 🎯 Cobertura Global de Código: **83.6%** (28 Pruebas Unitarias Superadas)

Below is the detailed test coverage report per package component:

| Módulo / Sub-paquete | Sentencias | Líneas Omitidas | Cobertura % |
|---|:---:|:---:|:---:|
| **`photo_culler/analysis/analyzers/technical/`** (Analyzers técnicos) | 260 | 6 | **97.7%** |
| **`photo_culler/analysis/engine/`** (Motor compilador & cache SQLite) | 201 | 29 | **84.5%** |
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
| **`photo_culler/scoring/`** (Scorers técnicos & RAW recovery) | 57 | 2 | **92.2%** |
| **`photo_culler/selection/`** (Reglas de decisión) | 29 | 13 | **48.8%** |
| **`photo_culler/volumes/`** (Detector de volúmenes) | 38 | 10 | **72.5%** |
| **TOTAL PROYECTO** | **1,679** | **214** | **83.6%** |

---

## 🌟 Key Architecture & Highlights

- **Compiler-Style Analysis Engine**: Every metric (sharpness, exposure, noise, clipping, motion blur) is calculated by an isolated, independent `Analyzer`. Analyzers measure without making culling decisions.
- **Persistent Metric Cache (SQLite)**: Intermediate raw measurements are never thrown away. If you refine your scoring algorithms 6 months later, scores can be dynamically recomputed from cached measurements in seconds without re-processing image files.
- **Strict Measurement vs. Scoring Separation**: Analyzers measure physics (e.g., Laplacian variance = 110.1, blown highlight % = 11.4%). Downstream Scorers and Decision Engines combine metrics based on customizable shoot profiles (e.g., `concert`, `portrait`, `crowd`).
- **Logical RAW + JPEG + Sidecar Pairing**: Automatically groups `DSC_1234.NEF`, `DSC_1234.JPG`, `DSC_1234.xmp`, and `DSC_1234.pp3` into a single logical `Photo` entity.
- **Volume & Storage Management**: Non-destructive, read-only camera card indexing with volume identity markers (`.photo-culler-volume.json`) and copy verification (`verify`).
- **Professional CLI**: Powered by `Typer`, `Rich`, `Pydantic`, and `SQLAlchemy` with colored tables, status panels, progress bars, global query selector (`PhotoSelector`), asset resolver (`AnalysisAssetResolver`), and standardized exit codes (0 to 10).

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

Para ejecutar la suite de 28 pruebas unitarias y calcular el reporte de cobertura:

```bash
# Cobertura en terminal con líneas faltantes
pytest --cov=photo_culler --cov-report=term-missing

# Generar informe HTML interactivo de cobertura
pytest --cov=photo_culler --cov-report=html
open htmlcov/index.html
```

---

## 🚀 Recommended Workflow

### Step 1: Initialize Catalog
Create local SQLite database and setup cache directories:

```bash
photo-culler init
```

### Step 2: Read-Only Camera Card Scan
Scan media directory to detect volume identity, pair RAW+JPEG files, extract metadata, and index photos into catalog:

```bash
photo-culler scan /media/edward/NIKON --quick --read-only
```

### Step 3: Group Shoot Sessions & Bursts
Cluster photos into chronological shoot sessions and high-speed burst sequences:

```bash
# Group sessions by 15-minute gap
photo-culler group --maximum-gap 15

# Detect burst sequences (gap <= 1.5s)
photo-culler bursts detect --maximum-gap 1.5
```

### Step 4: Run Technical Analysis
Execute compiler-style technical analysis pipeline (corruption, exposure, clipping, sharpness, motion blur, noise):

```bash
photo-culler analyze /media/edward/NIKON --profile fast
```

### Step 5: Evaluate Quality & Recoverability
Evaluate technical quality scores (0.0 - 1.0), RAW headroom recovery potential, and reject risks:

```bash
photo-culler evaluate --profile concert
```

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
