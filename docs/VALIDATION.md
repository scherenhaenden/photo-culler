# Protocolo de validación fotográfica y rendimiento

## Propósito y estado

Este protocolo fija cómo comparar versiones y cómo reportar errores de selección.
Hoy existe infraestructura Python para evaluar decisiones humanas contra
`GroundTruthLabel` y calcular accuracy, precision, recall, F1, tasa de falsos
rechazos (FRR) y tasa de falsas aceptaciones (FAR). No hay un runner común listo
que produzca el informe final versionado desde un manifiesto ni un motor de
análisis Rust con paridad. La CLI Rust no debe presentarse como evaluador completo.
Los comandos de abajo describen la evaluación Python actual; el contrato del
manifiesto proporciona la entrada reproducible para la futura comparación Rust.

## Corpus, derechos y manifiesto

Mantenga un corpus privado versionado por contenido o un identificador de dataset,
sin subir imágenes con datos personales. Para cada imagen incluya en el manifiesto
CSV/JSON versionado:

- `photo_id` estable, ruta relativa dentro de corpus y SHA-256.
- SHA-256 del RAW y JPEG emparejados cuando ambos existan; modelo de cámara,
  formato, software de revelado y procedencia autorizada.
- Etiqueta humana por dimensión: `technical_keep`, `technical_reject`,
  `artistic_keep`, `review`; anote por separado RAW recuperable,
  movimiento intencional, ojo de pez y luces de escenario.
- Razón concisa, severidad del error y anotador. No use una única etiqueta
  “mala” para decisiones estéticas y fallos técnicos.
- Revisión del manifiesto, fecha, licencia/consentimiento, criterios de inclusión
  y exclusión y reglas de partición.

No incluya metadatos identificativos en reportes compartidos. Deduplicate por hash
y mantenga sesiones/burst completas en la misma partición para que fotogramas casi
idénticos no filtren entre train/tuning y holdout. El holdout humano queda congelado
antes de comparar versiones.

Incluya como mínimo: ráfagas y duplicados exactos/cercanos; ojo de pez; movimiento
intencional (bailarín/instrumentista); luces de escenario con clipping y color
extremo; desenfoque accidental; subexposición; ojos cerrados si aplica; pares
RAW/JPEG y RAW con detalle recuperable; RAW corrupto/decodificador no disponible;
formatos y cámaras representativos. Los fixtures sintéticos ayudan a probar
ejecución y casos de fallo, pero no sustituyen ejemplos fotográficos autorizados.

## Anotación y desacuerdos

Use dos anotadores independientes que no vean la puntuación del producto. Indique
para cada uno el propósito del conjunto (selección técnica, preferencia editorial
o conservación artística). Adjudique discrepancias con un tercer revisor o una
sesión de consenso documentada. Conserve ambas etiquetas originales, etiqueta
adjudicada, razón, quién y cuándo; no elimine desacuerdos silenciosamente.

La decisión final “keep” debe significar que la persona aceptaría conservar esa
imagen bajo el propósito declarado. Un movimiento intencional no es defecto de
nitidez por defecto; ojo de pez no es distorsión no deseada por defecto; luces
recortadas pueden ser propias de una escena; y un RAW con altas luces recuperables
no debe rechazarse solo porque el JPEG de cámara parece recortado. Marque
`REVIEW` cuando la intención no pueda inferirse del píxel.

Informe acuerdo entre anotadores (conteos y porcentaje, y Cohen kappa si las
etiquetas son nominales y comparables). Muestre el acuerdo por clase y caso
especial; no use una cifra global para ocultar conflicto sistemático.

## Coste de errores y gates

Priorice una falsa eliminación: FRR = positivos humanos (keep) que el sistema
rechaza / todos los positivos humanos. FAR = negativos humanos que el sistema
acepta / todos los negativos humanos. Mantenga también precision, recall, F1,
matriz de confusión y cobertura; `REVIEW` cuenta como decisión no automática y se
reporta aparte, además de presentar el cálculo binario con la regla declarada.

