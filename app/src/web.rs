use crate::{
    frames::{self, Frame},
    shared::*,
};
use anyhow::Result;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, Query, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;

const PHONE: &str = include_str!("../../public/phone.html");
const PHONE_JS: &str = include_str!("../../public/phone.js");
const MANIFEST: &str = include_str!("../../public/manifest.webmanifest");
const OFFLINE_WORKER: &str = include_str!("../../public/worker.js");
const OFFLINE_PAGE: &str = include_str!("../../public/offline.html");
const CSS: &str = include_str!("../../public/style.css");
pub const ICON_192: &[u8] = include_bytes!("../../assets/pocketcam-192.png");
const ICON_512: &[u8] = include_bytes!("../../assets/pocketcam-512.png");
pub const TOUCH_ICON: &[u8] = include_bytes!("../../assets/pocketcam-touch.png");
const MAX_FRAME: usize = 1_500_000;
const STREAM_TIMEOUT: Duration = Duration::from_millis(1750);

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Quality {
    FullHd,
    Hd,
}
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Orientation {
    Landscape,
    Portrait,
    Auto,
}
#[derive(Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Rotation {
    #[default]
    None,
    Right,
    Half,
    Left,
}
impl Rotation {
    fn degrees(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Right => 90,
            Self::Half => 180,
            Self::Left => 270,
        }
    }
}
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Settings {
    pub quality: Quality,
    pub orientation: Orientation,
    pub fps: u32,
    #[serde(default)]
    pub rotation: Rotation,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            quality: Quality::FullHd,
            orientation: Orientation::Auto,
            fps: 30,
            rotation: Rotation::None,
        }
    }
}
impl Settings {
    pub fn size(self) -> Size {
        let size = if self.quality == Quality::FullHd {
            FULL_HD
        } else {
            HD
        };
        if self.orientation == Orientation::Portrait {
            size.rotated()
        } else {
            size
        }
    }
}

#[derive(Clone)]
pub struct App(Arc<Inner>);

struct Inner {
    state: Mutex<Session>,
    output: watch::Sender<Option<Arc<Frame>>>,
    incoming: watch::Sender<Option<Job>>,
    changed: watch::Sender<u64>,
}

