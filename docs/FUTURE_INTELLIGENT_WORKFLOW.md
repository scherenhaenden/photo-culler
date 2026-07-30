# Hoja de ruta: selección y edición inteligentes

Este documento registra una dirección de producto, no una promesa de modelos ni una recomendación automática. Photo Culler debe mantenerse local-first, no destructivo y con aprobación humana antes de exportar o publicar.

## Flujo objetivo: "las 10 mejores fotos de este concierto"

1. El usuario delimita galería, hora de inicio/fin, número de fotos y destino (por ejemplo, publicación web).
2. El catálogo agrupa ráfagas y tomas visualmente similares. La primera versión usa hora de captura y dHash; el usuario puede abrir un grupo y elegir su representante.
3. Se rankean las tomas dentro de cada grupo por nitidez, exposición, ruido, clipping y reglas específicas del perfil. Siempre se muestra la fórmula y las mediciones que explican la posición.
4. Se propone una selección diversa: no diez fotogramas casi idénticos, sino artistas, público, escenario y detalles cuando estén disponibles. La propuesta es editable antes de hacer nada con los originales.
5. Para cada foto aprobada se crea una receta no destructiva: exposición, balance de blancos, recorte/alineación y, más adelante, ajustes locales. La exportación crea archivos derivados; jamás reemplaza el RAW/JPEG original.

## Próximos módulos de producto

| Prioridad | Módulo | Contrato de seguridad y UX |
|---|---|---|
| Ahora | Agrupación de similitud | dHash + proximidad temporal, grupos visibles y representante explicable. |
| Siguiente | Selector editorial | Entrada: rango temporal, N y motivo. Salida: propuesta con diversidad y justificación por imagen; requiere aprobación. |
| Siguiente | Recorte y alineación | Detecta horizonte/sujeto como sugerencia, muestra overlay y guarda solo una receta reversible. |
| Después | Ajuste automático por motivo | Perfiles para concierto, retrato y deporte; cada ajuste es una receta revisable con antes/después. |
| Experimental | Restauración IA | Cola opcional por imagen, modelo/versionado y metadatos de procedencia; nunca se mezcla con el original ni altera la puntuación sin indicarlo. |
| Después | Export/publicación | Presets de destino, metadatos, marca de agua y carpeta de salida explícita; publicación siempre pide confirmación final. |

## Restauración IA: candidatos, no dependencias todavía

La afirmación de que un modelo "domina" no es suficiente para incorporarlo: los resultados dependen de la degradación, GPU, memoria, licencia, fidelidad requerida y tolerancia a contenido inventado. En fotografía documental/de conciertos, restaurar por difusión puede crear detalle plausible pero no necesariamente real. Por ello la primera integración debe comparar en un corpus propio, guardar la versión del modelo y exigir revisión humana al usar métodos generativos.

| Caso | Candidatos a evaluar | Criterio de decisión |
|---|---|---|
| Lote local, degradaciones mixtas | [AnyIR](https://github.com/Amazingren/AnyIR), [AdaIR](https://arxiv.org/abs/2403.14614) | Latencia por megapíxel, VRAM/RAM, licencia y fidelidad en el corpus de conciertos. |
| Poca luz, ruido y blur | [DarkIR](https://arxiv.org/abs/2412.13443) | Recuperación de detalle real sin halos ni cambio de color. |
| Restauración generativa de alta calidad | [HYPIR](https://github.com/XPixelGroup/HYPIR), [SUPIR](https://github.com/Fanghua-Yu/SUPIR) | Revisión humana de fidelidad, tasa de alucinación, coste y tiempo; no usar por defecto. |
| Restauración guiada por semántica | [DA-CLIP](https://arxiv.org/abs/2310.01018) | Comprobar si la detección de degradación ayuda al flujo sin enviar imágenes fuera del equipo. |

Antes de adoptar un candidato se necesita un benchmark reproducible: RAW/JPEG reales de conciertos con ISO alto, movimiento, focos recortados y piel/instrumentos; métricas de rendimiento y una revisión ciega de fidelidad por el fotógrafo. Las afirmaciones de reducción de cómputo o superioridad publicadas por cada autor se tratarán como hipótesis a validar en ese benchmark, no como garantía del producto.

## Arquitectura propuesta

Cada integración de IA vive detrás de un proveedor `RestorationEngine`: `analyze`, `suggest_recipe`, `render_preview` y `export_derivative`. El catálogo guarda el proveedor, versión, parámetros, checksum de entrada y receta/salida. Esto permite comparar modelos, desactivar uno y reproducir un resultado sin contaminar los originales.
