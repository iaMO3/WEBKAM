use crate::{
    frames::Frame as VideoFrame,
    network::{self, Adapter, AdbForward, Connection, Link},
    web::{App, LightKind, Orientation, Quality, Rotation, Settings, Status},
};
use anyhow::{Context, Result};
use eframe::egui::{
    self, Align, Color32, ColorImage, FontId, Frame, Layout, RichText, Stroke, TextureHandle,
    TextureOptions, Vec2, ViewportBuilder, vec2,
};
use std::{
    fs,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use tokio::{runtime::Handle, sync::oneshot};

#[path = "optics.rs"]
mod optics;

#[path = "appearance.rs"]
mod appearance;
use appearance::{Appearance, palette};
const STAGE: Color32 = Color32::from_rgb(18, 21, 29);

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let proportional = egui::FontFamily::Proportional;
    let mut display = fonts.families[&proportional].clone();
    if let Some(windows) = std::env::var_os("WINDIR") {
        let directory = PathBuf::from(windows).join("Fonts");
        for (file, name) in [
            ("segoeui.ttf", "Segoe UI"),
            ("seguisb.ttf", "Segoe Semibold"),
        ] {
            if let Ok(bytes) = fs::read(directory.join(file)) {
                fonts
                    .font_data
                    .insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
                if name == "Segoe UI" {
                    fonts
                        .families
                        .entry(proportional.clone())
                        .or_default()
                        .insert(0, name.into());
                }
                display.insert(0, name.into());
            }
        }
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("Display".into()), display);
    ctx.set_fonts(fonts);
}

pub fn load_settings(directory: &Path) -> Settings {
    fs::read(directory.join("settings.json"))
        .ok()
        .and_then(|file| serde_json::from_slice::<Settings>(&file).ok())
        .filter(|settings| [30, 60].contains(&settings.fps))
        .unwrap_or_default()
}

pub fn show(
    app: App,
    settings: Settings,
    directory: PathBuf,
    runtime: Handle,
    camera: mpsc::Receiver<Result<(), String>>,
) -> Result<()> {
    let icon = Arc::new(
        eframe::icon_data::from_png_bytes(crate::web::ICON_192).context("Read the WEBKAM icon")?,
    );
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_icon(icon.clone())
            .with_inner_size([1100.0, 745.0])
            .with_min_inner_size([360.0, 360.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "WEBKAM",
        options,
        Box::new(move |creation| {
            let ctx = &creation.egui_ctx;
            install_fonts(ctx);
            appearance::install(ctx);
            let motion = appearance::motion_enabled();
            appearance::apply_motion(ctx, motion);
            let appearance = Appearance::load(&directory);
            appearance.apply(ctx);
            appearance::title_bar(creation, ctx.theme() == egui::Theme::Dark);
            ctx.all_styles_mut(|style| {
                style.spacing.item_spacing = vec2(8.0, 8.0);
                style.spacing.button_padding = vec2(10.0, 6.0);
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, FontId::proportional(14.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, FontId::proportional(14.0));
            });
            let mut desktop = Desktop {
                app,
                settings,
                directory,
                runtime,
                camera,
                camera_status: None,
                appearance,
                caption_theme: None,
                motion,
                next_motion_check: Instant::now() + Duration::from_secs(2),
                connection: Connection::Network,
                selected: None,
                active_ip: None,
                adapters: network::adapters(),
                next_refresh: Instant::now() + Duration::from_secs(2),
                link: None,
                network_error: None,
                qr: None,
                adb: None,
                adb_pending: None,
                last_frame: None,
                preview: None,
                brand: ctx.load_texture(
                    "WEBKAM icon",
                    ColorImage::from_rgba_unmultiplied(
                        [icon.width as usize, icon.height as usize],
                        &icon.rgba,
                    ),
                    TextureOptions::LINEAR,
                ),
                compact_page: CompactPage::Phone,
                backdrop: ctx.load_texture(
                    "Preview backdrop",
                    optics::backdrop(),
                    TextureOptions::LINEAR,
                ),
            };
            desktop.refresh_link(ctx, false);
            Ok(Box::new(desktop))
        }),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))
}

