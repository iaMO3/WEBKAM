#![cfg_attr(windows, windows_subsystem = "windows")]
mod frames;
mod gui;
#[cfg(target_os = "macos")]
mod mac_camera;
mod network;
#[path = "../../shared.rs"]
mod shared;
mod tls;
mod web;
#[cfg(windows)]
mod windows_camera;

use anyhow::{Context, Result};
use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};
use tokio::{net::TcpListener, runtime::Runtime};
fn run() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("Could not initialize HTTPS"))?;
    #[cfg(windows)]
    let directory = PathBuf::from(env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is missing")?)
        .join("PocketCam");
    #[cfg(target_os = "macos")]
    let directory = PathBuf::from(env::var_os("HOME").context("Home directory is missing")?)
        .join("Library/Application Support/WEBKAM");
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)
            .context("Could not create the WEBKAM settings directory")?;
    }
    let runtime = Runtime::new()?;
    let settings = gui::load_settings(&directory);
    let (send, receive) = tokio::sync::watch::channel(None);
    let app = {
        let _runtime = runtime.enter();
        #[cfg(windows)]
        frames::serve(receive).context(
            "Could not start the Windows camera pipe. If WEBKAM is open, close its other window.",
        )?;
        web::App::new(settings, send)
    };
    if env::args().any(|argument| argument == "--diagnostics") {
        let diagnostics = app.clone();
        runtime.spawn(async move {
            let address = SocketAddr::from((Ipv4Addr::LOCALHOST, network::DIAGNOSTICS_PORT));
            let _usb_test = network::loopback(diagnostics.clone()).await.ok();
            if let Ok(listener) = TcpListener::bind(address).await {
                let _ = axum::serve(
                    listener,
                    web::diagnostics(diagnostics, &format!("http://{address}")),
                )
                .await;
            }
        });
    }

    #[cfg(windows)]
    let (_camera, camera_ready) = windows_camera::CameraHost::launch()?;
    #[cfg(target_os = "macos")]
    let camera_ready = mac_camera::launch(receive)?;
    gui::show(
        app,
        settings,
        directory,
        runtime.handle().clone(),
        camera_ready,
    )
}
fn main() {
    if let Err(error) = run() {
        #[cfg(target_os = "macos")]
        mac_camera::show_error(&format!("{error:#}"));
        #[cfg(windows)]
        unsafe {
            use windows::{
                Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MessageBoxW},
                core::{HSTRING, w},
            };
            MessageBoxW(
                None,
                &HSTRING::from(format!("{error:#}")),
                w!("WEBKAM"),
                MB_ICONERROR,
            );
        }
    }
}