#[derive(Clone)]
struct Job {
    stream: Uuid,
    socket: Uuid,
    settings: Settings,
    data: Bytes,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LightKind {
    #[default]
    None,
    Torch,
    Screen,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
pub struct LightState {
    pub light: LightKind,
    pub on: bool,
}

struct Session {
    light: LightState,
    light_request: Option<bool>,
    code: String,
    phone: Option<Uuid>,
    stream: Option<Uuid>,
    socket: Option<Uuid>,
    settings: Settings,
    attempts: VecDeque<Instant>,
    frames: VecDeque<Instant>,
    latest: Option<Arc<Frame>>,
}

#[derive(Serialize)]
pub struct Status {
    pub light: LightState,
    pub code: Option<String>,
    pub paired: bool,
    pub live: bool,
    pub fps: usize,
    pub settings: Settings,
}

fn pairing_code() -> String {
    format!("{:08}", rand::random_range(0..100_000_000))
}

impl App {
    pub fn new(settings: Settings, output: watch::Sender<Option<Arc<Frame>>>) -> Self {
        let (incoming, mut queue) = watch::channel::<Option<Job>>(None);
        let (changed, _) = watch::channel(0);
        let app = Self(Arc::new(Inner {
            state: Mutex::new(Session {
                light: LightState::default(),
                light_request: None,
                code: pairing_code(),
                phone: None,
                stream: None,
                socket: None,
                settings,
                attempts: VecDeque::new(),
                frames: VecDeque::new(),
                latest: None,
            }),
            incoming,
            output,
            changed,
        }));
        let decoder = app.clone();
        tokio::spawn(async move {
            let mut scratch = Vec::new();
            while queue.changed().await.is_ok() {
                let Some(job) = queue.borrow_and_update().clone() else {
                    continue;
                };
                let bytes = job.data;
                let result = tokio::task::spawn_blocking(move || {
                    let frame = frames::decode(
                        bytes,
                        job.settings.size(),
                        job.settings.orientation == Orientation::Auto,
                        &mut scratch,
                    )
                    .ok();
                    (scratch, frame)
                })
                .await;
                let Ok((buffer, frame)) = result else {
                    scratch = Vec::new();
                    continue;
                };
                scratch = buffer;
                if let Some(frame) = frame {
                    let mut active = decoder.0.state.lock().unwrap();
                    if active.stream != Some(job.stream)
                        || active.socket != Some(job.socket)
                        || active.settings != job.settings
                    {
                        continue;
                    }
                    let now = Instant::now();
                    active
                        .frames
                        .retain(|then| now.duration_since(*then) < Duration::from_secs(1));
                    active.frames.push_back(now);
                    let frame = Arc::new(frame);
                    active.latest = Some(frame.clone());
                    decoder.0.output.send_replace(Some(frame));
                }
            }
        });
        let expire = app.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(150));
            loop {
                interval.tick().await;
                let mut active = expire.0.state.lock().unwrap();
                if active
                    .frames
                    .back()
                    .is_some_and(|then| then.elapsed() > STREAM_TIMEOUT)
                {
                    expire.clear(&mut active);
                }
            }
        });
        app
    }

    fn clear(&self, active: &mut Session) {
        active.frames.clear();
        active.latest = None;
        self.0.output.send_replace(None);
    }
    fn notify(&self) {
        self.0
            .changed
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    pub fn reset(&self) {
        let mut active = self.0.state.lock().unwrap();
        active.phone = None;
        active.stream = None;
        active.socket = None;
        active.light = LightState::default();
        active.light_request = None;
        active.code = pairing_code();
        active.attempts.clear();
        self.clear(&mut active);
        self.notify();
    }

    pub fn toggle_light(&self) {
        let mut active = self.0.state.lock().unwrap();
        if active.socket.is_some() && active.light.light != LightKind::None {
            active.light_request = Some(!active.light.on);
            self.notify();
        }
    }

    pub fn settings(&self, settings: Settings) {
        let mut active = self.0.state.lock().unwrap();
        if active.settings != settings {
            active.settings = settings;
            self.clear(&mut active);
            self.notify();
        }
    }

    pub fn snapshot(&self) -> (Status, Option<Arc<Frame>>) {
        let active = self.0.state.lock().unwrap();
        let status = Status {
            light: active.light,
            code: active.phone.is_none().then(|| active.code.clone()),
            paired: active.phone.is_some(),
            live: active.latest.is_some(),
            fps: active
                .frames
                .iter()
                .filter(|t| t.elapsed() < Duration::from_secs(1))
                .count(),
            settings: active.settings,
        };
        (status, active.latest.clone())
    }
}

pub fn asset(kind: &'static str, content: impl IntoResponse) -> Response {
    ([(header::CONTENT_TYPE, kind)], content).into_response()
}

pub fn protected(router: Router, origin: &str) -> Router {
    protected_with_camera(router, origin, None)
}

pub fn protected_with_camera(router: Router, origin: &str, camera: Option<&str>) -> Router {
    let origin: Arc<str> = origin.into();
    let img_sources =
        camera.map_or_else(|| "'self'".to_string(), |camera| format!("'self' {camera}"));
    let connect_sources = match camera {
        Some(camera) => format!("'self' {camera}"),
        None => format!("'self' {}", origin.replacen("http", "ws", 1)),
    };
    let policy = HeaderValue::from_str(&format!(
        "default-src 'self'; connect-src {connect_sources}; img-src {img_sources}; media-src 'self' blob:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
    ))
    .expect("Policy contains socket addresses");
    router.layer(middleware::from_fn(move |request, next| {
        guard(origin.clone(), policy.clone(), request, next)
    }))
}
async fn guard(origin: Arc<str>, policy: HeaderValue, request: Request, next: Next) -> Response {
    let expected_host = origin.split_once("://").unwrap().1;
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| request.uri().authority().map(|value| value.as_str()));
    if host != Some(expected_host) {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    let needs_origin = request.method() == Method::POST || request.uri().path() == "/api/video";
    if needs_origin
        && request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            != Some(origin.as_ref())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::CONTENT_SECURITY_POLICY, policy);
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

#[derive(Clone)]
struct Phone {
    app: App,
    tls: bool,
}

pub async fn landscape() -> Response {
    asset(
        "image/png",
        include_bytes!("../../assets/studio-landscape.png").as_slice(),
    )
}

