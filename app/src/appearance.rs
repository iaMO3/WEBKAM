use eframe::egui::{self, Color32, Stroke};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use windows::Win32::{Foundation::HWND, Graphics::Dwm::*};

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}
impl Appearance {
    pub fn load(directory: &Path) -> Self {
        fs::read(directory.join("appearance.json"))
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }
    pub fn save(self, directory: &Path) {
        if let Ok(bytes) = serde_json::to_vec(&self) {
            let _ = fs::create_dir_all(directory)
                .and_then(|_| fs::write(directory.join("appearance.json"), bytes));
        }
    }
    pub fn apply(self, ctx: &egui::Context) {
        ctx.set_theme(match self {
            Self::System => egui::ThemePreference::System,
            Self::Light => egui::ThemePreference::Light,
            Self::Dark => egui::ThemePreference::Dark,
        });
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub track: Color32,
    pub selected: Color32,
    pub accent: Color32,
    pub tint: Color32,
    pub green: Color32,
    pub warning: Color32,
}
pub fn palette(dark: bool) -> Palette {
    let rgb = Color32::from_rgb;
    if dark {
        Palette {
            background: rgb(27, 28, 32),
            surface: rgb(37, 38, 44),
            ink: rgb(244, 244, 247),
            muted: rgb(186, 187, 199),
            border: rgb(62, 63, 71),
            track: rgb(47, 48, 56),
            selected: rgb(76, 77, 89),
            accent: rgb(126, 187, 255),
            tint: rgb(52, 68, 87),
            green: rgb(130, 220, 174),
            warning: rgb(253, 187, 125),
        }
    } else {
        Palette {
            background: rgb(246, 246, 249),
            surface: Color32::WHITE,
            ink: rgb(29, 29, 33),
            muted: rgb(89, 91, 104),
            border: rgb(221, 221, 228),
            track: rgb(237, 237, 242),
            selected: Color32::WHITE,
            accent: rgb(0, 91, 192),
            tint: rgb(224, 236, 253),
            green: rgb(25, 111, 76),
            warning: rgb(153, 74, 25),
        }
    }
}

pub fn install(ctx: &egui::Context) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let dark = theme == egui::Theme::Dark;
        let p = palette(dark);
        let mut v = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.override_text_color = Some(p.ink);
        v.panel_fill = p.background;
        v.window_fill = p.surface;
        v.window_stroke = Stroke::new(1.0_f32, p.border);
        v.selection.bg_fill = p.tint;
        v.selection.stroke = Stroke::new(1.0_f32, p.accent);
        v.widgets.inactive.bg_fill = p.track;
        v.widgets.inactive.weak_bg_fill = p.track;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
        v.widgets.hovered.bg_fill = p.tint;
        v.widgets.hovered.weak_bg_fill = p.tint;
        v.hyperlink_color = p.accent;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
        for widget in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            widget.corner_radius = 7.into();
        }
        ctx.set_visuals_of(theme, v);
    }
}

// Only color the native caption. Windows retains its drag, snap and caption buttons.
pub fn title_bar(window: &impl HasWindowHandle, dark: bool) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win.hwnd.get() as *mut _);
    let colorref = |c: Color32| u32::from_le_bytes([c.r(), c.g(), c.b(), 0]);
    let p = palette(dark);
    for (attribute, value) in [
        (DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(dark)),
        (DWMWA_CAPTION_COLOR, colorref(p.background)),
        (DWMWA_TEXT_COLOR, colorref(p.ink)),
    ] {
        unsafe {
            let _ = DwmSetWindowAttribute(hwnd, attribute, (&value as *const u32).cast(), 4);
        }
    }
}

pub fn motion_enabled() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut enabled = 1_i32;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut enabled as *mut i32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    enabled != 0
}
pub fn apply_motion(ctx: &egui::Context, enabled: bool) {
    ctx.all_styles_mut(|style| style.animation_time = if enabled { 0.20 } else { 0.0 });
}
