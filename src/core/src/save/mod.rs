//! Byte-exact save data serialization and conversion (`KhimeDL.exe` `UNK-08`).
//!
//! Supports:
//! - 2007 DL global save `KhDLsave.dat` (`0x41dcd0` / `0x41dd70`, `8,116+` bytes)
//! - 2007 DL slot save `KhDLsave%02d.dat` (`0x41de70` / `0x41dff0`, fixed `75,026` bytes)
//! - 2002 CD global save `KhDLsave.dat` (`206,024` bytes, e.g. `补丁/全CG档/KhDLsave.dat`)
//!   with lossless round-trip and explicit migration to 2007 DL `KhDLsave.dat`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::{FujihimeError, Result};
use crate::text::caret::TextEncoding;
use crate::text::rasterizer::{is_cp932_lead, is_cp932_trail, is_gbk_lead, is_gbk_trail};

/// Size of the `DAT_00471378..0x471b8b` system config + global string block (`0x814` B).
pub const SAVE_CONFIG_SIZE: usize = 0x814;
/// Byte offset of the 10x201B global string slots inside [`ConfigBlock2007`].
pub const GLOBAL_STR_OFFSET: usize = 0x3A;
/// Number of global string slots (`0x9e` / `0x9f`).
pub const GLOBAL_STR_COUNT: usize = 10;
/// Fixed stride (in bytes, including NUL) per string slot (`0xC9`).
pub const STR_VAR_STRIDE: usize = 201;

/// Starting index of persistent global variables in `vars`.
pub const GLOBAL_VARS_START: usize = 7000;
/// Number of persistent global variables `vars[7000..10000]`.
pub const GLOBAL_VARS_COUNT: usize = 3000;
/// Number of local variables `vars[0..7000]`.
pub const LOCAL_VARS_COUNT: usize = 7000;

/// Exact byte length of one engine scene snapshot (`0x471b8c..0x477750`, `0x16f1` dwords).
pub const SCENE_SNAPSHOT_SIZE: usize = 0x5BC4; // 23,492 bytes
/// Number of sprite/CG layers in the main scene (`0..=99`).
pub const LAYER_COUNT: usize = 100;
/// Byte stride per layer snapshot inside [`SceneSnapshot`] (`0x471c00..0x476570`).
pub const LAYER_SNAPSHOT_STRIDE: usize = 0xBC; // 188 bytes

/// Exact byte length of a `KhDLsave%02d.dat` slot header (`8*u16 + 26B`).
pub const SLOT_HEADER_SIZE: usize = 0x2A; // 42 bytes
/// Exact byte length of a 2007 DL slot save file `KhDLsave%02d.dat`.
pub const SLOT_SAVE_SIZE: usize = 75_026;
/// Minimum byte length of a 2007 DL global save `KhDLsave.dat` (empty bitmaps).
pub const DL_2007_GLOBAL_MIN_SIZE: usize = SAVE_CONFIG_SIZE + GLOBAL_VARS_COUNT * 2 + 8; // 8,116 bytes

/// Exact byte length of a 2002 CD global save `KhDLsave.dat`.
pub const CD_2002_SAVE_SIZE: usize = 206_024;
/// Number of script rows in the 2002 CD kidoku matrix.
pub const CD_2002_KIDOKU_ROWS: usize = 10_000;
/// Bytes per row in the 2002 CD kidoku matrix.
pub const CD_2002_KIDOKU_ROW_BYTES: usize = 20;

/// Variable-length `map<u16, Bitmap>` matching `KhimeDL.exe` `0x427240` / `0x427490` / `0x427550` / `0x4275d0`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitmapMap {
    entries: BTreeMap<u16, Vec<u8>>,
}

impl BitmapMap {
    /// Construct an empty bitmap map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of keyed bitmaps stored.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Access underlying sorted map.
    pub fn entries(&self) -> &BTreeMap<u16, Vec<u8>> {
        &self.entries
    }

    /// Insert or replace a raw bitmap byte vector for `key`.
    pub fn insert_raw(&mut self, key: u16, bytes: Vec<u8>) {
        self.entries.insert(key, bytes);
    }

    /// Query bit `bit_idx` under `key` (`0x427550`: `byte = bit >> 3`, `mask = 1 << (bit & 7)`).
    pub fn get_bit(&self, key: u16, bit_idx: u32) -> bool {
        let Some(buf) = self.entries.get(&key) else {
            return false;
        };
        let byte_idx = (bit_idx >> 3) as usize;
        if byte_idx >= buf.len() {
            return false;
        }
        (buf[byte_idx] & (1u8 << (bit_idx & 7))) != 0
    }

    /// Set bit `bit_idx` under `key` to `1` (`0x4275d0`).
    pub fn set_bit(&mut self, key: u16, bit_idx: u32) {
        let byte_idx = (bit_idx >> 3) as usize;
        let buf = self.entries.entry(key).or_default();
        if buf.len() <= byte_idx {
            buf.resize(byte_idx + 1, 0);
        }
        buf[byte_idx] |= 1u8 << (bit_idx & 7);
    }

    /// Parse from `blob` starting at `*pos`, advancing `*pos` (`0x427240`).
    pub fn parse_at(blob: &[u8], pos: &mut usize) -> Result<Self> {
        let count = read_u32_at(blob, *pos, "bitmap map count")? as usize;
        *pos += 4;
        let mut entries = BTreeMap::new();
        for i in 0..count {
            let key = read_u16_at(blob, *pos, "bitmap key")?;
            *pos += 2;
            let byte_len = read_u32_at(blob, *pos, "bitmap byte_len")? as usize;
            *pos += 4;
            if *pos + byte_len > blob.len() {
                return Err(FujihimeError::InvalidSave {
                    reason: format!(
                        "bitmap entry {i} (key={key}) truncated: need {byte_len} bytes at {}, len={}",
                        *pos,
                        blob.len()
                    ),
                });
            }
            if byte_len > 0 {
                entries.insert(key, blob[*pos..*pos + byte_len].to_vec());
            } else {
                entries.insert(key, Vec::new());
            }
            *pos += byte_len;
        }
        Ok(Self { entries })
    }

