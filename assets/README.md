# Temas Festivos Remotos de ZunzShot (`splash-festive.json`)

Desde **ZunzShot v1.1.0**, la animación de inicio consulta en segundo plano (sin bloquear el arranque ni mostrar ventanas de carga) el archivo [`assets/splash-festive.json`](splash-festive.json) de este repositorio.

Cuando la fecha actual del equipo coincide entre `startDate` y `endDate` (`YYYY-MM-DD`), la aplicación activa automáticamente el tema correspondiente **sin necesidad de publicar ni instalar una nueva versión**.

---

## Campos admitidos (100 % declarativos y validados en Rust)

| Campo | Tipo | Descripción |
| :--- | :--- | :--- |
| `id` | `string` | Identificador único del evento (ej. `"halloween-2027"`). |
| `startDate` | `string` | Fecha inicial inclusive (`YYYY-MM-DD`). |
| `endDate` | `string` | Fecha final inclusive (`YYYY-MM-DD`). |
| `badgeEs` | `string?` | Etiqueta corta debajo de *ZunzShot* en español (máx. 40 caracteres). |
| `badgeEn` | `string?` | Etiqueta corta debajo de *ZunzShot* en inglés (máx. 40 caracteres). |
| `accentColor` | `color?` | Color principal (`#hex`, `rgb(...)` o `rgba(...)`) para acento y pulso de impacto. |
| `accentSecondary` | `color?` | Color secundario del degradado en *Shot* y badge. |
| `flowerColorStart` | `color?` | Color central del recuadro-flor al florecer. |
| `flowerColorEnd` | `color?` | Color exterior del recuadro-flor y esquinas de captura. |
| `flowerAuraColor` | `color?` | Resplandor de la flor al formar el logo. |
| `birdGlowColor` | `color?` | Halo ambiental detrás del colibrí. |
| `cardBorderColor` | `color?` | Borde luminoso opcional alrededor de la tarjeta del logo. |
| `accessory` | `enum?` | Accesorio origami sobre el colibrí: `"none"`, `"witch-hat"`, `"santa-hat"`, `"party-hat"`, `"heart"`. |
| `particles` | `enum?` | Partículas al tocar la flor: `"none"`, `"bats"`, `"snow"`, `"confetti"`, `"hearts"`, `"sparkles"`. |

---

## Ejemplos listos para copiar en `assets/splash-festive.json`

### Halloween
```json
{
  "schemaVersion": 1,
  "themes": [
    {
      "id": "halloween",
      "startDate": "2027-10-25",
      "endDate": "2027-11-02",
      "badgeEs": "Edición Halloween",
      "badgeEn": "Halloween Edition",
      "accentColor": "#fb923c",
      "accentSecondary": "#a855f7",
      "flowerColorStart": "#fff7ed",
      "flowerColorEnd": "#fb923c",
      "flowerAuraColor": "#f97316",
      "birdGlowColor": "#a855f7",
      "cardBorderColor": "rgba(251, 146, 60, 0.55)",
      "accessory": "witch-hat",
      "particles": "bats"
    }
  ]
}
```

### Navidad
```json
{
  "schemaVersion": 1,
  "themes": [
    {
      "id": "navidad",
      "startDate": "2027-12-18",
      "endDate": "2027-12-27",
      "badgeEs": "Felices Fiestas",
      "badgeEn": "Happy Holidays",
      "accentColor": "#f87171",
      "accentSecondary": "#34d399",
      "flowerColorStart": "#fef2f2",
      "flowerColorEnd": "#fda4af",
      "flowerAuraColor": "#34d399",
      "birdGlowColor": "#ef4444",
      "cardBorderColor": "rgba(248, 113, 113, 0.5)",
      "accessory": "santa-hat",
      "particles": "snow"
    }
  ]
}
```

### Año Nuevo / San Valentín
```json
{
  "schemaVersion": 1,
  "themes": [
    {
      "id": "new-year",
      "startDate": "2027-12-30",
      "endDate": "2028-01-03",
      "badgeEs": "Feliz Año Nuevo",
      "badgeEn": "Happy New Year",
      "accentColor": "#facc15",
      "accentSecondary": "#38bdf8",
      "flowerColorStart": "#fefce8",
      "flowerColorEnd": "#fde047",
      "flowerAuraColor": "#facc15",
      "birdGlowColor": "#38bdf8",
      "cardBorderColor": "rgba(250, 204, 21, 0.55)",
      "accessory": "party-hat",
      "particles": "confetti"
    },
    {
      "id": "valentines",
      "startDate": "2028-02-13",
      "endDate": "2028-02-15",
      "badgeEs": "San Valentín",
      "badgeEn": "Valentine's Day",
      "accentColor": "#fb7185",
      "accentSecondary": "#f472b6",
      "flowerColorStart": "#fff1f2",
      "flowerColorEnd": "#fda4af",
      "flowerAuraColor": "#fb7185",
      "birdGlowColor": "#f472b6",
      "cardBorderColor": "rgba(251, 113, 133, 0.5)",
      "accessory": "heart",
      "particles": "hearts"
    }
  ]
}
```
