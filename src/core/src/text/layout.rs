//! Text line-wrapping, alignment, vertical column layout (`0x441fe0`),
//! 50 ms typewriter stepper (`0x447a90`, `0x440ea0`, `0x436110`), and surface rendering.

use crate::error::{FujihimeError, Result};
use crate::gfx::compositor::{blit, BlitParams, Rect};
use crate::gfx::surface::Surface;
use crate::text::caret::{GaijiBank, ParsedText, TextAlign, TextToken};
use crate::text::freetype::FontSet;
use crate::text::kinsoku::KinsokuClass;
use crate::text::rasterizer::{rasterize_glyph, scale_font_height, GlyphRequest, RasterizedGlyph};
use crate::vfs::AssetVfs;

/// Textbox line spacing added after each line/column (`0x442336`, `0x4425e5`: `+5 px`).
pub const LINE_SPACING_PX: i32 = 5;

/// Core engine text timer period in milliseconds (`0x447a90`: `timeSetEvent(..., 50, ...)`).
pub const TEXT_TIMER_PERIOD_MS: u32 = 50;

/// Verified `tbox00.lwg` text rectangle `(68, 26, 630, 108)`.
pub const TBOX00_TEXT_RECT: (i32, i32, i32, i32) = (68, 26, 630, 108);
/// Verified `tbox01.lwg` text rectangle `(68, 27, 630, 108)`.
pub const TBOX01_TEXT_RECT: (i32, i32, i32, i32) = (68, 27, 630, 108);
/// Verified `tbox11..14.lwg` text rectangle `(45, 0, 466, 159)`.
pub const TBOX11_TEXT_RECT: (i32, i32, i32, i32) = (45, 0, 466, 159);

/// Global text speed setting stored at `0x47137a` (`confscrn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextSpeed {
    /// `msp_now`: `0` (instant single-tick display).
    Instant = 0,
    /// `msp_nom`: `1` (`50 ms` per character).
    #[default]
    Normal = 1,
    /// `msp_slw`: `4` (`200 ms` per character).
    Slow = 4,
}

impl TextSpeed {
    /// Decode raw `u16` from `KhDLsave.dat` offset `0x02`.
    pub const fn from_raw(raw: u16) -> u16 {
        raw
    }

    /// Raw `u16` tick delay value (`0`, `1`, or `4`).
    pub const fn as_u16(self) -> u16 {
        self as u16
    }
}

/// A single positioned visual item (glyph or gaiji) ready for compositing.
#[derive(Debug, Clone)]
pub struct PlacedItem {
    pub x: i32,
    pub y: i32,
    pub surface: Surface,
    pub blend_params: BlitParams,
    pub delay_ticks: u16,
    pub line_index: usize,
    pub is_gaiji: bool,
}

/// Complete laid-out paragraph/textbox result.
#[derive(Debug, Clone)]
pub struct TextLayout {
    pub box_width: i32,
    pub box_height: i32,
    pub vertical: bool,
    pub line_count: usize,
    pub items: Vec<PlacedItem>,
    pub fits_box: bool,
}

#[derive(Debug, Clone)]
struct PreparedItem {
    glyph: RasterizedGlyph,
    delay_ticks: u16,
    kinsoku: KinsokuClass,
    align: TextAlign,
    newlines_before: u32,
    fallback_line_h: i32,
    is_gaiji: bool,
}