    /// Serialize in ascending `key` order (`0x427490`).
    pub fn write_to(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for (&key, bytes) in &self.entries {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(bytes);
        }
    }
}

/// 2,068-byte (`0x814`) system configuration & global string block (`0x471378..0x471b8b`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigBlock2007 {
    raw: [u8; SAVE_CONFIG_SIZE],
}

impl Default for ConfigBlock2007 {
    /// Exact defaults from `KhimeDL.exe` `FUN_0041d9d0`.
    fn default() -> Self {
        let mut cfg = Self {
            raw: [0u8; SAVE_CONFIG_SIZE],
        };
        cfg.set_fullscreen(false);
        cfg.set_text_speed(1); // Normal (50 ms/char)
        cfg.set_bgm_enable(true);
        cfg.set_se_enable(true);
        cfg.set_voice_enable(true);
        cfg.set_bgm_volume(255);
        cfg.set_se_volume(255);
        cfg.set_voice_volume(255);
        cfg.set_effect_enable(true);
        cfg.set_skip_unread(false);
        cfg.set_flag_8e(false);
        cfg.set_const_0x20(0x20);
        cfg.set_const_1(1);
        cfg.set_auto_wait_ms(1500);
        cfg.set_voice_cut_on_click(false);
        cfg.set_confirm_dialog_enable(false);
        cfg.set_last_save_page(0);
        cfg
    }
}

impl ConfigBlock2007 {
    /// Parse exact `0x814` bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != SAVE_CONFIG_SIZE {
            return Err(FujihimeError::InvalidSave {
                reason: format!(
                    "expected {SAVE_CONFIG_SIZE} bytes for ConfigBlock2007, got {}",
                    bytes.len()
                ),
            });
        }
        let mut raw = [0u8; SAVE_CONFIG_SIZE];
        raw.copy_from_slice(bytes);
        Ok(Self { raw })
    }

    /// Raw `0x814` bytes.
    pub fn as_bytes(&self) -> &[u8; SAVE_CONFIG_SIZE] {
        &self.raw
    }

    #[inline]
    fn get_u16(&self, off: usize) -> u16 {
        u16::from_le_bytes([self.raw[off], self.raw[off + 1]])
    }

    #[inline]
    fn set_u16(&mut self, off: usize, val: u16) {
        self.raw[off..off + 2].copy_from_slice(&val.to_le_bytes());
    }

    /// `+0x00` (`0x471378`): screen mode (`false` = window, `true` = fullscreen).
    pub fn fullscreen(&self) -> bool {
        self.get_u16(0x00) != 0
    }
    pub fn set_fullscreen(&mut self, v: bool) {
        self.set_u16(0x00, u16::from(v));
    }

    /// `+0x02` (`0x47137a`): text speed (`4` = slow, `1` = normal, `0` = instant).
    pub fn text_speed(&self) -> u16 {
        self.get_u16(0x02)
    }
    pub fn set_text_speed(&mut self, v: u16) {
        self.set_u16(0x02, v);
    }

    /// `+0x04` (`0x47137c`): BGM enable switch.
    pub fn bgm_enable(&self) -> bool {
        self.get_u16(0x04) != 0
    }
    pub fn set_bgm_enable(&mut self, v: bool) {
        self.set_u16(0x04, u16::from(v));
    }

    /// `+0x06` (`0x47137e`): SE enable switch (`0x424c00`).
    pub fn se_enable(&self) -> bool {
        self.get_u16(0x06) != 0
    }
    pub fn set_se_enable(&mut self, v: bool) {
        self.set_u16(0x06, u16::from(v));
    }

    /// `+0x08` (`0x471380`): Voice enable switch (`0x41e370`).
    pub fn voice_enable(&self) -> bool {
        self.get_u16(0x08) != 0
    }
    pub fn set_voice_enable(&mut self, v: bool) {
        self.set_u16(0x08, u16::from(v));
    }

    /// `+0x0A` (`0x471382`): BGM volume (`0..=255`).
    pub fn bgm_volume(&self) -> u8 {
        self.get_u16(0x0A).min(255) as u8
    }
    pub fn set_bgm_volume(&mut self, v: u8) {
        self.set_u16(0x0A, u16::from(v));
    }

    /// `+0x0C` (`0x471384`): SE volume (`0..=255`).
    pub fn se_volume(&self) -> u8 {
        self.get_u16(0x0C).min(255) as u8
    }
    pub fn set_se_volume(&mut self, v: u8) {
        self.set_u16(0x0C, u16::from(v));
    }

    /// `+0x0E` (`0x471386`): Voice volume (`0..=255`).
    pub fn voice_volume(&self) -> u8 {
        self.get_u16(0x0E).min(255) as u8
    }
    pub fn set_voice_volume(&mut self, v: u8) {
        self.set_u16(0x0E, u16::from(v));
    }

    /// `+0x12` (`0x47138a`): visual transition effects enable switch.
    pub fn effect_enable(&self) -> bool {
        self.get_u16(0x12) != 0
    }
    pub fn set_effect_enable(&mut self, v: bool) {
        self.set_u16(0x12, u16::from(v));
    }

    /// `+0x14` (`0x47138c`): skip unread text switch (`0` = read only, `1` = all).
    pub fn skip_unread(&self) -> bool {
        self.get_u16(0x14) != 0
    }
    pub fn set_skip_unread(&mut self, v: bool) {
        self.set_u16(0x14, u16::from(v));
    }

    /// `+0x16` (`0x47138e`): config toggle flag at `0x47138e`.
    pub fn flag_8e(&self) -> bool {
        self.get_u16(0x16) != 0
    }
    pub fn set_flag_8e(&mut self, v: bool) {
        self.set_u16(0x16, u16::from(v));
    }

    /// `+0x18` (`0x471390`): `const_0x20` (default `32`).
    pub fn const_0x20(&self) -> u16 {
        self.get_u16(0x18)
    }
    pub fn set_const_0x20(&mut self, v: u16) {
        self.set_u16(0x18, v);
    }

    /// `+0x1A` (`0x471392`): `const_1` (default `1`).
    pub fn const_1(&self) -> u16 {
        self.get_u16(0x1A)
    }
    pub fn set_const_1(&mut self, v: u16) {
        self.set_u16(0x1A, v);
    }

    /// `+0x1C` (`0x471394`): Auto-mode wait duration in ms (default `1500`).
    pub fn auto_wait_ms(&self) -> u16 {
        self.get_u16(0x1C)
    }
    pub fn set_auto_wait_ms(&mut self, v: u16) {
        self.set_u16(0x1C, v);
    }

    /// `+0x1E` (`0x471396`): voice cut flag (`0` = cut voice on dialogue advance).
    pub fn voice_cut_on_click(&self) -> bool {
        self.get_u16(0x1E) == 0
    }
    pub fn set_voice_cut_on_click(&mut self, cut: bool) {
        self.set_u16(0x1E, u16::from(!cut));
    }

    /// `+0x20` (`0x471398`): confirmation dialog / auxiliary switch.
    pub fn confirm_dialog_enable(&self) -> bool {
        self.get_u16(0x20) != 0
    }
    pub fn set_confirm_dialog_enable(&mut self, v: bool) {
        self.set_u16(0x20, u16::from(v));
    }

    /// `+0x36` (`0x4713ae`): last opened save/load page (`0..=9`).
    pub fn last_save_page(&self) -> u16 {
        self.get_u16(0x36)
    }
    pub fn set_last_save_page(&mut self, v: u16) {
        self.set_u16(0x36, v);
    }

    /// `+0x38` (`0x4713b0`): last saved/loaded slot (`1..=100`).
    pub fn last_save_slot(&self) -> u16 {
        self.get_u16(0x38)
    }
    pub fn set_last_save_slot(&mut self, v: u16) {
        self.set_u16(0x38, v);
    }

    /// Access global string slot `0..=9` (`0x4713b2 + slot * 201`).
    pub fn global_str_slot(&self, slot: usize) -> &[u8] {
        let off = GLOBAL_STR_OFFSET + slot * STR_VAR_STRIDE;
        &self.raw[off..off + STR_VAR_STRIDE]
    }

    /// Mutably access global string slot `0..=9`.
    pub fn global_str_slot_mut(&mut self, slot: usize) -> &mut [u8] {
        let off = GLOBAL_STR_OFFSET + slot * STR_VAR_STRIDE;
        &mut self.raw[off..off + STR_VAR_STRIDE]
    }
}

