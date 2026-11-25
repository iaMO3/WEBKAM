use crate::{
    tls::Certificates,
    web::{self, App},
};
use anyhow::{Context, Result, bail};
use axum::{
    Router,
    http::header,
    response::{Html, IntoResponse},
    routing::get,
};
use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};

pub const SETUP_PORT: u16 = 47744;
pub const CAMERA_PORT: u16 = 47745;
pub const ADB_PORT: u16 = 47746;
pub const DIAGNOSTICS_PORT: u16 = 47747;
const SETUP: &str = include_str!("../../public/setup.html");
const SETUP_JS: &str = include_str!("../../public/setup.js");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Network,
    IphoneUsb,
    AndroidTether,
    AndroidAdb,
}

impl Connection {
    #[cfg(windows)]
    pub const ALL: [Self; 4] = [
        Self::Network,
        Self::IphoneUsb,
        Self::AndroidTether,
        Self::AndroidAdb,
    ];
    #[cfg(target_os = "macos")]
    pub const ALL: [Self; 3] = [Self::Network, Self::IphoneUsb, Self::AndroidAdb];

    pub fn label(self) -> &'static str {
        match self {
            Self::Network => "Wi-Fi / Ethernet",
            Self::IphoneUsb => "iPhone USB",
            Self::AndroidTether => "Android USB tethering",
            Self::AndroidAdb => "Android USB (ADB)",
        }
    }
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Network => "Connect your phone and computer to the same trusted network.",
            Self::IphoneUsb => {
                #[cfg(target_os = "macos")]
                {
                    return "Connect your iPhone by USB, trust this Mac and enable Personal Hotspot. Select the iPhone adapter below.";
                }
                #[cfg(windows)]
                "Plug in your iPhone, tap Trust and turn on Personal Hotspot. WEBKAM Setup can install Apple USB support."
            }
            Self::AndroidTether => {
                "Plug in your Android phone and enable USB tethering under Hotspot & tethering. Choose the tethered adapter below."
            }
            Self::AndroidAdb => {
                "Plug in your Android phone, enable USB debugging and approve this PC on the phone."
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    pub description: String,
    pub ip: Ipv4Addr,
}

impl Adapter {
    pub fn iphone(&self) -> bool {
        let name = format!("{} {}", self.name, self.description).to_lowercase();
        self.ip.octets().starts_with(&[172, 20, 10])
            || name.contains("apple mobile")
            || name.contains("iphone")
    }
    pub fn android(&self) -> bool {
        let name = format!("{} {}", self.name, self.description).to_lowercase();
        name.contains("rndis")
            || name.contains("remote ndis")
            || name.contains("usb tether")
            || name.contains("android")
            || name.contains("usb ncm")
    }
}

pub fn adapters() -> Vec<Adapter> {
    let ignored = [
        "warp",
        "virtual",
        "vpn",
        "tun",
        "tap",
        "loopback",
        "wsl",
        "tailscale",
        "vethernet",
        "zerotier",
        "wireguard",
        "bluetooth",
        "vmware",
    ];
    let mut result = platform_adapters();
    result.retain(|adapter| {
        let name = format!("{} {}", adapter.name, adapter.description).to_lowercase();
        adapter.ip.is_private() && !ignored.iter().any(|term| name.contains(term))
    });
    result.sort_by_key(|adapter| {
        (
            adapter.iphone() || adapter.android(),
            adapter.name.clone(),
            adapter.ip,
        )
    });
    result.dedup_by_key(|adapter| adapter.ip);
    if let Ok(ip) = env::var("POCKETCAM_LAN_IP")
        .unwrap_or_default()
        .parse::<Ipv4Addr>()
    {
        if let Some(index) = result.iter().position(|adapter| adapter.ip == ip) {
            result.swap(0, index);
        }
    }
    result
}

