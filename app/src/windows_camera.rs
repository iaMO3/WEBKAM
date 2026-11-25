use crate::shared;
use anyhow::Result;
use std::{sync::mpsc, thread};
use windows::{
    Win32::{Media::MediaFoundation::*, System::Com::*},
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
                        w!("WEBKAM"),
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

pub struct CameraHost {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}
impl CameraHost {
    pub fn launch() -> Result<(Self, mpsc::Receiver<Result<(), String>>)> {
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