/// 2007 DL global persistent save `KhDLsave.dat` (`0x41dcd0` / `0x41dd70`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalSave2007 {
    pub config: ConfigBlock2007,
    pub vars_7000_9999: [i16; GLOBAL_VARS_COUNT],
    pub kidoku: BitmapMap,
    pub global_flags: BitmapMap,
}

impl Default for GlobalSave2007 {
    fn default() -> Self {
        Self {
            config: ConfigBlock2007::default(),
            vars_7000_9999: [0i16; GLOBAL_VARS_COUNT],
            kidoku: BitmapMap::new(),
            global_flags: BitmapMap::new(),
        }
    }
}

impl GlobalSave2007 {
    /// Parse a 2007 DL `KhDLsave.dat` byte buffer (`>= 8,116` bytes).
    pub fn from_bytes(blob: &[u8]) -> Result<Self> {
        if blob.len() < DL_2007_GLOBAL_MIN_SIZE {
            return Err(FujihimeError::InvalidSave {
                reason: format!(
                    "too short for 2007 DL KhDLsave.dat: got {}, min {DL_2007_GLOBAL_MIN_SIZE}",
                    blob.len()
                ),
            });
        }
        let config = ConfigBlock2007::from_bytes(&blob[..SAVE_CONFIG_SIZE])?;
        let mut vars_7000_9999 = [0i16; GLOBAL_VARS_COUNT];
        let vars_base = SAVE_CONFIG_SIZE;
        for (i, dst) in vars_7000_9999.iter_mut().enumerate() {
            let off = vars_base + i * 2;
            *dst = i16::from_le_bytes([blob[off], blob[off + 1]]);
        }
        let mut pos = vars_base + GLOBAL_VARS_COUNT * 2;
        let kidoku = BitmapMap::parse_at(blob, &mut pos)?;
        let global_flags = BitmapMap::parse_at(blob, &mut pos)?;
        if pos != blob.len() {
            return Err(FujihimeError::InvalidSave {
                reason: format!(
                    "trailing bytes in 2007 DL KhDLsave.dat: parsed {pos} != len {}",
                    blob.len()
                ),
            });
        }
        Ok(Self {
            config,
            vars_7000_9999,
            kidoku,
            global_flags,
        })
    }