#[cfg(windows)]
fn platform_adapters() -> Vec<Adapter> {
    use windows::Win32::{
        Foundation::ERROR_BUFFER_OVERFLOW,
        NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
        Networking::WinSock::{AF_INET, SOCKADDR_IN},
    };
    let mut needed = 15_000u32;
    for _ in 0..3 {
        // IP Helper writes a collection of aligned structs and links inside this buffer.
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let code = unsafe {
            GetAdaptersAddresses(u32::from(AF_INET.0), flags, None, Some(first), &mut needed)
        };
        if code == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if code != 0 {
            break;
        }
        let mut result = Vec::new();
        let mut current = first;
        // These pointers are owned by the live IP Helper buffer and are read only.
        unsafe {
            while let Some(adapter) = current.as_ref() {
                current = adapter.Next;
                if adapter.OperStatus != IfOperStatusUp {
                    continue;
                }
                let name = if adapter.FriendlyName.is_null() {
                    String::new()
                } else {
                    adapter.FriendlyName.to_string().unwrap_or_default()
                };
                let description = if adapter.Description.is_null() {
                    String::new()
                } else {
                    adapter.Description.to_string().unwrap_or_default()
                };
                let mut address = adapter.FirstUnicastAddress;
                while let Some(entry) = address.as_ref() {
                    address = entry.Next;
                    let raw = entry.Address.lpSockaddr;
                    if let Some(sockaddr) = raw.as_ref() {
                        if sockaddr.sa_family == AF_INET {
                            let data = &*raw.cast::<SOCKADDR_IN>();
                            let ip = Ipv4Addr::from(data.sin_addr.S_un.S_addr.to_ne_bytes());
                            result.push(Adapter {
                                name: name.clone(),
                                description: description.clone(),
                                ip,
                            });
                        }
                    }
                }
            }
        }
        return result;
    }
    Vec::new()
}

pub struct Link {
    pub url: String,
    pub certificate: Option<String>,
    tasks: Vec<JoinHandle<()>>,
}
impl Link {
    pub async fn close(mut self) {
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
    }
}
impl Drop for Link {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

pub async fn lan(ip: Ipv4Addr, directory: &Path, app: App) -> Result<Link> {
    let setup_addr = SocketAddr::from((ip, SETUP_PORT));
    let camera_addr = SocketAddr::from((ip, CAMERA_PORT));
    let bootstrap = TcpListener::bind(setup_addr)
        .await
        .with_context(|| format!("Can't open {setup_addr}"))?;
    let camera = std::net::TcpListener::bind(camera_addr)
        .with_context(|| format!("Can't open {camera_addr}"))?;
    camera.set_nonblocking(true)?;
    let certificate = Certificates::load(directory, ip)?;
    let tls = certificate.config().await?;
    let setup_origin = format!("http://{setup_addr}");
    let camera_origin = format!("https://{camera_addr}");
    let setup = setup_router(&setup_origin, &camera_origin, &certificate)?;
    let phone = web::phone_router(app, &camera_origin);
    let secure_server = axum_server::from_tcp_rustls(camera, tls)?;
    let tasks = vec![
        tokio::spawn(async move {
            let _ = axum::serve(bootstrap, setup).await;
        }),
        tokio::spawn(async move {
            let _ = secure_server.serve(phone.into_make_service()).await;
        }),
    ];
    Ok(Link {
        url: setup_origin,
        certificate: Some(certificate.label),
        tasks,
    })
}

fn setup_router(origin: &str, camera_origin: &str, certificate: &Certificates) -> Result<Router> {
    let setup = SETUP
        .replace("__CAMERA_URL__", camera_origin)
        .replace("__CERTIFICATE__", &certificate.label);
    let profile = certificate.profile(origin)?;
    let der = certificate.der()?;
    let router = Router::new()
        .route(
            "/",
            get({
                let setup = setup.clone();
                move || async move { Html(setup) }
            }),
        )
        .route("/iphone", get(move || async move { Html(setup) }))
        .route("/style.css", get(web::css))
        .route("/icons/pocketcam-192.png", get(web::icon))
        .route(
            "/setup.js",
            get(|| async { web::asset("text/javascript; charset=utf-8", SETUP_JS) }),
        )
        .route(
            "/pair.mobileconfig",
            get(move || async move {
                (
                    [
                        (header::CONTENT_TYPE, "application/x-apple-aspen-config"),
                        (
                            header::CONTENT_DISPOSITION,
                            "attachment; filename=WEBKAM.mobileconfig",
                        ),
                    ],
                    profile,
                )
                    .into_response()
            }),
        )
        .route(
            "/pocketcam.cer",
            get(move || async move {
                (
                    [
                        (header::CONTENT_TYPE, "application/x-x509-ca-cert"),
                        (
                            header::CONTENT_DISPOSITION,
                            "attachment; filename=WEBKAM.cer",
                        ),
                    ],
                    der,
                )
                    .into_response()
            }),
        );
    Ok(web::protected_with_camera(
        router,
        origin,
        Some(camera_origin),
    ))
}

pub async fn loopback(app: App) -> Result<Link> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, ADB_PORT));
    let listener = TcpListener::bind(addr)
        .await
        .context("Can't open the Android USB listener")?;
    let url = format!("http://{addr}");
    let router = web::phone_router(app, &url);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Ok(Link {
        url,
        certificate: None,
        tasks: vec![task],
    })
}

