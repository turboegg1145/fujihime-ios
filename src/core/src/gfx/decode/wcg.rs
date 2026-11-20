//! `.wcg` (`WGq\x02`) image decoder (`KhimeDL.exe` `0x42f3b0`, `0x42f450`, `0x42f520`, `0x42f700`).

use crate::error::{FujihimeError, Result};
use crate::gfx::surface::Surface;

/// Result of decoding a `.wcg` image, including diagnostic padding info.
#[derive(Debug, Clone)]
pub struct WcgDecoded {
    /// Decoded top-down 32-bit BGRA surface with inverted alpha (`0x00`=opaque, `0xFF`=transparent).
    pub surface: Surface,
    /// Number of unused trailing bytes in the compressed streams (e.g. `0x00` padding bytes
    /// produced by CHS repacking tools and ignored by `KhimeDL.exe`).
    pub trailing_pad_bytes: usize,
}

/// Decode a `.wcg` byte slice into a [`Surface`].
pub fn decode_wcg(data: &[u8]) -> Result<Surface> {
    decode_wcg_with_info(data).map(|d| d.surface)
}

/// Decode a `.wcg` byte slice into a [`WcgDecoded`] with trailing padding statistics.
pub fn decode_wcg_with_info(data: &[u8]) -> Result<WcgDecoded> {
    if data.len() < 16 {
        return Err(FujihimeError::InvalidWcg {
            reason: format!("truncated header ({} bytes)", data.len()),
        });
    }
    if &data[0..2] != b"WG" {
        return Err(FujihimeError::InvalidWcg {
            reason: format!("bad signature {:02x?}", &data[0..2]),
        });
    }

    let format_flags = u16::from_le_bytes([data[2], data[3]]);
    let pixel_flags = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let width = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let height = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    if (format_flags & 0xF) != 1 || (format_flags & 0x1C0) != 0x40 || (pixel_flags & 0xFF) != 0x20
    {
        return Err(FujihimeError::UnsupportedWcg {
            format_flags,
            pixel_flags,
        });
    }

    if width == 0 || height == 0 {
        return Err(FujihimeError::InvalidWcg {
            reason: format!("invalid dimensions {width}x{height}"),
        });
    }

    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| FujihimeError::InvalidWcg {
            reason: "dimension overflow".to_string(),
        })?;
    let expected_stream_bytes =
        pixel_count
            .checked_mul(2)
            .ok_or_else(|| FujihimeError::InvalidWcg {
                reason: "stream size overflow".to_string(),
            })?;
    let total_bytes = pixel_count
        .checked_mul(4)
        .ok_or_else(|| FujihimeError::InvalidWcg {
            reason: "surface buffer overflow".to_string(),
        })?;

    let mut out = vec![0u8; total_bytes];
    let mut pos = 16usize;
    let mut trailing_pad_bytes = 0usize;

    // Phase 1 fills high 16-bit pair (R, A at +2, +3); Phase 0 fills low 16-bit pair (B, G at +0, +1).
    for phase in [1usize, 0usize] {
        if pos + 12 > data.len() {
            return Err(FujihimeError::InvalidWcg {
                reason: format!("truncated stream header at offset {pos}"),
            });
        }
        let uncomp_len =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let comp_len =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;
        let dict_field = u32::from_le_bytes([
            data[pos + 8],
            data[pos + 9],
            data[pos + 10],
            data[pos + 11],
        ]);
        pos += 12;

        if uncomp_len != expected_stream_bytes {
            return Err(FujihimeError::InvalidWcg {
                reason: format!(
                    "stream uncomp_len ({uncomp_len}) != expected ({expected_stream_bytes})"
                ),
            });
        }

        let dict_cnt = (dict_field & 0xFFFF) as usize;
        let dict_byte_len = dict_cnt * 2;
        let payload_end = pos
            .checked_add(dict_byte_len)
            .and_then(|v| v.checked_add(comp_len))
            .ok_or_else(|| FujihimeError::InvalidWcg {
                reason: "stream payload offset overflow".to_string(),
            })?;
        if payload_end > data.len() {
            return Err(FujihimeError::InvalidWcg {
                reason: format!(
                    "truncated dictionary or compressed stream ({payload_end} > {})",
                    data.len()
                ),
            });
        }

        let dict_bytes = &data[pos..pos + dict_byte_len];
        pos += dict_byte_len;
        let comp_data = &data[pos..pos + comp_len];
        pos += comp_len;

        let consumed = decode_stream(
            &mut out,
            expected_stream_bytes,
            comp_data,
            dict_bytes,
            dict_cnt,
            phase,
        )?;
        // KhimeDL.exe (0x4330ed / 0x4333f0) stops as soon as `idx >= uncomp_bytes`
        // without checking `consumed == comp_len`, allowing harmless trailing padding bytes.
        trailing_pad_bytes += comp_len.saturating_sub(consumed);
    }

    if pos != data.len() {
        return Err(FujihimeError::InvalidWcg {
            reason: format!("unexpected trailing data after streams: {pos} != {}", data.len()),
        });
    }

    let surface = Surface::from_bgra(width, height, out)?;
    Ok(WcgDecoded {
        surface,
        trailing_pad_bytes,
    })
}

