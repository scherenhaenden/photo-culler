# Despliegue experimental en NAS

> **Estado y límite de seguridad:** esta guía prepara una aceptación controlada;
> no certifica que el servidor actual sea una instalación NAS segura. El servidor
> headless, autenticación administrativa, aislamiento de rutas y adaptación
> verificada de sensores ASUSTOR siguen siendo objetivos de la migración. El comando
> Python actual `photo-culler nas` enlaza por defecto a `0.0.0.0`, y las rutas de
> estado/configuración térmica no implementan autenticación en sí. No lo exponga
> directamente a una LAN o Internet ni lo considere protegido por el nombre
> “NAS”. Use loopback en una prueba local; la aceptación remota queda bloqueada
> hasta aportar una capa de autenticación/TLS y restricciones de red verificadas.

## Separación de datos y permisos

Use un usuario de servicio dedicado, sin privilegios de administrador. Cree rutas
distintas para aplicación, datos mutables, logs y fotos:

- `/volume1/Apps/photo-culler/releases/<version>/`: ejecutable/entorno, solo
  lectura para el servicio.
- `/volume1/Apps/photo-culler/data/catalog.db`: catálogo; lectura/escritura solo
  para el servicio y respaldo autorizado.
- `/volume1/Apps/photo-culler/cache/`: caché regenerable, escritura del servicio.
- `/volume1/Apps/photo-culler/logs/`: logs con rotación y permisos restringidos.
- `/volume1/Photos/`: originales, montaje de solo lectura para Photo Culler.
- `/volume1/Photo-Culler-Exports/`: destino aparte y explícito para derivados.

Adapte los ejemplos a las rutas del dispositivo. Compruebe con el usuario de
servicio que puede abrir los originales y no crear, renombrar o borrar archivos
en esa raíz. Compruebe escritura solo en catálogo, caché, logs y destino aprobado.
No reutilice credenciales de administrador ni abra permisos globales. El escáner
actual intenta crear un marcador de volumen y tolera que falle; el ensayo debe
registrar ese intento y confirmar la política del montaje antes de usar originales.

## Puesta en marcha local de laboratorio

Primero use una carpeta sintética y catálogo nuevo. Instale la versión fijada y
anote checksum/commit. No inicie el daemon como `root`.

Para la implementación Python actual, pruebe únicamente desde loopback:

```bash
photo-culler --catalog /volume1/Apps/photo-culler/data/lab-catalog.db \
  nas --host 127.0.0.1 --port 8765 --high-temp 75 --low-temp 60 --interval 5
```

La consola debe mostrar la ruta de catálogo, bind, umbrales e intervalo. En otra
terminal local:

```bash
curl --fail http://127.0.0.1:8765/api/v1/nas/status
```

La respuesta contiene temperatura, estado, umbrales, si el monitor está habilitado
y si el job fue pausado térmicamente. En la implementación actual, el lector
devuelve 35°C por defecto cuando no encuentra sensores; por tanto, un estado
“normal” no demuestra sensor disponible. Inspeccione el log y las rutas del kernel
en el host. No use un umbral artificial para concluir que la NAS se enfría o
protege correctamente: pruebe la política con sensor inyectado en pruebas, y mida
temperatura física solo según el procedimiento autorizado del fabricante.

La ruta `POST /api/v1/nas/config` cambia parámetros en ejecución. No la habilite
en red hasta que autenticación y autorización estén verificadas; no se debe
interpretar que existe un endpoint como control de acceso.

## Arranque, logs y reversión

El supervisor del sistema debe iniciar el ejecutable fijado con el usuario
dedicado, rutas explícitas, directorio de trabajo conocido y reinicio limitado.
Mantenga el puerto cerrado en firewall hasta superar el gate de seguridad. La
receta de servicio depende de ADM/firmware y del paquete real: no hay unidad
systemd universal para ADM, así que registre el mecanismo soportado en el equipo
y evite instalar archivos de sistema no documentados.

Al revertir:

1. Detenga el proceso y confirme que ya no escucha.
2. Conserve logs y catálogo del ensayo; no borre datos para “limpiar”.
3. Restaure la versión anterior desde una carpeta de release inmutable.
4. Abra la copia de catálogo con una versión compatible según
   [tarea 58 de la hoja de ruta](RUST_SLINT_ROADMAP.md#las-64-tareas).
5. Verifique la salud local y cierre de nuevo. Registre proceso, código de salida,
   puerto y versión.

No comparta logs sin retirar rutas, nombres de usuario, direcciones y tokens.

## Aceptación inicial ASUSTOR

Registrar equipo/modelo, versión ADM, arquitectura/ABI, CPU/RAM, firmware,
commit/tag, checksum, usuario de servicio, bind, reglas de firewall, rutas,
permisos efectivos, método de arranque, montaje de originales y destino separado.
No copiar credenciales en el acta.

Completar con fixtures y, posteriormente, fotos autorizadas en solo lectura:

| Caso | Procedimiento | Evidencia requerida |
|---|---|---|
| Sensor disponible | Identificar fuente de lectura y contrastar valor con herramienta local del dispositivo durante intervalo registrado. | Ruta del sensor, unidad, valor/antigüedad, muestra temporal y logs; demostrar que no es el fallback 35°C. |
| Sensor ausente | Repetir en entorno simulado sin sensores o en equipo aprobado sin acceso al sensor. | La condición queda explícitamente como indisponible; documentar que el Python actual informa un fallback y no satisface este criterio por sí solo. |
| Originales protegidos | Escanear carpeta sintética montada en solo lectura; comprobar que archivos no cambiaron. | Hashes antes/después y permisos/montaje; anotar intento de marcador. |
| Almacenamiento separado | Generar catálogo, caché, logs y una hoja de contacto en sus rutas asignadas. | Dueño/modo, espacio usado, ruta del derivado y hashes; fotos originales intactas. |
| Red y administración | Mantener bind loopback; revisar firewall y probar controles autenticados solo en implementación que los ofrezca. | Socket escuchando, pruebas permitidas/denegadas y configuración. Hoy el endpoint Python no cumple autenticación administrativa. |
| Temperatura alta y recuperación | Aceptar solo con sensor/calibración que permita una transición controlada sin riesgo al equipo. | Lecturas con hora, pausa, histéresis/umbral y reanudación; distinguir pausa manual y térmica. |

**Aprobación:** no habilitar servicio LAN hasta que cada gate de bind, autenticación,
TLS, firewall, permisos de escritura, backup/restore, logs y sensor esté aprobado
con evidencia. Si falla un gate, mantener la NAS fuera del servicio remoto y dejar
constancia de la prueba fallida; no inferir seguridad por tener la UI inaccesible.
