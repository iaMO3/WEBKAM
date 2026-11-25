use crate::shared::*;
use std::{
    fs::OpenOptions,
    io::Read,
    ptr,
    sync::{Arc, Mutex, OnceLock, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{E_FAIL, S_OK},
        Media::{KernelStreaming::PINNAME_VIDEO_CAPTURE, MediaFoundation::*},
        System::Com::{
            COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, StructuredStorage::PROPVARIANT,
        },
    },
    core::{AgileReference, GUID, HRESULT, IUnknown, Interface, Ref, Result, Weak, implement},
};

pub const STREAM_ID: u32 = 0;
type Token = Option<AgileReference<IUnknown>>;

struct FeedFrame {
    size: Size,
    pixels: Vec<u8>,
    time: Instant,
}
type Feed = RwLock<Option<Arc<FeedFrame>>>;

fn read_feed(pipe: &mut impl Read, feed: &Feed) -> std::io::Result<()> {
    loop {
        let mut header = [0; HEADER_BYTES];
        pipe.read_exact(&mut header)?;
        if header == [0; HEADER_BYTES] {
            *feed.write().unwrap() = None;
            continue;
        }
        let size = Size::new(
            u16::from_le_bytes([header[0], header[1]]) as usize,
            u16::from_le_bytes([header[2], header[3]]) as usize,
        );
        if !SIZES.contains(&size) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unknown video size",
            ));
        }
        let mut pixels = vec![0; size.bytes()];
        pipe.read_exact(&mut pixels)?;
        *feed.write().unwrap() = Some(Arc::new(FeedFrame {
            size,
            pixels,
            time: Instant::now(),
        }));
    }
}

fn feed() -> &'static Feed {
    static FEED: OnceLock<Feed> = OnceLock::new();
    FEED.get_or_init(|| {
        let thread = thread::Builder::new().name("PocketCam frames".into());
        let _ = thread.spawn(|| {
            let feed = feed();
            loop {
                if let Ok(mut pipe) = OpenOptions::new().read(true).open(PIPE_NAME) {
                    let _ = read_feed(&mut pipe, feed);
                }
                *feed.write().unwrap() = None;
                thread::sleep(Duration::from_millis(350));
            }
        });
        RwLock::new(None)
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mode {
    pub size: Size,
    pub fps: u32,
}

impl Mode {
    pub fn from_mf(media_type: &IMFMediaType) -> Result<Self> {
        let size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE)? };
        let rate = unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE)? };
        let size = Size::new((size >> 32) as usize, (size as u32) as usize);
        let fps = (rate >> 32) as u32;
        if !SIZES.contains(&size)
            || ![30, 60].contains(&fps)
            || rate as u32 != 1
            || unsafe { media_type.GetGUID(&MF_MT_SUBTYPE)? } != MFVideoFormat_NV12
        {
            return Err(MF_E_INVALIDMEDIATYPE.into());
        }
        Ok(Self { size, fps })
    }
}

fn make_media_type(size: Size, fps: u32) -> Result<IMFMediaType> {
    unsafe {
        let media_type = MFCreateMediaType()?;
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        media_type.SetUINT64(
            &MF_MT_FRAME_SIZE,
            ((size.width as u64) << 32) | size.height as u64,
        )?;
        let rate = ((fps as u64) << 32) | 1;
        media_type.SetUINT64(&MF_MT_FRAME_RATE, rate)?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE_RANGE_MIN, rate)?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE_RANGE_MAX, rate)?;
        media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media_type.SetUINT32(&MF_MT_DEFAULT_STRIDE, size.width as u32)?;
        media_type.SetUINT32(&MF_MT_SAMPLE_SIZE, size.bytes() as u32)?;
        media_type.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        media_type.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1)?;
        media_type.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?;
        media_type.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
        media_type.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
        media_type.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
        Ok(media_type)
    }
}

pub fn descriptor() -> Result<IMFStreamDescriptor> {
    let mut modes = Vec::new();
    for fps in [30, 60] {
        for size in SIZES {
            modes.push(Some(make_media_type(size, fps)?));
        }
    }
    unsafe {
        let descriptor = MFCreateStreamDescriptor(STREAM_ID, &modes)?;
        descriptor
            .GetMediaTypeHandler()?
            .SetCurrentMediaType(modes[0].as_ref())?;
        descriptor.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
        descriptor.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, STREAM_ID)?;
        descriptor.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
        descriptor.SetUINT32(&MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, 1)?;
        Ok(descriptor)
    }
}

struct Timeline {
    next_worker: u64,
    sender: Option<mpsc::SyncSender<Token>>,
    mode: Option<Mode>,
    shutdown: bool,
}

