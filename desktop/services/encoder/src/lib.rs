use core_types::{EncodeJob, EncodeResult, VideoCodec, VideoEncoderFactory};

#[cfg(feature = "h264")]
pub mod h264;

#[cfg(any(feature = "vp8", feature = "vp9"))]
mod vpx_common;

#[cfg(feature = "vp9")]
#[path = "vp9_vpx.rs"]
pub mod vp9;

#[cfg(feature = "vp8")]
#[path = "vp8_vpx.rs"]
pub mod vp8;
