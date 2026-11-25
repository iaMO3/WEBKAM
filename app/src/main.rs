#![windows_subsystem = "windows"]
mod frames;
mod gui;
mod network;
#[path = "../../shared.rs"]
mod shared;
mod tls;
mod web;

use anyhow::{Context, Result};
use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::mpsc,
    thread,
};
use tokio::{net::TcpListener, runtime::Runtime};
use windows::{
    Win32::{
        Media::MediaFoundation::*,
        System::Com::*,
        UI::WindowsAndMessaging::{MB_ICONERROR, MessageBoxW},
    },
    core::{HSTRING, w},
};

struct Camera(Option<IMFVirtualCamera>);
impl Camera {
    fn start() -> windows::core::Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            let result = (|| {
                MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
                let result = (|| {
                    let camera = MFCreateVirtualCamera(
                        MFVirtualCameraType_SoftwareCameraSource,
                        MFVirtualCameraLifetime_Session,
                        MFVirtualCameraAccess_CurrentUser,
                        w!("PocketCam"),
                        &HSTRING::from(format!("{{{:?}}}", shared::SOURCE_CLSID)),
                        None,
                    )?;
                    camera.Start(None)?;
                    Ok(Self(Some(camera)))
                })();
                if result.is_err() {
                    let _ = MFShutdown();
                }
                result
            })();
            if result.is_err() {
                CoUninitialize();
            }
            result
        }
    }
}
impl Drop for Camera {
    fn drop(&mut self) {
        unsafe {
            if let Some(camera) = self.0.take() {
                let _ = camera.Stop();
                let _ = camera.Shutdown();
            }
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

struct CameraHost {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}
impl CameraHost {
    fn launch() -> Result<(Self, mpsc::Receiver<Result<(), String>>)> {
        let (signal, ready) = mpsc::channel();
        let (stop, wait) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("Windows camera registration".into())
            .spawn(move || {
                let camera = Camera::start();
                let _ = signal.send(camera.as_ref().map(|_| ()).map_err(ToString::to_string));
                let _ = wait.recv();
                drop(camera);
            })?;
        Ok((
            Self {
                stop,
                worker: Some(worker),
            },
            ready,
        ))
    }
}
impl Drop for CameraHost {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("Could not initialize HTTPS"))?;
    let directory = PathBuf::from(env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is missing")?)
        .join("PocketCam");
    let runtime = Runtime::new()?;
    let settings = gui::load_settings(&directory);
    let (send, receive) = tokio::sync::watch::channel(None);
    let app = {
        let _runtime = runtime.enter();
        frames::serve(receive).context("Could not start the Windows camera pipe. If PocketCam is open, close its other window.")?;
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

    let (_camera, camera_ready) = CameraHost::launch()?;
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
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(format!("{error:#}")),
                w!("PocketCam"),
                MB_ICONERROR,
            );
        }
    }
}