struct SampleQueue(IMFMediaEventQueue);
// Media Foundation's event queue supports concurrent access from MTA workers.
unsafe impl Send for SampleQueue {}
unsafe impl Sync for SampleQueue {}

#[implement(IMFMediaStream2)]
pub struct Stream {
    pub events: IMFMediaEventQueue,
    pub descriptor: IMFStreamDescriptor,
    source: Weak<IMFMediaSource>,
    timeline: Arc<Mutex<Timeline>>,
}

impl Stream {
    pub fn new(descriptor: IMFStreamDescriptor, source: Weak<IMFMediaSource>) -> Result<Self> {
        Ok(Self {
            events: unsafe { MFCreateEventQueue()? },
            descriptor,
            source,
            timeline: Arc::new(Mutex::new(Timeline {
                next_worker: 0,
                sender: None,
                mode: None,
                shutdown: false,
            })),
        })
    }

    pub fn set_running(&self, mode: Option<Mode>) -> Result<()> {
        let mut state = self.timeline.lock().unwrap();
        if state.shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        if mode == state.mode {
            return Ok(());
        }
        state.next_worker = state.next_worker.wrapping_add(1);
        if let Some(mode) = mode {
            let (sender, reader) = mpsc::sync_channel(8);
            let timeline = self.timeline.clone();
            let events = SampleQueue(self.events.clone());
            let id = state.next_worker;
            thread::Builder::new()
                .name("PocketCam samples".into())
                .spawn(move || deliver_samples(events, timeline, id, reader, mode))
                .map_err(|_| E_FAIL)?;
            state.sender = Some(sender);
        } else {
            state.sender = None;
        }
        state.mode = mode;
        Ok(())
    }

