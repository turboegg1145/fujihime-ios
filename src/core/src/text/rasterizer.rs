//! Glyph rasterizer implementing all 6 modes of `KhimeDL.exe` (`0x44f080`..`0x45037f`).
//!
//! Modes:
//! - `0` (`0x44f1b0`): 1-bit `GGO_BITMAP`, supports `@` 90° CCW vertical rotation, no shadow.
//! - `1` (`0x44f530`): 65-level `GGO_GRAY8_BITMAP` quantized to 5-bit `0..=31` (`B=G=R=val`),
//!   `blend_mode = 0x16` color fade, supports `@` 90° CCW vertical rotation.
//! - `2` (`0x44f880`): 1-bit `GGO_BITMAP` with `(+3, +3)` pure-black hard drop shadow, supports `@` vertical.
//! - `3` (`0x44fca0`): Main dialogue default! 65-level `GGO_GRAY8_BITMAP`, horizontal only,
//!   inverted alpha `A = ~((g * 255) >> 6)`, Porter-Duff `Over` (`mode=0, sub=0`),
//!   forced monospace override (`gmCellIncX = is_dbcs ? font_h : (font_h >> 1)`, `gmCellIncY = font_h`).
//! - `4` (`0x44ff30`): Horizontal compatibility `TextOutA` path (used by `^a` animated glyphs).
//! - `5` (`0x450190`): Horizontal `TextOutA` with `(+3, +3)` pure-black drop shadow.

use crate::error::Result;
use crate::gfx::compositor::BlitParams;
use crate::gfx::surface::Surface;
use crate::text::freetype::{FontFace, FtPixelMode, RawFtGlyph};

/// 6 glyph rasterization modes dispatched by `0x44f080`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RasterMode {
    Mode0 = 0,
    Mode1 = 1,
    Mode2 = 2,
    #[default]
    Mode3 = 3,
    Mode4 = 4,
    Mode5 = 5,
}

impl RasterMode {
    /// Convert raw `u16` mode (`*(u16*)(glyph + 0x1ac)`) matching `0x44f080` switch (defaults to `Mode0`).
    pub const fn from_raw(val: u16) -> Self {
        match val {
            1 => Self::Mode1,
            2 => Self::Mode2,
            3 => Self::Mode3,
            4 => Self::Mode4,
            5 => Self::Mode5,
            _ => Self::Mode0,
        }
    }
}

/// Scale level height conversion matching `0x441b00(scale)`.
///
/// - `0` -> `base / 2`
/// - `1` -> `base`
/// - `2` -> `(base * 3) / 2`
/// - `3` -> `base * 2`
/// - `>= 4` -> `base * (scale - 1)`
#[inline]
pub const fn scale_font_height(base: u16, scale: u16) -> u16 {
    match scale {
        0 => base / 2,
        1 => base,
        2 => (base * 3) / 2,
        3 => base * 2,
        s => base * (s - 1),
    }
}

/// CP932 DBCS lead byte predicate (`0x452a2a`): `0x81..=0x9F || 0xE0..=0xFC`.
#[inline]
pub const fn is_cp932_lead(b: u8) -> bool {
    matches!(b, 0x81..=0x9F | 0xE0..=0xFC)
}

/// CP932 DBCS trail byte predicate (`0x452a3b`): `0x40..=0x7E || 0x80..=0xFC`.
#[inline]
pub const fn is_cp932_trail(b: u8) -> bool {
    matches!(b, 0x40..=0x7E | 0x80..=0xFC)
}

/// GBK DBCS lead byte predicate: `0x81..=0xFE`.
#[inline]
pub const fn is_gbk_lead(b: u8) -> bool {
    matches!(b, 0x81..=0xFE)
}

/// GBK DBCS trail byte predicate: `0x40..=0x7E || 0x80..=0xFE`.
#[inline]
pub const fn is_gbk_trail(b: u8) -> bool {
    matches!(b, 0x40..=0x7E | 0x80..=0xFE)
}

/// Return `true` when `ch` is single-byte halfwidth in CP932/GBK (ASCII or halfwidth katakana).
#[inline]
pub const fn is_halfwidth_char(ch: char) -> bool {
    ch.is_ascii() || (ch >= '\u{FF61}' && ch <= '\u{FF9F}')
}

/// Request parameters for rasterizing a single glyph.
#[derive(Debug, Clone, Copy)]
pub struct GlyphRequest {
    pub ch: char,
    pub is_dbcs: bool,
    pub pixel_height: u16,
    pub color_rgb: u32,
    pub bold: bool,
    pub italic: bool,
    pub mode: RasterMode,
    pub vertical: bool,
}