    /// Serialize to exact 2007 DL `KhDLsave.dat` binary layout.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DL_2007_GLOBAL_MIN_SIZE);
        out.extend_from_slice(self.config.as_bytes());
        for &v in &self.vars_7000_9999 {
            out.extend_from_slice(&v.to_le_bytes());
        }
        self.kidoku.write_to(&mut out);
        self.global_flags.write_to(&mut out);
        out
    }

    /// Load from disk.
    pub fn load_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|e| FujihimeError::io(path, e))?;
        Self::from_bytes(&bytes)
    }

    /// Save to disk atomically (creating parent directories if needed).
    pub fn save_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| FujihimeError::io(parent, e))?;
            }
        }
        let bytes = self.to_bytes();
        fs::write(path, bytes).map_err(|e| FujihimeError::io(path, e))
    }
}

/// 188-byte (`0xBC`) per-layer snapshot stored at `0x471c00 + layer * 0xBC` (`FUN_00406c50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerSnapshotView<'a> {
    bytes: &'a [u8],
}

impl<'a> LayerSnapshotView<'a> {
    pub fn x(&self) -> i32 {
        i32::from_le_bytes(self.bytes[0x00..0x04].try_into().unwrap())
    }
    pub fn y(&self) -> i32 {
        i32::from_le_bytes(self.bytes[0x04..0x08].try_into().unwrap())
    }
    pub fn layer_type(&self) -> u16 {
        u16::from_le_bytes(self.bytes[0x08..0x0A].try_into().unwrap())
    }
    pub fn source_kind(&self) -> u16 {
        u16::from_le_bytes(self.bytes[0x0A..0x0C].try_into().unwrap())
    }
    pub fn alpha_or_mode(&self) -> u16 {
        u16::from_le_bytes(self.bytes[0x0C..0x0E].try_into().unwrap())
    }
    pub fn img_id(&self) -> u16 {
        u16::from_le_bytes(self.bytes[0x0E..0x10].try_into().unwrap())
    }
    pub fn fill_color(&self) -> u32 {
        u32::from_le_bytes(self.bytes[0x10..0x14].try_into().unwrap())
    }
    pub fn z_mode(&self) -> i16 {
        i16::from_le_bytes(self.bytes[0x14..0x16].try_into().unwrap())
    }
    pub fn blend_mode(&self) -> u16 {
        u16::from_le_bytes(self.bytes[0x16..0x18].try_into().unwrap())
    }
    pub fn blend_param(&self) -> u16 {
        u16::from_le_bytes(self.bytes[0x18..0x1A].try_into().unwrap())
    }
    pub fn clip_rect(&self) -> [i16; 4] {
        [
            i16::from_le_bytes(self.bytes[0x1A..0x1C].try_into().unwrap()),
            i16::from_le_bytes(self.bytes[0x1C..0x1E].try_into().unwrap()),
            i16::from_le_bytes(self.bytes[0x1E..0x20].try_into().unwrap()),
            i16::from_le_bytes(self.bytes[0x20..0x22].try_into().unwrap()),
        ]
    }
    pub fn visible(&self) -> bool {
        u32::from_le_bytes(self.bytes[0x24..0x28].try_into().unwrap()) != 0
    }
    pub fn anchor_x(&self) -> i32 {
        i32::from_le_bytes(self.bytes[0x28..0x2C].try_into().unwrap())
    }
    pub fn anchor_y(&self) -> i32 {
        i32::from_le_bytes(self.bytes[0x2C..0x30].try_into().unwrap())
    }
    pub fn anim_active(&self) -> bool {
        u32::from_le_bytes(self.bytes[0x30..0x34].try_into().unwrap()) != 0
    }
    pub fn subframe_img_ids(&self) -> [u16; 2] {
        [
            u16::from_le_bytes(self.bytes[0x38..0x3A].try_into().unwrap()),
            u16::from_le_bytes(self.bytes[0x3A..0x3C].try_into().unwrap()),
        ]
    }
    pub fn text_or_path_bytes(&self) -> &[u8] {
        let slice = &self.bytes[0x54..0xA5];
        let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
        &slice[..end]
    }
    pub fn dir_prefix_bytes(&self) -> &[u8] {
        let slice = &self.bytes[0xA5..LAYER_SNAPSHOT_STRIDE];
        let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
        &slice[..end]
    }
}

/// 23,492-byte (`0x5BC4`) engine state snapshot (`0x471b8c..0x477750`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneSnapshot {
    pub raw: [u8; SCENE_SNAPSHOT_SIZE],
}

impl Default for SceneSnapshot {
    /// Default initialization matching `FUN_0041daa0`.
    fn default() -> Self {
        let mut s = Self {
            raw: [0u8; SCENE_SNAPSHOT_SIZE],
        };
        s.set_auto_snapshot_enable(true);
        s.set_tbox_style_p2(1);
        s.set_rollback_enable(true);
        s.set_auto_clear_anim(true);
        s.set_coord_scale(1, 1);

        for i in 0..LAYER_COUNT {
            s.set_layer_z(i, i as i16);
            s.set_layer_group(i, 1);
            s.set_layer_visible(i, true);
            s.set_layer_anchors(i, 1, 1);
            s.set_layer_extra_50(i, 0x1e);
            s.set_layer_dir_prefix(i, b".\\grpo");
        }
        // Layer 0 special init in 0x41daa0
        s.set_layer_img_id(0, 999);
        s.set_layer_anchors(0, 0, 0);
        s.set_layer_blend_mode(0, 99);
        s.set_layer_type(0, 0);
        s.set_layer_group(0, 0);
        s.set_layer_dir_prefix(0, b".\\grpe");

        // richedit_cfg defaults at 0x477262 (+0x56D6)
        for slot in 0..10 {
            s.write_u32(0x56D6 + slot * 4, 4);
        }
        s
    }
}

