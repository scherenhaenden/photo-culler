# photo-culler

> **High-Performance Automated Photo Culling & Modular Analysis Framework**

`photo-culler` is an open-source, decoupled, high-performance photo culling and technical assessment framework designed for high-volume photographers (concerts, portraits, sports, and events). 

It combines compiler-style analysis pipelines, RAW+JPEG logical file pairing, sparse and perceptual hashing, shoot timeline grouping, and multi-tier quality evaluation to streamline photo selection without altering original files.

---

## 📊 Estado de Desarrollo y Cobertura (Progress Overview)

### 🎯 Resumen de Progreso: **100% de la Fase 1 (Núcleo Esencial)** | **~65% de la Hoja de Ruta Completa**

| # | Módulo / Componente | Descripción | Estado | Progreso % |
|---|---|---|---|:---:|
| 1 | **`core/`** | Modelos de dominio (`Photo`, `FileRecord`, `VolumeRecord`, `SessionRecord`, `BurstGroup`), Enums y Contratos | Completado | **100%** |
| 2 | **`catalog/`** | Motor SQLite, Esquemas ORM SQLAlchemy (`photos`, `files`, `volumes`, `metadata`, `sessions`), `PhotoRepository` | Completado | **100%** |
| 3 | **`volumes/`** | Detector de puntos de montaje, uso de disco y archivo marcador `.photo-culler-volume.json` | Completado | **100%** |
| 4 | **`scanner/`** | Escáner recursivo de directorios, clasificador de extensiones (`NEF`, `CR2`, `ARW`, `JPEG`, `XMP`, `PP3`) | Completado | **100%** |
| 5 | **`identity/`** | Hashes rápidos (Quick SHA-256 64KB), Hash completo (Full SHA-256) y Hash Perceptual (`dHash`) | Completado | **100%** |
| 6 | **`metadata/`** | Extractor EXIF (Fecha, Subsegundos, Cámara, Objetivo, ISO, Apertura, Velocidad, Distancia focal, Orientación) | Completado | **100%** |
| 7 | **`pairing/`** | Emparejador lógico de capturas (`RAW` + `JPEG` + `XMP` / `PP3`) | Completado | **100%** |
| 8 | **`previews/`** | Generador de miniaturas multirresolución (`256px`, `800px`, `1600px`, `3200px`) y caché en disco | Completado | **100%** |
| 9 | **`grouping/`** | Detector de sesiones de disparo por línea de tiempo (`timeline.py`) | Completado | **100%** |
| 10 | **`bursts/`** | Detector de ráfagas continuas de alta velocidad (`temporal_bursts.py`) | Completado | **100%** |
| 11 | **`analysis/engine/`** | Motor tipo compilador: `Analyzer` ABC, `AnalysisContext`, `AnalysisResult`, `Registry`, `MetricCache` (SQLite), `Pipeline` | Completado | **100%** |
| 12 | **`analysis/analyzers/technical/`** | Analizadores técnicos Tier 1 (`corruption`, `dimensions`, `histogram`, `clipping`, `exposure`, `sharpness`, `motion_blur`, `noise`) | Completado | **100%** |
| 13 | **`scoring/`** | Evaluador técnico (`technical_score.py`) y recuperabilidad RAW (`recoverability_score.py`) | Completado | **100%** |
| 14 | **`selection/`** | Detector de redundancias y motor de reglas de decisión (`KEEP`, `BEST`, `ALTERNATE`, `REJECT_TECHNICAL`, etc.) | Completado | **90%** |
| 15 | **`cli/`** | CLI avanzada con Typer + Rich + Pydantic (`PhotoSelector`, `AnalysisAssetResolver`, códigos de salida 0-10 y 14 subcomandos) | Completado | **100%** |
| 16 | **`reports/`** | Generador de informes (`summary_report.py`) y formateadores (`human`, `json`, `csv`) | Completado | **100%** |
| 17 | **`tests/`** | Suite de pruebas unitarias y CLI con Pytest (23 pruebas superadas) | Completado | **100%** |
| 18 | **`config/`** | Configuración YAML y perfiles de disparo (`concert.yaml`, `portrait.yaml`) | Base Lista | **50%** |
| 19 | **`logging/`** | Registro estructurado y diagnósticos de rendimiento | Base Lista | **50%** |
| 20 | **`jobs/`** | Cola de trabajos en segundo plano y programador | Estructura Lista | **30%** |
| 21 | **`plugins/`** | Registro de plugins y cargador dinámico | Estructura Lista | **20%** |
| 22 | **`editing/`** | Integración con perfiles de revelado RawTherapee (.pp3) y darktable (.xmp) | Estructura Lista | **10%** |
| 23 | **`restoration/`** | Adaptadores de IA para denoise, deblur y escalado (Real-ESRGAN) | Estructura Lista | **10%** |
| 24 | **`generative/`** | Seguimiento de ediciones generativas y procedencia de imágenes | Estructura Lista | **10%** |
| 25 | **`interchange/`** | Sincronización XMP/DigiKam de estrellas, etiquetas y colores | Estructura Lista | **10%** |
| 26 | **`analysis/analyzers/vision/`** | Detectores IA visuales (personas, caras, landmarks, instrumentos, oclusiones) | Fase 2 | **0%** |
| 27 | **`analysis/analyzers/embeddings/`** | Codificador de imágenes, vector store y búsqueda semántica (FAISS) | Fase 2 | **0%** |
| 28 | **`analysis/analyzers/quality/`** | Modelos de calidad estética (MUSIQ / NIMA) | Fase 2 | **0%** |
| 29 | **`learning/`** | Feedback de preferencias del usuario y modelos de ranking (LightGBM/XGBoost) | Fase 2 | **0%** |
| 30 | **`web/`** | Interfaz Web local con FastAPI | Fase 3 | **0%** |