/// Lay out parsed tokens using [`FontSet`] and optional [`AssetVfs`] / [`GaijiBank`] (`0x441fe0`).
pub fn layout_text(
    parsed: &ParsedText,
    fonts: &FontSet,
    vfs: Option<&AssetVfs>,
    gaiji_bank: &mut GaijiBank,
    box_width: i32,
    box_height: i32,
    vertical: bool,
) -> Result<TextLayout> {
    let mut prepared = Vec::new();
    let mut pending_newlines = 0u32;

    for tok in &parsed.tokens {
        match tok {
            TextToken::Newline => {
                pending_newlines += 1;
            }
            TextToken::Char {
                ch,
                raw_code: _,
                is_dbcs,
                style,
                delay_ticks,
                kinsoku,
            } => {
                let px_h = scale_font_height(style.base_height, style.scale).max(1);
                let face = fonts.select(style.font_family);
                let glyph = rasterize_glyph(
                    face,
                    &GlyphRequest {
                        ch: *ch,
                        is_dbcs: *is_dbcs,
                        pixel_height: px_h,
                        color_rgb: style.color_rgb,
                        bold: style.bold,
                        italic: style.italic,
                        mode: style.raster_mode,
                        vertical: vertical || style.vertical,
                    },
                )?;
                prepared.push(PreparedItem {
                    glyph,
                    delay_ticks: *delay_ticks,
                    kinsoku: *kinsoku,
                    align: style.align,
                    newlines_before: pending_newlines,
                    fallback_line_h: i32::from(px_h),
                    is_gaiji: false,
                });
                pending_newlines = 0;
            }
            TextToken::Gaiji {
                id,
                animated: _,
                style,
                delay_ticks,
            } => {
                let surf = if let Some(cached) = gaiji_bank.get(*id) {
                    cached.clone()
                } else if let Some(v) = vfs {
                    gaiji_bank.load_or_get(v, *id)?.clone()
                } else {
                    return Err(FujihimeError::InvalidText {
                        reason: format!("gaiji ^g{id:03} requested without AssetVfs"),
                    });
                };
                let w = surf.width() as i16;
                let h = surf.height() as i16;
                let glyph = RasterizedGlyph {
                    surface: surf,
                    origin_x: 0,
                    origin_y: i32::from(h),
                    cell_inc_x: w,
                    cell_inc_y: h,
                    blend_params: BlitParams {
                        blend_mode: 0,
                        alpha_channel_mode: 0,
                        ..Default::default()
                    },
                    is_empty_bitmap: false,
                };
                prepared.push(PreparedItem {
                    glyph,
                    delay_ticks: *delay_ticks,
                    kinsoku: KinsokuClass::None,
                    align: style.align,
                    newlines_before: pending_newlines,
                    fallback_line_h: i32::from(h),
                    is_gaiji: true,
                });
                pending_newlines = 0;
            }
        }
    }

    if vertical {
        layout_vertical(prepared, box_width, box_height)
    } else {
        layout_horizontal(prepared, box_width, box_height)
    }
}

fn layout_horizontal(
    prepared: Vec<PreparedItem>,
    box_width: i32,
    box_height: i32,
) -> Result<TextLayout> {
    let mut items = Vec::with_capacity(prepared.len());
    let mut cur_y = 0i32;
    let mut start = 0usize;
    let mut line_index = 0usize;
    let mut fits_box = true;

    while start < prepared.len() {
        let extra_blank_lines = if start == 0 {
            prepared[start].newlines_before
        } else {
            prepared[start].newlines_before.saturating_sub(1)
        };
        if extra_blank_lines > 0 {
            cur_y += (extra_blank_lines as i32) * (prepared[start].fallback_line_h + LINE_SPACING_PX);
            line_index += extra_blank_lines as usize;
        }

        let mut cur_x = 0i32;
        let mut overhang_used = false;
        let mut end = start;

        while end < prepared.len() {
            if end > start && prepared[end].newlines_before > 0 {
                break;
            }
            let inc_x = i32::from(prepared[end].glyph.cell_inc_x);
            let next_x = cur_x + inc_x;
            if next_x > box_width {
                if cur_x == 0 {
                    // Single character wider than box: cannot break before first item (`0x442072`).
                    end += 1;
                    break;
                }
                if prepared[end].kinsoku == KinsokuClass::LineStartForbidden && !overhang_used {
                    overhang_used = true;
                    cur_x = next_x;
                    end += 1;
                    continue;
                }
                // Check previous item for LineEndForbidden (`0x4420c5`).
                if end > start + 1 && prepared[end - 1].kinsoku == KinsokuClass::LineEndForbidden {
                    end -= 1;
                }
                break;
            }
            cur_x = next_x;
            end += 1;
        }

        if end == start {
            end = start + 1;
        }

        let mut line_max_h = 0i32;
        let mut line_w = 0i32;
        let mut line_align = prepared[start].align;
        for item in &prepared[start..end] {
            line_max_h = line_max_h.max(i32::from(item.glyph.cell_inc_y));
            line_w += i32::from(item.glyph.cell_inc_x);
            if item.align != TextAlign::Left {
                line_align = item.align;
            }
        }

        let align_offset = match line_align {
            TextAlign::Left => 0,
            TextAlign::Center => ((box_width - line_w) / 2).max(0),
            TextAlign::Right => (box_width - line_w).max(0),
        };

        let mut x_cursor = 0i32;
        for item in &prepared[start..end] {
            let g = &item.glyph;
            let dst_x = x_cursor + align_offset + g.origin_x;
            let dst_y = cur_y + (line_max_h - g.origin_y);
            items.push(PlacedItem {
                x: dst_x,
                y: dst_y,
                surface: g.surface.clone(),
                blend_params: g.blend_params,
                delay_ticks: item.delay_ticks,
                line_index,
                is_gaiji: item.is_gaiji,
            });
            x_cursor += i32::from(g.cell_inc_x);
        }

        cur_y += line_max_h + LINE_SPACING_PX;
        if end < prepared.len() && cur_y > box_height {
            fits_box = false;
        }
        start = end;
        line_index += 1;
    }

    Ok(TextLayout {
        box_width,
        box_height,
        vertical: false,
        line_count: line_index,
        items,
        fits_box,
    })
}