#[derive(Clone, Copy, PartialEq)]
enum CompactPage {
    Phone,
    Video,
}

struct Desktop {
    app: App,
    settings: Settings,
    directory: PathBuf,
    runtime: Handle,
    camera: mpsc::Receiver<Result<(), String>>,
    appearance: Appearance,
    motion: bool,
    next_motion_check: Instant,
    caption_theme: Option<(egui::Theme, Option<egui::Theme>)>,
    camera_status: Option<Result<(), String>>,
    connection: Connection,
    adapters: Vec<Adapter>,
    selected: Option<Ipv4Addr>,
    active_ip: Option<Ipv4Addr>,
    next_refresh: Instant,
    link: Option<Link>,
    network_error: Option<String>,
    qr: Option<TextureHandle>,
    adb: Option<AdbForward>,
    adb_pending: Option<oneshot::Receiver<Result<AdbForward>>>,
    last_frame: Option<Arc<VideoFrame>>,
    preview: Option<TextureHandle>,
    brand: TextureHandle,
    compact_page: CompactPage,
    backdrop: TextureHandle,
}

impl Desktop {
    fn close_link(&mut self) {
        if let Some(link) = self.link.take() {
            self.runtime.block_on(link.close());
        }
    }

    fn refresh_link(&mut self, ctx: &egui::Context, force: bool) {
        if self.connection == Connection::AndroidAdb {
            if self.link.is_none() || self.active_ip.is_some() {
                self.app.reset();
                self.close_link();
                self.qr = None;
                self.active_ip = None;
                self.network_error = None;
                match self.runtime.block_on(network::loopback(self.app.clone())) {
                    Ok(link) => self.link = Some(link),
                    Err(error) => self.network_error = Some(format!("{error:#}")),
                }
            }
            return;
        }
        if self
            .selected
            .is_some_and(|ip| !self.adapters.iter().any(|adapter| adapter.ip == ip))
        {
            self.selected = None;
        }
        let automatic = self
            .adapters
            .iter()
            .find(|adapter| match self.connection {
                Connection::Network => !adapter.iphone() && !adapter.android(),
                Connection::IphoneUsb => adapter.iphone(),
                Connection::AndroidTether => adapter.android(),
                Connection::AndroidAdb => false,
            })
            .map(|adapter| adapter.ip);
        let ip = self.selected.or(automatic);
        if !force && self.active_ip == ip && self.link.is_some() {
            return;
        }
        self.close_link();
        self.qr = None;
        self.active_ip = ip;
        self.network_error = None;
        self.app.reset();
        if let Some(ip) = ip {
            match self
                .runtime
                .block_on(network::lan(ip, &self.directory, self.app.clone()))
            {
                Ok(link) => {
                    self.qr = qr_texture(ctx, &link.url);
                    self.link = Some(link);
                }
                Err(error) => self.network_error = Some(format!("{error:#}")),
            }
        }
    }

    fn change_connection(&mut self, ctx: &egui::Context, connection: Connection) {
        self.connection = connection;
        self.active_ip = None;
        self.selected = None;
        self.close_link();
        self.qr = None;
        self.adb = None;
        self.adb_pending = None;
        self.adapters = network::adapters();
        self.refresh_link(ctx, true);
    }

    fn connect_adb(&mut self) {
        self.network_error = None;
        let (send, ready) = oneshot::channel();
        self.runtime.spawn(async move {
            let _ = send.send(network::connect_adb().await);
        });
        self.adb_pending = Some(ready);
    }

    fn save_settings(&self) {
        if fs::create_dir_all(&self.directory).is_ok() {
            if let Ok(contents) = serde_json::to_vec(&self.settings) {
                let _ = fs::write(self.directory.join("settings.json"), contents);
            }
        }
        self.app.settings(self.settings);
    }