pub fn phone_router(app: App, origin: &str) -> Router {
    // The setup page is the HTTP listener on the same PC. Only the empty TLS
    // probe is readable cross-origin. Pairing and camera APIs keep their guards.
    let setup_origin = origin
        .strip_prefix("https://")
        .and_then(|address| address.rsplit_once(':'))
        .map(|(ip, _)| format!("http://{ip}:{}", crate::network::SETUP_PORT));
    let router = Router::new()
        .route("/", get(|| async { Html(PHONE) }))
        .route(
            "/phone.js",
            get(|| async { asset("text/javascript; charset=utf-8", PHONE_JS) }),
        )
        .route(
            "/frame-encoder.js",
            get(|| async {
                asset(
                    "text/javascript; charset=utf-8",
                    include_str!("../../public/frame-encoder.js"),
                )
            }),
        )
        .route("/style.css", get(css))
        .route(
            "/manifest.webmanifest",
            get(|| async { asset("application/manifest+json", MANIFEST) }),
        )
        .route(
            "/worker.js",
            get(|| async { asset("text/javascript; charset=utf-8", OFFLINE_WORKER) }),
        )
        .route("/offline.html", get(|| async { Html(OFFLINE_PAGE) }))
        .route("/icons/pocketcam-192.png", get(icon))
        .route("/icons/studio-landscape.png", get(landscape))
        .route(
            "/icons/pocketcam-512.png",
            get(|| async { asset("image/png", ICON_512) }),
        )
        .route(
            "/icons/apple-touch-icon.png",
            get(|| async { asset("image/png", TOUCH_ICON) }),
        )
        .route(
            "/certificate-check",
            get(move |headers: HeaderMap| {
                let setup_origin = setup_origin.clone();
                async move {
                    let Some(origin) = headers.get(header::ORIGIN).filter(|origin| {
                        setup_origin
                            .as_deref()
                            .is_some_and(|setup| origin.to_str().ok() == Some(setup))
                    }) else {
                        return StatusCode::FORBIDDEN.into_response();
                    };
                    (
                        [
                            (header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone()),
                            (header::VARY, HeaderValue::from_static("Origin")),
                        ],
                        StatusCode::NO_CONTENT,
                    )
                        .into_response()
                }
            }),
        )
        .route("/api/phone", get(phone_status))
        .route("/api/pair", post(pair))
        .route("/api/start", post(start))
        .route("/api/stop", post(stop))
        .route("/api/video", get(video))
        .layer(DefaultBodyLimit::max(2048))
        .with_state(Phone {
            app,
            tls: origin.starts_with("https:"),
        });
    protected(router, origin)
}

pub async fn css() -> Response {
    asset("text/css; charset=utf-8", CSS)
}

pub async fn icon() -> Response {
    asset("image/png", ICON_192)
}

fn phone_id(headers: &HeaderMap) -> Option<Uuid> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|item| item.trim().strip_prefix("pocketcam2="))
        .and_then(|id| id.parse().ok())
}

fn authenticated(active: &Session, id: Option<Uuid>) -> bool {
    active.phone.is_some() && active.phone == id
}

#[derive(Serialize)]
struct PhoneMode {
    width: usize,
    height: usize,
    fps: u32,
    follow_phone: bool,
    rotation: u16,
}

impl From<Settings> for PhoneMode {
    fn from(settings: Settings) -> Self {
        let size = settings.size();
        Self {
            width: size.width,
            height: size.height,
            fps: settings.fps,
            follow_phone: settings.orientation == Orientation::Auto,
            rotation: settings.rotation.degrees(),
        }
    }
}

async fn phone_status(State(phone): State<Phone>, headers: HeaderMap) -> Response {
    let active = phone.app.0.state.lock().unwrap();
    if !authenticated(&active, phone_id(&headers)) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(PhoneMode::from(active.settings)).into_response()
}

#[derive(Deserialize)]
struct Pairing {
    code: String,
}