---

## 🌟 Key Architecture & Highlights

- **Compiler-Style Analysis Engine**: Every metric (sharpness, exposure, noise, clipping, motion blur) is calculated by an isolated, independent `Analyzer`. Analyzers measure without making culling decisions.
- **Persistent Metric Cache (SQLite)**: Intermediate raw measurements are never thrown away. If you refine your scoring algorithms 6 months later, scores can be dynamically recomputed from cached measurements in seconds without re-processing image files.
- **Strict Measurement vs. Scoring Separation**: Analyzers measure physics (e.g., Laplacian variance = 110.1, blown highlight % = 11.4%). Downstream Scorers and Decision Engines combine metrics based on customizable shoot profiles (e.g., `concert`, `portrait`, `crowd`).
- **Logical RAW + JPEG + Sidecar Pairing**: Automatically groups `DSC_1234.NEF`, `DSC_1234.JPG`, `DSC_1234.xmp`, and `DSC_1234.pp3` into a single logical `Photo` entity.
- **Volume & Storage Management**: Non-destructive, read-only camera card indexing with volume identity markers (`.photo-culler-volume.json`) and copy verification (`verify`).
- **Professional CLI**: Powered by `Typer`, `Rich`, `Pydantic`, and `SQLAlchemy` with colored tables, status panels, progress bars, global query selector (`PhotoSelector`), asset resolver (`AnalysisAssetResolver`), and standardized exit codes (0 to 10).

---

## 📁 Repository & Module Layout

The codebase is organized into modular, independent sub-packages under `photo_culler/`:

```text
photo-culler/
├── photo_culler/
│   ├── core/              # Domain models (Photo, FileRecord, VolumeRecord, SessionRecord, BurstGroup)
│   ├── catalog/           # SQLite database schema, SQLAlchemy ORM models, and PhotoRepository
│   ├── volumes/           # Disk volume detection and .photo-culler-volume.json marker persistence
│   ├── scanner/           # Fast recursive directory crawler and extension classifier
│   ├── identity/          # Sparse Quick SHA-256, Full SHA-256, and perceptual dHash hashing
│   ├── metadata/          # EXIF & camera metadata extraction (ISO, Aperture, Shutter Speed, Lens, Date)
│   ├── pairing/           # RAW + JPEG + Sidecar logical file pairer
│   ├── previews/          # Multi-resolution preview thumbnail cache (256px, 800px, 1600px, 3200px)
│   ├── grouping/          # Chronological timeline and shoot session clustering
│   ├── bursts/            # High-speed sequence burst detector
│   ├── analysis/          # Modular Analysis Framework
│   │   ├── engine/        # Analyzer ABC, AnalysisContext, AnalysisResult, Registry, MetricCache, Pipeline
│   │   └── analyzers/    # Independent analyzers (corruption, dimensions, histogram, clipping, exposure, sharpness, motion_blur, noise)
│   ├── scoring/           # Technical quality scorers & RAW recoverability scorers
│   ├── selection/         # Redundancy, coverage, and decision engine rules
│   ├── learning/          # User preference learning & ranking models
│   ├── jobs/              # Background worker queue & task scheduler
│   ├── editing/           # RawTherapee (.pp3) & darktable (.xmp) profile integration
│   ├── restoration/       # AI denoise, deblur, sharpening, and upscaling adapters
│   ├── generative/        # Generative AI edit tracking & provenance
│   ├── cli/               # Typer + Rich + Pydantic CLI layer
│   │   ├── app.py         # Main CLI application entry point
│   │   ├── context.py     # Global CliContext dataclass
│   │   ├── output.py      # Rich Console formatting wrapper
│   │   ├── exit_codes.py  # Standardized exit status codes (0 to 10)
│   │   ├── commands/      # init, doctor, scan, verify, volumes, photos, analyze, evaluate, group, bursts, sessions, decisions, report, config
│   │   └── helpers/       # PhotoSelector & AnalysisAssetResolver
│   ├── web/               # FastAPI local Web UI backend
│   ├── reports/           # HTML, JSON, CSV, Contact Sheet report generators
│   ├── interchange/       # XMP rating, DigiKam, and CSV sidecar sync
│   ├── config/            # Environment & shoot profiles (concert.yaml, portrait.yaml)
│   ├── logging/           # Structured audit logging & performance diagnostics
│   └── plugins/           # Extension registry & loader
├── tests/                 # Comprehensive Pytest test suite
├── pyproject.toml         # Packaging & dependency configuration
└── README.md
```

---

## ⚡ Quick Start

### 1. Installation

Requires **Python 3.9+**.

```bash
# Clone repository
git clone https://github.com/edwardflores/photo-culler.git
cd photo-culler

# Create virtual environment and install dependencies
python3 -m venv .venv
source .venv/bin/activate
pip install -e .
```

### 2. Environment Diagnostics

Run `doctor` to verify Python version, SQLite integrity, and installed tool dependencies:

```bash
photo-culler doctor
```

Output:
```text
                  Photo Culler Environment Diagnostics                  
┏━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━┓
┃ Component         ┃ Details                             ┃ Status     ┃
┡━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━┩
│ Python Version    │ 3.11.6                              │ OK         │
│ OS Platform       │ Darwin 24.1.0                       │ OK         │
│ Catalog Database  │ catalog.db                          │ OK         │
│ ExifTool          │ Installed                           │ INFO       │
│ libvips           │ Installed                           │ INFO       │
│ RawTherapee       │ Installed                           │ INFO       │
└───────────────────┴─────────────────────────────────────┴────────────┘
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

Output:
```text
                 Evaluation Report (Profile: CONCERT)                  
