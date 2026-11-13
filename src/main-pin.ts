import { invoke } from "@tauri-apps/api/core";

const params = new URLSearchParams(window.location.search);
const pinId = params.get("id") ?? "";
const lang = params.get("lang") === "en" ? "en" : "es";

const imgEl = document.getElementById("pin-img") as HTMLImageElement | null;
const shellEl = document.getElementById("pin-shell") as HTMLDivElement | null;
const btnOpacity = document.getElementById("btn-opacity") as HTMLButtonElement | null;
const btnCopy = document.getElementById("btn-copy") as HTMLButtonElement | null;
const btnClose = document.getElementById("btn-close") as HTMLButtonElement | null;

if (btnCopy) {
  btnCopy.textContent = lang === "en" ? "Copy" : "Copiar";
  btnCopy.title = btnCopy.textContent;
}
if (btnOpacity) {
  btnOpacity.title = lang === "en" ? "Opacity" : "Opacidad";
}
if (btnClose) {
  btnClose.title = lang === "en" ? "Close (Esc)" : "Cerrar (Esc)";
}

let objectUrl: string | null = null;
const OPACITY_STEPS = [1, 0.78, 0.5];
let opacityIdx = 0;

async function initPin() {
  if (!pinId || !imgEl) return;
  try {
    const buf = await invoke<ArrayBuffer>("get_pinned_png", { id: pinId });
    const blob = new Blob([buf], { type: "image/png" });
    objectUrl = URL.createObjectURL(blob);
    imgEl.src = objectUrl;
  } catch (err) {
    console.error("Failed loading pinned capture:", err);
  }
}

const closePin = () => {
  if (objectUrl) {
    URL.revokeObjectURL(objectUrl);
    objectUrl = null;
  }
  void invoke("close_pinned_window", { id: pinId });
};

btnOpacity?.addEventListener("click", (e) => {
  e.stopPropagation();
  opacityIdx = (opacityIdx + 1) % OPACITY_STEPS.length;
  const val = OPACITY_STEPS[opacityIdx];
  if (shellEl) shellEl.style.opacity = String(val);
  if (btnOpacity) btnOpacity.textContent = `${Math.round(val * 100)}%`;
});

btnCopy?.addEventListener("click", async (e) => {
  e.stopPropagation();
  try {
    await invoke("copy_pinned_to_clipboard", { id: pinId });
    if (btnCopy) {
      btnCopy.textContent = lang === "en" ? "Copied!" : "¡Copiado!";
      window.setTimeout(() => {
        if (btnCopy) btnCopy.textContent = lang === "en" ? "Copy" : "Copiar";
      }, 1100);
    }
  } catch (err) {
    console.error(err);
  }
});

btnClose?.addEventListener("click", (e) => {
  e.stopPropagation();
  closePin();
});

shellEl?.addEventListener("dblclick", closePin);
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    closePin();
  }
});

void initPin();