fn layout_vertical(
    prepared: Vec<PreparedItem>,
    box_width: i32,
    box_height: i32,
) -> Result<TextLayout> {
    let mut items = Vec::with_capacity(prepared.len());
    let mut col_x = 0i32;
    let mut start = 0usize;
    let mut col_index = 0usize;
    let mut fits_box = true;

    while start < prepared.len() {
        let extra_blank_cols = if start == 0 {
            prepared[start].newlines_before
        } else {
            prepared[start].newlines_before.saturating_sub(1)
        };
        if extra_blank_cols > 0 {
            col_x += (extra_blank_cols as i32) * (prepared[start].fallback_line_h + LINE_SPACING_PX);
            col_index += extra_blank_cols as usize;
        }

        let mut cur_y = 0i32;
        let mut overhang_used = false;
        let mut end = start;

        while end < prepared.len() {
            if end > start && prepared[end].newlines_before > 0 {
                break;
            }
            let inc_y = i32::from(prepared[end].glyph.cell_inc_y);
            let next_y = cur_y + inc_y;
            if next_y > box_height {
                if cur_y == 0 {
                    end += 1;
                    break;
                }
                if prepared[end].kinsoku == KinsokuClass::LineStartForbidden && !overhang_used {
                    overhang_used = true;
                    cur_y = next_y;
                    end += 1;
                    continue;
                }
                if end > start + 1 && prepared[end - 1].kinsoku == KinsokuClass::LineEndForbidden {
                    end -= 1;
                }
                break;
            }
            cur_y = next_y;
            end += 1;
        }

        if end == start {
            end = start + 1;
        }

        let mut col_max_w = 0i32;
        let mut col_h = 0i32;
        let mut col_align = prepared[start].align;
        for item in &prepared[start..end] {
            col_max_w = col_max_w.max(i32::from(item.glyph.cell_inc_x));
            col_h += i32::from(item.glyph.cell_inc_y);
            if item.align != TextAlign::Left {
                col_align = item.align;
            }
        }

        let align_y_offset = match col_align {
            TextAlign::Left => 0,
            TextAlign::Center => ((box_height - col_h) / 2).max(0),
            TextAlign::Right => (box_height - col_h).max(0),
        };

        let mut y_cursor = 0i32;
        for item in &prepared[start..end] {
            let g = &item.glyph;
            // `0x442513..0x44252a`: right-to-left columns (`box_width - col_x - col_max_w + origin_x`).
            let dst_x = (box_width - col_x - col_max_w) + g.origin_x;
            let dst_y = y_cursor + align_y_offset + (i32::from(g.cell_inc_y) - g.origin_y);
            items.push(PlacedItem {
                x: dst_x,
                y: dst_y,
                surface: g.surface.clone(),
                blend_params: g.blend_params,
                delay_ticks: item.delay_ticks,
                line_index: col_index,
                is_gaiji: item.is_gaiji,
            });
            y_cursor += i32::from(g.cell_inc_y);
        }

        col_x += col_max_w + LINE_SPACING_PX;
        if end < prepared.len() && col_x > box_width {
            fits_box = false;
        }
        start = end;
        col_index += 1;
    }

    Ok(TextLayout {
        box_width,
        box_height,
        vertical: true,
        line_count: col_index,
        items,
        fits_box,
    })
}

