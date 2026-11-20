//! Image and mask format decoders (`.wcg`, `.lwg`, `.msk`, `.bmp`).

pub mod bmp;
pub mod lwg;
pub mod msk;
pub mod wcg;

pub use bmp::{decode_bmp, decode_bmp_with_options, BmpDecodeOptions, BmpDecoded};
pub use lwg::{decode_lwg, LwgImage, LwgLayer};
pub use msk::{decode_msk, AlphaMask};
pub use wcg::{decode_wcg, decode_wcg_with_info, WcgDecoded};
