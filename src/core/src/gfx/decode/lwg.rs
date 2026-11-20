//! `.lwg` (`LG\x01\x00`) layered image container decoder (`KhimeDL.exe` `0x42b640`, `0x42b6e0`).

use encoding_rs::SHIFT_JIS;

use crate::error::{FujihimeError, Result};
use crate::gfx::decode::wcg::{decode_wcg_with_info, WcgDecoded};
use crate::gfx::surface::Surface;

const LWG_MAGIC: &[u8; 4] = b"LG\x01\x00";

/// A single decoded layer inside an `.lwg` container.
#[derive(Debug, Clone)]
pub struct LwgLayer {
    /// Layer name decoded from Shift-JIS (CP932).
    pub name: String,
    /// Layer X offset on the `.lwg` canvas.
    pub x: i32,
    /// Layer Y offset on the `.lwg` canvas.
    pub y: i32,
    /// Static metadata flag byte (`0x42d0b0`).
    pub flag: u8,
    /// Decoded layer surface (`width x height` top-down BGRA, inverted alpha).
    pub surface: Surface,
    /// Byte length of the embedded `.wcg` payload.
    pub raw_wcg_len: usize,
    /// Trailing padding bytes ignored inside the embedded `.wcg` stream.
    pub trailing_pad_bytes: usize,
}

/// Decoded `.lwg` layered container.
#[derive(Debug, Clone)]
pub struct LwgImage {
    /// Logical canvas width.
    pub width: u32,
    /// Logical canvas height.
    pub height: u32,
    /// Header flags (`u16` at offset `0x0E`).
    pub flags: u16,
    /// Decoded layers in index order.
    pub layers: Vec<LwgLayer>,
}

impl LwgImage {
    /// Look up a layer by name (ASCII case-insensitive).
    pub fn find_layer(&self, name: &str) -> Option<&LwgLayer> {
        self.layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Total trailing padding bytes across all embedded `.wcg` layers.
    pub fn total_trailing_pad_bytes(&self) -> usize {
        self.layers.iter().map(|l| l.trailing_pad_bytes).sum()
    }
}

/// Decode an `.lwg` container and all embedded `.wcg` layers.
pub fn decode_lwg(data: &[u8]) -> Result<LwgImage> {
    if data.len() < 24 {
        return Err(FujihimeError::InvalidLwg {
            reason: format!("truncated header ({} bytes)", data.len()),
        });
    }
    if &data[0..4] != LWG_MAGIC {
        return Err(FujihimeError::InvalidLwg {
            reason: format!("bad magic {:02x?}", &data[0..4]),
        });
    }

    // Note: height is at offset 0x04, width is at offset 0x08 (`0x42b6e0`).
    let height = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let width = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let entry_count = u16::from_le_bytes([data[12], data[13]]) as usize;
    let flags = u16::from_le_bytes([data[14], data[15]]);
    let _reserved = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let index_bytes = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;

    let mut pos = 24usize;
    let mut raw_entries = Vec::with_capacity(entry_count);

    for i in 0..entry_count {
        if pos + 18 > data.len() {
            return Err(FujihimeError::InvalidLwg {
                reason: format!("truncated index entry {i} at offset {pos}"),
            });
        }
        let x = i32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let y = i32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let flag = data[pos + 8];
        let rel_offset =
            u32::from_le_bytes([data[pos + 9], data[pos + 10], data[pos + 11], data[pos + 12]])
                as usize;
        let size =
            u32::from_le_bytes([data[pos + 13], data[pos + 14], data[pos + 15], data[pos + 16]])
                as usize;
        let name_len = data[pos + 17] as usize;
        pos += 18;

        if pos + name_len > data.len() {
            return Err(FujihimeError::InvalidLwg {
                reason: format!("truncated layer name at entry {i}"),
            });
        }
        let (decoded_name, _, _) = SHIFT_JIS.decode(&data[pos..pos + name_len]);
        let name = decoded_name.into_owned();
        pos += name_len;

        raw_entries.push((name, x, y, flag, rel_offset, size));
    }

    if pos - 24 != index_bytes {
        return Err(FujihimeError::InvalidLwg {
            reason: format!("index size mismatch: parsed {} != {index_bytes}", pos - 24),
        });
    }

    if pos + 4 > data.len() {
        return Err(FujihimeError::InvalidLwg {
            reason: "missing total_data_size field".to_string(),
        });
    }
    let total_data_size =
        u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4;

    if pos + total_data_size != data.len() {
        return Err(FujihimeError::InvalidLwg {
            reason: format!(
                "payload size mismatch: {pos} + {total_data_size} != {}",
                data.len()
            ),
        });
    }

    let payload_base = pos;
    let mut layers = Vec::with_capacity(entry_count);

    for (name, x, y, flag, rel_offset, size) in raw_entries {
        let start = payload_base
            .checked_add(rel_offset)
            .ok_or_else(|| FujihimeError::InvalidLwg {
                reason: format!("layer {name} offset overflow"),
            })?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| FujihimeError::InvalidLwg {
                reason: format!("layer {name} end overflow"),
            })?;
        if end > data.len() {
            return Err(FujihimeError::InvalidLwg {
                reason: format!("layer {name} out of bounds [{start}..{end}) > {}", data.len()),
            });
        }
        let WcgDecoded {
            surface,
            trailing_pad_bytes,
        } = decode_wcg_with_info(&data[start..end])?;

        layers.push(LwgLayer {
            name,
            x,
            y,
            flag,
            surface,
            raw_wcg_len: size,
            trailing_pad_bytes,
        });
    }

    Ok(LwgImage {
        width,
        height,
        flags,
        layers,
    })
}
