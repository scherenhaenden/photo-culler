# photo-culler

> **High-Performance Automated Photo Culling & Modular Analysis Framework**

`photo-culler` is an open-source, decoupled, high-performance photo culling and technical assessment framework designed for high-volume photographers (concerts, portraits, sports, and events). 

It combines compiler-style analysis pipelines, RAW+JPEG logical file pairing, sparse and perceptual hashing, shoot timeline grouping, and multi-tier quality evaluation to streamline photo selection without altering original files.

---

## 📊 Estado de Desarrollo y Cobertura de Código (Coverage Table)

### 🎯 Cobertura Global de Código: **84.2%** (28 Pruebas Unitarias Superadas)

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
| **`photo_culler/volumes/`** (Detector de volúmenes) | 38 | 10 | **72.5%** |
| **TOTAL PROYECTO** | **1,713** | **207** | **84.2%** |

---

## 📈 Evaluación Honesta de Madurez (System Readiness Index)

| Dimensión | Madurez Arquitectura | Readiness Operativo | Descripción |
|---|:---:|:---:|---|
| **Arquitectura & Modularidad** | **85%** | **75%** | Tubería tipo compilador desacoplada, caché en SQLite y contratos limpios |
| **Motor de Análisis & Rendimiento** | **78%** | **65%** | Normalización espacial (1920px max) y procesamiento en milisegundos |
| **Analizadores Técnicos & ROI** | **70%** | **55%** | Evaluación global + ROI central (Subject Zone) para nitidez y clipping |
| **Scoring & Decisiones** | **60%** | **45%** | Puntuaciones contextuales (`concert`, `portrait`) con métrica explícita de confianza |
| **Catálogo, Hashes & Seguridad** | **80%** | **60%** | Hashes rápidos/completos, pairing de RAW/JPEG/Sidecars y guardado no destructivo |
| **CLI & Experiencia de Usuario** | **80%** | **65%** | Comandos Typer/Rich completos con `PhotoSelector` y `AnalysisAssetResolver` |
| **Validación Fotográfica Real** | **40%** | **30%** | Pendiente calibración sobre corpus real de conciertos y retratos |

---

## 🌟 Key Architecture & Highlights

- **Compiler-Style Analysis Engine**: Every metric (sharpness, exposure, noise, clipping, motion blur) is calculated by an isolated, independent `Analyzer`. Analyzers measure without making culling decisions.
- **Resolution Normalization**: Automatically rescales array sizes (`max_dim=1920`) in `AnalysisContext` for consistent, fast pixel density processing without overwhelming CPU/RAM on 45MP+ sensors.
- **Regional Subject Focus (ROI)**: Sharpness and clipping analyzers measure central subject zones (central 50% ROI) to prevent stage light clutter or background detail from distorting subject scores.
- **Persistent Metric Cache (SQLite)**: Intermediate raw measurements are never thrown away. If you refine your scoring algorithms 6 months later, scores can be dynamically recomputed from cached measurements in seconds without re-processing image files.
- **Strict Measurement vs. Scoring Separation**: Analyzers measure physics (e.g., Laplacian variance, blown highlight %). Downstream Scorers and Decision Engines combine metrics based on customizable shoot profiles (e.g., `concert`, `portrait`, `crowd`) and return explicit confidence scores.
- **Logical RAW + JPEG + Sidecar Pairing**: Automatically groups `DSC_1234.NEF`, `DSC_1234.JPG`, `DSC_1234.xmp`, and `DSC_1234.pp3` into a single logical `Photo` entity.
- **Volume & Storage Management**: Non-destructive, read-only camera card indexing with volume identity markers (`.photo-culler-volume.json`) and copy verification (`verify`).

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