    pub fn shutdown(&self) -> Result<()> {
        let mut state = self.timeline.lock().unwrap();
        if state.shutdown {
            return Ok(());
        }
        state.shutdown = true;
        state.sender = None;
        unsafe { self.events.Shutdown() }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn deliver_samples(
    events: SampleQueue,
    timeline: Arc<Mutex<Timeline>>,
    id: u64,
    requests: mpsc::Receiver<Token>,
    mode: Mode,
) {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    let mut clock = None;
    let fps = u64::from(mode.fps);
    let mut scaled = vec![0; mode.size.bytes()];
    let mut last_scaled = None;
    for token in requests {
        let (start, time, index) =
            clock.get_or_insert_with(|| (Instant::now(), unsafe { MFGetSystemTime() }, 0_u64));
        // Skip slots when the consumer stops requesting samples. Never send a catch-up burst.
        let elapsed = start.elapsed().as_nanos() as u64;
        *index = (*index).max(elapsed * fps / 1_000_000_000);
        let deadline = *start + Duration::from_nanos(*index * 1_000_000_000 / fps);
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        let state = timeline.lock().unwrap();
        if state.shutdown || state.sender.is_none() || state.next_worker != id {
            break;
        }
        let offset = *index * 10_000_000 / fps;
        *index += 1;
        let duration = *index * 10_000_000 / fps - offset;
        let result = unsafe {
            make_sample(
                token,
                mode,
                &mut scaled,
                &mut last_scaled,
                *time + offset as i64,
                duration as i64,
            )
            .and_then(|sample| {
                events
                    .0
                    .QueueEventParamUnk(MEMediaSample.0 as u32, &GUID::zeroed(), S_OK, &sample)
            })
        };
        if let Err(error) = result {
            let _ = unsafe {
                events.0.QueueEventParamVar(
                    MEError.0 as u32,
                    &GUID::zeroed(),
                    error.code(),
                    ptr::null(),
                )
            };
        }
    }
    if initialized {
        unsafe { CoUninitialize() };
    }
}

fn clear_frame(size: Size, data: &mut [u8]) {
    data[..size.pixels()].fill(16);
    data[size.pixels()..].fill(128);
}

fn fit_frame(from: Size, data: &[u8], to: Size, output: &mut [u8]) {
    clear_frame(to, output);
    let width = to.width.min(from.width * to.height / from.height) & !1;
    let height = to.height.min(from.height * to.width / from.width) & !1;
    let left = ((to.width - width) / 2) & !1;
    let top = ((to.height - height) / 2) & !1;
    let x: Vec<_> = (0..width)
        .map(|column| column * from.width / width)
        .collect();
    for row in 0..height {
        let offset = row * from.height / height * from.width;
        let dst = (top + row) * to.width + left;
        for col in 0..width {
            output[dst + col] = data[offset + x[col]];
        }
    }
    for row in 0..height / 2 {
        let offset = from.pixels() + row * from.height / height * from.width;
        let dst = to.pixels() + (top / 2 + row) * to.width + left;
        for col in (0..width).step_by(2) {
            let source = offset + (x[col] & !1);
            output[dst + col..dst + col + 2].copy_from_slice(&data[source..source + 2]);
        }
    }
}

unsafe fn make_sample(
    token: Token,
    mode: Mode,
    scratch: &mut [u8],
    last_scaled: &mut Option<Arc<FeedFrame>>,
    timestamp: i64,
    duration: i64,
) -> Result<IMFSample> {
    let frame = feed()
        .read()
        .unwrap()
        .as_ref()
        .filter(|frame| frame.time.elapsed() < Duration::from_secs(2))
        .cloned();
    let size = mode.size;
    let pixels = match &frame {
        Some(frame) if frame.size == size => frame.pixels.as_slice(),
        Some(frame) => {
            if !last_scaled
                .as_ref()
                .is_some_and(|last| Arc::ptr_eq(frame, last))
            {
                fit_frame(frame.size, &frame.pixels, size, scratch);
                *last_scaled = Some(frame.clone());
            }
            scratch
        }
        None => {
            *last_scaled = None;
            clear_frame(size, scratch);
            scratch
        }
    };
    unsafe {
        let buffer = MFCreate2DMediaBuffer(
            size.width as u32,
            size.height as u32,
            MFVideoFormat_NV12.data1,
            false,
        )?;
        buffer.cast::<IMF2DBuffer>()?.ContiguousCopyFrom(pixels)?;
        buffer.SetCurrentLength(size.bytes() as u32)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(timestamp)?;
        sample.SetSampleDuration(duration)?;
        if let Some(token) = token {
            sample.SetUnknown(&MFSampleExtension_Token, &token.resolve()?)?;
        }
        Ok(sample)
    }
}

macro_rules! event_queue {
    ($type:ident) => {
        impl IMFMediaEventGenerator_Impl for $type {
            fn GetEvent(
                &self,
                flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS,
            ) -> Result<IMFMediaEvent> {
                unsafe { self.events.GetEvent(flags.0) }
            }
            fn BeginGetEvent(
                &self,
                callback: Ref<IMFAsyncCallback>,
                state: Ref<IUnknown>,
            ) -> Result<()> {
                unsafe { self.events.BeginGetEvent(callback.as_ref(), state.as_ref()) }
            }
            fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> Result<IMFMediaEvent> {
                unsafe { self.events.EndGetEvent(result.as_ref()) }
            }
            fn QueueEvent(
                &self,
                event: u32,
                kind: *const GUID,
                status: HRESULT,
                value: *const PROPVARIANT,
            ) -> Result<()> {
                unsafe { self.events.QueueEventParamVar(event, kind, status, value) }
            }
        }
    };
}
pub(crate) use event_queue;
event_queue!(Stream_Impl);

impl IMFMediaStream_Impl for Stream_Impl {
    fn GetMediaSource(&self) -> Result<IMFMediaSource> {
        self.source.upgrade().ok_or_else(|| MF_E_SHUTDOWN.into())
    }
    fn GetStreamDescriptor(&self) -> Result<IMFStreamDescriptor> {
        if self.timeline.lock().unwrap().shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        Ok(self.descriptor.clone())
    }
    fn RequestSample(&self, token: Ref<IUnknown>) -> Result<()> {
        let state = self.timeline.lock().unwrap();
        if state.shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        let sender = state.sender.as_ref().ok_or(MF_E_MEDIA_SOURCE_WRONGSTATE)?;
        let token = token.as_ref().map(AgileReference::new).transpose()?;
        sender.try_send(token).map_err(|_| MF_E_NOTACCEPTING.into())
    }
}

impl IMFMediaStream2_Impl for Stream_Impl {
    fn SetStreamState(&self, value: MF_STREAM_STATE) -> Result<()> {
        match value {
            MF_STREAM_STATE_RUNNING => {
                let media_type = unsafe {
                    self.descriptor
                        .GetMediaTypeHandler()?
                        .GetCurrentMediaType()?
                };
                self.set_running(Some(Mode::from_mf(&media_type)?))
            }
            MF_STREAM_STATE_STOPPED => self.set_running(None),
            _ => Err(MF_E_INVALID_STATE_TRANSITION.into()),
        }
    }
    fn GetStreamState(&self) -> Result<MF_STREAM_STATE> {
        let state = self.timeline.lock().unwrap();
        if state.shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        Ok(if state.sender.is_some() {
            MF_STREAM_STATE_RUNNING
        } else {
            MF_STREAM_STATE_STOPPED
        })
    }
}
