use crate::{frames::Frame, shared::HEADER_BYTES};
use std::{
    ffi::{CString, c_char, c_void},
    ptr::NonNull,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use tokio::sync::watch;

unsafe extern "C" {
    fn webkam_device() -> u32;
    fn webkam_open(device: u32) -> *mut c_void;
    fn webkam_close(output: *mut c_void);
    fn webkam_send(output: *mut c_void, width: usize, height: usize, data: *const u8) -> bool;
    fn webkam_reduce_motion() -> bool;
    fn webkam_error(message: *const c_char);
}

struct Sink(NonNull<c_void>);
impl Drop for Sink {
    fn drop(&mut self) {
        unsafe { webkam_close(self.0.as_ptr()) };
    }
}

pub fn motion_enabled() -> bool {
    unsafe { !webkam_reduce_motion() }
}

pub fn show_error(message: &str) {
    if let Ok(text) = CString::new(message) {
        unsafe { webkam_error(text.as_ptr()) };
    }
}

pub fn launch(
    mut frames: watch::Receiver<Option<Arc<Frame>>>,
) -> anyhow::Result<mpsc::Receiver<Result<(), String>>> {
    let (status, updates) = mpsc::channel();
    std::thread::Builder::new().name("Mac camera output".into()).spawn(move || {
        let mut device = 0;
        let mut sink = None;
        let mut previous = None;
        let mut next_probe = Instant::now();
        let mut retry = Instant::now();
        loop {
            let tick = Instant::now();
            if frames.has_changed().is_err() { break; }
            if Instant::now() >= next_probe {
                let detected = unsafe { webkam_device() };
                if detected != device { sink = None; device = detected; }
                next_probe = Instant::now() + Duration::from_secs(1);
            }
            let current = frames.borrow_and_update().clone();
            let live = current.is_some();
            if let Some(frame) = current {
                if device != 0 && sink.is_none() && Instant::now() >= retry {
                    sink = NonNull::new(unsafe { webkam_open(device) }).map(Sink);
                    retry = Instant::now() + Duration::from_secs(1);
                }
                if let Some(Sink(output)) = &sink {
                    let ok = unsafe { webkam_send(output.as_ptr(), frame.size.width, frame.size.height, frame.wire[HEADER_BYTES..].as_ptr()) };
                    if !ok { sink = None; }
                }
            } else { sink = None; }
            let result = if device == 0 {
                Err("Install VirtualCamera4Mac from the DMG and approve its camera extension.".to_owned())
            } else if live && sink.is_none() {
                Err("The camera output is busy or unavailable. Close other producers and retry.".to_owned())
            } else { Ok(()) };
            if previous.as_ref() != Some(&result) {
                previous = Some(result.clone());
                if status.send(result).is_err() { break; }
            }
            let period = if live { Duration::from_nanos(1_000_000_000 / 30) } else { Duration::from_millis(100) };
            std::thread::sleep(period.saturating_sub(tick.elapsed()));
        }
    })?;
    Ok(updates)
}