    fn load_preview(&mut self, ctx: &egui::Context, current: Option<Arc<VideoFrame>>) {
        let same = match (&self.last_frame, &current) {
            (Some(previous), Some(next)) => Arc::ptr_eq(previous, next),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some(frame) = &current {
            let dimensions = [frame.preview_size.width, frame.preview_size.height];
            let image = ColorImage::from_rgb(dimensions, &frame.preview);
            if let Some(texture) = &mut self.preview {
                texture.set(image, TextureOptions::LINEAR);
            } else {
                self.preview =
                    Some(ctx.load_texture("Video preview", image, TextureOptions::LINEAR));
            }
        } else {
            self.preview = None;
        }
        self.last_frame = current;
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui, status: &Status) {
        let width = ui.available_width();
        let height = (width * 9.0 / 16.0).clamp(176.0, 510.0);
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 17, STAGE);
        let reveal = transition(ui, "preview-reveal", self.preview.is_some());
        if let Some(preview) = &self.preview {
            let image = preview.size_vec2();
            let scale = (rect.width() / image.x).min(rect.height() / image.y);
            let target = egui::Rect::from_center_size(rect.center(), image * scale);
            egui::Image::new(preview)
                .tint(Color32::WHITE.gamma_multiply(reveal))
                .corner_radius(14)
                .paint_at(ui, target);
        } else {
            let size = self.backdrop.size_vec2();
            let aspect = size.x / size.y;
            let crop = rect.aspect_ratio() / aspect;
            let uv = egui::Rect::from_center_size(
                egui::pos2(0.5, 0.5),
                vec2(crop.min(1.0), (1.0 / crop).min(1.0)),
            );
            egui::Image::new(&self.backdrop)
                .uv(uv)
                .corner_radius(17)
                .paint_at(ui, rect);
            let inset = if width < 440.0 { 18.0 } else { 26.0 };
            let caption = rect.left_bottom() + vec2(inset, -inset);
            let (message, hint) = if status.paired {
                ("Your phone is paired", "Tap Start camera on your phone.")
            } else if self.qr.is_some() {
                ("Ready to pair", "Scan the QR code with your phone.")
            } else {
                (
                    "Waiting for a connection",
                    "Choose how to connect your phone.",
                )
            };
            painter.text(
                caption - vec2(0.0, 22.0),
                egui::Align2::LEFT_BOTTOM,
                message,
                display_font(if width < 440.0 { 25.0 } else { 34.0 }),
                Color32::WHITE,
            );
            painter.text(
                caption,
                egui::Align2::LEFT_BOTTOM,
                hint,
                FontId::proportional(14.0),
                Color32::from_rgb(224, 241, 235),
            );
            // Static framing marks stay out of the live camera feed.
            let mark = rect.shrink(16.0);
            for (point, direction) in [
                (mark.right_top(), vec2(-1.0, 1.0)),
                (mark.right_bottom(), vec2(-1.0, -1.0)),
            ] {
                let stroke = Stroke::new(1.0_f32, Color32::from_white_alpha(144));
                painter.line_segment([point, point + vec2(direction.x * 14.0, 0.0)], stroke);
                painter.line_segment([point, point + vec2(0.0, direction.y * 14.0)], stroke);
            }
        }
        let label = if status.live {
            format!("Live  ·  {} FPS in", status.fps)
        } else {
            "Standby".to_owned()
        };
        let text = painter.layout_no_wrap(label, display_font(12.0), Color32::WHITE);
        let chip = egui::Rect::from_min_size(
            rect.min + vec2(16.0, 16.0),
            vec2(text.size().x + 34.0, 26.0),
        );
        painter.rect_filled(chip, 13, Color32::from_black_alpha(155));
        painter.circle_filled(
            chip.left_center() + vec2(12.0, 0.0),
            3.0,
            if status.live {
                Color32::from_rgb(146, 243, 187)
            } else {
                Color32::from_gray(207)
            },
        );
        painter.galley(
            chip.left_center() + vec2(22.0, -text.size().y / 2.0),
            text,
            Color32::WHITE,
        );
    }