impl SceneSnapshot {
    /// Construct from an exact `23,492`-byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != SCENE_SNAPSHOT_SIZE {
            return Err(FujihimeError::InvalidSave {
                reason: format!(
                    "expected {SCENE_SNAPSHOT_SIZE} bytes for SceneSnapshot, got {}",
                    bytes.len()
                ),
            });
        }
        let mut raw = [0u8; SCENE_SNAPSHOT_SIZE];
        raw.copy_from_slice(bytes);
        Ok(Self { raw })
    }

    #[inline]
    fn read_u16(&self, off: usize) -> u16 {
        u16::from_le_bytes([self.raw[off], self.raw[off + 1]])
    }
    #[inline]
    fn write_u16(&mut self, off: usize, v: u16) {
        self.raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    #[inline]
    fn read_i16(&self, off: usize) -> i16 {
        i16::from_le_bytes([self.raw[off], self.raw[off + 1]])
    }
    #[inline]
    fn write_i16(&mut self, off: usize, v: i16) {
        self.raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    #[inline]
    fn read_u32(&self, off: usize) -> u32 {
        u32::from_le_bytes(self.raw[off..off + 4].try_into().unwrap())
    }
    #[inline]
    fn write_u32(&mut self, off: usize, v: u32) {
        self.raw[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    #[inline]
    fn read_i32(&self, off: usize) -> i32 {
        i32::from_le_bytes(self.raw[off..off + 4].try_into().unwrap())
    }
    #[inline]
    fn write_i32(&mut self, off: usize, v: i32) {
        self.raw[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// `0x471b8c` (`+0x00`): active BGM track ID.
    pub fn bgm_id(&self) -> u16 {
        self.read_u16(0x00)
    }
    pub fn set_bgm_id(&mut self, v: u16) {
        self.write_u32(0x00, u32::from(v));
    }

    /// `0x471b90` (`+0x04`): active SE0 ID.
    pub fn se_id(&self) -> u16 {
        self.read_u16(0x04)
    }
    pub fn set_se_id(&mut self, v: u16) {
        self.write_u16(0x04, v);
    }

    /// `0x471b92` (`+0x06`): SE0 infinite loop flag (`999`).
    pub fn se_loop(&self) -> bool {
        self.read_u16(0x06) != 0
    }
    pub fn set_se_loop(&mut self, looping: bool) {
        self.write_u16(0x06, u16::from(looping));
    }

    /// `0x471b94` (`+0x08`): SE0 fade/delay parameter.
    pub fn se_param(&self) -> i16 {
        self.read_i16(0x08)
    }
    pub fn set_se_param(&mut self, v: i16) {
        self.write_i16(0x08, v);
    }

    /// `0x471b9e` (`+0x12`): call stack script IDs (`[u16; 11]`).
    pub fn script_id(&self, depth: usize) -> u16 {
        self.read_u16(0x12 + depth * 2)
    }
    pub fn set_script_id(&mut self, depth: usize, id: u16) {
        self.write_u16(0x12 + depth * 2, id);
    }

    /// `0x471bb4` (`+0x28`): call stack return PCs (`[u32; 10]`).
    pub fn return_pc(&self, depth: usize) -> u32 {
        self.read_u32(0x28 + depth * 4)
    }
    pub fn set_return_pc(&mut self, depth: usize, pc: u32) {
        self.write_u32(0x28 + depth * 4, pc);
    }

    /// `0x471bdc` (`+0x50`): script call stack depth (`0..=9`).
    pub fn call_depth(&self) -> u16 {
        self.read_u16(0x50)
    }
    pub fn set_call_depth(&mut self, depth: u16) {
        self.write_u16(0x50, depth);
    }

    /// `0x471bde` (`+0x52`): eyes overlay `es_id`.
    pub fn eyes_es_id(&self) -> u16 {
        self.read_u16(0x52)
    }
    pub fn set_eyes_es_id(&mut self, v: u16) {
        self.write_u16(0x52, v);
    }

    /// `0x471be0` (`+0x54`): eyes overlay blend flag.
    pub fn eyes_blend_flag(&self) -> u16 {
        self.read_u16(0x54)
    }
    pub fn set_eyes_blend_flag(&mut self, v: u16) {
        self.write_u16(0x54, v);
    }

    /// `0x471be2` (`+0x56`): eyes overlay Z order.
    pub fn eyes_z(&self) -> i16 {
        self.read_i16(0x56)
    }
    pub fn set_eyes_z(&mut self, v: i16) {
        self.write_i16(0x56, v);
    }

    /// `0x471be4` (`+0x58`): fullscreen filter `alpha_pct`.
    pub fn filter_alpha_pct(&self) -> u16 {
        self.read_u16(0x58)
    }
    pub fn set_filter_alpha_pct(&mut self, v: u16) {
        self.write_u16(0x58, v);
    }

    /// `0x471be6` (`+0x5A`): fullscreen filter `mode_flag`.
    pub fn filter_mode_flag(&self) -> u16 {
        self.read_u16(0x5A)
    }
    pub fn set_filter_mode_flag(&mut self, v: u16) {
        self.write_u16(0x5A, v);
    }

    /// `0x471be8` (`+0x5C`): fullscreen filter Z order.
    pub fn filter_z(&self) -> i16 {
        self.read_i16(0x5C)
    }
    pub fn set_filter_z(&mut self, v: i16) {
        self.write_i16(0x5C, v);
    }

    /// `0x471bec` (`+0x60`): `tbox_style` p2.
    pub fn tbox_style_p2(&self) -> i32 {
        self.read_i32(0x60)
    }
    pub fn set_tbox_style_p2(&mut self, v: i32) {
        self.write_i32(0x60, v);
    }

    /// `0x471bf0` (`+0x64`): auto snapshot enable (`0x38` p5).
    pub fn auto_snapshot_enable(&self) -> bool {
        self.read_i32(0x64) != 0
    }
    pub fn set_auto_snapshot_enable(&mut self, v: bool) {
        self.write_i32(0x64, i32::from(v));
    }

    /// `0x471bf4` (`+0x68`): rollback auto-save enable (`0x35`).
    pub fn rollback_enable(&self) -> bool {
        self.read_i32(0x68) != 0
    }
    pub fn set_rollback_enable(&mut self, v: bool) {
        self.write_i32(0x68, i32::from(v));
    }

    /// `0x471bf8` (`+0x6C`): auto-clear layer animations (`0x4A`).
    pub fn auto_clear_anim(&self) -> bool {
        self.read_i32(0x6C) != 0
    }
    pub fn set_auto_clear_anim(&mut self, v: bool) {
        self.write_i32(0x6C, i32::from(v));
    }

    /// `0x471bfc` (`+0x70`): `text_voice_wait_mode` (`0x66`).
    pub fn text_voice_wait_mode(&self) -> i32 {
        self.read_i32(0x70)
    }
    pub fn set_text_voice_wait_mode(&mut self, v: i32) {
        self.write_i32(0x70, v);
    }

    /// Read-only view of layer `0..=99` (`0x471c00 + layer * 0xBC`).
    pub fn layer(&self, layer: usize) -> LayerSnapshotView<'_> {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        LayerSnapshotView {
            bytes: &self.raw[base..base + LAYER_SNAPSHOT_STRIDE],
        }
    }

    pub fn set_layer_xy(&mut self, layer: usize, x: i32, y: i32) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_i32(base, x);
        self.write_i32(base + 0x04, y);
    }
    pub fn set_layer_type(&mut self, layer: usize, t: u16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u16(base + 0x08, t);
    }
    pub fn set_layer_source_kind(&mut self, layer: usize, kind: u16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u16(base + 0x0A, kind);
    }
    pub fn set_layer_alpha_or_mode(&mut self, layer: usize, v: u16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u16(base + 0x0C, v);
    }
    pub fn set_layer_img_id(&mut self, layer: usize, img_id: u16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u16(base + 0x0E, img_id);
    }
    pub fn set_layer_fill_color(&mut self, layer: usize, color: u32) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u32(base + 0x10, color);
    }
    pub fn set_layer_z_mode(&mut self, layer: usize, z_mode: i16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_i16(base + 0x14, z_mode);
    }
    pub fn set_layer_blend_mode(&mut self, layer: usize, mode: u16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u16(base + 0x16, mode);
    }
    pub fn set_layer_blend_param(&mut self, layer: usize, param: u16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u16(base + 0x18, param);
    }
    pub fn set_layer_clip_rect(&mut self, layer: usize, l: i16, t: i16, r: i16, b: i16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_i16(base + 0x1A, l);
        self.write_i16(base + 0x1C, t);
        self.write_i16(base + 0x1E, r);
        self.write_i16(base + 0x20, b);
    }
    pub fn set_layer_visible(&mut self, layer: usize, visible: bool) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u32(base + 0x24, u32::from(visible));
    }
    pub fn set_layer_anchors(&mut self, layer: usize, ax: i32, ay: i32) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_i32(base + 0x28, ax);
        self.write_i32(base + 0x2C, ay);
    }
    pub fn set_layer_anim_active(&mut self, layer: usize, active: bool) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u32(base + 0x30, u32::from(active));
    }
    pub fn set_layer_extra_50(&mut self, layer: usize, val: u16) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE;
        self.write_u16(base + 0x50, val);
    }
    pub fn set_layer_text_or_path(&mut self, layer: usize, bytes: &[u8]) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE + 0x54;
        let slot = &mut self.raw[base..base + 0x51];
        slot.fill(0);
        let n = bytes.len().min(slot.len() - 1);
        slot[..n].copy_from_slice(&bytes[..n]);
    }
    pub fn set_layer_dir_prefix(&mut self, layer: usize, dir: &[u8]) {
        let base = 0x74 + layer * LAYER_SNAPSHOT_STRIDE + 0xA5;
        let max_len = LAYER_SNAPSHOT_STRIDE - 0xA5;
        let slot = &mut self.raw[base..base + max_len];
        slot.fill(0);
        let n = dir.len().min(max_len - 1);
        slot[..n].copy_from_slice(&dir[..n]);
    }

    /// `0x476570` (`+0x49E4`): layer Z order `[i16; 100]`.
    pub fn layer_z(&self, layer: usize) -> i16 {
        self.read_i16(0x49E4 + layer * 2)
    }
    pub fn set_layer_z(&mut self, layer: usize, z: i16) {
        self.write_i16(0x49E4 + layer * 2, z);
    }

    /// `0x476638` (`+0x4AAC`): layer group ID `[u16; 100]`.
    pub fn layer_group(&self, layer: usize) -> u16 {
        self.read_u16(0x4AAC + layer * 2)
    }
    pub fn set_layer_group(&mut self, layer: usize, group: u16) {
        self.write_u16(0x4AAC + layer * 2, group);
    }

    /// `0x476908` (`+0x4D7C`): coordinate scale factor `(sx, sy)`.
    pub fn coord_scale(&self) -> (i16, i16) {
        (self.read_i16(0x4D7C), self.read_i16(0x4D7E))
    }
    pub fn set_coord_scale(&mut self, sx: i16, sy: i16) {
        self.write_i16(0x4D7C, sx);
        self.write_i16(0x4D7E, sy);
    }

    /// `0x476a88` (`+0x4EFC`): local string variable `$0..$9` (`[u8; 201]`).
    pub fn str_var(&self, idx: usize) -> &[u8] {
        let off = 0x4EFC + idx * STR_VAR_STRIDE;
        &self.raw[off..off + STR_VAR_STRIDE]
    }
    pub fn set_str_var(&mut self, idx: usize, bytes: &[u8]) {
        let off = 0x4EFC + idx * STR_VAR_STRIDE;
        let slot = &mut self.raw[off..off + STR_VAR_STRIDE];
        slot.fill(0);
        let n = bytes.len().min(STR_VAR_STRIDE - 1);
        slot[..n].copy_from_slice(&bytes[..n]);
    }
}

