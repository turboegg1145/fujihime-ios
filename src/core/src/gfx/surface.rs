//! Top-down 32-bit BGRA surface with inverted alpha (`0x00` = opaque, `0xFF` = transparent).

use crate::error::{FujihimeError, Result};

/// Standard game logical canvas width.
pub const GAME_WIDTH: u32 = 800;
/// Standard game logical canvas height.
pub const GAME_HEIGHT: u32 = 600;

/// Top-down 32-bit BGRA pixel buffer (`[B, G, R, A]` per pixel).
///
/// Alpha follows the original `KhimeDL.exe` inverted convention:
/// - `0x00` = fully opaque
/// - `0xFF` = fully transparent
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Surface {
    /// Allocate a new zero-initialized surface (black, `A = 0x00` opaque).
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let byte_len = checked_surface_byte_len(width, height)?;
        Ok(Self {
            width,
            height,
            pixels: vec![0u8; byte_len],
        })
    }

    /// Allocate a surface filled with a single 32-bit LE BGRA value (`0xAARRGGBB`).
    pub fn filled(width: u32, height: u32, bgra_u32: u32) -> Result<Self> {
        let mut surface = Self::new(width, height)?;
        surface.fill_u32(bgra_u32);
        Ok(surface)
    }

    /// Wrap an existing tightly packed top-down BGRA byte buffer.
    pub fn from_bgra(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        let expected = checked_surface_byte_len(width, height)?;
        if pixels.len() != expected {
            return Err(FujihimeError::InvalidSurface {
                reason: format!(
                    "expected {expected} bytes for {width}x{height} BGRA, got {}",
                    pixels.len()
                ),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Surface width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Surface height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Immutable view of raw BGRA bytes (inverted alpha).
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    /// Mutable view of raw BGRA bytes (inverted alpha).
    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Consume the surface and return its raw BGRA byte vector.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.pixels
    }

    /// Fill every pixel with `bgra_u32` (little-endian `0xAARRGGBB` -> `[B, G, R, A]`).
    pub fn fill_u32(&mut self, bgra_u32: u32) {
        let px = bgra_u32.to_le_bytes();
        for chunk in self.pixels.as_chunks_mut::<4>().0 {
            chunk.copy_from_slice(&px);
        }
    }

    /// Read pixel `[B, G, R, A]` at `(x, y)`. Panics if out of bounds.
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let off = self.pixel_offset(x, y);
        [
            self.pixels[off],
            self.pixels[off + 1],
            self.pixels[off + 2],
            self.pixels[off + 3],
        ]
    }

    /// Write pixel `[B, G, R, A]` at `(x, y)`. Panics if out of bounds.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, bgra: [u8; 4]) {
        let off = self.pixel_offset(x, y);
        self.pixels[off..off + 4].copy_from_slice(&bgra);
    }

    /// Read pixel as little-endian `u32` (`0xAARRGGBB`) at `(x, y)`.
    #[inline]
    pub fn get_pixel_u32(&self, x: u32, y: u32) -> u32 {
        u32::from_le_bytes(self.get_pixel(x, y))
    }

    /// Write pixel as little-endian `u32` (`0xAARRGGBB`) at `(x, y)`.
    #[inline]
    pub fn set_pixel_u32(&mut self, x: u32, y: u32, bgra: u32) {
        self.set_pixel(x, y, bgra.to_le_bytes());
    }

    /// Convert to standard presentation BGRA8 (`0xFF` = opaque, `0x00` = transparent)
    /// by inverting each pixel's alpha byte (`255 - a`), suitable for Metal texture upload.
    pub fn to_present_bgra8(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.pixels.len()];
        self.write_present_bgra8(&mut out);
        out
    }

    /// Write standard presentation BGRA8 (`A = 255 - inv_a`) into `out` (must match `as_bytes().len()`).
    pub fn write_present_bgra8(&self, out: &mut [u8]) {
        assert_eq!(
            out.len(),
            self.pixels.len(),
            "presentation buffer length mismatch"
        );
        for (dst, src) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.pixels.as_chunks::<4>().0.iter())
        {
            dst[0] = src[0];
            dst[1] = src[1];
            dst[2] = src[2];
            dst[3] = !src[3];
        }
    }

    /// Write opaque presentation BGRA8 (`A = 0xFF` unconditionally) for final screen display.
    pub fn write_present_opaque_bgra8(&self, out: &mut [u8]) {
        assert_eq!(
            out.len(),
            self.pixels.len(),
            "presentation buffer length mismatch"
        );
        for (dst, src) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.pixels.as_chunks::<4>().0.iter())
        {
            dst[0] = src[0];
            dst[1] = src[1];
            dst[2] = src[2];
            dst[3] = 0xFF;
        }
    }

    #[inline]
    fn pixel_offset(&self, x: u32, y: u32) -> usize {
        debug_assert!(x < self.width && y < self.height);
        ((y as usize) * (self.width as usize) + (x as usize)) * 4
    }
}

fn checked_surface_byte_len(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        return Err(FujihimeError::InvalidSurface {
            reason: format!("zero dimension {width}x{height}"),
        });
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| FujihimeError::InvalidSurface {
            reason: format!("byte length overflow for {width}x{height}"),
        })
}