    fn video_controls(&mut self, ui: &mut egui::Ui, status: &Status) {
        let previous = self.settings;
        section(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                title(ui, "Video settings");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let size = self.settings.size();
                    let dimensions = match (&self.last_frame, self.settings.orientation) {
                        (Some(frame), Orientation::Auto) => {
                            format!("{} × {}", frame.size.width, frame.size.height)
                        }
                        (None, Orientation::Auto) => "Follows phone".into(),
                        _ => format!("{} × {}", size.width, size.height),
                    };
                    detail(ui, &dimensions);
                });
            });
            ui.add_space(5.0);
            // Give each group its own cell. At narrow widths every label and choice gets a full row.
            let columns = if ui.available_width() >= 468.0 { 2 } else { 1 };
            egui::Grid::new("video-grid")
                .num_columns(columns)
                .min_col_width(if columns == 2 {
                    (ui.available_width() - 16.0) * 0.5
                } else {
                    0.0
                })
                .spacing(vec2(16.0, 12.0))
                .show(ui, |ui| {
                    segment_group(
                        ui,
                        "Orientation",
                        &mut self.settings.orientation,
                        [
                            (Orientation::Auto, "Auto"),
                            (Orientation::Landscape, "Landscape"),
                            (Orientation::Portrait, "Portrait"),
                        ],
                        61.0,
                    );
                    if columns == 1 {
                        ui.end_row();
                    }
                    ui.horizontal_top(|ui| {
                        segment_group(
                            ui,
                            "Resolution",
                            &mut self.settings.quality,
                            [(Quality::Hd, "720p"), (Quality::FullHd, "1080p")],
                            60.0,
                        );
                        ui.vertical(|ui| {
                            let light = status.light;
                            let label = if light.light == LightKind::Screen {
                                "Screen light"
                            } else {
                                "Flash"
                            };
                            detail(ui, label);
                            let button = egui::Button::selectable(
                                light.on,
                                if light.on { "      On" } else { "      Off" },
                            )
                            .min_size(vec2(60.0, 32.0));
                            let response = ui.add_enabled(light.light != LightKind::None, button);
                            let center = response.rect.left_center() + vec2(13.0, 0.0);
                            let bolt = [
                                (2., -7.),
                                (-4., 1.),
                                (0., 1.),
                                (-2., 7.),
                                (5., -2.),
                                (1., -2.),
                                (2., -7.),
                            ]
                            .map(|(x, y)| center + vec2(x, y))
                            .to_vec();
                            let color = ui.style().interact(&response).fg_stroke.color;
                            ui.painter()
                                .add(egui::Shape::line(bolt, Stroke::new(1.4_f32, color)));
                            if response.clicked() {
                                self.app.toggle_light();
                            }
                            response.on_hover_text(match light.light {
                                LightKind::Torch => "Switch the phone torch on or off",
                                LightKind::Screen => "Use the phone screen to light your face",
                                LightKind::None => {
                                    "Start a supported camera on the updated phone app"
                                }
                            });
                        });
                    });
                    ui.end_row();
                    segment_group(
                        ui,
                        "Rotate clockwise",
                        &mut self.settings.rotation,
                        [
                            (Rotation::None, "0°"),
                            (Rotation::Right, "90°"),
                            (Rotation::Half, "180°"),
                            (Rotation::Left, "270°"),
                        ],
                        45.0,
                    );
                    if columns == 1 {
                        ui.end_row();
                    }
                    segment_group(
                        ui,
                        if cfg!(target_os = "macos") {
                            "Phone FPS"
                        } else {
                            "Frame rate"
                        },
                        &mut self.settings.fps,
                        [(30, "30 FPS"), (60, "60 FPS")],
                        60.0,
                    );
                    ui.end_row();
                });
        });
        if self.settings != previous {
            self.save_settings();
        }
    }

    fn pairing_panel(&mut self, ui: &mut egui::Ui, status: &Status) {
        section(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            title(ui, "Phone connection");
            ui.add_space(2.0);
            let mut connection = self.connection;
            egui::ComboBox::from_id_salt("connection")
                .width(ui.available_width())
                .selected_text(connection.label())
                .show_ui(ui, |ui| {
                    for mode in Connection::ALL {
                        ui.selectable_value(&mut connection, mode, mode.label());
                    }
                });
            if connection != self.connection {
                self.change_connection(ui.ctx(), connection);
            }
            detail(ui, connection.instructions());
            if connection == Connection::AndroidAdb && self.adb.is_none() {
                if ui
                    .add_enabled(
                        self.adb_pending.is_none() && self.link.is_some(),
                        egui::Button::new(if self.adb_pending.is_some() {
                            "Connecting…"
                        } else {
                            "Connect Android USB"
                        }),
                    )
                    .clicked()
                {
                    self.connect_adb();
                }
            }
            if let Some(error) = &self.network_error {
                ui.label(
                    RichText::new(error)
                        .size(13.0)
                        .color(palette(ui.visuals().dark_mode).warning),
                );
            } else if self.link.is_none() {
                detail(ui, "Waiting for a network adapter.");
            }

            if let (Some(link), Some(qr)) = (&self.link, &self.qr) {
                ui.add_space(3.0);
                Frame::new()
                    .fill(palette(ui.visuals().dark_mode).ticket)
                    .corner_radius(12)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            if let Some(code) = &status.code {
                                let side = qr.size()[0] as f32 * 4.0;
                                let side = side.min(ui.available_width());
                                ui.add(
                                    egui::Image::new((qr.id(), Vec2::splat(side))).corner_radius(5),
                                );
                                ui.add_space(2.0);
                                ui.separator();
                                detail(ui, "Pairing code");
                                ui.label(
                                    RichText::new(format!("{}  {}", &code[..4], &code[4..]))
                                        .font(display_font(26.0))
                                        .color(palette(ui.visuals().dark_mode).ink),
                                );
                            } else {
                                ui.label(
                                    RichText::new("Phone paired")
                                        .font(display_font(16.0))
                                        .color(palette(ui.visuals().dark_mode).green),
                                );
                                if ui.button("Disconnect").clicked() {
                                    self.app.reset();
                                }
                            }
                        });
                    });
                if status.code.is_some() {
                    detail(ui, "On iPhone, use Safari to download the profile.");
                }
                ui.horizontal_wrapped(|ui| {
                    detail(ui, &link.url);
                    if ui
                        .link("Copy")
                        .on_hover_text("Copy phone address")
                        .clicked()
                    {
                        ui.ctx().copy_text(link.url.clone());
                    }
                });
                if let Some(certificate) = &link.certificate {
                    detail(ui, &format!("Phone certificate · {certificate}"));
                }
            }
            if connection != Connection::AndroidAdb {
                ui.add_space(2.0);
                ui.separator();
                egui::CollapsingHeader::new("Network adapter")
                    .default_open(self.link.is_none())
                    .show(ui, |ui| self.adapter_controls(ui));
            }
        });
    }

    fn connection_column(&mut self, ui: &mut egui::Ui, status: &Status) {
        self.pairing_panel(ui, status);
        ui.add_space(3.0);
        section(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            title(ui, "Camera output");
            self.camera_footer(ui);
            if matches!(self.camera_status, Some(Ok(()))) {
                detail(
                    ui,
                    if cfg!(target_os = "macos") {
                        "Choose VirtualCamera4Mac in your video app."
                    } else {
                        "Choose WEBKAM (Windows Virtual Camera) in your video app."
                    },
                );
            }
        });
    }

    fn adapter_controls(&mut self, ui: &mut egui::Ui) {
        let mut selected = self.selected;
        let label = selected
            .or(self.active_ip)
            .and_then(|ip| self.adapters.iter().find(|adapter| adapter.ip == ip))
            .map(|adapter| format!("{} · {}", adapter.name, adapter.ip))
            .unwrap_or_else(|| "Choose adapter".into());
        egui::ComboBox::from_id_salt("adapter")
            .width(ui.available_width())
            .truncate()
            .selected_text(label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected, None, "Auto-detect");
                for adapter in &self.adapters {
                    ui.selectable_value(
                        &mut selected,
                        Some(adapter.ip),
                        format!("{} · {}", adapter.name, adapter.ip),
                    )
                    .on_hover_text(&adapter.description);
                }
            });
        if selected != self.selected {
            self.selected = selected;
            self.refresh_link(ui.ctx(), false);
        }
        if ui.link("Refresh adapters").clicked() {
            self.adapters = network::adapters();
            self.refresh_link(ui.ctx(), self.link.is_none());
        }
    }

    fn appearance_switch(&mut self, ui: &mut egui::Ui) {
        let before = self.appearance;
        let choices = [Appearance::System, Appearance::Light, Appearance::Dark]
            .map(|mode| (mode, mode.label()));
        ui.allocate_ui_with_layout(
            vec2(178.0, 34.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                segments(ui, "Appearance", &mut self.appearance, &choices, 56.0);
            },
        );
        if self.appearance != before {
            self.appearance.apply(ui.ctx());
            self.appearance.save(&self.directory);
            ui.ctx().request_repaint();
        }
    }

    fn camera_footer(&self, ui: &mut egui::Ui) {
        match &self.camera_status {
            Some(Ok(())) => {
                ui.label(
                    RichText::new(if cfg!(target_os = "macos") {
                        "Mac camera ready"
                    } else {
                        "Windows camera ready"
                    })
                    .size(12.0)
                    .color(palette(ui.visuals().dark_mode).green),
                )
                .on_hover_text(if cfg!(target_os = "macos") {
                    "Select VirtualCamera4Mac in your calling app. Camera output runs at 30 FPS."
                } else {
                    "Select WEBKAM (Windows Virtual Camera) in Discord or your video app."
                });
            }
            Some(Err(error)) => {
                let hint = if cfg!(target_os = "macos") {
                    "Open VirtualCamera4Mac from Applications and activate its camera extension."
                } else if error.contains("80040154") {
                    "Run Install WEBKAM to enable the virtual camera."
                } else {
                    "Windows couldn’t start the virtual camera. Check Camera privacy settings."
                };
                ui.label(
                    RichText::new(if cfg!(target_os = "macos") {
                        "Activate VirtualCamera4Mac"
                    } else {
                        "Camera setup needed"
                    })
                    .size(12.0)
                    .color(palette(ui.visuals().dark_mode).warning),
                )
                .on_hover_text(error);
                detail(ui, hint);
            }
            None => {
                ui.label(
                    RichText::new("Starting virtual camera…")
                        .small()
                        .color(palette(ui.visuals().dark_mode).muted),
                );
            }
        }
    }
}

impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if Instant::now() >= self.next_motion_check {
            self.next_motion_check = Instant::now() + Duration::from_secs(2);
            let enabled = appearance::motion_enabled();
            if enabled != self.motion {
                self.motion = enabled;
                appearance::apply_motion(ctx, enabled);
            }
        }
        for result in self.camera.try_iter() {
            self.camera_status = Some(result);
        }
        if self.adb_pending.is_some() {
            match self.adb_pending.as_mut().unwrap().try_recv() {
                Ok(Ok(forward)) => {
                    self.adb = Some(forward);
                    self.adb_pending = None;
                    self.qr = self
                        .link
                        .as_ref()
                        .and_then(|link| qr_texture(ctx, &link.url));
                }
                Ok(Err(error)) => {
                    self.network_error = Some(error.to_string());
                    self.adb_pending = None;
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.adb_pending = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }
        if self.connection != Connection::AndroidAdb && Instant::now() >= self.next_refresh {
            self.next_refresh = Instant::now() + Duration::from_secs(2);
            let adapters = network::adapters();
            if adapters != self.adapters {
                self.adapters = adapters;
                self.refresh_link(ctx, false);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let themes = (ui.ctx().theme(), ui.ctx().system_theme());
        if self.caption_theme != Some(themes) {
            appearance::title_bar(frame, themes.0 == egui::Theme::Dark);
            self.caption_theme = Some(themes);
        }
        let (status, frame) = self.app.snapshot();
        self.settings = status.settings;
        self.load_preview(ui.ctx(), frame);
        let delay = if status.live {
            1000 / u64::from(status.settings.fps)
        } else {
            400
        };
        ui.ctx().request_repaint_after(Duration::from_millis(delay));
        let margin = if ui.available_width() < 590.0 { 12 } else { 24 };
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(palette(ui.visuals().dark_mode).background)
                    .inner_margin(egui::Margin::symmetric(margin, 12)),
            )
            .show_inside(ui, |panel| {
                let bounds = panel.available_rect_before_wrap();
                let centered = egui::Rect::from_center_size(
                    bounds.center(),
                    vec2(bounds.width().min(1480.0), bounds.height()),
                );
                let mut workspace = panel.new_child(egui::UiBuilder::new().max_rect(centered));
                let ui = &mut workspace;
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(Vec2::splat(36.0), egui::Sense::hover());
                    egui::Image::new(&self.brand).paint_at(ui, mark);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("WEBKAM").font(display_font(21.0)));
                        detail(ui, "Camera studio");
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.appearance_switch(ui);
                    });
                });
                ui.add_space(3.0);
                ui.separator();
                ui.add_space(5.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        if width < 760.0 {
                            self.preview_panel(ui, &status);
                            ui.add_space(3.0);
                            segments(
                                ui,
                                "Studio controls",
                                &mut self.compact_page,
                                &[
                                    (CompactPage::Phone, "Connection"),
                                    (CompactPage::Video, "Video settings"),
                                ],
                                ((width - 10.0) / 2.0).floor(),
                            );
                            match self.compact_page {
                                CompactPage::Phone => self.connection_column(ui, &status),
                                CompactPage::Video => self.video_controls(ui, &status),
                            }
                        } else {
                            let left = (width * 0.28).clamp(236.0, 340.0);
                            let gap = 16.0;
                            let right = width - left - gap;
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                ui.allocate_ui_with_layout(
                                    vec2(left, 0.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.set_width(left);
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        self.connection_column(ui, &status);
                                    },
                                );
                                ui.allocate_ui_with_layout(
                                    vec2(right, 0.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.set_width(right);
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        self.preview_panel(ui, &status);
                                        ui.add_space(3.0);
                                        self.video_controls(ui, &status);
                                    },
                                );
                            });
                        }
                        ui.add_space(4.0);
                    });
            });
    }
}

