//! Text subsystem: FreeType 2 memory-face loading, 6 glyph rasterization modes (`0x44f080`),
//! DBCS-safe `^` control codes (`0x441180`), 58/60-item kinsoku table (`0x441bf0`),
//! horizontal/vertical layout (`0x441fe0`), and `50 ms` typewriter timing (`0x447a90`).

pub mod caret;
pub mod freetype;
pub mod kinsoku;
pub mod layout;
pub mod rasterizer;

pub use caret::{
    caret_delay_ticks, parse_caret_bytes, parse_caret_utf8, resolve_caret_color, CaretParseStats,
    GaijiBank, ParsedText, TextAlign, TextEncoding, TextStyle, TextToken,
};
pub use freetype::{FontFace, FontFamily, FontLibrary, FontSet, FtPixelMode, RawFtGlyph};
pub use kinsoku::{
    classify_cp932_kinsoku, classify_unicode_kinsoku, KinsokuClass, KINSOKU_END_SJIS,
    KINSOKU_START_ASCII, KINSOKU_START_SJIS,
};
pub use layout::{
    layout_text, render_layout_to_surface, PlacedItem, TextLayout, TextSpeed, TypewriterState,
    LINE_SPACING_PX, TBOX00_TEXT_RECT, TBOX01_TEXT_RECT, TBOX11_TEXT_RECT, TEXT_TIMER_PERIOD_MS,
};
pub use rasterizer::{
    is_cp932_lead, is_cp932_trail, is_gbk_lead, is_gbk_trail, is_halfwidth_char, mode1_gray5,
    mode3_inverted_alpha, quantize_gray8_to_64, rasterize_glyph, scale_font_height, GlyphRequest,
    RasterMode, RasterizedGlyph,
};
