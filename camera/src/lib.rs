#![allow(non_snake_case)]
mod attributes;
#[path = "../../shared.rs"]
mod shared;
mod stream;

use std::{
    ffi::c_void,
    mem::ManuallyDrop,
    ptr,
    sync::{Mutex, OnceLock},
};
use stream::{STREAM_ID, Stream, event_queue};
use windows::{
    Win32::{
        Foundation::{
            CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_NOTIMPL,
            E_POINTER, ERROR_SET_NOT_FOUND, S_FALSE, S_OK,
        },
        Media::{
            KernelStreaming::{IKsControl, IKsControl_Impl, KSIDENTIFIER},
            MediaFoundation::*,
        },
        System::{
            Com::{
                IClassFactory, IClassFactory_Impl,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            Variant::VT_I8,
        },
    },
    core::{
        BOOL, ComObject, GUID, HRESULT, IUnknown, IUnknownImpl, Interface, Ref, Result, implement,
        w,
    },
};

use shared::SOURCE_CLSID;

#[implement(IMFMediaSourceEx, IMFActivate, IKsControl, IMFGetService)]
struct Source {
    events: IMFMediaEventQueue,
    attributes: IMFAttributes,
    presentation: IMFPresentationDescriptor,
    stream: OnceLock<ComObject<Stream>>,
    state: Mutex<SourceState>,
}

enum SourceState {
    Stopped,
    Started,
    Shutdown,
}

fn position(ticks: i64) -> PROPVARIANT {
    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_I8,
                Anonymous: PROPVARIANT_0_0_0 { hVal: ticks },
                ..Default::default()
            }),
        },
    }
}

impl Source {
    fn create() -> Result<IMFMediaSourceEx> {
        unsafe {
            let descriptor = stream::descriptor()?;
            let presentation = MFCreatePresentationDescriptor(Some(&[Some(descriptor.clone())]))?;
            presentation.SelectStream(0)?;
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 3)?;
            let attributes = attributes.ok_or(E_FAIL)?;
            attributes.SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )?;
            attributes.SetString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, w!("PocketCam"))?;
            let object = ComObject::new(Self {
                events: MFCreateEventQueue()?,
                attributes,
                presentation,
                stream: OnceLock::new(),
                state: Mutex::new(SourceState::Stopped),
            });
            let source: IMFMediaSource = object.cast()?;
            let stream = ComObject::new(Stream::new(descriptor, source.downgrade()?)?);
            object.stream.set(stream).map_err(|_| E_FAIL)?;
            Ok(object.into_interface())
        }
    }

    fn stream(&self) -> &ComObject<Stream> {
        self.stream
            .get()
            .expect("Camera stream is initialized before exposing the source")
    }

    fn ensure_open(&self) -> Result<()> {
        if matches!(*self.state.lock().unwrap(), SourceState::Shutdown) {
            Err(MF_E_SHUTDOWN.into())
        } else {
            Ok(())
        }
    }
}

event_queue!(Source_Impl);

impl IMFMediaSource_Impl for Source_Impl {
    fn GetCharacteristics(&self) -> Result<u32> {
        self.ensure_open()?;
        Ok(MFMEDIASOURCE_IS_LIVE.0 as u32)
    }
    fn CreatePresentationDescriptor(&self) -> Result<IMFPresentationDescriptor> {
        self.ensure_open()?;
        unsafe { self.presentation.Clone() }
    }
    fn Start(
        &self,
        presentation: Ref<IMFPresentationDescriptor>,
        format: *const GUID,
        _: *const PROPVARIANT,
    ) -> Result<()> {
        let presentation = presentation.as_ref().ok_or(E_INVALIDARG)?;
        let mut state = self.state.lock().unwrap();
        if matches!(*state, SourceState::Shutdown) {
            return Err(MF_E_SHUTDOWN.into());
        }
        let stream = self.stream();
        unsafe {
            if !format.is_null() && *format != GUID::zeroed() {
                return Err(MF_E_UNSUPPORTED_TIME_FORMAT.into());
            }
            let mut selected = BOOL::default();
            let mut descriptor = None;
            if presentation.GetStreamDescriptorCount()? != 1 {
                return Err(E_INVALIDARG.into());
            }
            presentation.GetStreamDescriptorByIndex(0, &mut selected, &mut descriptor)?;
            let descriptor = descriptor.ok_or(E_INVALIDARG)?;
            if !selected.as_bool() || descriptor.GetStreamIdentifier()? != STREAM_ID {
                return Err(E_INVALIDARG.into());
            }
            let media_type = descriptor.GetMediaTypeHandler()?.GetCurrentMediaType()?;
            stream
                .descriptor
                .GetMediaTypeHandler()?
                .SetCurrentMediaType(&media_type)?;
            let event = if matches!(*state, SourceState::Started) {
                MEUpdatedStream
            } else {
                MENewStream
            };
            let interface = stream.as_interface::<IMFMediaStream2>();
            stream.set_running(Some(stream::Mode::from_mf(&media_type)?))?;
            *state = SourceState::Started;
            let time = position(MFGetSystemTime());
            self.events.QueueEventParamVar(
                MESourceStarted.0 as u32,
                &GUID::zeroed(),
                S_OK,
                &time,
            )?;
            self.events
                .QueueEventParamUnk(event.0 as u32, &GUID::zeroed(), S_OK, &*interface)?;
            stream
                .events
                .QueueEventParamVar(MEStreamStarted.0 as u32, &GUID::zeroed(), S_OK, &time)
        }
    }
    fn Stop(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, SourceState::Shutdown) {
            return Err(MF_E_SHUTDOWN.into());
        }
        if matches!(*state, SourceState::Stopped) {
            return Ok(());
        }
        let stream = self.stream();
        stream.set_running(None)?;
        *state = SourceState::Stopped;
        unsafe {
            stream.events.QueueEventParamVar(
                MEStreamStopped.0 as u32,
                &GUID::zeroed(),
                S_OK,
                ptr::null(),
            )?;
            self.events.QueueEventParamVar(
                MESourceStopped.0 as u32,
                &GUID::zeroed(),
                S_OK,
                ptr::null(),
            )
        }
    }
    fn Pause(&self) -> Result<()> {
        self.ensure_open()?;
        Err(MF_E_INVALID_STATE_TRANSITION.into())
    }
    fn Shutdown(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, SourceState::Shutdown) {
            return Ok(());
        }
        *state = SourceState::Shutdown;
        let result = self.stream().shutdown();
        let events = unsafe { self.events.Shutdown() };
        result.and(events)
    }
}

impl IMFMediaSourceEx_Impl for Source_Impl {
    fn GetSourceAttributes(&self) -> Result<IMFAttributes> {
        self.ensure_open()?;
        Ok(self.attributes.clone())
    }
    fn GetStreamAttributes(&self, stream: u32) -> Result<IMFAttributes> {
        self.ensure_open()?;
        if stream != STREAM_ID {
            return Err(MF_E_INVALIDSTREAMNUMBER.into());
        }
        self.stream().descriptor.cast()
    }
    fn SetD3DManager(&self, _: Ref<IUnknown>) -> Result<()> {
        self.ensure_open()
    }
}