fn section(ui: &egui::Ui) -> Frame {
    Frame::new()
        .fill(palette(ui.visuals().dark_mode).surface)
        .stroke(Stroke::new(1.0_f32, palette(ui.visuals().dark_mode).border))
        .corner_radius(15)
        .inner_margin(16)
}

fn display_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("Display".into()))
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(display_font(17.0)));
}

fn detail(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(13.0)
                .color(palette(ui.visuals().dark_mode).muted),
        )
        .wrap(),
    );
}

fn transition(ui: &egui::Ui, name: &str, active: bool) -> f32 {
    let duration = ui.style().animation_time;
    if duration == 0.0 {
        return if active { 1.0 } else { 0.0 };
    }
    ui.ctx().animate_bool_with_time_and_easing(
        ui.id().with(name),
        active,
        duration,
        egui::emath::easing::quadratic_out,
    )
}

fn segment_group<T: Copy + PartialEq, const N: usize>(
    ui: &mut egui::Ui,
    label: &str,
    current: &mut T,
    choices: [(T, &str); N],
    width: f32,
) {
    ui.vertical(|ui| {
        ui.set_width(N as f32 * (width + 2.0) + 4.0);
        ui.label(
            RichText::new(label)
                .color(palette(ui.visuals().dark_mode).muted)
                .size(13.0),
        );
        segments(ui, label, current, &choices, width);
    });
}