┏━━━━━━━━━━┳━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━┳━━━━━━━━━━━┓
┃ Photo    ┃ Tech Quality ┃ RAW Recoverability ┃ Decision  ┃ Tier      ┃
┡━━━━━━━━━━╇━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━╇━━━━━━━━━━━┩
│ DSC_4812 │ 82           │ 91%                │ BEST      │ EXCELLENT │
│ DSC_4813 │ 37           │ 44%                │ REJECT    │ POOR      │
│ DSC_4814 │ 64           │ 83%                │ ALTERNATE │ GOOD      │
└──────────┴──────────────┴────────────────────┴───────────┴───────────┘
```

### Step 6: Apply Decisions & Generate Report
Review decisions and emit summary culling report:

```bash
photo-culler report summary
```

Output:
```text
              Photo Culling Summary Report              
┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ Metric                      ┃ Value                  ┃
┡━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━┩
│ Total Photos                │ 2,406                  │
│ RAW Files                   │ 2,406                  │
│ JPEG Files                  │ 2,406                  │
│ Kept Photos                 │ 1,812                  │
│ Rejected Photos             │ 594                    │
│ Keep Rate                   │ 75.3%                  │
└─────────────────────────────┴────────────────────────┘
```

### Step 7: Verify Backup Copies
Verify file copy completeness against backup drive using full streaming SHA-256 hashes:

```bash
photo-culler verify /media/edward/T7/Wacken2026 --against /media/edward/NIKON
```

---

## 🛠️ Global CLI Options & Query Selectors

### Global Flags
```bash
photo-culler [GLOBAL_OPTIONS] COMMAND [ARGS]...

  --catalog -c PATH   Specify custom SQLite catalog database path [default: catalog.db]
  --json              Format output as structured JSON
  --csv               Format output as CSV
  --quiet -q          Suppress terminal output
  --verbose -v        Enable verbose logging
  --dry-run           Simulate actions without committing database changes
  --no-color          Disable ANSI color styling
```

### `PhotoSelector` Query Parameters
Commands like `photos`, `analyze`, `evaluate`, and `decisions` accept flexible selection filters:

```bash
# Filter by session
photo-culler analyze --session "Iron Maiden"

# Filter by time range
photo-culler photos --from "2026-08-01 18:00" --to "2026-08-01 19:00"

# Filter by culling decision
photo-culler photos --decision review

# Filter by photo ID or hash
photo-culler photos --photo-id photo_abc123
```

---

## 🔬 Analysis Engine Architecture

### Standardized `Analyzer` Interface
Every analyzer inherits from `Analyzer` and returns `AnalysisResult`:

```python
class Analyzer(ABC):
    name: str = "sharpness"
    version: str = "1.0"
    category: str = "technical"
    enabled_by_default: bool = True

    @abstractmethod
    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        pass
```

### Standardized `AnalysisResult` JSON Schema
```json
{
  "analyzer": "sharpness",
  "version": "1.0",
  "metrics": {
    "global_sharpness": 0.74,
    "laplacian_variance": 245.8,
    "gradient_energy": 310.2,
    "edge_density": 0.048,
    "fft_high_freq_ratio": 0.712,
    "is_tack_sharp": true
  },
  "confidence": 0.94,
  "execution_time_ms": 12.4
}
```

---

## 🧪 Testing & Verification

Run unit test suite via `pytest`:

```bash
.venv/bin/pytest tests/
```

```text
============================= test session starts ==============================
collected 23 items

tests/test_catalog.py .                                                  [  4%]
tests/test_cli.py ...                                                    [ 17%]
tests/test_engine.py ....                                                [ 34%]
tests/test_grouping_and_bursts.py ..                                     [ 43%]
tests/test_hashing.py ..                                                 [ 52%]
tests/test_scanner_and_pairing.py ..                                     [ 60%]
tests/test_scoring.py ...                                                [ 73%]
tests/test_technical_analyzers.py ......                                 [100%]

======================== 23 passed in 0.45s ========================
```

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
