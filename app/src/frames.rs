use crate::shared::*;
use axum::body::Bytes;
#[cfg(windows)]
use std::{io, sync::Arc, time::Duration};
#[cfg(windows)]
use tokio::{io::AsyncWriteExt, net::windows::named_pipe::ServerOptions, sync::watch};
#[cfg(windows)]
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
    },
    core::w,
};
use yuv::{BufferStoreMut, YuvBiPlanarImageMut, YuvConversionMode, YuvRange, YuvStandardMatrix};
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

pub struct Frame {
    pub size: Size,
    pub preview_size: Size,
    pub preview: Vec<u8>,
    pub(crate) wire: Vec<u8>,
}

pub fn decode(
    jpeg: Bytes,
    requested: Size,
    follow_phone: bool,
    rgb: &mut Vec<u8>,
) -> anyhow::Result<Frame> {
    anyhow::ensure!(SIZES.contains(&requested), "Unsupported frame size");
    let options = DecoderOptions::default()
        .set_max_width(1920)
        .set_max_height(1920)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(jpeg.as_ref()), options);
    decoder
        .decode_headers()
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    let info = decoder
        .info()
        .ok_or_else(|| anyhow::anyhow!("JPEG has no dimensions"))?;
    let size = Size::new(usize::from(info.width), usize::from(info.height));
    anyhow::ensure!(
        size == requested || (follow_phone && size == requested.rotated()),
        "JPEG does not match the selected camera mode"
    );
    rgb.resize(size.pixels() * 3, 0);
    decoder
        .decode_into(rgb)
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;

    let mut wire = vec![0; HEADER_BYTES + size.bytes()];
    wire[..2].copy_from_slice(&(size.width as u16).to_le_bytes());
    wire[2..HEADER_BYTES].copy_from_slice(&(size.height as u16).to_le_bytes());
    let (y, uv) = wire[HEADER_BYTES..].split_at_mut(size.pixels());
    let mut planes = YuvBiPlanarImageMut {
        y_plane: BufferStoreMut::Borrowed(y),
        uv_plane: BufferStoreMut::Borrowed(uv),
        y_stride: size.width as u32,
        uv_stride: size.width as u32,
        width: size.width as u32,
        height: size.height as u32,
    };
    yuv::rgb_to_yuv_nv12(
        &mut planes,
        rgb,
        (size.width * 3) as u32,
        YuvRange::Limited,
        YuvStandardMatrix::Bt709,
        YuvConversionMode::Balanced,
    )?;

    let step = if size.pixels() > HD.pixels() { 3 } else { 2 };
    let preview_size = Size::new(size.width / step, size.height / step);
    let mut preview = Vec::with_capacity(preview_size.pixels() * 3);
    for row in rgb.chunks_exact(size.width * 3).step_by(step) {
        for pixel in row.chunks_exact(3).step_by(step) {
            preview.extend_from_slice(pixel);
        }
    }
    Ok(Frame {
        size,
        preview_size,
        preview,
        wire,
    })
}

#[cfg(windows)]
fn new_pipe(first: bool) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    // The camera runs under Local Service. Other users receive no pipe access.
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:P(A;;GA;;;OW)(A;;GR;;;LS)(A;;GA;;;SY)"),
            1,
            &mut descriptor,
            None,
        )
        .map_err(io::Error::other)?;
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let pipe = ServerOptions::new()
            .access_inbound(false)
            .access_outbound(true)
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(16)
            .out_buffer_size((HEADER_BYTES + FULL_HD.bytes()) as u32)
            .create_with_security_attributes_raw(
                PIPE_NAME,
                (&attrs as *const SECURITY_ATTRIBUTES).cast_mut().cast(),
            );
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        pipe
    }
}

#[cfg(windows)]
pub fn serve(frames: watch::Receiver<Option<Arc<Frame>>>) -> io::Result<()> {
    let mut listener = new_pipe(true)?;
    tokio::spawn(async move {
        loop {
            if let Err(error) = listener.connect().await {
                eprintln!("Camera pipe: {error}");
                return;
            }
            let mut client = listener;
            let mut receiver = frames.clone();
            tokio::spawn(async move {
                loop {
                    let frame = receiver.borrow_and_update().clone();
                    let data = frame
                        .as_ref()
                        .map_or(&[0; HEADER_BYTES][..], |f| f.wire.as_slice());
                    let write =
                        tokio::time::timeout(Duration::from_secs(2), client.write_all(data)).await;
                    if !matches!(write, Ok(Ok(()))) || receiver.changed().await.is_err() {
                        return;
                    }
                }
            });
            loop {
                match new_pipe(false) {
                    Ok(next) => {
                        listener = next;
                        break;
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(300)).await,
                }
            }
        }
    });
    Ok(())
}