fn segments<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    label: &str,
    current: &mut T,
    choices: &[(T, &str)],
    width: f32,
) {
    let p = palette(ui.visuals().dark_mode);
    ui.push_id(label, |ui| {
        Frame::new()
            .fill(p.track)
            .corner_radius(10)
            .inner_margin(3)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let selected_index = choices
                        .iter()
                        .position(|&(value, _)| value == *current)
                        .unwrap_or(0);
                    let duration = ui.style().animation_time;
                    let position = if duration > 0.0 {
                        ui.ctx().animate_value_with_time(
                            ui.id().with("selection"),
                            selected_index as f32,
                            duration,
                        )
                    } else {
                        selected_index as f32
                    };
                    let thumb = ui.painter().add(egui::Shape::Noop);
                    let mut first = egui::Rect::NOTHING;
                    let mut second = egui::Rect::NOTHING;
                    for (index, &(value, text)) in choices.iter().enumerate() {
                        let weight = (1.0 - (position - index as f32).abs()).clamp(0.0, 1.0);
                        let button = egui::Button::selectable(
                            *current == value,
                            RichText::new(text)
                                .font(display_font(13.0))
                                .color(p.muted.lerp_to_gamma(p.selected_ink, weight)),
                        )
                        .min_size(vec2(width, 28.0))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .corner_radius(7);
                        let response = ui.add(button);
                        if response.hovered() || response.has_focus() {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.0_f32, p.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if index == position.floor() as usize {
                            first = response.rect;
                        }
                        if index == position.ceil() as usize {
                            second = response.rect;
                        }
                        if response.clicked() {
                            *current = value;
                        }
                    }
                    if first.is_positive() && second.is_positive() {
                        let rect = egui::Rect::from_min_max(
                            first.min.lerp(second.min, position.fract()),
                            first.max.lerp(second.max, position.fract()),
                        );
                        ui.painter()
                            .set(thumb, egui::Shape::rect_filled(rect, 7, p.selected));
                    }
                });
            });
    });
}

