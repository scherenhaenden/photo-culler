# Versionado de releases

El formato de versión canónico y obligatorio para Photo Culler es exactamente:

```text
yyyy.MM.dd-HH.mm.sss
```

Reglas:

- `yyyy`: año de cuatro dígitos.
- `MM`: mes de dos dígitos, con cero inicial.
- `dd`: día de dos dígitos, con cero inicial.
- `HH`: hora de dos dígitos en formato de 24 horas.
- `mm`: minuto de dos dígitos.
- `sss`: milisegundos de tres dígitos.

Ejemplo: `2026.08.03-11.34.567`.

Cada build de release debe recibir una versión nueva con este formato. No se usan
versiones semánticas como `0.2.0` para identificar releases de producto.