struct MsbBitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    bits: u8,
}

impl<'a> MsbBitReader<'a> {
    #[inline]
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            acc: 0,
            bits: 0,
        }
    }

    #[inline]
    fn read_bits(&mut self, count: u8) -> Result<u32> {
        while self.bits < count {
            if self.pos >= self.data.len() {
                return Err(FujihimeError::InvalidWcg {
                    reason: format!(
                        "bitstream overrun at {}/{}",
                        self.pos,
                        self.data.len()
                    ),
                });
            }
            self.acc = (self.acc << 8) | (self.data[self.pos] as u32);
            self.pos += 1;
            self.bits += 8;
        }
        self.bits -= count;
        let mask = (1u32 << count) - 1;
        let val = (self.acc >> self.bits) & mask;
        self.acc &= (1u32 << self.bits) - 1;
        Ok(val)
    }

    #[inline]
    fn read_bit(&mut self) -> Result<u32> {
        self.read_bits(1)
    }
}

fn decode_stream(
    out: &mut [u8],
    uncomp_bytes: usize,
    comp_data: &[u8],
    dict_bytes: &[u8],
    dict_cnt: usize,
    phase: usize,
) -> Result<usize> {
    let large = dict_cnt >= 0x1001;
    let prefix_bits: u8 = if large { 4 } else { 3 };
    let mut reader = MsbBitReader::new(comp_data);
    let mut idx = phase;

    while idx < uncomp_bytes {
        let c = reader.read_bits(prefix_bits)?;
        let (run, code) = if c == 0 {
            let r = reader.read_bits(4)? as usize + 2;
            let c2 = reader.read_bits(prefix_bits)?;
            if c2 == 0 {
                return Err(FujihimeError::InvalidWcg {
                    reason: "invalid dictionary code 0 inside RLE token".to_string(),
                });
            }
            (r, c2)
        } else {
            (1usize, c)
        };

        let (extra_bits, base): (u8, usize) = if !large {
            match code {
                1 => (1, 0),
                2..=6 => {
                    let b = (code - 1) as u8;
                    (b, 1usize << b)
                }
                7 => {
                    let mut found = None;
                    for b in 6u8..=11 {
                        if reader.read_bit()? == 0 {
                            found = Some((b, 1usize << b));
                            break;
                        }
                    }
                    found.ok_or_else(|| FujihimeError::InvalidWcg {
                        reason: "invalid small-table unary prefix".to_string(),
                    })?
                }
                _ => unreachable!(),
            }
        } else {
            match code {
                1 => (1, 0),
                2..=14 => {
                    let b = (code - 1) as u8;
                    (b, 1usize << b)
                }
                15 => {
                    let mut found = None;
                    for b in 14u8..=15 {
                        if reader.read_bit()? == 0 {
                            found = Some((b, 1usize << b));
                            break;
                        }
                    }
                    found.ok_or_else(|| FujihimeError::InvalidWcg {
                        reason: "invalid large-table unary prefix".to_string(),
                    })?
                }
                _ => unreachable!(),
            }
        };

        let low = reader.read_bits(extra_bits)? as usize;
        let dict_idx = base | low;
        if dict_idx >= dict_cnt {
            return Err(FujihimeError::InvalidWcg {
                reason: format!("dictionary index {dict_idx} out of bounds (dict_cnt={dict_cnt})"),
            });
        }

        let p0 = dict_bytes[dict_idx * 2];
        let p1 = dict_bytes[dict_idx * 2 + 1];

        for _ in 0..run {
            if idx >= uncomp_bytes {
                return Err(FujihimeError::InvalidWcg {
                    reason: format!("RLE output overflow: idx={idx} >= {uncomp_bytes}"),
                });
            }
            let off = idx * 2;
            out[off] = p0;
            out[off + 1] = p1;
            idx += 2;
        }
    }

    Ok(reader.pos)
}
