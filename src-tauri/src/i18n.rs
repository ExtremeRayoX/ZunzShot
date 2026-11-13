#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    pub fn from_code(code: &str) -> Self {
        if code.trim().to_lowercase().starts_with("en") {
            Lang::En
        } else {
            Lang::Es
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }
}

pub struct TrayLabels {
    pub permission_required: &'static str,
    pub capture_region: &'static str,
    pub capture_quick_copy: &'static str,
    pub capture_circle: &'static str,
    pub capture_window: &'static str,
    pub capture_fullscreen: &'static str,
    pub settings: &'static str,
    pub check_updates: &'static str,
    pub about: &'static str,
    pub quit: &'static str,
}

pub fn tray_labels(lang: Lang) -> TrayLabels {
    match lang {
        Lang::Es => TrayLabels {
            permission_required: "⚠️ Activar Grabación de Pantalla...",
            capture_region: "Capturar encuadre",
            capture_quick_copy: "Copiado rápido (seleccionar y copiar)",
            capture_circle: "Captura circular",
            capture_window: "Capturar aplicación / ventana",
            capture_fullscreen: "Pantalla completa",
            settings: "Configuración...",
            check_updates: "Buscar actualizaciones...",
            about: "Acerca de ZunzShot",
            quit: "Salir",
        },
        Lang::En => TrayLabels {
            permission_required: "⚠️ Grant Screen Recording...",
            capture_region: "Capture Region",
            capture_quick_copy: "Quick Copy Area",
            capture_circle: "Circular Capture",
            capture_window: "Capture App / Window",
            capture_fullscreen: "Fullscreen Capture",
            settings: "Settings...",
            check_updates: "Check for Updates...",
            about: "About ZunzShot",
            quit: "Quit",
        },
    }
}