/// Rasterized glyph surface + GDI-compatible metrics + compositor parameters.
#[derive(Debug, Clone)]
pub struct RasterizedGlyph {
    pub surface: Surface,
    pub origin_x: i32,
    pub origin_y: i32,
    pub cell_inc_x: i16,
    pub cell_inc_y: i16,
    pub blend_params: BlitParams,
    pub is_empty_bitmap: bool,
}

#[inline]
fn color_bgr(color_rgb: u32) -> [u8; 3] {
    let r = ((color_rgb >> 16) & 0xFF) as u8;
    let g = ((color_rgb >> 8) & 0xFF) as u8;
    let b = (color_rgb & 0xFF) as u8;
    [b, g, r]
}

#[inline]
fn mono_bit_set(raw: &RawFtGlyph, x: usize, y: usize) -> bool {
    let byte = raw.buffer[y * raw.pitch + (x >> 3)];
    (byte & (0x80u8 >> (x & 7))) != 0
}

/// Quantize FreeType 8-bit gray (`0..=255`) into Win32 `GGO_GRAY8_BITMAP` (`0..=64`).
#[inline]
pub const fn quantize_gray8_to_64(gray: u8) -> u8 {
    (((gray as u32) * 64 + 127) / 255) as u8
}

/// Compute `mode 3` (`0x44fe0e..0x44fe23`) inverted alpha from `g ∈ 0..=64`:
/// `A = ~((g * 255) >> 6) & 0xFF`.
#[inline]
pub const fn mode3_inverted_alpha(g64: u8) -> u8 {
    !(((g64 as u32 * 255) >> 6) as u8)
}

/// Compute `mode 1` (`0x44f73c..0x44f758`) 5-bit grayscale value from `g ∈ 0..=64`:
/// `val = ((g * 255) >> 6) >> 3` (`0..=31`).
#[inline]
pub const fn mode1_gray5(g64: u8) -> u8 {
    (((g64 as u32 * 255) >> 6) >> 3) as u8
}