Defina antes del experimento límites por clase para FRR y FAR junto con el
responsable editorial; no invente un umbral después de ver resultados. El coste de
perder una toma irrepetible pesa más que revisar un duplicado. Un candidato no
puede ascender si excede el límite de falsos rechazos, aunque mejore velocidad o
F1 agregado. No hay umbrales universales acordados todavía: registre el gate
aprobado y su responsable en cada plan de prueba.

Cualquier cambio de analizador, modelo, umbral o política debe correr contra el
mismo manifiesto y holdout. Versione corpus, etiqueta, configuración, código y
entorno. Mantenga comparaciones por cámara, formato, clase y dificultad, no solo
promedios.

## Fixture sintético para probar el reporte

Antes de usar fotos, pruebe el formato del manifiesto con cuatro archivos de color.
Sirve para verificar hashes, carga de CSV y generación determinista de métricas; no
etiquete estos colores como evidencia de nitidez, ojo de pez, movimiento, luces de
escenario o recuperabilidad RAW.

```bash
WORK="$(mktemp -d)"
mkdir -p "$WORK/corpus"
python - "$WORK" <<'PY'
from pathlib import Path
import csv, hashlib, sys
from PIL import Image
root=Path(sys.argv[1]); corpus=root/"corpus"
rows=[
    ("synthetic-keep", "KEEP", "normal"),
    ("synthetic-reject", "REJECT_TECHNICAL", "normal"),
    ("synthetic-motion", "KEEP", "intentional_motion"),
    ("synthetic-raw", "KEEP", "recoverable_raw"),
]
with (root/"manifest.csv").open("w", newline="", encoding="utf-8") as f:
    w=csv.DictWriter(f, fieldnames=["photo_id","path","sha256","expected_decision","case"])
    w.writeheader()
    for i,(photo_id,label,case) in enumerate(rows):
        path=corpus/f"{photo_id}.jpg"
        Image.new("RGB",(160,100),(30+i*40,80,140)).save(path,quality=90)
        w.writerow({"photo_id":photo_id,"path":str(path.relative_to(root)),
                    "sha256":hashlib.sha256(path.read_bytes()).hexdigest(),
                    "expected_decision":label,"case":case})
with (root/"python-decisions.csv").open("w", newline="", encoding="utf-8") as f:
    w=csv.DictWriter(f, fieldnames=["photo_id","decision"]); w.writeheader()
    for photo_id,label,_ in rows: w.writerow({"photo_id":photo_id,"decision":label})
PY
```

La etiqueta `intentional_motion`/ `recoverable_raw` en esta prueba solo confirma
que el metadato de caso viaja con el ejemplo; no convierte los píxeles sintéticos
en un caso fotográfico válido. Para la prueba de dominio, sustituya o amplíe las
filas con fotos permitidas, hashes reales y adjudicación humana.

Con un manifiesto adjudicado y una exportación de decisiones por
implementación, regenere métricas con este evaluador mínimo. Guarde este bloque
como artefacto de prueba junto con el commit que lo ejecuta. Rechaza IDs duplicados,
hashes incorrectos y predicciones ausentes; el hash del CSV enlaza el resultado con
su entrada exacta.

```bash
python - "$WORK/manifest.csv" "$WORK/python-decisions.csv" <<'PY'
import csv, hashlib, json, sys
from pathlib import Path
from photo_culler.validation.corpus import BenchmarkEvaluator, GroundTruthLabel

manifest, predictions = map(Path, sys.argv[1:3])
def rows(path):
    with path.open(newline="", encoding="utf-8") as stream:
        return list(csv.DictReader(stream))
truth_rows, prediction_rows = rows(manifest), rows(predictions)
ids = [row["photo_id"] for row in truth_rows]
if len(ids) != len(set(ids)):
    raise SystemExit("duplicate photo_id in manifest")
for row in truth_rows:
    image = manifest.parent / row["path"]
    actual_hash = hashlib.sha256(image.read_bytes()).hexdigest()
    if actual_hash != row["sha256"]:
        raise SystemExit(f"hash mismatch: {row['photo_id']}")
actual_ids = [row["photo_id"] for row in prediction_rows]
if len(actual_ids) != len(set(actual_ids)):
    raise SystemExit("duplicate photo_id in predictions")
actual = {row["photo_id"]: row["decision"] for row in prediction_rows}
if set(actual) != set(ids):
    raise SystemExit(f"ID mismatch: missing={sorted(set(ids)-set(actual))}, extra={sorted(set(actual)-set(ids))}")
labels = [GroundTruthLabel(row["photo_id"], row["expected_decision"]) for row in truth_rows]
metrics = BenchmarkEvaluator(labels).evaluate(actual)
keep = {"KEEP", "BEST", "ALTERNATE", "keep", "best", "alternate"}
tp=tn=fp=fn=review=0
for row in truth_rows:
    expected, got = row["expected_decision"], actual[row["photo_id"]]
    review += got.lower() == "review"
    pos, predicted = expected.upper() in keep, got.upper() in keep
    if pos and predicted: tp += 1
    elif pos: fn += 1
    elif predicted: fp += 1
    else: tn += 1
print(json.dumps({
    "manifest_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(),
    "predictions_sha256": hashlib.sha256(predictions.read_bytes()).hexdigest(),
    "evaluated": len(ids), "review": review,
    "confusion_matrix": {"tp":tp,"tn":tn,"fp":fp,"fn":fn},
    **metrics
}, indent=2, sort_keys=True))
PY
```