impl IMFActivate_Impl for Source_Impl {
    fn ActivateObject(&self, iid: *const GUID, out: *mut *mut c_void) -> Result<()> {
        self.ensure_open()?;
        if iid.is_null() || out.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { self.QueryInterface(iid, out).ok() }
    }
    fn ShutdownObject(&self) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn DetachObject(&self) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

impl IMFGetService_Impl for Source_Impl {
    fn GetService(&self, _: *const GUID, _: *const GUID, out: *mut *mut c_void) -> Result<()> {
        if !out.is_null() {
            unsafe { out.write(ptr::null_mut()) };
        }
        self.ensure_open()?;
        Err(MF_E_UNSUPPORTED_SERVICE.into())
    }
}

fn no_control(returned: *mut u32) -> Result<()> {
    if !returned.is_null() {
        unsafe { returned.write(0) };
    }
    Err(HRESULT::from_win32(ERROR_SET_NOT_FOUND.0).into())
}

impl IKsControl_Impl for Source_Impl {
    fn KsProperty(
        &self,
        _: *const KSIDENTIFIER,
        _: u32,
        _: *mut c_void,
        _: u32,
        returned: *mut u32,
    ) -> Result<()> {
        no_control(returned)
    }
    fn KsMethod(
        &self,
        _: *const KSIDENTIFIER,
        _: u32,
        _: *mut c_void,
        _: u32,
        returned: *mut u32,
    ) -> Result<()> {
        no_control(returned)
    }
    fn KsEvent(
        &self,
        _: *const KSIDENTIFIER,
        _: u32,
        _: *mut c_void,
        _: u32,
        returned: *mut u32,
    ) -> Result<()> {
        no_control(returned)
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> Result<()> {
        if iid.is_null() || out.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            out.write(ptr::null_mut());
        }
        if outer.as_ref().is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        unsafe { Source::create()?.query(iid, out).ok() }
    }
    fn LockServer(&self, _: BOOL) -> Result<()> {
        Ok(())
    }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if clsid.is_null() || iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    unsafe {
        out.write(ptr::null_mut());
        if *clsid != SOURCE_CLSID {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory.into();
        factory.query(iid, out)
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    // A frame-reader thread can still be blocked on a local pipe.
    S_FALSE
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    #[test]
    fn source_reader_gets_nv12_video() -> Result<()> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            let result = read_camera();
            MFShutdown()?;
            CoUninitialize();
            result
        }
    }

    fn read_camera() -> Result<()> {
        unsafe {
            let mut factory = ptr::null_mut();
            DllGetClassObject(&SOURCE_CLSID, &IClassFactory::IID, &mut factory).ok()?;
            let factory = IClassFactory::from_raw(factory);
            let activate: IMFActivate = factory.CreateInstance(None)?;
            assert_eq!(
                activate.GetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE)?,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            );
            let source: IMFMediaSource = activate.ActivateObject()?;
            let reader = MFCreateSourceReaderFromMediaSource(&source, None)?;
            let video = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            let index = std::env::var("POCKETCAM_TEST_MODE")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let mode = reader.GetNativeMediaType(video, index)?;
            assert_eq!(mode.GetGUID(&MF_MT_SUBTYPE)?, MFVideoFormat_NV12);
            let output = stream::Mode::from_mf(&mode)?;
            let size = output.size;
            reader.SetCurrentMediaType(video, None, &mode)?;
            let expect_feed = std::env::var_os("POCKETCAM_EXPECT_FEED").is_some();
            let mut pixels = vec![0; size.bytes()];
            let mut matched = false;
            for _ in 0..40 {
                let mut sample = None;
                let mut flags = 0;
                reader.ReadSample(video, 0, None, Some(&mut flags), None, Some(&mut sample))?;
                assert_eq!(flags & (MF_SOURCE_READERF_ERROR.0 as u32), 0);
                let Some(sample) = sample else {
                    continue;
                };
                assert_eq!(sample.GetTotalLength()?, size.bytes() as u32);
                sample
                    .GetBufferByIndex(0)?
                    .cast::<IMF2DBuffer>()?
                    .ContiguousCopyTo(&mut pixels)?;
                let black = pixels[..size.pixels()].iter().all(|&luma| luma == 16);
                if black != expect_feed {
                    matched = true;
                    let fps = i64::from(output.fps);
                    let mut prior = sample.GetSampleTime()?;
                    for _ in 0..5 {
                        let mut next = None;
                        reader.ReadSample(
                            video,
                            0,
                            None,
                            Some(&mut flags),
                            None,
                            Some(&mut next),
                        )?;
                        let next = next.expect("camera should return a paced sample");
                        let timestamp = next.GetSampleTime()?;
                        let duration = next.GetSampleDuration()?;
                        assert!(timestamp > prior);
                        assert!((duration * fps - 10_000_000).abs() < fps);
                        prior = timestamp;
                    }
                    break;
                }
            }
            assert!(matched, "The camera did not deliver the expected frame");
            drop(reader);
            source.Shutdown()?;
            Ok(())
        }
    }
}