/// Rasterize a glyph using FreeType 2 following the exact semantics of `0x44f080`.
pub fn rasterize_glyph(face: &FontFace, req: &GlyphRequest) -> Result<RasterizedGlyph> {
    let font_h = req.pixel_height.max(1) as u32;
    let [b, g_ch, r] = color_bgr(req.color_rgb);

    match req.mode {
        RasterMode::Mode0 => {
            let raw = face.render_char(req.ch, font_h, req.bold, req.italic, true, req.vertical)?;
            let (cell_inc_x, cell_inc_y) = resolve_inc(&raw, font_h, req.vertical);
            if raw.width == 0 || raw.rows == 0 {
                return empty_glyph(raw.bitmap_left, raw.bitmap_top, cell_inc_x, cell_inc_y);
            }
            let mut surf = Surface::filled(raw.width, raw.rows, 0xFF00_0000)?;
            for y in 0..raw.rows as usize {
                for x in 0..raw.width as usize {
                    if mono_bit_set(&raw, x, y) {
                        surf.set_pixel(x as u32, y as u32, [b, g_ch, r, 0x00]);
                    }
                }
            }
            Ok(RasterizedGlyph {
                surface: surf,
                origin_x: raw.bitmap_left,
                origin_y: raw.bitmap_top,
                cell_inc_x,
                cell_inc_y,
                blend_params: BlitParams {
                    blend_mode: 0,
                    alpha_channel_mode: 0,
                    ..Default::default()
                },
                is_empty_bitmap: false,
            })
        }
        RasterMode::Mode1 => {
            let raw = face.render_char(req.ch, font_h, req.bold, req.italic, false, req.vertical)?;
            let (cell_inc_x, cell_inc_y) = resolve_inc(&raw, font_h, req.vertical);
            if raw.width == 0 || raw.rows == 0 {
                return empty_glyph(raw.bitmap_left, raw.bitmap_top, cell_inc_x, cell_inc_y);
            }
            let mut surf = Surface::new(raw.width, raw.rows)?;
            for y in 0..raw.rows as usize {
                for x in 0..raw.width as usize {
                    let g64 = sample_gray64(&raw, x, y);
                    let val = mode1_gray5(g64);
                    // Exact 0x44f752..0x44f758: B=G=R=val (0..=31), A=0.
                    surf.set_pixel(x as u32, y as u32, [val, val, val, 0x00]);
                }
            }
            Ok(RasterizedGlyph {
                surface: surf,
                origin_x: raw.bitmap_left,
                origin_y: raw.bitmap_top,
                cell_inc_x,
                cell_inc_y,
                blend_params: BlitParams {
                    blend_mode: 0x16,
                    alpha_channel_mode: 0,
                    color_or_center: req.color_rgb & 0x00FF_FFFF,
                    ..Default::default()
                },
                is_empty_bitmap: false,
            })
        }
        RasterMode::Mode2 => {
            let raw = face.render_char(req.ch, font_h, req.bold, req.italic, true, req.vertical)?;
            let (cell_inc_x, cell_inc_y) = resolve_inc(&raw, font_h, req.vertical);
            if raw.width == 0 || raw.rows == 0 {
                return empty_glyph(raw.bitmap_left, raw.bitmap_top, cell_inc_x, cell_inc_y);
            }
            let mut surf = Surface::filled(raw.width + 3, raw.rows + 3, 0xFF00_0000)?;
            // Shadow pass (+3, +3) pure black (0x00000000 opaque)
            for y in 0..raw.rows as usize {
                for x in 0..raw.width as usize {
                    if mono_bit_set(&raw, x, y) {
                        surf.set_pixel((x + 3) as u32, (y + 3) as u32, [0, 0, 0, 0x00]);
                    }
                }
            }
            // Foreground pass (0, 0)
            for y in 0..raw.rows as usize {
                for x in 0..raw.width as usize {
                    if mono_bit_set(&raw, x, y) {
                        surf.set_pixel(x as u32, y as u32, [b, g_ch, r, 0x00]);
                    }
                }
            }
            Ok(RasterizedGlyph {
                surface: surf,
                origin_x: raw.bitmap_left,
                origin_y: raw.bitmap_top,
                cell_inc_x,
                cell_inc_y,
                blend_params: BlitParams {
                    blend_mode: 0,
                    alpha_channel_mode: 0,
                    ..Default::default()
                },
                is_empty_bitmap: false,
            })
        }
        RasterMode::Mode3 => {
            // 0x44fca0: Fixed MAT2 = I (horizontal only), GGO_GRAY8_BITMAP, forced monospace override.
            let raw = face.render_char(req.ch, font_h, req.bold, req.italic, false, false)?;
            let cell_inc_y = font_h as i16;
            let cell_inc_x = if req.is_dbcs {
                font_h as i16
            } else {
                (font_h >> 1).max(1) as i16
            };
            if raw.width == 0 || raw.rows == 0 {
                return empty_glyph(raw.bitmap_left, raw.bitmap_top, cell_inc_x, cell_inc_y);
            }
            let mut surf = Surface::new(raw.width, raw.rows)?;
            for y in 0..raw.rows as usize {
                for x in 0..raw.width as usize {
                    let g64 = sample_gray64(&raw, x, y);
                    let inv_a = mode3_inverted_alpha(g64);
                    surf.set_pixel(x as u32, y as u32, [b, g_ch, r, inv_a]);
                }
            }
            Ok(RasterizedGlyph {
                surface: surf,
                origin_x: raw.bitmap_left,
                origin_y: raw.bitmap_top,
                cell_inc_x,
                cell_inc_y,
                blend_params: BlitParams {
                    blend_mode: 0,
                    alpha_channel_mode: 0,
                    ..Default::default()
                },
                is_empty_bitmap: false,
            })
        }
        RasterMode::Mode4 => {
            // 0x44ff30: Horizontal compatibility TextOutA mode.
            let raw = face.render_char(req.ch, font_h, req.bold, req.italic, false, false)?;
            let extent_x = (raw.advance_x_px.max(raw.width as i32)).max(1) as u32;
            let extent_y = font_h.max(raw.rows);
            let mut surf = Surface::filled(extent_x, extent_y, 0xFF00_0000)?;
            let baseline = raw.ascender_px.max(raw.bitmap_top);
            blit_raw_to_canvas(&raw, &mut surf, 0, baseline - raw.bitmap_top, [b, g_ch, r]);
            Ok(RasterizedGlyph {
                surface: surf,
                origin_x: 0,
                origin_y: extent_y as i32,
                cell_inc_x: extent_x as i16,
                cell_inc_y: extent_y as i16,
                blend_params: BlitParams {
                    blend_mode: 0,
                    alpha_channel_mode: 0,
                    ..Default::default()
                },
                is_empty_bitmap: raw.width == 0 || raw.rows == 0,
            })
        }
        RasterMode::Mode5 => {
            // 0x450190: Horizontal TextOutA with (+3, +3) pure-black shadow.
            let raw = face.render_char(req.ch, font_h, req.bold, req.italic, false, false)?;
            let extent_x = (raw.advance_x_px.max(raw.width as i32)).max(1) as u32;
            let extent_y = font_h.max(raw.rows);
            let mut surf = Surface::filled(extent_x + 3, extent_y + 3, 0xFF00_0000)?;
            let baseline = raw.ascender_px.max(raw.bitmap_top);
            let top_y = baseline - raw.bitmap_top;
            // Shadow at (+3, +3)
            blit_raw_to_canvas(&raw, &mut surf, 3, top_y + 3, [0, 0, 0]);
            // Foreground at (0, 0) over shadow
            blit_raw_over_canvas(&raw, &mut surf, 0, top_y, [b, g_ch, r]);
            Ok(RasterizedGlyph {
                surface: surf,
                origin_x: 0,
                origin_y: extent_y as i32,
                cell_inc_x: extent_x as i16,
                cell_inc_y: extent_y as i16,
                blend_params: BlitParams {
                    blend_mode: 0,
                    alpha_channel_mode: 0,
                    ..Default::default()
                },
                is_empty_bitmap: raw.width == 0 || raw.rows == 0,
            })
        }
    }
}

