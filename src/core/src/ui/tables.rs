//! Authoritative UI metadata tables extracted from `KhimeDL.exe` & `0399.gsc`.

use crate::vfs::AssetCategory;

/// Single CG set metadata from `KhimeDL.exe` `0x4648a8` (`7 tabs x 30 slots x 22 B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CgGalleryEntry {
    pub tab: u8,
    pub slot: u8,
    /// Global variable index (`9001..=9320`) checked via `vars[var_id] != 0`.
    pub var_id: u16,
    /// Thumbnail stem index (`grps/cg_%04d` or `grpo_ex` in 2007 DL).
    pub thumb_id: u16,
    /// Non-zero full-size CG image IDs (`variants[0..9]`).
    pub variants: [u16; 9],
}

impl CgGalleryEntry {
    /// Target archive category for full-size CG images (`0x41cc5c`: Tab 6 uses `Grpo`, Tabs 0..5 use `Grpe`).
    pub const fn full_image_category(&self) -> AssetCategory {
        if self.tab == 6 {
            AssetCategory::Grpo
        } else {
            AssetCategory::Grpe
        }
    }

    /// Collect all non-zero variant IDs in presentation order.
    pub fn active_variants(&self) -> impl Iterator<Item = u16> + '_ {
        self.variants.iter().copied().filter(|&id| id != 0)
    }
}

/// Single scene replay entry from `KhimeDL.exe` `0x46db08` (`7 tabs x 24 slots x 6 B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneReplayEntry {
    pub tab: u8,
    pub slot: u8,
    /// Unlock variable index (`8111..=8721`).
    pub flag_var: u16,
    /// Thumbnail index (`sc_%04d`).
    pub thumb_id: u16,
    /// Legacy EXE table script ID (`8111..=8721`; 2007 DL uses `0399.gsc` dual-track).
    pub script_id: u16,
}

/// Single `0399.gsc` 2007 DL scene replay route (`vars[999] = 8010..=8036`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recall0399Route {
    /// Menu key written into `vars[999]` by `0301..0303.gsc`.
    pub route_id: u16,
    /// Target gameplay script ID called via `0x0F call_script`.
    pub script_id: u16,
    /// Optional non-zero label name string index in `0399.gsc` (e.g. `"8011"`..`"8031"`).
    pub has_recall_label: bool,
    /// Optional pre-call BGM ID (`0` = unchanged).
    pub bgm_id: u16,
    /// Optional pre-call background image ID (`999` = black/clear).
    pub bg_id: Option<u16>,
}

/// CG entry counts across the 7 tabs (`sum = 102`).
pub const CG_TAB_COUNTS: [usize; 7] = [12, 10, 17, 11, 12, 25, 15];

/// Scene replay entry counts across the 7 tabs (`sum = 79`).
pub const REPLAY_TAB_COUNTS: [usize; 7] = [12, 12, 15, 11, 11, 18, 0];

/// Available BGM track IDs verified present in 2007 DL `bgm.xfl` (`Track03.wav..Track23.wav`).
pub const DL2007_BGM_TRACK_RANGE: (u16, u16) = (3, 23);

/// The 9 scripts whose `"8011".."8031"` recall labels are zeroed in CHS V1.0 `scr.xfl`.
pub const RECALL_ZEROED_LABEL_SCRIPTS: [u16; 9] = [
    1125, 2058, 2059, 2245, 3177, 3179, 3262, 4203, 5110,
];

/// EXE-mode Title button top-left `(x, y)` coordinates (`x = 54`, `y = 223 + 23 * i`, `0x4264d0`).
pub const TITLE_ME_COORDS: [(i32, i32); 7] = [
    (54, 223), // top_me01: Start game (script 0)
    (54, 246), // top_me02: Continue / entry 2 (script 1)
    (54, 269), // top_me03: Load
    (54, 292), // top_me04: Config
    (54, 315), // top_me05: Extra
    (54, 338), // top_me06: Omake (requires vars[7000] != 0 -> script 7000)
    (54, 361), // top_me07: Exit confirm
];

/// 2007 DL `0101.gsc` Title/Extra button coordinates (`x = 253`, `y = 342 + 35 * i`).
pub const TITLE_0101_BTN_COORDS: [(i32, i32); 6] = [
    (253, 342),
    (253, 377),
    (253, 412),
    (253, 447),
    (253, 482),
    (253, 517),
];