async fn pair(State(phone): State<Phone>, Json(request): Json<Pairing>) -> Response {
    let mut active = phone.app.0.state.lock().unwrap();
    if active.phone.is_some() {
        return StatusCode::CONFLICT.into_response();
    }
    active
        .attempts
        .retain(|date| date.elapsed() < Duration::from_secs(30));
    if active.attempts.len() >= 10 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    active.attempts.push_back(Instant::now());
    if request.code != active.code {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let id = Uuid::new_v4();
    let secure = if phone.tls { "; Secure" } else { "" };
    let cookie = format!("pocketcam2={id}; HttpOnly; SameSite=Strict; Path=/{secure}");
    active.phone = Some(id);
    (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response()
}

async fn start(State(phone): State<Phone>, headers: HeaderMap) -> Response {
    let mut active = phone.app.0.state.lock().unwrap();
    if !authenticated(&active, phone_id(&headers)) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let id = Uuid::new_v4();
    active.stream = Some(id);
    active.socket = None;
    active.light = LightState::default();
    active.light_request = None;
    phone.app.clear(&mut active);
    phone.app.notify();
    Json(serde_json::json!({ "stream": id.to_string() })).into_response()
}

async fn stop(State(phone): State<Phone>, headers: HeaderMap) -> StatusCode {
    let mut active = phone.app.0.state.lock().unwrap();
    let stream = headers
        .get("x-pocketcam-stream")
        .and_then(|id| id.to_str().ok())
        .and_then(|id| id.parse().ok());
    if !authenticated(&active, phone_id(&headers)) || stream.is_none() || active.stream != stream {
        return StatusCode::UNAUTHORIZED;
    }
    active.stream = None;
    active.socket = None;
    active.light = LightState::default();
    active.light_request = None;
    phone.app.clear(&mut active);
    phone.app.notify();
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct StreamQuery {
    stream: String,
}

async fn video(
    State(phone): State<Phone>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Ok(stream) = query.stream.parse::<Uuid>() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let user = phone_id(&headers);
    {
        let active = phone.app.0.state.lock().unwrap();
        if !authenticated(&active, user) || active.stream != Some(stream) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    upgrade
        .max_frame_size(MAX_FRAME)
        .max_message_size(MAX_FRAME)
        .on_upgrade(move |socket| receive(socket, phone.app, stream, user))
}

async fn receive(mut socket: WebSocket, app: App, stream: Uuid, user: Option<Uuid>) {
    let id = Uuid::new_v4();
    let mut changed = app.0.changed.subscribe();
    let mut settings = {
        let mut active = app.0.state.lock().unwrap();
        if !authenticated(&active, user) || active.stream != Some(stream) {
            return;
        }
        active.socket = Some(id);
        active.light = LightState::default();
        active.light_request = None;
        active.settings
    };
    app.notify();
    if send_mode(&mut socket, settings).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            result = changed.changed() => {
                if result.is_err() { break; }
                let current = {
                    let mut active = app.0.state.lock().unwrap();
                    let current = active.stream == Some(stream) && active.socket == Some(id);
                    current.then(|| (active.settings, active.light_request.take()))
                };
                let Some((current, light)) = current else {
                    let _ = socket.send(Message::Close(Some(axum::extract::ws::CloseFrame {
                        code: 4001, reason: "Pairing ended".into(),
                    }))).await;
                    break;
                };
                if let Some(on) = light {
                    let command = serde_json::json!({ "setLight": on }).to_string();
                    if socket.send(Message::Text(command.into())).await.is_err() { break; }
                }
                if current != settings {
                    settings = current;
                    if send_mode(&mut socket, settings).await.is_err() { break; }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Binary(data))) => {
                        if data.starts_with(&[0xff, 0xd8]) {
                            app.0.incoming.send_replace(Some(Job { stream, socket: id, settings, data }));
                        }
                    }
                    Some(Ok(Message::Text(text))) if text.len() <= 128 => {
                        if let Ok(light) = serde_json::from_str::<LightState>(&text) {
                            let mut active = app.0.state.lock().unwrap();
                            if active.socket == Some(id) {
                                active.light = light;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {},
                }
            }
        }
    }
    let mut active = app.0.state.lock().unwrap();
    if active.socket == Some(id) {
        active.socket = None;
        active.light = LightState::default();
        active.light_request = None;
        app.clear(&mut active);
    }
}

async fn send_mode(socket: &mut WebSocket, settings: Settings) -> Result<()> {
    socket
        .send(Message::Text(
            serde_json::to_string(&PhoneMode::from(settings))?.into(),
        ))
        .await?;
    Ok(())
}

pub fn diagnostics(app: App, origin: &str) -> Router {
    protected(
        Router::new()
            .route(
                "/api/status",
                get(|State(app): State<App>| async move { Json(app.snapshot().0) }),
            )
            .route(
                "/api/settings",
                post(
                    |State(app): State<App>, Json(settings): Json<Settings>| async move {
                        if ![30, 60].contains(&settings.fps) {
                            return StatusCode::BAD_REQUEST;
                        }
                        app.settings(settings);
                        StatusCode::NO_CONTENT
                    },
                ),
            )
            .route(
                "/api/reset",
                post(|State(app): State<App>| async move {
                    app.reset();
                    StatusCode::NO_CONTENT
                }),
            )
            .with_state(app),
        origin,
    )
}