/// 42-byte (`0x2A`) header of `KhDLsave%02d.dat` (`0x41de70`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSaveHeader2007 {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub var1: u16,
    pub var2: u16,
    pub var3: u16,
    pub title_raw: [u8; 26],
}

impl SlotSaveHeader2007 {
    /// Non-empty preview slice up to first NUL byte.
    pub fn title_bytes_trimmed(&self) -> &[u8] {
        let end = self
            .title_raw
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.title_raw.len());
        &self.title_raw[..end]
    }
}

impl Default for SlotSaveHeader2007 {
    fn default() -> Self {
        Self {
            year: 2007,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            var1: 0,
            var2: 0,
            var3: 0,
            title_raw: [0u8; 26],
        }
    }
}

/// DBCS-safe preview truncation matching `0x41de70` (max 24/25 payload bytes + NUL in 26-byte slot).
pub fn dbcs_truncate_title_26b(raw: &[u8], encoding: TextEncoding) -> [u8; 26] {
    let mut out = [0u8; 26];
    let mut i = 0usize;
    let mut o = 0usize;
    let max_payload = 24;
    while i < raw.len() && raw[i] != 0 && o < max_payload {
        let b0 = raw[i];
        let is_dbcs = if i + 1 < raw.len() {
            let b1 = raw[i + 1];
            match encoding {
                TextEncoding::Cp932 => is_cp932_lead(b0) && is_cp932_trail(b1),
                TextEncoding::Gbk => is_gbk_lead(b0) && is_gbk_trail(b1),
            }
        } else {
            false
        };
        if is_dbcs {
            if o + 2 > max_payload {
                break;
            }
            out[o] = b0;
            out[o + 1] = raw[i + 1];
            o += 2;
            i += 2;
        } else {
            out[o] = b0;
            o += 1;
            i += 1;
        }
    }
    out[o] = 0;
    out
}

