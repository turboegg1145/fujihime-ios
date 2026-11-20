//! `.xfl` (`LB\x01\x00`) archive reader (`KhimeDL.exe` `0x431e60`).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use encoding_rs::SHIFT_JIS;

use crate::error::{FujihimeError, Result};

const XFL_MAGIC: &[u8; 4] = b"LB\x01\x00";
const XFL_HEADER_SIZE: usize = 12;
const XFL_ENTRY_SIZE: usize = 40;

/// Metadata for a single entry inside an `.xfl` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XflEntry {
    /// Entry filename decoded from Shift-JIS (CP932) / ASCII.
    pub name: String,
    /// Relative offset stored in the directory entry (relative to `12 + dir_size`).
    pub rel_offset: u32,
    /// Absolute byte offset within the `.xfl` file.
    pub abs_offset: u64,
    /// Uncompressed payload length in bytes.
    pub size: u32,
}

#[derive(Debug)]
enum XflSource {
    File(File),
    Memory(Cursor<Vec<u8>>),
}

impl XflSource {
    fn read_at(&mut self, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        match self {
            Self::File(file) => {
                file.seek(SeekFrom::Start(offset))?;
                file.read_exact(&mut buf)?;
            }
            Self::Memory(cursor) => {
                cursor.seek(SeekFrom::Start(offset))?;
                cursor.read_exact(&mut buf)?;
            }
        }
        Ok(buf)
    }
}

/// Reader for `.xfl` (`LB\x01\x00`) resource archives.
#[derive(Debug)]
pub struct XflArchive {
    label: String,
    source: Mutex<XflSource>,
    entries: Vec<XflEntry>,
    index_by_lower: HashMap<String, usize>,
}

impl XflArchive {
    /// Open an `.xfl` archive from disk and parse its directory table.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path).map_err(|e| FujihimeError::io(path, e))?;
        let file_len = file
            .metadata()
            .map_err(|e| FujihimeError::io(path, e))?
            .len();
        let label = path.display().to_string();
        let (entries, index_by_lower) = parse_xfl_directory(&mut file, file_len, &label)?;
        Ok(Self {
            label,
            source: Mutex::new(XflSource::File(file)),
            entries,
            index_by_lower,
        })
    }

    /// Parse an `.xfl` archive from an in-memory buffer (useful for tests).
    pub fn from_bytes(label: impl Into<String>, data: Vec<u8>) -> Result<Self> {
        let label = label.into();
        let file_len = data.len() as u64;
        let mut cursor = Cursor::new(data);
        let (entries, index_by_lower) = parse_xfl_directory(&mut cursor, file_len, &label)?;
        Ok(Self {
            label,
            source: Mutex::new(XflSource::Memory(cursor)),
            entries,
            index_by_lower,
        })
    }

    /// Return all parsed directory entries in archive order.
    pub fn entries(&self) -> &[XflEntry] {
        &self.entries
    }

    /// Number of entries in the archive.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the archive contains zero entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up an entry by filename (ASCII case-insensitive).
    pub fn find_entry(&self, name: &str) -> Option<&XflEntry> {
        let key = normalize_key(name);
        self.index_by_lower.get(&key).map(|&idx| &self.entries[idx])
    }

    /// Read the raw payload bytes of an entry by filename (ASCII case-insensitive).
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| FujihimeError::AssetNotFound {
                name: format!("{}/{}", self.label, name),
            })?;
        self.read_entry(entry)
    }

    /// Read the raw payload bytes for a specific [`XflEntry`].
    pub fn read_entry(&self, entry: &XflEntry) -> Result<Vec<u8>> {
        let mut guard = self
            .source
            .lock()
            .map_err(|_| FujihimeError::InvalidXfl {
                context: self.label.clone(),
                reason: "archive mutex poisoned".to_string(),
            })?;
        guard
            .read_at(entry.abs_offset, entry.size as usize)
            .map_err(|e| FujihimeError::io(PathBuf::from(&self.label), e))
    }
}

fn parse_xfl_directory<R: Read>(
    reader: &mut R,
    total_len: u64,
    context: &str,
) -> Result<(Vec<XflEntry>, HashMap<String, usize>)> {
    let mut header = [0u8; XFL_HEADER_SIZE];
    reader
        .read_exact(&mut header)
        .map_err(|e| FujihimeError::InvalidXfl {
            context: context.to_string(),
            reason: format!("short header: {e}"),
        })?;

    if &header[0..4] != XFL_MAGIC {
        return Err(FujihimeError::InvalidXfl {
            context: context.to_string(),
            reason: format!("bad magic {:02x?}", &header[0..4]),
        });
    }

    let dir_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let entry_count = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);

    let expected_dir_size =
        (entry_count as u64)
            .checked_mul(XFL_ENTRY_SIZE as u64)
            .ok_or_else(|| FujihimeError::InvalidXfl {
                context: context.to_string(),
                reason: "entry count overflow".to_string(),
            })?;
    if dir_size as u64 != expected_dir_size {
        return Err(FujihimeError::InvalidXfl {
            context: context.to_string(),
            reason: format!(
                "dir_size ({dir_size}) != entry_count * 40 ({expected_dir_size})"
            ),
        });
    }

    let data_base = XFL_HEADER_SIZE as u64 + dir_size as u64;
    if data_base > total_len {
        return Err(FujihimeError::InvalidXfl {
            context: context.to_string(),
            reason: format!("directory extends past EOF ({data_base} > {total_len})"),
        });
    }

    let mut dir_buf = vec![0u8; dir_size as usize];
    reader
        .read_exact(&mut dir_buf)
        .map_err(|e| FujihimeError::InvalidXfl {
            context: context.to_string(),
            reason: format!("truncated directory: {e}"),
        })?;

    let count = entry_count as usize;
    let mut entries = Vec::with_capacity(count);
    let mut index_by_lower = HashMap::with_capacity(count);

    for (idx, chunk) in dir_buf.as_chunks::<XFL_ENTRY_SIZE>().0.iter().enumerate() {
        let name_bytes = &chunk[0..32];
        let nul_pos = name_bytes.iter().position(|&b| b == 0).unwrap_or(32);
        let (decoded, _, _) = SHIFT_JIS.decode(&name_bytes[..nul_pos]);
        let name = decoded.into_owned();

        let rel_offset = u32::from_le_bytes([chunk[32], chunk[33], chunk[34], chunk[35]]);
        let size = u32::from_le_bytes([chunk[36], chunk[37], chunk[38], chunk[39]]);
        let abs_offset = data_base + rel_offset as u64;
        let end_offset = abs_offset
            .checked_add(size as u64)
            .ok_or_else(|| FujihimeError::InvalidXfl {
                context: context.to_string(),
                reason: format!("offset overflow on entry {name}"),
            })?;
        if end_offset > total_len {
            return Err(FujihimeError::InvalidXfl {
                context: context.to_string(),
                reason: format!(
                    "entry {name} range [{abs_offset}, {end_offset}) exceeds file size {total_len}"
                ),
            });
        }

        let key = normalize_key(&name);
        index_by_lower.entry(key).or_insert(idx);
        entries.push(XflEntry {
            name,
            rel_offset,
            abs_offset,
            size,
        });
    }

    Ok((entries, index_by_lower))
}

#[inline]
pub(crate) fn normalize_key(name: &str) -> String {
    name.to_ascii_lowercase()
}
