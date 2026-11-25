use eframe::egui::{self, Color32, Stroke};
use raw_window_handle::HasWindowHandle;
#[cfg(windows)]
use raw_window_handle::RawWindowHandle;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
#[cfg(windows)]
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
    pub selected_ink: Color32,
    pub ticket: Color32,
    pub accent: Color32,
    pub tint: Color32,
    pub green: Color32,
    pub warning: Color32,
}
pub fn palette(dark: bool) -> Palette {
    let rgb = Color32::from_rgb;
    if dark {
        Palette {
            background: rgb(18, 25, 26),
            surface: rgb(26, 35, 36),
            ink: rgb(241, 246, 243),
            muted: rgb(175, 193, 186),
            border: rgb(52, 66, 63),
            track: rgb(34, 46, 44),
            selected: rgb(191, 231, 192),
            selected_ink: rgb(25, 49, 38),
            ticket: rgb(36, 53, 46),
            accent: rgb(171, 228, 194),
            tint: rgb(48, 72, 61),
            green: rgb(130, 220, 174),
            warning: rgb(253, 187, 125),
        }
    } else {
        Palette {
            background: rgb(242, 246, 242),
            surface: Color32::WHITE,
            ink: rgb(29, 46, 40),
            muted: rgb(80, 99, 91),
            border: rgb(208, 220, 212),
            track: rgb(235, 241, 236),
            selected: rgb(38, 83, 65),
            selected_ink: Color32::WHITE,
            ticket: rgb(229, 241, 230),
            accent: rgb(27, 98, 74),
            tint: rgb(209, 232, 216),
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
#[cfg(windows)]
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

#[cfg(target_os = "macos")]
pub fn title_bar(_: &impl HasWindowHandle, _: bool) {}

#[cfg(target_os = "macos")]
pub fn motion_enabled() -> bool {
    crate::mac_camera::motion_enabled()
}

#[cfg(windows)]
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