/// Render laid-out glyphs onto `dst` starting at `origin` (`0x4422c3` / `0x44256e`).
///
/// If `revealed_count` is `Some(n)`, only the first `n` glyphs are rendered.
pub fn render_layout_to_surface(
    dst: &mut Surface,
    layout: &TextLayout,
    revealed_count: Option<usize>,
    origin: (i32, i32),
) {
    let limit = revealed_count.unwrap_or(layout.items.len()).min(layout.items.len());
    let clip = Rect::new(
        origin.0,
        origin.1,
        origin.0 + layout.box_width,
        origin.1 + layout.box_height,
    );
    // Allow 1 overhanging kinsoku character slightly past `box_width`/`box_height` by clipping
    // only to the destination surface bounds when overhang occurs (matching `0x4422f8` blit).
    let _ = clip;
    for item in &layout.items[..limit] {
        blit(
            dst,
            &item.surface,
            &item.blend_params,
            (origin.0 + item.x, origin.1 + item.y),
            None,
        );
    }
}

/// Typewriter character-by-character reveal controller (`0x447a90`, `0x440ea0`, `0x436110`).
#[derive(Debug, Clone)]
pub struct TypewriterState {
    delays: Vec<u16>,
    revealed: usize,
    wait_ticks: u16,
    ms_accum: u32,
}

impl TypewriterState {
    /// Initialize typewriter state for a [`TextLayout`].
    pub fn from_layout(layout: &TextLayout) -> Self {
        let delays = layout.items.iter().map(|it| it.delay_ticks).collect();
        Self {
            delays,
            revealed: 0,
            wait_ticks: 0,
            ms_accum: 0,
        }
    }

    /// Number of currently revealed items.
    #[inline]
    pub fn revealed_count(&self) -> usize {
        self.revealed
    }

    /// Whether all items in the layout have been revealed.
    #[inline]
    pub fn is_finished(&self) -> bool {
        self.revealed >= self.delays.len()
    }

    /// Instantly reveal all remaining items (e.g. click/Enter/Skip).
    pub fn reveal_all(&mut self) {
        self.revealed = self.delays.len();
        self.wait_ticks = 0;
    }

    /// Advance elapsed wall time by `delta_ms` milliseconds, firing `50 ms` timer ticks.
    pub fn advance_ms(&mut self, delta_ms: u32) -> usize {
        self.ms_accum = self.ms_accum.saturating_add(delta_ms);
        while self.ms_accum >= TEXT_TIMER_PERIOD_MS {
            self.ms_accum -= TEXT_TIMER_PERIOD_MS;
            self.tick_50ms();
        }
        self.revealed
    }

    /// Execute one `50 ms` timer tick (`0x440ea0` + `0x436110`).
    ///
    /// - If waiting (`wait_ticks > 0`), decrements `wait_ticks` and returns.
    /// - Otherwise reveals the next item; if its `delay == 0`, loops in the same tick to reveal
    ///   all consecutive zero-delay items; if `delay > 0`, loads `wait_ticks = delay - 1`.
    pub fn tick_50ms(&mut self) -> usize {
        if self.is_finished() {
            return self.revealed;
        }
        if self.wait_ticks > 0 {
            self.wait_ticks -= 1;
            return self.revealed;
        }
        while self.revealed < self.delays.len() {
            let d = self.delays[self.revealed];
            self.revealed += 1;
            if d > 0 {
                self.wait_ticks = d - 1;
                break;
            }
        }
        self.revealed
    }
}