/// 2007 DL slot save `KhDLsave%02d.dat` (`0x41de70` / `0x41dff0`, fixed `75,026` bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSave2007 {
    pub header: SlotSaveHeader2007,
    pub primary_snapshot: SceneSnapshot,
    pub primary_vars: [i16; LOCAL_VARS_COUNT],
    pub rollback_snapshot: SceneSnapshot,
    pub rollback_vars: [i16; LOCAL_VARS_COUNT],
}

impl SlotSave2007 {
    /// Parse an exact `75,026`-byte `KhDLsave%02d.dat`.
    pub fn from_bytes(blob: &[u8]) -> Result<Self> {
        if blob.len() != SLOT_SAVE_SIZE {
            return Err(FujihimeError::InvalidSave {
                reason: format!(
                    "expected {SLOT_SAVE_SIZE} bytes for KhDLsave%02d.dat, got {}",
                    blob.len()
                ),
            });
        }
        let header = SlotSaveHeader2007 {
            year: read_u16_at(blob, 0x00, "slot year")?,
            month: read_u16_at(blob, 0x02, "slot month")?,
            day: read_u16_at(blob, 0x04, "slot day")?,
            hour: read_u16_at(blob, 0x06, "slot hour")?,
            minute: read_u16_at(blob, 0x08, "slot minute")?,
            var1: read_u16_at(blob, 0x0A, "slot var1")?,
            var2: read_u16_at(blob, 0x0C, "slot var2")?,
            var3: read_u16_at(blob, 0x0E, "slot var3")?,
            title_raw: blob[0x10..0x2A].try_into().unwrap(),
        };

        let p_snap_off = SLOT_HEADER_SIZE;
        let p_vars_off = p_snap_off + SCENE_SNAPSHOT_SIZE;
        let r_snap_off = p_vars_off + LOCAL_VARS_COUNT * 2;
        let r_vars_off = r_snap_off + SCENE_SNAPSHOT_SIZE;

        let primary_snapshot =
            SceneSnapshot::from_bytes(&blob[p_snap_off..p_snap_off + SCENE_SNAPSHOT_SIZE])?;
        let mut primary_vars = [0i16; LOCAL_VARS_COUNT];
        for (i, dst) in primary_vars.iter_mut().enumerate() {
            let off = p_vars_off + i * 2;
            *dst = i16::from_le_bytes([blob[off], blob[off + 1]]);
        }

        let rollback_snapshot =
            SceneSnapshot::from_bytes(&blob[r_snap_off..r_snap_off + SCENE_SNAPSHOT_SIZE])?;
        let mut rollback_vars = [0i16; LOCAL_VARS_COUNT];
        for (i, dst) in rollback_vars.iter_mut().enumerate() {
            let off = r_vars_off + i * 2;
            *dst = i16::from_le_bytes([blob[off], blob[off + 1]]);
        }

        Ok(Self {
            header,
            primary_snapshot,
            primary_vars,
            rollback_snapshot,
            rollback_vars,
        })
    }