/// EXE-mode Extra button coordinates (`0x419210`).
pub const EXTRA_BTN_COORDS: [(i32, i32); 5] = [
    (431, 274), // top_ex01: CG Gallery
    (431, 310), // top_ex02: Scene Replay
    (431, 346), // top_ex03: Music Mode
    (431, 382), // top_ex04: Omake (9000.gsc; absent in 2007 DL scr.xfl, 9001.gsc present)
    (573, 443), // top_exre: Return to Title
];

/// Complete 102-set / 266-image CG gallery table (`KhimeDL.exe` `0x4648a8`).
pub const CG_GALLERY_TABLE: &[CgGalleryEntry] = &[
    CgGalleryEntry { tab: 0, slot: 0, var_id: 9001, thumb_id: 1, variants: [1, 2, 3, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 1, var_id: 9004, thumb_id: 4, variants: [4, 5, 7, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 2, var_id: 9006, thumb_id: 6, variants: [6, 8, 9, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 3, var_id: 9010, thumb_id: 10, variants: [10, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 4, var_id: 9011, thumb_id: 11, variants: [11, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 5, var_id: 9012, thumb_id: 12, variants: [12, 13, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 6, var_id: 9016, thumb_id: 14, variants: [14, 16, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 7, var_id: 9015, thumb_id: 15, variants: [15, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 8, var_id: 9017, thumb_id: 17, variants: [17, 18, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 9, var_id: 9019, thumb_id: 19, variants: [19, 20, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 10, var_id: 9021, thumb_id: 21, variants: [21, 22, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 0, slot: 11, var_id: 9023, thumb_id: 23, variants: [23, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 0, var_id: 9031, thumb_id: 31, variants: [31, 32, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 1, var_id: 9033, thumb_id: 33, variants: [33, 34, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 2, var_id: 9035, thumb_id: 35, variants: [35, 36, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 3, var_id: 9037, thumb_id: 37, variants: [37, 38, 39, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 4, var_id: 9040, thumb_id: 40, variants: [40, 41, 42, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 5, var_id: 9043, thumb_id: 43, variants: [43, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 6, var_id: 9044, thumb_id: 44, variants: [44, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 7, var_id: 9045, thumb_id: 45, variants: [45, 46, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 8, var_id: 9047, thumb_id: 47, variants: [47, 48, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 1, slot: 9, var_id: 9049, thumb_id: 49, variants: [49, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 0, var_id: 9061, thumb_id: 61, variants: [61, 62, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 1, var_id: 9070, thumb_id: 70, variants: [70, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 2, var_id: 9071, thumb_id: 71, variants: [71, 74, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 3, var_id: 9072, thumb_id: 72, variants: [72, 73, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 4, var_id: 9065, thumb_id: 65, variants: [65, 67, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 5, var_id: 9066, thumb_id: 66, variants: [66, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 6, var_id: 9068, thumb_id: 68, variants: [68, 69, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 7, var_id: 9075, thumb_id: 75, variants: [75, 76, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 8, var_id: 9077, thumb_id: 77, variants: [77, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 9, var_id: 9063, thumb_id: 63, variants: [63, 64, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 10, var_id: 9131, thumb_id: 131, variants: [131, 132, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 11, var_id: 9078, thumb_id: 78, variants: [78, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 12, var_id: 9079, thumb_id: 79, variants: [79, 81, 83, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 13, var_id: 9080, thumb_id: 80, variants: [80, 82, 84, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 14, var_id: 9085, thumb_id: 85, variants: [85, 87, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 15, var_id: 9086, thumb_id: 86, variants: [86, 88, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 2, slot: 16, var_id: 9089, thumb_id: 89, variants: [89, 90, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 0, var_id: 9091, thumb_id: 91, variants: [91, 92, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 1, var_id: 9093, thumb_id: 93, variants: [93, 94, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 2, var_id: 9095, thumb_id: 95, variants: [95, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 3, var_id: 9096, thumb_id: 96, variants: [96, 97, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 4, var_id: 9098, thumb_id: 98, variants: [98, 99, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 5, var_id: 9100, thumb_id: 100, variants: [100, 101, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 6, var_id: 9102, thumb_id: 102, variants: [102, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 7, var_id: 9103, thumb_id: 103, variants: [103, 104, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 8, var_id: 9105, thumb_id: 105, variants: [105, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 9, var_id: 9106, thumb_id: 106, variants: [106, 107, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 3, slot: 10, var_id: 9108, thumb_id: 108, variants: [108, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 0, var_id: 9121, thumb_id: 121, variants: [121, 122, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 1, var_id: 9126, thumb_id: 126, variants: [126, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 2, var_id: 9130, thumb_id: 130, variants: [130, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 3, var_id: 9123, thumb_id: 123, variants: [123, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 4, var_id: 9124, thumb_id: 124, variants: [124, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 5, var_id: 9125, thumb_id: 125, variants: [125, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 6, var_id: 9127, thumb_id: 127, variants: [127, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 7, var_id: 9128, thumb_id: 128, variants: [128, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 8, var_id: 9129, thumb_id: 129, variants: [129, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 9, var_id: 9133, thumb_id: 133, variants: [133, 134, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 10, var_id: 9135, thumb_id: 135, variants: [135, 136, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 4, slot: 11, var_id: 9137, thumb_id: 137, variants: [137, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 0, var_id: 9151, thumb_id: 151, variants: [151, 152, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 1, var_id: 9153, thumb_id: 153, variants: [153, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 2, var_id: 9154, thumb_id: 154, variants: [154, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 3, var_id: 9155, thumb_id: 155, variants: [155, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 4, var_id: 9201, thumb_id: 201, variants: [201, 202, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 5, var_id: 9203, thumb_id: 203, variants: [203, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 6, var_id: 9204, thumb_id: 204, variants: [204, 205, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 7, var_id: 9211, thumb_id: 211, variants: [211, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 8, var_id: 9212, thumb_id: 212, variants: [212, 213, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 9, var_id: 9221, thumb_id: 221, variants: [221, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 10, var_id: 9222, thumb_id: 222, variants: [222, 223, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 11, var_id: 9231, thumb_id: 231, variants: [231, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 12, var_id: 9232, thumb_id: 232, variants: [232, 233, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 13, var_id: 9241, thumb_id: 241, variants: [241, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 14, var_id: 9242, thumb_id: 242, variants: [242, 243, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 15, var_id: 9251, thumb_id: 251, variants: [251, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 16, var_id: 9252, thumb_id: 252, variants: [252, 253, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 17, var_id: 9261, thumb_id: 261, variants: [261, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 18, var_id: 9262, thumb_id: 262, variants: [262, 263, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 19, var_id: 9271, thumb_id: 271, variants: [271, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 20, var_id: 9272, thumb_id: 272, variants: [272, 273, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 21, var_id: 9281, thumb_id: 281, variants: [281, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 22, var_id: 9282, thumb_id: 282, variants: [282, 283, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 23, var_id: 9291, thumb_id: 291, variants: [291, 292, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 5, slot: 24, var_id: 9293, thumb_id: 293, variants: [293, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 0, var_id: 9301, thumb_id: 301, variants: [301, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 1, var_id: 9302, thumb_id: 302, variants: [302, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 2, var_id: 9303, thumb_id: 303, variants: [303, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 3, var_id: 9304, thumb_id: 304, variants: [304, 305, 306, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 4, var_id: 9307, thumb_id: 307, variants: [307, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 5, var_id: 9308, thumb_id: 308, variants: [308, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 6, var_id: 9309, thumb_id: 309, variants: [309, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 7, var_id: 9310, thumb_id: 310, variants: [310, 311, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 8, var_id: 9312, thumb_id: 312, variants: [312, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 9, var_id: 9313, thumb_id: 313, variants: [313, 314, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 10, var_id: 9315, thumb_id: 315, variants: [315, 316, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 11, var_id: 9317, thumb_id: 317, variants: [317, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 12, var_id: 9318, thumb_id: 318, variants: [318, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 13, var_id: 9319, thumb_id: 319, variants: [319, 0, 0, 0, 0, 0, 0, 0, 0] },
    CgGalleryEntry { tab: 6, slot: 14, var_id: 9320, thumb_id: 320, variants: [320, 0, 0, 0, 0, 0, 0, 0, 0] },
];

/// Complete 79-entry Scene Replay table (`KhimeDL.exe` `0x46db08`).
pub const SCENE_REPLAY_TABLE: &[SceneReplayEntry] = &[
    SceneReplayEntry { tab: 0, slot: 0, flag_var: 8111, thumb_id: 111, script_id: 8111 },
    SceneReplayEntry { tab: 0, slot: 1, flag_var: 8121, thumb_id: 121, script_id: 8121 },
    SceneReplayEntry { tab: 0, slot: 2, flag_var: 8122, thumb_id: 122, script_id: 8122 },
    SceneReplayEntry { tab: 0, slot: 3, flag_var: 8131, thumb_id: 131, script_id: 8131 },
    SceneReplayEntry { tab: 0, slot: 4, flag_var: 8132, thumb_id: 132, script_id: 8132 },
    SceneReplayEntry { tab: 0, slot: 5, flag_var: 8141, thumb_id: 141, script_id: 8141 },
    SceneReplayEntry { tab: 0, slot: 6, flag_var: 8142, thumb_id: 142, script_id: 8142 },
    SceneReplayEntry { tab: 0, slot: 7, flag_var: 8151, thumb_id: 151, script_id: 8151 },
    SceneReplayEntry { tab: 0, slot: 8, flag_var: 8152, thumb_id: 152, script_id: 8152 },
    SceneReplayEntry { tab: 0, slot: 9, flag_var: 8161, thumb_id: 161, script_id: 8161 },
    SceneReplayEntry { tab: 0, slot: 10, flag_var: 8171, thumb_id: 171, script_id: 8171 },
    SceneReplayEntry { tab: 0, slot: 11, flag_var: 8181, thumb_id: 181, script_id: 8181 },
    SceneReplayEntry { tab: 1, slot: 0, flag_var: 8211, thumb_id: 211, script_id: 8211 },
    SceneReplayEntry { tab: 1, slot: 1, flag_var: 8221, thumb_id: 221, script_id: 8221 },
    SceneReplayEntry { tab: 1, slot: 2, flag_var: 8222, thumb_id: 222, script_id: 8222 },
    SceneReplayEntry { tab: 1, slot: 3, flag_var: 8231, thumb_id: 231, script_id: 8231 },
    SceneReplayEntry { tab: 1, slot: 4, flag_var: 8232, thumb_id: 232, script_id: 8232 },
    SceneReplayEntry { tab: 1, slot: 5, flag_var: 8241, thumb_id: 241, script_id: 8241 },
    SceneReplayEntry { tab: 1, slot: 6, flag_var: 8242, thumb_id: 242, script_id: 8242 },
    SceneReplayEntry { tab: 1, slot: 7, flag_var: 8251, thumb_id: 251, script_id: 8251 },
    SceneReplayEntry { tab: 1, slot: 8, flag_var: 8261, thumb_id: 261, script_id: 8261 },
    SceneReplayEntry { tab: 1, slot: 9, flag_var: 8271, thumb_id: 271, script_id: 8271 },
    SceneReplayEntry { tab: 1, slot: 10, flag_var: 8281, thumb_id: 281, script_id: 8281 },
    SceneReplayEntry { tab: 1, slot: 11, flag_var: 8291, thumb_id: 291, script_id: 8291 },
    SceneReplayEntry { tab: 2, slot: 0, flag_var: 8311, thumb_id: 311, script_id: 8311 },
    SceneReplayEntry { tab: 2, slot: 1, flag_var: 8321, thumb_id: 321, script_id: 8321 },
    SceneReplayEntry { tab: 2, slot: 2, flag_var: 8322, thumb_id: 322, script_id: 8322 },
    SceneReplayEntry { tab: 2, slot: 3, flag_var: 8323, thumb_id: 323, script_id: 8323 },
    SceneReplayEntry { tab: 2, slot: 4, flag_var: 8331, thumb_id: 331, script_id: 8331 },
    SceneReplayEntry { tab: 2, slot: 5, flag_var: 8332, thumb_id: 332, script_id: 8332 },
    SceneReplayEntry { tab: 2, slot: 6, flag_var: 8333, thumb_id: 333, script_id: 8333 },
    SceneReplayEntry { tab: 2, slot: 7, flag_var: 8341, thumb_id: 341, script_id: 8341 },
    SceneReplayEntry { tab: 2, slot: 8, flag_var: 8342, thumb_id: 342, script_id: 8342 },
    SceneReplayEntry { tab: 2, slot: 9, flag_var: 8351, thumb_id: 351, script_id: 8351 },
    SceneReplayEntry { tab: 2, slot: 10, flag_var: 8361, thumb_id: 361, script_id: 8361 },
    SceneReplayEntry { tab: 2, slot: 11, flag_var: 8371, thumb_id: 371, script_id: 8371 },
    SceneReplayEntry { tab: 2, slot: 12, flag_var: 8381, thumb_id: 381, script_id: 8381 },
    SceneReplayEntry { tab: 2, slot: 13, flag_var: 8382, thumb_id: 382, script_id: 8382 },
    SceneReplayEntry { tab: 2, slot: 14, flag_var: 8391, thumb_id: 391, script_id: 8391 },
    SceneReplayEntry { tab: 3, slot: 0, flag_var: 8411, thumb_id: 411, script_id: 8411 },
    SceneReplayEntry { tab: 3, slot: 1, flag_var: 8421, thumb_id: 421, script_id: 8421 },
    SceneReplayEntry { tab: 3, slot: 2, flag_var: 8422, thumb_id: 422, script_id: 8422 },
    SceneReplayEntry { tab: 3, slot: 3, flag_var: 8431, thumb_id: 431, script_id: 8431 },
    SceneReplayEntry { tab: 3, slot: 4, flag_var: 8432, thumb_id: 432, script_id: 8432 },
    SceneReplayEntry { tab: 3, slot: 5, flag_var: 8441, thumb_id: 441, script_id: 8441 },
    SceneReplayEntry { tab: 3, slot: 6, flag_var: 8442, thumb_id: 442, script_id: 8442 },
    SceneReplayEntry { tab: 3, slot: 7, flag_var: 8451, thumb_id: 451, script_id: 8451 },
    SceneReplayEntry { tab: 3, slot: 8, flag_var: 8461, thumb_id: 461, script_id: 8461 },
    SceneReplayEntry { tab: 3, slot: 9, flag_var: 8471, thumb_id: 471, script_id: 8471 },
    SceneReplayEntry { tab: 3, slot: 10, flag_var: 8481, thumb_id: 481, script_id: 8481 },
    SceneReplayEntry { tab: 4, slot: 0, flag_var: 8511, thumb_id: 511, script_id: 8511 },
    SceneReplayEntry { tab: 4, slot: 1, flag_var: 8521, thumb_id: 521, script_id: 8521 },
    SceneReplayEntry { tab: 4, slot: 2, flag_var: 8531, thumb_id: 531, script_id: 8531 },
    SceneReplayEntry { tab: 4, slot: 3, flag_var: 8541, thumb_id: 541, script_id: 8541 },
    SceneReplayEntry { tab: 4, slot: 4, flag_var: 8542, thumb_id: 542, script_id: 8542 },
    SceneReplayEntry { tab: 4, slot: 5, flag_var: 8543, thumb_id: 543, script_id: 8543 },
    SceneReplayEntry { tab: 4, slot: 6, flag_var: 8551, thumb_id: 551, script_id: 8551 },
    SceneReplayEntry { tab: 4, slot: 7, flag_var: 8552, thumb_id: 552, script_id: 8552 },
    SceneReplayEntry { tab: 4, slot: 8, flag_var: 8553, thumb_id: 553, script_id: 8553 },
    SceneReplayEntry { tab: 4, slot: 9, flag_var: 8561, thumb_id: 561, script_id: 8561 },
    SceneReplayEntry { tab: 4, slot: 10, flag_var: 8571, thumb_id: 571, script_id: 8571 },
    SceneReplayEntry { tab: 5, slot: 0, flag_var: 8611, thumb_id: 611, script_id: 8611 },
    SceneReplayEntry { tab: 5, slot: 1, flag_var: 8621, thumb_id: 621, script_id: 8621 },
    SceneReplayEntry { tab: 5, slot: 2, flag_var: 8622, thumb_id: 622, script_id: 8622 },
    SceneReplayEntry { tab: 5, slot: 3, flag_var: 8623, thumb_id: 623, script_id: 8623 },
    SceneReplayEntry { tab: 5, slot: 4, flag_var: 8624, thumb_id: 624, script_id: 8624 },
    SceneReplayEntry { tab: 5, slot: 5, flag_var: 8625, thumb_id: 625, script_id: 8625 },
    SceneReplayEntry { tab: 5, slot: 6, flag_var: 8631, thumb_id: 631, script_id: 8631 },
    SceneReplayEntry { tab: 5, slot: 7, flag_var: 8641, thumb_id: 641, script_id: 8641 },
    SceneReplayEntry { tab: 5, slot: 8, flag_var: 8651, thumb_id: 651, script_id: 8651 },
    SceneReplayEntry { tab: 5, slot: 9, flag_var: 8661, thumb_id: 661, script_id: 8661 },
    SceneReplayEntry { tab: 5, slot: 10, flag_var: 8671, thumb_id: 671, script_id: 8671 },
    SceneReplayEntry { tab: 5, slot: 11, flag_var: 8681, thumb_id: 681, script_id: 8681 },
    SceneReplayEntry { tab: 5, slot: 12, flag_var: 8691, thumb_id: 691, script_id: 8691 },
    SceneReplayEntry { tab: 5, slot: 13, flag_var: 8701, thumb_id: 701, script_id: 8701 },
    SceneReplayEntry { tab: 5, slot: 14, flag_var: 8711, thumb_id: 711, script_id: 8711 },
    SceneReplayEntry { tab: 5, slot: 15, flag_var: 8712, thumb_id: 712, script_id: 8712 },
    SceneReplayEntry { tab: 5, slot: 16, flag_var: 8713, thumb_id: 713, script_id: 8713 },
    SceneReplayEntry { tab: 5, slot: 17, flag_var: 8721, thumb_id: 721, script_id: 8721 },
];

/// Complete 26-route table inside `0399.gsc` (`vars[999] = 8010..=8036`, excluding `8030`).
pub const RECALL_0399_ROUTES: &[Recall0399Route] = &[
    Recall0399Route { route_id: 8010, script_id: 3236, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8011, script_id: 1125, has_recall_label: true, bgm_id: 4, bg_id: Some(999) },
    Recall0399Route { route_id: 8012, script_id: 1109, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8013, script_id: 3441, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8014, script_id: 4098, has_recall_label: false, bgm_id: 0, bg_id: Some(533) },
    Recall0399Route { route_id: 8015, script_id: 4206, has_recall_label: false, bgm_id: 5, bg_id: Some(181) },
    Recall0399Route { route_id: 8016, script_id: 4203, has_recall_label: true, bgm_id: 5, bg_id: Some(181) },
    Recall0399Route { route_id: 8017, script_id: 4203, has_recall_label: true, bgm_id: 5, bg_id: Some(181) },
    Recall0399Route { route_id: 8018, script_id: 3676, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8019, script_id: 3673, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8020, script_id: 1179, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8021, script_id: 3276, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8022, script_id: 3262, has_recall_label: true, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8023, script_id: 2058, has_recall_label: true, bgm_id: 5, bg_id: None },
    Recall0399Route { route_id: 8024, script_id: 2059, has_recall_label: true, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8025, script_id: 3379, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8026, script_id: 3177, has_recall_label: true, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8027, script_id: 3179, has_recall_label: true, bgm_id: 0, bg_id: Some(999) },
    Recall0399Route { route_id: 8028, script_id: 1148, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8029, script_id: 2245, has_recall_label: true, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8031, script_id: 5110, has_recall_label: true, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8032, script_id: 3478, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8033, script_id: 3479, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8034, script_id: 2379, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8035, script_id: 3363, has_recall_label: false, bgm_id: 0, bg_id: None },
    Recall0399Route { route_id: 8036, script_id: 3503, has_recall_label: false, bgm_id: 0, bg_id: None },
];

/// Look up a `CG_GALLERY_TABLE` entry by `(tab, slot)` (`tab in 0..7`, `slot in 0..30`).
pub fn find_cg_entry(tab: u8, slot: u8) -> Option<&'static CgGalleryEntry> {
    CG_GALLERY_TABLE
        .iter()
        .find(|e| e.tab == tab && e.slot == slot)
}

/// Look up a `SCENE_REPLAY_TABLE` entry by `(tab, slot)` (`tab in 0..7`, `slot in 0..24`).
pub fn find_replay_entry(tab: u8, slot: u8) -> Option<&'static SceneReplayEntry> {
    SCENE_REPLAY_TABLE
        .iter()
        .find(|e| e.tab == tab && e.slot == slot)
}

/// Look up a `0399.gsc` replay route by `route_id` (`8010..=8036`).
pub fn find_recall_0399_route(route_id: u16) -> Option<&'static Recall0399Route> {
    RECALL_0399_ROUTES.iter().find(|r| r.route_id == route_id)
}