fn qr_texture(ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
    let qr = qrcode::QrCode::new(url).ok()?;
    let canvas = qr.width() + 8;
    let width = qr.width();
    let mut pixels = vec![Color32::WHITE; canvas * canvas];
    for y in 0..width {
        for x in 0..width {
            if qr[(x, y)] == qrcode::Color::Dark {
                pixels[(y + 4) * canvas + x + 4] = STAGE;
            }
        }
    }
    let image = ColorImage::new([canvas, canvas], pixels);
    Some(ctx.load_texture("Pairing QR", image, TextureOptions::NEAREST))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_segments_select_and_restore_each_mode() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        appearance::install(&ctx);
        let mut mode = Appearance::System;
        let choices =
            [Appearance::System, Appearance::Light, Appearance::Dark].map(|m| (m, m.label()));
        let directory = std::env::temp_dir().join(format!("WEBKAM-theme-{}", uuid::Uuid::new_v4()));
        for target in [Appearance::Light, Appearance::Dark, Appearance::System] {
            let mut draw = |events| {
                ctx.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        segments(ui, "Appearance", &mut mode, &choices, 56.0);
                    },
                )
            };
            let frame = draw(Vec::new());
            let point = frame
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        if text.galley.text() == target.label() {
                            return Some(text.pos + text.galley.size() * 0.5);
                        }
                    }
                    None
                })
                .expect("each theme choice is visible without opening a menu");
            draw(vec![egui::Event::PointerMoved(point)]);
            for pressed in [true, false] {
                draw(vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
            assert!(mode == target);
            mode.apply(&ctx);
            mode.save(&directory);
            assert!(Appearance::load(&directory) == target);
        }
        fs::remove_file(directory.join("appearance.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