    /// Serialize to exact `75,026`-byte `KhDLsave%02d.dat`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SLOT_SAVE_SIZE);
        out.extend_from_slice(&self.header.year.to_le_bytes());
        out.extend_from_slice(&self.header.month.to_le_bytes());
        out.extend_from_slice(&self.header.day.to_le_bytes());
        out.extend_from_slice(&self.header.hour.to_le_bytes());
        out.extend_from_slice(&self.header.minute.to_le_bytes());
        out.extend_from_slice(&self.header.var1.to_le_bytes());
        out.extend_from_slice(&self.header.var2.to_le_bytes());
        out.extend_from_slice(&self.header.var3.to_le_bytes());
        out.extend_from_slice(&self.header.title_raw);
        out.extend_from_slice(&self.primary_snapshot.raw);
        for &v in &self.primary_vars {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.rollback_snapshot.raw);
        for &v in &self.rollback_vars {
            out.extend_from_slice(&v.to_le_bytes());
        }
        debug_assert_eq!(out.len(), SLOT_SAVE_SIZE);
        out
    }

    /// Load from disk.
    pub fn load_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|e| FujihimeError::io(path, e))?;
        Self::from_bytes(&bytes)
    }

    /// Save to disk.
    pub fn save_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| FujihimeError::io(parent, e))?;
            }
        }
        fs::write(path, self.to_bytes()).map_err(|e| FujihimeError::io(path, e))
    }
}

/// 2002 CD Edition `KhDLsave.dat` (`206,024` bytes = `24 B + 6,000 B + 10,000 * 20 B`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalSave2002Cd {
    pub header_u16: [u16; 12],
    pub vars_7000_9999: [i16; GLOBAL_VARS_COUNT],
    pub kidoku_matrix: Vec<[u8; CD_2002_KIDOKU_ROW_BYTES]>,
}

impl GlobalSave2002Cd {
    /// Parse an exact `206,024`-byte 2002 CD `KhDLsave.dat`.
    pub fn from_bytes(blob: &[u8]) -> Result<Self> {
        if blob.len() != CD_2002_SAVE_SIZE {
            return Err(FujihimeError::InvalidSave {
                reason: format!(
                    "expected {CD_2002_SAVE_SIZE} bytes for 2002 CD KhDLsave.dat, got {}",
                    blob.len()
                ),
            });
        }
        let mut header_u16 = [0u16; 12];
        for (i, dst) in header_u16.iter_mut().enumerate() {
            *dst = u16::from_le_bytes([blob[i * 2], blob[i * 2 + 1]]);
        }
        let mut vars_7000_9999 = [0i16; GLOBAL_VARS_COUNT];
        for (i, dst) in vars_7000_9999.iter_mut().enumerate() {
            let off = 24 + i * 2;
            *dst = i16::from_le_bytes([blob[off], blob[off + 1]]);
        }
        let mut kidoku_matrix = Vec::with_capacity(CD_2002_KIDOKU_ROWS);
        let base = 24 + GLOBAL_VARS_COUNT * 2;
        for row_idx in 0..CD_2002_KIDOKU_ROWS {
            let off = base + row_idx * CD_2002_KIDOKU_ROW_BYTES;
            let row: [u8; CD_2002_KIDOKU_ROW_BYTES] = blob[off..off + CD_2002_KIDOKU_ROW_BYTES]
                .try_into()
                .unwrap();
            kidoku_matrix.push(row);
        }
        Ok(Self {
            header_u16,
            vars_7000_9999,
            kidoku_matrix,
        })
    }

    /// Serialize back to exact `206,024`-byte 2002 CD `KhDLsave.dat`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(CD_2002_SAVE_SIZE);
        for &h in &self.header_u16 {
            out.extend_from_slice(&h.to_le_bytes());
        }
        for &v in &self.vars_7000_9999 {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for row in &self.kidoku_matrix {
            out.extend_from_slice(row);
        }
        out
    }

    /// Load from disk.
    pub fn load_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|e| FujihimeError::io(path, e))?;
        Self::from_bytes(&bytes)
    }

    /// Explicitly migrate global variables (`vars[7000..9999]`), CG unlock flags,
    /// and non-zero kidoku bitmaps into a 2007 DL [`GlobalSave2007`].
    pub fn to_2007_dl(&self) -> GlobalSave2007 {
        let mut dl = GlobalSave2007 {
            vars_7000_9999: self.vars_7000_9999,
            ..Default::default()
        };

        // Mirror unlocked CG flags in `vars[9000..9999]` into `global_flags[0]` (`0x41e230`).
        for (offset, &val) in self.vars_7000_9999[2000..].iter().enumerate() {
            if val != 0 {
                dl.global_flags.set_bit(0, offset as u32);
            }
        }

        // Convert any non-zero 20-byte kidoku rows into `BitmapMap` entries.
        for (script_id, row) in self.kidoku_matrix.iter().enumerate() {
            if let Some(last_nz) = row.iter().rposition(|&b| b != 0) {
                if let Ok(key) = u16::try_from(script_id) {
                    dl.kidoku.insert_raw(key, row[..=last_nz].to_vec());
                }
            }
        }

        dl
    }
}

fn read_u16_at(blob: &[u8], off: usize, label: &str) -> Result<u16> {
    if off + 2 > blob.len() {
        return Err(FujihimeError::InvalidSave {
            reason: format!("truncated {label} at offset {off}"),
        });
    }
    Ok(u16::from_le_bytes([blob[off], blob[off + 1]]))
}

fn read_u32_at(blob: &[u8], off: usize, label: &str) -> Result<u32> {
    if off + 4 > blob.len() {
        return Err(FujihimeError::InvalidSave {
            reason: format!("truncated {label} at offset {off}"),
        });
    }
    Ok(u32::from_le_bytes(blob[off..off + 4].try_into().unwrap()))
}
