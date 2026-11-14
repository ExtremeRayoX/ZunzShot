<p align="right">
  <strong>🇪🇸 Español</strong> &nbsp;|&nbsp; <a href="README.en.md">🇺🇸 English</a>
</p>

<div align="center">

<img src="public/zunzshot-logo.png" alt="ZunzShot Logo" width="116" />

# ZunzShot

**Capturador de pantalla instantáneo, ultra liviano y creativo para macOS y Windows.**

[![Versión](https://img.shields.io/github/v/release/ExtremeRayoX/ZunzShot?label=Versi%C3%B3n&color=06d6c6&style=for-the-badge)](#descargas-v109)
[![Plataformas](https://img.shields.io/badge/macOS_%7C_Windows-Descarga_Directa-10b981?style=for-the-badge)](#descargas-v109)
[![English README](https://img.shields.io/badge/Idioma-Espa%C3%B1ol_%7C_English-6366f1?style=for-the-badge)](README.en.md)

<br />

<a href="https://github.com/ExtremeRayoX/ZunzShot/releases/download/v1.0.9/ZunzShot_1.0.9_x64-setup.exe"><img src="docs/icons/download-windows.svg" alt="Descargar ZunzShot v1.0.9 para Windows" width="320" /></a>
&nbsp;&nbsp;
<a href="https://github.com/ExtremeRayoX/ZunzShot/releases/download/v1.0.9/ZunzShot_1.0.9_universal.dmg"><img src="docs/icons/download-macos.svg" alt="Descargar ZunzShot v1.0.9 para macOS" width="320" /></a>

</div>

---

## Descargas (`v1.0.9`)

Versión actual disponible: **ZunzShot v1.0.9**. Haz clic directamente en la imagen de tu sistema operativo arriba o en tu instalador abajo para iniciar la **descarga directa al instante** *(sin páginas intermedias)*:

| Sistema | Plataforma | Descarga directa (`v1.0.9`) | Detalles |
| :---: | :--- | :--- | :--- |
| <img src="docs/icons/os-windows.svg" width="26" height="26" alt="Windows" /> | **Windows 10 / 11 (64-bit)** | [Descargar v1.0.9 (.exe)](https://github.com/ExtremeRayoX/ZunzShot/releases/download/v1.0.9/ZunzShot_1.0.9_x64-setup.exe) | Instalador oficial compacto con selector de idioma (**Español / English**) y actualizaciones automáticas. |
| <img src="docs/icons/os-macos.svg" width="26" height="26" alt="macOS" /> | **macOS 12.3+ (Apple Silicon e Intel)** | [Descargar v1.0.9 para macOS (.dmg)](https://github.com/ExtremeRayoX/ZunzShot/releases/download/v1.0.9/ZunzShot_1.0.9_universal.dmg) | Arrastra **ZunzShot** a **Aplicaciones** y otorga permiso de **Grabación de Pantalla** la primera vez. |

> **Nota para macOS:** Si al abrir por primera vez aparece un aviso de desarrollador no verificado, haz **clic derecho → Abrir** sobre la app en Aplicaciones o ejecuta en Terminal:
> ```bash
> xattr -cr /Applications/ZunzShot.app
> ```

---

## Privacidad y Funcionamiento 100 % Local

ZunzShot está diseñado bajo un enfoque **local-first** para proteger tu pantalla y tus datos:

- **Procesamiento 100 % en tu equipo:** Todas las capturas, anotaciones, stickers, textos y recortes se procesan y guardan exclusivamente de forma local (RAM, portapapeles o la carpeta que elijas).
- **Sin nube ni terceros:** Tus imágenes nunca se suben ni se comparten con servidores externos salvo que tú mismo uses la acción de **Compartir** o **Guardar**.
- **Sin telemetría ni rastreo:** No recopila estadísticas de uso, analítica, publicidad ni requiere cuentas o inicio de sesión.
- **Conexión de red opcional únicamente para actualizaciones:** La única conexión externa posible es consultar los lanzamientos oficiales en GitHub (`github.com/ExtremeRayoX/ZunzShot/releases`) si activas la búsqueda automática o pulsas *Buscar actualizaciones* manualmente en Ajustes (puedes desactivarlo en cualquier momento).

---

## Funcionalidades e Íconos

Al activar una captura aparece la interfaz flotante con los **modos de captura**, la **barra horizontal de edición**, las **herramientas creativas** y la **barra vertical de acciones**. Aquí tienes qué hace cada ícono tal como aparece en la aplicación:

### Modos de captura

| Ícono | Modo | Atajo macOS | Atajo Windows | Para qué sirve |
| :---: | :--- | :---: | :---: | :--- |
| <img src="docs/icons/mode-quick.svg" width="26" height="26" alt="Copiado rápido" /> | **Copiado rápido** | `⌘ + ⇧ + D` | `Ctrl + Shift + D` | Selecciona cualquier área y, al soltar el cursor, copia la imagen al portapapeles de inmediato, cierra el selector y muestra un aviso flotante sin quitar el foco de tu ventana activa. |
| <img src="docs/icons/mode-region.svg" width="26" height="26" alt="Encuadre libre" /> | **Capturar encuadre** | `⌘ + ⇧ + X` | `Ctrl + Shift + X` | Dibuja una selección rectangular movible y redimensionable con lupa de precisión de píxeles y barra de edición completa. |
| <img src="docs/icons/mode-circle.svg" width="26" height="26" alt="Captura circular" /> | **Captura circular** | `⌘ + ⇧ + C` | `Ctrl + Shift + C` | Recorta en forma circular o elíptica con fondo transparente (exportado en PNG), ideal para avatares y detalles circulares. |
| <img src="docs/icons/mode-window.svg" width="26" height="26" alt="Capturar ventana" /> | **Capturar ventana** | `⌘ + ⇧ + W` | `Ctrl + Shift + W` | Detecta y resalta automáticamente ventanas y aplicaciones abiertas para capturarlas con un clic. |
| <img src="docs/icons/mode-fullscreen.svg" width="26" height="26" alt="Pantalla completa" /> | **Pantalla completa** | `⌘ + ⇧ + F` | `Ctrl + Shift + F` | Selecciona al instante todo el monitor activo listo para editar, copiar, fijar o guardar. |

> **Atajos personalizables en Ajustes:** Puedes cambiar cualquier combinación desde **Ajustes → Atajos**. En Windows también puedes asignar la tecla **`Impr Pant` (`PrtSc`)** por sí sola o combinada *(por ejemplo `Shift + PrtSc`)*.

---

### Barra horizontal de herramientas y edición

| Ícono | Herramienta | Para qué sirve |
| :---: | :--- | :--- |
| <img src="docs/icons/tool-select.svg" width="26" height="26" alt="Seleccionar y mover" /> | **Seleccionar / Mover** | Ajusta libremente el recuadro de captura o arrastra y ajusta textos, stickers, fotos y pasos agregados encima. |
| <img src="docs/icons/tool-pen.svg" width="26" height="26" alt="Lápiz libre" /> | **Lápiz libre** *(submenú Trazos)* | Dibuja anotaciones a mano alzada sobre cualquier parte de la captura. |
| <img src="docs/icons/tool-marker.svg" width="26" height="26" alt="Resaltador" /> | **Resaltador** *(submenú Trazos)* | Destaca texto o áreas importantes con trazo translúcido tipo marcador. |
| <img src="docs/icons/tool-underline.svg" width="26" height="26" alt="Subrayar" /> | **Subrayar** *(submenú Trazos)* | Traza líneas rectas horizontales perfectas para subrayar frases o cifras rápidamente. |
| <img src="docs/icons/tool-arrow.svg" width="26" height="26" alt="Flecha" /> | **Flecha** | Señala botones, menús o elementos clave con flechas nítidas y personalizables. |
| <img src="docs/icons/tool-rect.svg" width="26" height="26" alt="Rectángulo" /> | **Rectángulo** *(submenú Formas)* | Enmarca zonas o bloques completos mediante rectángulos limpios. |
| <img src="docs/icons/tool-circle.svg" width="26" height="26" alt="Círculo / Óvalo" /> | **Círculo / Óvalo** *(submenú Formas)* | Rodea íconos, números o elementos concretos con óvalos o círculos. |
| <img src="docs/icons/tool-text.svg" width="26" height="26" alt="Texto" /> | **Texto** | Inserta etiquetas **editables** sobre la imagen: cambia en vivo o al seleccionarlas el **fondo** (*Con fondo* / *Sin fondo*), **negrita (`B`)**, **cursiva (`I`)**, tamaño (`A-` / `A+` de `12px` a `72px`) y color; arrástralas, muévelas con las flechas o haz **doble clic** para volver a editarlas. |
| <img src="docs/icons/tool-pixelate.svg" width="26" height="26" alt="Pixelar / Censurar" /> | **Pixelar / Censurar** | Aplica efecto mosaico para ocultar contraseñas, correos, rostros o datos confidenciales. |
| <img src="docs/icons/tool-crop-circle.svg" width="26" height="26" alt="Alternar forma de recorte" /> <img src="docs/icons/tool-crop-rect.svg" width="26" height="26" alt="Cambiar a rectángulo" /> | **Recorte Rectangular / Circular** | Convierte tu selección actual entre rectángulo y círculo en cualquier momento sin perder tus trazos. |
| <img src="docs/icons/tool-sticker.svg" width="26" height="26" alt="Extras y Stickers" /> | **Extras y Stickers** | Abre el menú de elementos creativos para añadir imágenes, hora congelada y pasos numerados. |
| <img src="docs/icons/tool-palette.svg" width="26" height="26" alt="Color, grosor y cuentagotas" /> | **Color y Grosor** | Elige entre los colores de la paleta, ajusta el grosor (`S / M / L`) o activa el cuentagotas. |
| <img src="docs/icons/tool-eyedropper.svg" width="26" height="26" alt="Cuentagotas" /> | **Cuentagotas `#HEX`** *(dentro de Color)* | Amplía los píxeles bajo el cursor y copia el código `#HEX` exacto del color al portapapeles. |
| <img src="docs/icons/tool-undo.svg" width="26" height="26" alt="Deshacer" /> | **Deshacer** | Revierte el último trazo, forma o sticker agregado *(también `⌘/Ctrl + Z` y rehacer con `⌘/Ctrl + ⇧ + Z` o `Ctrl + Y`)*. |

#### Dentro de Extras y Stickers

| Ícono | Extra | Para qué sirve |
| :---: | :--- | :--- |
| <img src="docs/icons/tool-photo.svg" width="26" height="26" alt="Agregar foto o sticker" /> | **Foto / Imagen superpuesta** | Inserta una imagen estática sobre tu captura (desde archivo o pegando directo con `⌘/Ctrl + V`) con recorte cuadrado, redondeado o circular. |
| <img src="docs/icons/tool-clock.svg" width="26" height="26" alt="Sticker de fecha y hora" /> | **Sticker de Fecha y Hora** | Coloca un sello visual elegante con la fecha y hora exacta en que se tomó la captura (configurable en 12h/24h y estilos). |
| <img src="docs/icons/tool-step.svg" width="26" height="26" alt="Pasos numerados" /> | **Pasos numerados (`1, 2, 3...`)** | Agrega burbujas numeradas consecutivas para crear guías visuales y tutoriales paso a paso en segundos. |

> **Preferencias por defecto en Ajustes → Extras:** Desde la pestaña **Extras** en Ajustes puedes configurar los valores iniciales tanto del **Sticker de Hora y Fecha** *(formato 12h/24h, segundos, fecha y estilo)* como de **Texto y Etiquetas** *(fondo por defecto, negrita, cursiva y tamaño inicial `16px`–`36px`)*.

---

### Barra vertical de acciones *(editable en Ajustes)*

| Ícono | Acción | Atajo macOS | Atajo Windows | Para qué sirve |
| :---: | :--- | :---: | :---: | :--- |
| <img src="docs/icons/action-copy.svg" width="26" height="26" alt="Copiar captura" /> | **Copiar** | `⌘ + C` | `Ctrl + C` | Copia la captura final con todas sus anotaciones y stickers al portapapeles. |
| <img src="docs/icons/action-save.svg" width="26" height="26" alt="Guardar imagen" /> | **Guardar** | `⌘ + S` | `Ctrl + S` | Exporta la captura a disco en formato **PNG** o **JPG** en la carpeta que prefieras. |
| <img src="docs/icons/action-pin.svg" width="26" height="26" alt="Fijar en pantalla" /> | **Fijar en pantalla (Pin)** | `⌘ + ⇧ + P` | `Ctrl + Shift + P` | Convierte el recorte en una ventana flotante siempre visible sobre tus demás apps *(con control de opacidad y copiado rápido)*. |
| <img src="docs/icons/action-share.svg" width="26" height="26" alt="Compartir" /> | **Compartir** | `⌘ + ⇧ + E` | `Ctrl + Shift + E` | Abre las opciones de uso compartido del sistema para enviar la captura al instante. |
| <img src="docs/icons/action-print.svg" width="26" height="26" alt="Imprimir" /> | **Imprimir** | `⌘ + P` | `Ctrl + P` | Envía la captura directamente al diálogo de impresión del sistema. |
| <img src="docs/icons/action-close.svg" width="26" height="26" alt="Cerrar captura" /> | **Cancelar / Cerrar** | `Esc` | `Esc` | Cierra la captura limpia y vuelve al estado en segundo plano sin dejar rastros. |

---

## Notas de Versión

### `v1.0.9`
- **Actualizaciones en su propia ventana y sección:** *Buscar actualizaciones* ahora tiene su propia pantalla en el menú (justo encima de *Acerca de*) y se abre en una ventana independiente desde la bandeja del sistema o ante un aviso automático, sin abrir el menú principal detrás.
- **Revisión cada 30 minutos y recordatorios de 2, 4 o 6 horas:** Cuando activas la búsqueda automática, ZunzShot comprueba cada **30 minutos** en segundo plano y al pulsar *Recordar más tarde* puedes elegir si volver a avisar en **2**, **4** o **6 horas**.
- **Detalles de versión más legibles:** Las novedades de cada actualización ahora se presentan con títulos, listas y resaltados claros para leerlos de un vistazo.
- **Control centralizado:** La preferencia de búsqueda automática se gestiona únicamente desde *Buscar actualizaciones*, dejando *General* y *Acerca de* más ordenados.

> **Historial completo:** Consulta todas las versiones anteriores (`v1.0.0` – `v1.0.8`) en [CHANGELOG.md](CHANGELOG.md) o en [GitHub Releases](https://github.com/ExtremeRayoX/ZunzShot/releases).

---

<div align="center">

Desarrollado por **ERX**

</div>
