//! DBCS-safe `^` control code parser (`KhimeDL.exe` `0x441180` / `0x4416d0`) and `gf*.wcg` gaiji cache.

use std::collections::HashMap;

use encoding_rs::{GBK, SHIFT_JIS};

use crate::error::{FujihimeError, Result};
use crate::gfx::decode::decode_wcg;
use crate::gfx::surface::Surface;
use crate::text::freetype::FontFamily;
use crate::text::kinsoku::{classify_cp932_kinsoku, classify_unicode_kinsoku, KinsokuClass};
use crate::text::rasterizer::{
    is_cp932_lead, is_cp932_trail, is_gbk_lead, is_gbk_trail, is_halfwidth_char, RasterMode,
};
use crate::vfs::{AssetCategory, AssetVfs};

/// Text script encoding mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextEncoding {
    /// Original Japanese DL edition (CP932 / Shift-JIS).
    Cp932,
    /// CHS V1.0 translation (GBK).
    Gbk,
}

/// Line alignment (`0x44215e..0x44231e`, `*(u16*)(this+0x1aa)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    #[default]
    Left = 0,
    Center = 1,
    Right = 2,
}

/// Active styling state tracked during `^` control code parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyle {
    pub font_family: FontFamily,
    pub base_height: u16,
    pub scale: u16,
    pub color_rgb: u32,
    pub bold: bool,
    pub italic: bool,
    pub align: TextAlign,
    pub raster_mode: RasterMode,
    pub vertical: bool,
}

impl Default for TextStyle {
    /// Main dialogue textbox defaults (`0x440cf0`: `mode = 3`, white `0xFFFFFF`, Gothic, `scale = 1`, `base = 18`).
    fn default() -> Self {
        Self {
            font_family: FontFamily::Gothic,
            base_height: 18,
            scale: 1,
            color_rgb: 0x00FF_FFFF,
            bold: false,
            italic: false,
            align: TextAlign::Left,
            raster_mode: RasterMode::Mode3,
            vertical: false,
        }
    }
}

/// Color preset lookup matching `0x441180` `^c<ch>`.
pub fn resolve_caret_color(ch: u8, custom_palette: &[u32; 10]) -> Option<u32> {
    match ch.to_ascii_lowercase() {
        b'b' => Some(0x0020_20FF),
        b'g' => Some(0x007F_DFA5),
        b'k' => Some(0x0000_0000),
        b'o' => Some(0x00FA_A25A),
        b'p' => Some(0x00F8_B1EF),
        b'r' => Some(0x00B7_3333),
        b's' => Some(0x0079_F1F2),
        b'v' => Some(0x00C1_87F6),
        b'w' => Some(0x00FF_FFFF),
        b'y' => Some(0x00FF_DE00),
        b'0'..=b'9' => Some(custom_palette[(ch - b'0') as usize]),
        _ => None,
    }
}

/// Compute per-glyph delay ticks (`50 ms` units) for `^d<digit>` (`0x441180`).
///
/// - If `base_speed == 0` (instant mode): always `0`.
/// - If `digit == 0`: `0`.
/// - Otherwise (`1..=9`): `max(0, base_speed - 2 + 2 * digit)`.
#[inline]
pub fn caret_delay_ticks(base_speed: u16, digit: u8) -> u16 {
    if base_speed == 0 || digit == 0 {
        return 0;
    }
    let val = (i32::from(base_speed) - 2) + 2 * i32::from(digit);
    val.max(0) as u16
}

/// Single parsed element emitted by [`parse_caret_bytes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextToken {
    /// Visible character slot.
    Char {
        ch: char,
        raw_code: u16,
        is_dbcs: bool,
        style: TextStyle,
        delay_ticks: u16,
        kinsoku: KinsokuClass,
    },
    /// Static (`^gNNN`) or animated (`^aNNN`) external gaiji image (`gf%03d.wcg`).
    Gaiji {
        id: u16,
        animated: bool,
        style: TextStyle,
        delay_ticks: u16,
    },
    /// Explicit newline (`^n` or `'\n'`).
    Newline,
}

/// Diagnostic summary of `^` control codes encountered during parsing.
#[derive(Debug, Clone, Default)]
pub struct CaretParseStats {
    pub total_caret_seqs: usize,
    pub default_drop_2b_count: usize,
    pub unsupported_seqs: Vec<String>,
    pub gaiji_ids: Vec<u16>,
}

/// Complete result of parsing a script dialogue/UI string.
#[derive(Debug, Clone)]
pub struct ParsedText {
    pub tokens: Vec<TextToken>,
    pub stats: CaretParseStats,
}

/// Parse a raw CP932/GBK string with DBCS-safe `^` control code handling (`0x441180`).
pub fn parse_caret_bytes(
    raw: &[u8],
    encoding: TextEncoding,
    initial_style: TextStyle,
    base_speed: u16,
    vars: Option<&[i16]>,
    custom_palette: &[u32; 10],
) -> ParsedText {
    let mut style = initial_style;
    let mut delay_ticks = base_speed;
    let mut tokens = Vec::new();
    let mut stats = CaretParseStats::default();

    let mut i = 0usize;
    let n = raw.len();

    while i < n {
        let b = raw[i];

        // 1. DBCS 2-byte character check MUST precede '^' (0x5E) check so trail byte 0x5E is safe.
        if i + 1 < n && is_dbcs_pair(encoding, b, raw[i + 1]) {
            let b1 = raw[i + 1];
            let raw_code = ((b as u16) << 8) | (b1 as u16);
            let ch = decode_two_bytes(encoding, b, b1);
            let kinsoku = match encoding {
                TextEncoding::Cp932 => classify_cp932_kinsoku(raw_code),
                TextEncoding::Gbk => classify_unicode_kinsoku(ch),
            };
            tokens.push(TextToken::Char {
                ch,
                raw_code,
                is_dbcs: true,
                style,
                delay_ticks,
                kinsoku,
            });
            i += 2;
            continue;
        }

        // 2. Caret escape sequence ('^')
        if b == b'^' {
            if i + 1 >= n {
                break;
            }
            stats.total_caret_seqs += 1;
            let cmd = raw[i + 1];
            let cl = cmd.to_ascii_lowercase();
            match cl {
                b'a' | b'g' => {
                    let (id, next_idx) = parse_decimal_i32(raw, i + 2);
                    let gid = id.max(0) as u16;
                    let animated = cl == b'a';
                    if !animated {
                        stats.gaiji_ids.push(gid);
                    }
                    let mut item_style = style;
                    if animated {
                        item_style.raster_mode = RasterMode::Mode4;
                    }
                    tokens.push(TextToken::Gaiji {
                        id: gid,
                        animated,
                        style: item_style,
                        delay_ticks,
                    });
                    i = next_idx;
                }
                b'b' => {
                    style.bold = !style.bold;
                    i += 2;
                }
                b'c' => {
                    if i + 2 < n {
                        let arg = raw[i + 2];
                        if let Some(rgb) = resolve_caret_color(arg, custom_palette) {
                            style.color_rgb = rgb;
                        } else {
                            stats
                                .unsupported_seqs
                                .push(format!("^c\\x{arg:02x}"));
                        }
                        i += 3;
                    } else {
                        break;
                    }
                }
                b'd' => {
                    if i + 2 < n {
                        let arg = raw[i + 2];
                        if arg.is_ascii_digit() {
                            delay_ticks = caret_delay_ticks(base_speed, arg - b'0');
                        } else {
                            stats
                                .unsupported_seqs
                                .push(format!("^d\\x{arg:02x}"));
                        }
                        i += 3;
                    } else {
                        break;
                    }
                }
                b'f' => {
                    if i + 2 < n {
                        let arg = raw[i + 2].to_ascii_lowercase();
                        match arg {
                            b'g' => style.font_family = FontFamily::Gothic,
                            b'm' => style.font_family = FontFamily::Mincho,
                            _ => stats
                                .unsupported_seqs
                                .push(format!("^f\\x{:02x}", raw[i + 2])),
                        }
                        i += 3;
                    } else {
                        break;
                    }
                }
                b'i' => {
                    style.italic = !style.italic;
                    i += 2;
                }
                b'l' => {
                    style.bold = initial_style.bold;
                    style.italic = initial_style.italic;
                    style.scale = initial_style.scale;
                    style.font_family = initial_style.font_family;
                    style.color_rgb = initial_style.color_rgb;
                    i += 2;
                }
                b'm' => {
                    style.align = TextAlign::Center;
                    i += 2;
                }
                b'n' => {
                    tokens.push(TextToken::Newline);
                    i += 2;
                }
                b's' => {
                    if i + 2 < n {
                        let arg = raw[i + 2];
                        if arg.is_ascii_digit() {
                            style.scale = u16::from(arg - b'0');
                        } else {
                            stats
                                .unsupported_seqs
                                .push(format!("^s\\x{arg:02x}"));
                        }
                        i += 3;
                    } else {
                        break;
                    }
                }
                b'v' => {
                    let (var_idx, next_idx) = parse_decimal_i32(raw, i + 2);
                    let val = if let Some(var_slice) = vars {
                        usize::try_from(var_idx)
                            .ok()
                            .and_then(|idx| var_slice.get(idx).copied())
                            .unwrap_or(0)
                    } else {
                        var_idx as i16
                    };
                    emit_fullwidth_number(val, style, delay_ticks, &mut tokens);
                    i = next_idx;
                }
                _ => {
                    // `0x441180` default branch: silently drop 2 bytes (`'^'` + following byte).
                    stats.default_drop_2b_count += 1;
                    i += 2;
                }
            }
            continue;
        }

        // 3. Literal single-byte character (ASCII or CP932 halfwidth katakana)
        if b == b'\n' {
            tokens.push(TextToken::Newline);
            i += 1;
            continue;
        }
        if b == b'\r' {
            i += 1;
            continue;
        }

        let ch = decode_one_byte(encoding, b);
        let raw_code = u16::from(b);
        let kinsoku = match encoding {
            TextEncoding::Cp932 => classify_cp932_kinsoku(raw_code),
            TextEncoding::Gbk => classify_unicode_kinsoku(ch),
        };
        tokens.push(TextToken::Char {
            ch,
            raw_code,
            is_dbcs: false,
            style,
            delay_ticks,
            kinsoku,
        });
        i += 1;
    }

    ParsedText { tokens, stats }
}

/// Convenience helper for UTF-8 strings (e.g., synthetic tests).
pub fn parse_caret_utf8(
    text: &str,
    initial_style: TextStyle,
    base_speed: u16,
    vars: Option<&[i16]>,
    custom_palette: &[u32; 10],
) -> ParsedText {
    let mut style = initial_style;
    let mut delay_ticks = base_speed;
    let mut tokens = Vec::new();
    let mut stats = CaretParseStats::default();

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let n = chars.len();

    while i < n {
        let ch = chars[i];
        if ch == '^' {
            if i + 1 >= n {
                break;
            }
            stats.total_caret_seqs += 1;
            let cmd = chars[i + 1].to_ascii_lowercase();
            match cmd {
                'a' | 'g' => {
                    let (id, next_idx) = parse_decimal_chars(&chars, i + 2);
                    let gid = id.max(0) as u16;
                    let animated = cmd == 'a';
                    if !animated {
                        stats.gaiji_ids.push(gid);
                    }
                    let mut item_style = style;
                    if animated {
                        item_style.raster_mode = RasterMode::Mode4;
                    }
                    tokens.push(TextToken::Gaiji {
                        id: gid,
                        animated,
                        style: item_style,
                        delay_ticks,
                    });
                    i = next_idx;
                }
                'b' => {
                    style.bold = !style.bold;
                    i += 2;
                }
                'c' if i + 2 < n => {
                    let arg = chars[i + 2];
                    if arg.is_ascii() {
                        if let Some(rgb) = resolve_caret_color(arg as u8, custom_palette) {
                            style.color_rgb = rgb;
                        }
                    }
                    i += 3;
                }
                'd' if i + 2 < n => {
                    if let Some(d) = chars[i + 2].to_digit(10) {
                        delay_ticks = caret_delay_ticks(base_speed, d as u8);
                    }
                    i += 3;
                }
                'f' if i + 2 < n => {
                    match chars[i + 2].to_ascii_lowercase() {
                        'g' => style.font_family = FontFamily::Gothic,
                        'm' => style.font_family = FontFamily::Mincho,
                        _ => {}
                    }
                    i += 3;
                }
                'i' => {
                    style.italic = !style.italic;
                    i += 2;
                }
                'l' => {
                    style.bold = initial_style.bold;
                    style.italic = initial_style.italic;
                    style.scale = initial_style.scale;
                    style.font_family = initial_style.font_family;
                    style.color_rgb = initial_style.color_rgb;
                    i += 2;
                }
                'm' => {
                    style.align = TextAlign::Center;
                    i += 2;
                }
                'n' => {
                    tokens.push(TextToken::Newline);
                    i += 2;
                }
                's' if i + 2 < n => {
                    if let Some(d) = chars[i + 2].to_digit(10) {
                        style.scale = d as u16;
                    }
                    i += 3;
                }
                'v' => {
                    let (var_idx, next_idx) = parse_decimal_chars(&chars, i + 2);
                    let val = if let Some(var_slice) = vars {
                        usize::try_from(var_idx)
                            .ok()
                            .and_then(|idx| var_slice.get(idx).copied())
                            .unwrap_or(0)
                    } else {
                        var_idx as i16
                    };
                    emit_fullwidth_number(val, style, delay_ticks, &mut tokens);
                    i = next_idx;
                }
                _ => {
                    stats.default_drop_2b_count += 1;
                    i += 2;
                }
            }
            continue;
        }

        if ch == '\n' {
            tokens.push(TextToken::Newline);
            i += 1;
            continue;
        }
        if ch == '\r' {
            i += 1;
            continue;
        }

        let is_dbcs = !is_halfwidth_char(ch);
        let raw_code = if ch.is_ascii() { ch as u16 } else { 0 };
        tokens.push(TextToken::Char {
            ch,
            raw_code,
            is_dbcs,
            style,
            delay_ticks,
            kinsoku: classify_unicode_kinsoku(ch),
        });
        i += 1;
    }

    ParsedText { tokens, stats }
}

fn emit_fullwidth_number(
    val: i16,
    style: TextStyle,
    delay_ticks: u16,
    tokens: &mut Vec<TextToken>,
) {
    let s = val.to_string();
    for ch in s.chars() {
        let (fw_ch, sjis_code) = match ch {
            '-' => ('－', 0x817C),
            '0'..='9' => {
                let d = ch as u32 - '0' as u32;
                let fw = char::from_u32('０' as u32 + d).unwrap_or(ch);
                (fw, 0x824F + d as u16)
            }
            _ => (ch, ch as u16),
        };
        tokens.push(TextToken::Char {
            ch: fw_ch,
            raw_code: sjis_code,
            is_dbcs: true,
            style,
            delay_ticks,
            kinsoku: KinsokuClass::None,
        });
    }
}

#[inline]
fn is_dbcs_pair(encoding: TextEncoding, b0: u8, b1: u8) -> bool {
    match encoding {
        TextEncoding::Cp932 => is_cp932_lead(b0) && is_cp932_trail(b1),
        TextEncoding::Gbk => is_gbk_lead(b0) && is_gbk_trail(b1),
    }
}

fn decode_two_bytes(encoding: TextEncoding, b0: u8, b1: u8) -> char {
    let pair = [b0, b1];
    let enc = match encoding {
        TextEncoding::Cp932 => SHIFT_JIS,
        TextEncoding::Gbk => GBK,
    };
    let (decoded, _, _) = enc.decode(&pair);
    decoded.chars().next().unwrap_or('\u{FFFD}')
}

fn decode_one_byte(encoding: TextEncoding, b: u8) -> char {
    if b < 0x80 {
        return b as char;
    }
    let single = [b];
    let enc = match encoding {
        TextEncoding::Cp932 => SHIFT_JIS,
        TextEncoding::Gbk => GBK,
    };
    let (decoded, _, _) = enc.decode(&single);
    decoded.chars().next().unwrap_or('\u{FFFD}')
}

fn parse_decimal_i32(raw: &[u8], start: usize) -> (i32, usize) {
    let mut j = start;
    if j < raw.len() && raw[j] == b'-' {
        j += 1;
    }
    while j < raw.len() && raw[j].is_ascii_digit() {
        j += 1;
    }
    let val = std::str::from_utf8(&raw[start..j])
        .ok()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    (val, j)
}

fn parse_decimal_chars(chars: &[char], start: usize) -> (i32, usize) {
    let mut j = start;
    if j < chars.len() && chars[j] == '-' {
        j += 1;
    }
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    let s: String = chars[start..j].iter().collect();
    let val = s.parse::<i32>().unwrap_or(0);
    (val, j)
}

/// Cache of decoded `gf%03d.wcg` (`186x34`) gaiji images from `grps.xfl` (`^g<int>`).
#[derive(Debug, Default)]
pub struct GaijiBank {
    surfaces: HashMap<u16, Surface>,
}

impl GaijiBank {
    /// Construct an empty gaiji cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Preload all 11 standard gaiji assets (`gf001..=gf010`, `gf999`) from [`AssetVfs`].
    pub fn preload_all_standard(&mut self, vfs: &AssetVfs) -> Result<usize> {
        let mut count = 0usize;
        for id in (1..=10u16).chain(std::iter::once(999u16)) {
            self.load_or_get(vfs, id)?;
            count += 1;
        }
        Ok(count)
    }

    /// Load `gf%03d.wcg` from `AssetCategory::Grps` (or return cached surface).
    pub fn load_or_get(&mut self, vfs: &AssetVfs, id: u16) -> Result<&Surface> {
        use std::collections::hash_map::Entry;
        if let Entry::Vacant(slot) = self.surfaces.entry(id) {
            let name = format!("gf{id:03}.wcg");
            let bytes = vfs.read(AssetCategory::Grps, &name).map_err(|e| {
                FujihimeError::InvalidText {
                    reason: format!("missing gaiji asset {name}: {e}"),
                }
            })?;
            let surf = decode_wcg(&bytes)?;
            slot.insert(surf);
        }
        Ok(&self.surfaces[&id])
    }

    /// Retrieve an already-cached gaiji surface.
    pub fn get(&self, id: u16) -> Option<&Surface> {
        self.surfaces.get(&id)
    }
}