#[cfg(target_os = "macos")]
fn platform_adapters() -> Vec<Adapter> {
    let Ok(output) = std::process::Command::new("/sbin/ifconfig")
        .arg("-a")
        .output()
    else {
        return Vec::new();
    };
    let mut adapters = Vec::new();
    let mut name = "";
    let listing = String::from_utf8_lossy(&output.stdout);
    for line in listing.lines() {
        if !line.starts_with(char::is_whitespace) {
            name = line.split(':').next().unwrap_or("");
        } else if let Some(address) = line.trim().strip_prefix("inet ")
            && let Some(ip) = address
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
        {
            adapters.push(Adapter {
                name: name.into(),
                description: String::new(),
                ip,
            });
        }
    }
    adapters
}

#[cfg(target_os = "macos")]
fn adb_path() -> std::path::PathBuf {
    if let Some(path) = env::var_os("POCKETCAM_ADB") {
        return path.into();
    }
    [
        env::var_os("ANDROID_HOME")
            .map(std::path::PathBuf::from)
            .map(|d| d.join("platform-tools/adb")),
        env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .map(|d| d.join("Library/Android/sdk/platform-tools/adb")),
        Some("/opt/homebrew/bin/adb".into()),
        Some("/usr/local/bin/adb".into()),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
    .unwrap_or_else(|| "adb".into())
}

#[cfg(windows)]
fn adb_path() -> std::path::PathBuf {
    if let Some(override_path) = env::var_os("POCKETCAM_ADB") {
        return override_path.into();
    }
    if let Some(installed) = env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|dir| dir.join("platform-tools").join("adb.exe"))
        })
        .filter(|path| path.is_file())
    {
        return installed;
    }
    [
        env::var_os("ANDROID_HOME").map(std::path::PathBuf::from),
        env::var_os("ANDROID_SDK_ROOT").map(std::path::PathBuf::from),
        env::var_os("LOCALAPPDATA")
            .map(|base| std::path::PathBuf::from(base).join("Android").join("Sdk")),
    ]
    .into_iter()
    .flatten()
    .map(|directory| directory.join("platform-tools").join("adb.exe"))
    .find(|path| path.is_file())
    .unwrap_or_else(|| "adb.exe".into())
}

async fn adb(args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new(adb_path());
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(
        Duration::from_secs(8),
        command
            .args(["-d", "reverse"])
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("ADB timed out. Check the USB debugging prompt on the phone.")?
    .context("ADB could not start. Install Android Platform-Tools for your computer.")?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        bail!("ADB: {}", detail.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub async fn connect_adb() -> Result<AdbForward> {
    let port = format!("tcp:{ADB_PORT}");
    let mappings = adb(&["--list"]).await?;
    let present = mappings.lines().any(|line| {
        let mut fields = line.split_whitespace().rev();
        fields.next() == Some(port.as_str()) && fields.next() == Some(port.as_str())
    });
    if !present {
        adb(&["--no-rebind", &port, &port]).await?;
    }
    Ok(AdbForward)
}

pub struct AdbForward;
impl Drop for AdbForward {
    fn drop(&mut self) {
        use std::process::Stdio;
        let mut command = std::process::Command::new(adb_path());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let _ = command
            .args(["-d", "reverse", "--remove", &format!("tcp:{ADB_PORT}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}