This runner treats KEEP/BEST/ALTERNATE as positive and every other actual state,
including REVIEW, as not-keep for binary metrics; it reports REVIEW count so it is
visible. Keep that mapping fixed across Python and Rust runs. The included
`BenchmarkEvaluator` uses the same binary keep set. To test a candidate, export
the candidate's per-ID predictions to a separate CSV with the same IDs and use
that file in place of `python-decisions.csv`. The Rust exporter/engine is not yet
integrated, so collection of Rust predictions remains pending until that executable
path exists.


## Flujo de reproducción con la infraestructura actual

1. Compruebe los hashes del corpus contra el manifiesto y revise que todos los
   `photo_id` sean únicos.
2. Ejecute el analizador Python actual con perfil/commit registrados y exporte
   una decisión por `photo_id`. `photo-culler evaluate` muestra evaluación de
   filas del catálogo; no consume automáticamente el manifiesto ni las etiquetas.
3. Use el runner anterior sobre el manifiesto y CSV de predicciones, guardando
   stdout/stderr, configuración, versión del evaluador y hashes.
4. Para Rust, use las mismas fotos y etiquetas cuando el motor acepte el contrato;
   registre engine version, flags, salida, fallos RAW, tiempos y decisiones por ID.
   La recolección Rust queda pendiente mientras no exista ese ejecutable integrado.
5. Compare solo filas idénticas y enumere omitidas/errores en lugar de contarlas
   como aciertos.

El test unitario existente puede comprobar las métricas base:

```bash
pytest -q tests/test_validation_benchmark.py
```

## Benchmark de rendimiento

Fije máquina y alimentación, SO/kernel, CPU/GPU, RAM, Python/Rust version, flags,
dependencias, commit, tamaños del catálogo y corpus. Mida importación/análisis
end-to-end, latencia p50/p95 por imagen, throughput, tiempo a primera miniatura,
RAM pico, CPU y espacio de caché. Reporte cold cache y warm cache por separado.
Repita al menos tres ejecuciones por condición e informe cada resultado y mediana;
no elimine outliers sin justificación. No mezcle tiempos de UI/red/RAW decode
entre implementaciones sin identificarlos.

Antes de correr, defina valores de referencia y margen de mejora que justificaría
promoción. Acepte Rust solo si mantiene el límite de falsos rechazos y muestra
mejora material en la misma máquina/corpus, además de manejo de errores y formatos
declarados.

## Plantilla de evidencia por corrida

```text
run_id:
purpose:
date_utc:
commit_python / commit_rust:
corpus_revision / manifest_sha256:
labels_revision / adjudication_record:
environment / hardware:
config_and_profile:
commands:
photos_total / evaluated / review / skipped / failed_by_format:
confusion_matrix (TP, TN, FP, FN):
accuracy / precision / recall / F1 / FRR / FAR:
special_case_counts_and_rates:
wall_time / p50 / p95 / throughput / RAM_peak / cache_state:
raw_decode_errors / other_errors:
artifacts_and_hashes:
human_review_notes:
acceptance_thresholds / owner / result:
