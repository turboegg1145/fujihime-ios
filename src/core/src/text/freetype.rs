//! Minimal safe FreeType 2 FFI bindings over `third_party/freetype2_shim/ft_shim.c`.
//!
//! Loads TTF/OTF/TTC font data strictly from in-memory `&[u8]` buffers (`FT_New_Memory_Face`),
//! supporting Japanese (`ＭＳ ゴシック` / `ＭＳ 明朝`) and Chinese (`黑体`) font sets.

use std::sync::{Arc, Mutex};

use crate::error::{FujihimeError, Result};

#[repr(C)]
struct FtLibraryOpaque {
    _private: [u8; 0],
}

#[repr(C)]
struct FtFaceOpaque {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct FujihimeFtGlyphOut {
    width: u32,
    rows: u32,
    pitch: i32,
    pixel_mode: u8,
    bitmap_left: i32,
    bitmap_top: i32,
    advance_x_26_6: i32,
    advance_y_26_6: i32,
    hori_advance_26_6: i32,
    vert_advance_26_6: i32,
    ascender_px: i32,
    descender_px: i32,
    height_px: i32,
    buffer: *const u8,
}

impl Default for FujihimeFtGlyphOut {
    fn default() -> Self {
        Self {
            width: 0,
            rows: 0,
            pitch: 0,
            pixel_mode: 0,
            bitmap_left: 0,
            bitmap_top: 0,
            advance_x_26_6: 0,
            advance_y_26_6: 0,
            hori_advance_26_6: 0,
            vert_advance_26_6: 0,
            ascender_px: 0,
            descender_px: 0,
            height_px: 0,
            buffer: std::ptr::null(),
        }
    }
}

extern "C" {
    fn fujihime_ft_init(out_lib: *mut *mut FtLibraryOpaque) -> i32;
    fn fujihime_ft_done(lib: *mut FtLibraryOpaque);
    fn fujihime_ft_new_memory_face(
        lib: *mut FtLibraryOpaque,
        font_bytes: *const u8,
        font_len: u32,
        face_index: i32,
        out_face: *mut *mut FtFaceOpaque,
    ) -> i32;
    fn fujihime_ft_done_face(face: *mut FtFaceOpaque);
    fn fujihime_ft_render_char(
        face: *mut FtFaceOpaque,
        codepoint: u32,
        pixel_height: u32,
        bold: i32,
        italic: i32,
        mono: i32,
        vertical_ccw90: i32,
        out_glyph: *mut FujihimeFtGlyphOut,
    ) -> i32;
}

/// Active font family selector (`0x440cf0`, `^fg` / `^fm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontFamily {
    /// `font_family == 0`: `"ＭＳ ゴシック"` (JP) or `"黑体"` (CHS).
    #[default]
    Gothic = 0,
    /// `font_family == 1`: `"ＭＳ 明朝"` (JP) or `"黑体"` (CHS).
    Mincho = 1,
}

/// FreeType bitmap pixel format returned by `FT_Render_Glyph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtPixelMode {
    /// 1-bit MSB-first monochrome bitmap (`FT_PIXEL_MODE_MONO = 1`).
    Mono,
    /// 8-bit antialiased grayscale (`FT_PIXEL_MODE_GRAY = 2`, `0..=255`).
    Gray8,
}

/// Owned glyph bitmap and metrics produced by FreeType 2.
#[derive(Debug, Clone)]
pub struct RawFtGlyph {
    pub width: u32,
    pub rows: u32,
    pub pitch: usize,
    pub pixel_mode: FtPixelMode,
    pub bitmap_left: i32,
    pub bitmap_top: i32,
    pub advance_x_px: i32,
    pub advance_y_px: i32,
    pub hori_advance_px: i32,
    pub vert_advance_px: i32,
    pub ascender_px: i32,
    pub descender_px: i32,
    pub height_px: i32,
    pub buffer: Vec<u8>,
}

#[derive(Debug)]
struct FontLibraryInner {
    lib: Mutex<*mut FtLibraryOpaque>,
}

// Accesses to `FT_Library` are strictly serialized through the inner `Mutex`.
unsafe impl Send for FontLibraryInner {}
unsafe impl Sync for FontLibraryInner {}

impl Drop for FontLibraryInner {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.lib.lock() {
            if !guard.is_null() {
                unsafe { fujihime_ft_done(*guard) };
                *guard = std::ptr::null_mut();
            }
        }
    }
}

/// Thread-safe handle to an initialized FreeType 2 library instance.
#[derive(Debug, Clone)]
pub struct FontLibrary {
    inner: Arc<FontLibraryInner>,
}

impl FontLibrary {
    /// Initialize a FreeType 2 library handle (`FT_Init_FreeType`).
    pub fn new() -> Result<Self> {
        let mut lib: *mut FtLibraryOpaque = std::ptr::null_mut();
        let rc = unsafe { fujihime_ft_init(&mut lib) };
        if rc != 0 || lib.is_null() {
            return Err(FujihimeError::FreeType {
                reason: format!("FT_Init_FreeType failed with error {rc}"),
            });
        }
        Ok(Self {
            inner: Arc::new(FontLibraryInner {
                lib: Mutex::new(lib),
            }),
        })
    }

    /// Create a [`FontFace`] from in-memory TTF/OTF/TTC font bytes (`FT_New_Memory_Face`).
    pub fn load_face_from_bytes(&self, font_bytes: Vec<u8>, face_index: i32) -> Result<FontFace> {
        FontFace::from_bytes(self.clone(), font_bytes, face_index)
    }
}

/// In-memory FreeType 2 font face (`FT_Face` + owned backing font bytes).
pub struct FontFace {
    // Drop order: `face` before `_font_bytes` before `lib`.
    face: *mut FtFaceOpaque,
    _font_bytes: Box<[u8]>,
    lib: FontLibrary,
}

// `FontFace` methods require `&self` (or `&mut self`) and serialize `FT_Library` usage via `self.lib`.
unsafe impl Send for FontFace {}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFace")
            .field("bytes_len", &self._font_bytes.len())
            .finish()
    }
}

impl Drop for FontFace {
    fn drop(&mut self) {
        if !self.face.is_null() {
            let _guard = self.lib.inner.lib.lock();
            unsafe { fujihime_ft_done_face(self.face) };
            self.face = std::ptr::null_mut();
        }
    }
}

impl FontFace {
    /// Load a font face from memory bytes (`FT_New_Memory_Face`).
    pub fn from_bytes(lib: FontLibrary, font_bytes: Vec<u8>, face_index: i32) -> Result<Self> {
        if font_bytes.is_empty() {
            return Err(FujihimeError::FreeType {
                reason: "empty font buffer".to_string(),
            });
        }
        let len_u32 = u32::try_from(font_bytes.len()).map_err(|_| FujihimeError::FreeType {
            reason: "font buffer exceeds u32 length".to_string(),
        })?;
        let backing = font_bytes.into_boxed_slice();
        let mut face: *mut FtFaceOpaque = std::ptr::null_mut();
        let rc = {
            let lib_guard = lib.inner.lib.lock().map_err(|_| FujihimeError::FreeType {
                reason: "FreeType library mutex poisoned".to_string(),
            })?;
            unsafe {
                fujihime_ft_new_memory_face(
                    *lib_guard,
                    backing.as_ptr(),
                    len_u32,
                    face_index,
                    &mut face,
                )
            }
        };
        if rc != 0 || face.is_null() {
            return Err(FujihimeError::FreeType {
                reason: format!("FT_New_Memory_Face failed with error {rc}"),
            });
        }
        Ok(Self {
            face,
            _font_bytes: backing,
            lib,
        })
    }

    /// Render a single Unicode codepoint via FreeType 2.
    pub fn render_char(
        &self,
        ch: char,
        pixel_height: u32,
        bold: bool,
        italic: bool,
        mono: bool,
        vertical_ccw90: bool,
    ) -> Result<RawFtGlyph> {
        let _lib_guard = self.lib.inner.lib.lock().map_err(|_| FujihimeError::FreeType {
            reason: "FreeType library mutex poisoned".to_string(),
        })?;
        let mut out = FujihimeFtGlyphOut::default();
        let rc = unsafe {
            fujihime_ft_render_char(
                self.face,
                ch as u32,
                pixel_height.max(1),
                i32::from(bold),
                i32::from(italic),
                i32::from(mono),
                i32::from(vertical_ccw90),
                &mut out,
            )
        };
        if rc != 0 {
            return Err(FujihimeError::FreeType {
                reason: format!("FT_Load_Glyph/FT_Render_Glyph failed for U+{:04X}: {rc}", ch as u32),
            });
        }

        let pixel_mode = match out.pixel_mode {
            1 => FtPixelMode::Mono,
            2 | 0 => FtPixelMode::Gray8,
            other => {
                return Err(FujihimeError::FreeType {
                    reason: format!("unexpected FreeType pixel_mode {other}"),
                });
            }
        };

        let abs_pitch = out.pitch.unsigned_abs() as usize;
        let mut buffer = Vec::new();
        if out.width > 0 && out.rows > 0 && !out.buffer.is_null() && abs_pitch > 0 {
            let rows = out.rows as usize;
            buffer.resize(rows * abs_pitch, 0u8);
            if out.pitch >= 0 {
                let src = unsafe { std::slice::from_raw_parts(out.buffer, rows * abs_pitch) };
                buffer.copy_from_slice(src);
            } else {
                for r in 0..rows {
                    let src_row = unsafe {
                        std::slice::from_raw_parts(
                            out.buffer.add((rows - 1 - r) * abs_pitch),
                            abs_pitch,
                        )
                    };
                    buffer[r * abs_pitch..(r + 1) * abs_pitch].copy_from_slice(src_row);
                }
            }
        }

        Ok(RawFtGlyph {
            width: out.width,
            rows: out.rows,
            pitch: abs_pitch,
            pixel_mode,
            bitmap_left: out.bitmap_left,
            bitmap_top: out.bitmap_top,
            advance_x_px: (out.advance_x_26_6 + 32) >> 6,
            advance_y_px: (out.advance_y_26_6 + 32) >> 6,
            hori_advance_px: (out.hori_advance_26_6 + 32) >> 6,
            vert_advance_px: (out.vert_advance_26_6 + 32) >> 6,
            ascender_px: out.ascender_px,
            descender_px: out.descender_px,
            height_px: out.height_px,
            buffer,
        })
    }
}

/// Dual-family font set (`Gothic` = `"ＭＳ ゴシック"`/`"黑体"`, `Mincho` = `"ＭＳ 明朝"`/`"黑体"`).
#[derive(Debug)]
pub struct FontSet {
    pub gothic: FontFace,
    pub mincho: Option<FontFace>,
}

impl FontSet {
    /// Construct a font set with mandatory `gothic` (`font_family == 0`) and optional `mincho` (`font_family == 1`).
    pub fn new(gothic: FontFace, mincho: Option<FontFace>) -> Self {
        Self { gothic, mincho }
    }

    /// Select the active [`FontFace`] for `family`, falling back to `gothic` (as in CHS `"黑体"`).
    #[inline]
    pub fn select(&self, family: FontFamily) -> &FontFace {
        match family {
            FontFamily::Gothic => &self.gothic,
            FontFamily::Mincho => self.mincho.as_ref().unwrap_or(&self.gothic),
        }
    }
}
