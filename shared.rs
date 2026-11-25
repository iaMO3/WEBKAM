pub const PIPE_NAME: &str = r"\\.\pipe\PocketCam.Frames.2.659F03EB";
pub const SOURCE_CLSID: windows::core::GUID =
    windows::core::GUID::from_u128(0x659f03eb_0bae_4d0a_972e_8b6a6f323807);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    pub const fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }
    pub const fn pixels(self) -> usize {
        self.width * self.height
    }
    pub const fn bytes(self) -> usize {
        self.pixels() * 3 / 2
    }
    pub const fn rotated(self) -> Self {
        Self::new(self.height, self.width)
    }
}

pub const FULL_HD: Size = Size::new(1920, 1080);
pub const HD: Size = Size::new(1280, 720);
pub const SIZES: [Size; 4] = [FULL_HD, FULL_HD.rotated(), HD, HD.rotated()];
pub const HEADER_BYTES: usize = 4;
