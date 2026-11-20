//! `.bmp` decoder (`KhimeDL.exe` `0x430cd0`, `0x430d80`, `0x430f20..0x431400`).
//!
//! Supports uncompressed Bottom-Up Windows BMP (`1 / 8 / 24 / 32 bpp`):
//! - `32 bpp`: preserves raw `+3` alpha byte directly (CHS `grpo_tp.xfl` `.bmp` assets
//!   already store inverted alpha `0x00=opaque, 0xFF=transparent`).
//! - `1 / 8 / 24 bpp`: initializes `A = 0x00`, optionally applies a companion `w*h*4`
//!   buffer (`0x431370`), or applies the `0x00FF00` pure-green color key (`0x431400`).

use crate::error::{FujihimeError, Result};
use crate::gfx::surface::Surface;

/// Options controlling `1 / 8 / 24 bpp` BMP alpha post-processing.
#[derive(Debug, Clone, Copy, Default)]
pub struct BmpDecodeOptions<'a> {
    /// Optional companion alpha/pixel buffer (`w * h * 4` bytes, `0x431370`).
    pub companion_stream: Option<&'a [u8]>,
    /// Apply `0x431400` pure-green (`0x00FF00`) transparency color key on `1/8/24bpp`.
    pub green_colorkey: bool,
}

/// Decoded BMP result including original bit depth.
#[derive(Debug, Clone)]
pub struct BmpDecoded {
    pub bpp: u16,
    pub surface: Surface,
}

/// Decode a `.bmp` using default `KhimeDL.exe` options (raw 32bpp alpha preserved;
/// `1/8/24bpp` initialized to `A = 0x00` opaque unless `green_colorkey` is enabled).
pub fn decode_bmp(data: &[u8]) -> Result<Surface> {
    decode_bmp_with_options(data, BmpDecodeOptions::default()).map(|d| d.surface)
}

/// Decode a `.bmp` with explicit options and return [`BmpDecoded`].
pub fn decode_bmp_with_options(data: &[u8], opts: BmpDecodeOptions<'_>) -> Result<BmpDecoded> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return Err(FujihimeError::InvalidBmp {
            reason: "invalid BMP signature".to_string(),
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
                "unsupported BMP header: biSize={bi_size} {w}x{h} planes={planes} bpp={bpp} comp={comp}"
            ),
        });
    }
    if !matches!(bpp, 1 | 8 | 24 | 32) {
        return Err(FujihimeError::InvalidBmp {
            reason: format!("unsupported BMP bpp={bpp}"),
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
                reason: "truncated BMP palette".to_string(),
            });
        }
        palette = &data[pos..pos + pal_bytes];
        pos += pal_bytes;
    }

    let row_stride = ((width * (bpp as usize)).div_ceil(8) + 3) & !3;
    let needed = pos + row_stride * height;
    if needed > data.len() {
        return Err(FujihimeError::InvalidBmp {
            reason: format!("truncated BMP pixel data: need {needed}, have {}", data.len()),
        });
    }

    let mut out = vec![0u8; width * height * 4];
    for row in 0..height {
        let src_row = &data[pos + row * row_stride..pos + (row + 1) * row_stride];
        let dst_y = height - 1 - row;
        let dst_row = &mut out[dst_y * width * 4..(dst_y + 1) * width * 4];

        match bpp {
            32 => {
                dst_row.copy_from_slice(&src_row[..width * 4]);
            }
            24 => {
                for x in 0..width {
                    let s = x * 3;
                    let d = x * 4;
                    dst_row[d] = src_row[s];
                    dst_row[d + 1] = src_row[s + 1];
                    dst_row[d + 2] = src_row[s + 2];
                    dst_row[d + 3] = 0x00;
                }
            }
            8 => {
                for (x, &idx) in src_row.iter().enumerate().take(width) {
                    let p = (idx as usize) * 4;
                    if p + 3 > palette.len() {
                        return Err(FujihimeError::InvalidBmp {
                            reason: format!("palette index {idx} out of bounds"),
                        });
                    }
                    let d = x * 4;
                    dst_row[d] = palette[p];
                    dst_row[d + 1] = palette[p + 1];
                    dst_row[d + 2] = palette[p + 2];
                    dst_row[d + 3] = 0x00;
                }
            }
            1 => {
                for x in 0..width {
                    let v = if (src_row[x >> 3] & (0x80 >> (x & 7))) != 0 {
                        0xFF
                    } else {
                        0x00
                    };
                    let d = x * 4;
                    dst_row[d] = v;
                    dst_row[d + 1] = v;
                    dst_row[d + 2] = v;
                    dst_row[d + 3] = 0x00;
                }
            }
            _ => unreachable!(),
        }
    }

    if bpp != 32 {
        if let Some(comp_stream) = opts.companion_stream {
            if comp_stream.len() != out.len() {
                return Err(FujihimeError::InvalidBmp {
                    reason: format!(
                        "companion alpha stream length {} != expected {}",
                        comp_stream.len(),
                        out.len()
                    ),
                });
            }
            out.copy_from_slice(comp_stream);
        } else if opts.green_colorkey {
            // 0x431400: (px & 0x00FFFFFF) == 0x00FF00 -> 0xFF00FF00 (transparent)
            for px in out.as_chunks_mut::<4>().0 {
                if px[0] == 0x00 && px[1] == 0xFF && px[2] == 0x00 {
                    px[3] = 0xFF;
                } else {
                    px[3] = 0x00;
                }
            }
        }
    }

    let surface = Surface::from_bgra(width as u32, height as u32, out)?;
    Ok(BmpDecoded { bpp, surface })
}