fn resolve_inc(raw: &RawFtGlyph, font_h: u32, vertical: bool) -> (i16, i16) {
    if vertical {
        // After 90° CCW FT_Matrix rotation, advance.y carries vertical advance or fallback to font_h.
        let adv_y = raw.advance_y_px.abs().max(raw.vert_advance_px);
        let inc_y = if adv_y > 0 { adv_y as i16 } else { font_h as i16 };
        let inc_x = font_h as i16;
        (inc_x, inc_y)
    } else {
        let inc_x = if raw.advance_x_px > 0 {
            raw.advance_x_px as i16
        } else {
            (font_h >> 1).max(1) as i16
        };
        let inc_y = font_h as i16;
        (inc_x, inc_y)
    }
}

fn sample_gray64(raw: &RawFtGlyph, x: usize, y: usize) -> u8 {
    match raw.pixel_mode {
        FtPixelMode::Gray8 => quantize_gray8_to_64(raw.buffer[y * raw.pitch + x]),
        FtPixelMode::Mono => {
            if mono_bit_set(raw, x, y) {
                64
            } else {
                0
            }
        }
    }
}

fn blit_raw_to_canvas(raw: &RawFtGlyph, dst: &mut Surface, ox: i32, oy: i32, bgr: [u8; 3]) {
    if raw.width == 0 || raw.rows == 0 {
        return;
    }
    let dw = dst.width() as i32;
    let dh = dst.height() as i32;
    for y in 0..raw.rows as i32 {
        let dy = oy + y;
        if dy < 0 || dy >= dh {
            continue;
        }
        for x in 0..raw.width as i32 {
            let dx = ox + x;
            if dx < 0 || dx >= dw {
                continue;
            }
            let g64 = sample_gray64(raw, x as usize, y as usize);
            let inv_a = mode3_inverted_alpha(g64);
            if inv_a < 0xFF {
                dst.set_pixel(dx as u32, dy as u32, [bgr[0], bgr[1], bgr[2], inv_a]);
            }
        }
    }
}

fn blit_raw_over_canvas(raw: &RawFtGlyph, dst: &mut Surface, ox: i32, oy: i32, bgr: [u8; 3]) {
    if raw.width == 0 || raw.rows == 0 {
        return;
    }
    let dw = dst.width() as i32;
    let dh = dst.height() as i32;
    for y in 0..raw.rows as i32 {
        let dy = oy + y;
        if dy < 0 || dy >= dh {
            continue;
        }
        for x in 0..raw.width as i32 {
            let dx = ox + x;
            if dx < 0 || dx >= dw {
                continue;
            }
            let g64 = sample_gray64(raw, x as usize, y as usize);
            let inv_a = mode3_inverted_alpha(g64);
            if inv_a == 0xFF {
                continue;
            }
            let d = dst.get_pixel(dx as u32, dy as u32);
            let out = crate::gfx::cmath::porter_duff_over(d, [bgr[0], bgr[1], bgr[2], inv_a]);
            dst.set_pixel(dx as u32, dy as u32, out);
        }
    }
}

fn empty_glyph(
    origin_x: i32,
    origin_y: i32,
    cell_inc_x: i16,
    cell_inc_y: i16,
) -> Result<RasterizedGlyph> {
    Ok(RasterizedGlyph {
        surface: Surface::filled(1, 1, 0xFF00_0000)?,
        origin_x,
        origin_y,
        cell_inc_x,
        cell_inc_y,
        blend_params: BlitParams {
            blend_mode: 0,
            alpha_channel_mode: 0,
            ..Default::default()
        },
        is_empty_bitmap: true,
    })
}
