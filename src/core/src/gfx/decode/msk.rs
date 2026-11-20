//! `.msk` rule transition mask decoder (`KhimeDL.exe` `0x430700`, `0x4307a0`, `0x430900..0x430c10`).

use crate::error::{FujihimeError, Result};
use crate::gfx::surface::Surface;

/// Top-down 8-bit rule transition mask decoded from an uncompressed BMP `.msk` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlphaMask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl AlphaMask {
    /// Apply this mask's values into the inverted-alpha (`+3`) channel of `dst`.
    ///
    /// Used by `0x448170` to prepare the transition source surface's alpha channel
    /// before running `mode = 0, sub = 2` (`0x4375f0`) rule wipes.
    pub fn apply_to_surface_alpha(&self, dst: &mut Surface) -> Result<()> {
        if dst.width() != self.width || dst.height() != self.height {
            return Err(FujihimeError::InvalidSurface {
                reason: format!(
                    "mask size {}x{} does not match surface {}x{}",
                    self.width,
                    self.height,
                    dst.width(),
                    dst.height()
                ),
            });
        }
        let bytes = dst.as_bytes_mut();
        for (px, &mask_val) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(self.data.iter()) {
            px[3] = mask_val;
        }
        Ok(())
    }
}

/// Decode an uncompressed `1 / 8 / 24 / 32 bpp` `.msk` BMP into a top-down [`AlphaMask`].
pub fn decode_msk(data: &[u8]) -> Result<AlphaMask> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return Err(FujihimeError::InvalidBmp {
            reason: "invalid BMP/MSK signature".to_string(),
        });
    }

    let bi_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
    let w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let h = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    let planes = u16::from_le_bytes([data[26], data[27]]);
    let bpp = u16::from_le_bytes([data[28], data[29]]);
    let comp = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);

    if bi_size != 40 || planes != 1 || comp != 0 || w <= 0 || h <= 0 {
        return Err(FujihimeError::InvalidBmp {
            reason: format!(
                "unsupported MSK header: biSize={bi_size} {w}x{h} planes={planes} bpp={bpp} comp={comp}"
            ),
        });
    }
    if !matches!(bpp, 1 | 8 | 24 | 32) {
        return Err(FujihimeError::InvalidBmp {
            reason: format!("unsupported MSK bpp={bpp}"),
        });
    }

    let width = w as usize;
    let height = h as usize;
    let mut pos = 54usize;
    let mut palette: &[u8] = &[];

    if bpp <= 8 {
        let mut clr_used = u32::from_le_bytes([data[46], data[47], data[48], data[49]]) as usize;
        if clr_used == 0 {
            clr_used = if bpp == 1 { 2 } else { 256 };
        }
        let pal_bytes = clr_used * 4;
        if pos + pal_bytes > data.len() {
            return Err(FujihimeError::InvalidBmp {
                reason: "truncated MSK palette".to_string(),
            });
        }
        palette = &data[pos..pos + pal_bytes];
        pos += pal_bytes;
    }

    let row_stride = ((width * (bpp as usize)).div_ceil(8) + 3) & !3;
    let needed = pos + row_stride * height;
    if needed > data.len() {
        return Err(FujihimeError::InvalidBmp {
            reason: format!("truncated MSK pixel data: need {needed}, have {}", data.len()),
        });
    }

    let mut mask_out = vec![0u8; width * height];
    for row in 0..height {
        let src_row = &data[pos + row * row_stride..pos + (row + 1) * row_stride];
        let dst_y = height - 1 - row;
        let dst_row = &mut mask_out[dst_y * width..(dst_y + 1) * width];

        match bpp {
            1 => {
                for (x, dst) in dst_row.iter_mut().enumerate() {
                    *dst = if (src_row[x >> 3] & (0x80 >> (x & 7))) != 0 {
                        0xFF
                    } else {
                        0x00
                    };
                }
            }
            8 => {
                for (x, dst) in dst_row.iter_mut().enumerate() {
                    let pal_off = (src_row[x] as usize) * 4;
                    if pal_off >= palette.len() {
                        return Err(FujihimeError::InvalidBmp {
                            reason: format!("palette index {} out of bounds", src_row[x]),
                        });
                    }
                    *dst = palette[pal_off]; // Blue channel
                }
            }
            24 => {
                for (x, dst) in dst_row.iter_mut().enumerate() {
                    *dst = src_row[x * 3]; // Blue channel per 0x430b50
                }
            }
            32 => {
                for (x, dst) in dst_row.iter_mut().enumerate() {
                    *dst = src_row[x * 4]; // Blue channel per 0x430c10
                }
            }
            _ => unreachable!(),
        }
    }

    Ok(AlphaMask {
        width: width as u32,
        height: height as u32,
        data: mask_out,
    })
}
