//! Virtual File System (`AssetVfs`) for layered asset resolution.
//!
//! Resolution priority:
//! 1. `override_dir` (e.g. iOS `Documents/override/`) — highest priority in all modes.
//! 2. Mode-specific layer:
//!    - [`LanguageMode::Japanese`]: Original `.xfl` archives (`scr.xfl` & `grpo_tp.xfl`
//!      loaded from `orig_backup_dir` / `补丁/原版备份/` when present, else `game_dir`).
//!      Scattered CHS override folders (`grpe/`, `grpo_ex/`, `grps/`) are ignored.
//!    - [`LanguageMode::ChineseV1`]: Scattered override folders (`grpe/`, `grpo_ex/`,
//!      `grps/`, etc. under `game_dir`) take precedence over root `.xfl` archives
//!      (including patched `scr.xfl` & `grpo_tp.xfl`).
//! 3. Base `.xfl` archives (`grpe.xfl`, `grpo.xfl`, `grpp.xfl`, `grps.xfl`, `grpf.xfl`,
//!    `grpo_ex.xfl`, `bgm.xfl`, `voice.xfl`, `wav.xfl`) and `mov/` directory.

pub mod xfl;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{FujihimeError, Result};
pub use xfl::{XflArchive, XflEntry};

/// Active game language / patch resolution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageMode {
    /// Original Japanese DL edition (ground truth).
    Japanese,
    /// CHS V1.0 fan translation compatibility mode.
    ChineseV1,
}

/// Logical asset categories corresponding to original archives/directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetCategory {
    Grpe,
    Grpo,
    Grpp,
    Grps,
    Grpf,
    GrpoEx,
    GrpoTp,
    Scr,
    Bgm,
    Voice,
    Wav,
    Mov,
}

impl AssetCategory {
    /// Directory / archive base stem (e.g. `"grpe"`, `"grpo_tp"`, `"mov"`).
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Grpe => "grpe",
            Self::Grpo => "grpo",
            Self::Grpp => "grpp",
            Self::Grps => "grps",
            Self::Grpf => "grpf",
            Self::GrpoEx => "grpo_ex",
            Self::GrpoTp => "grpo_tp",
            Self::Scr => "scr",
            Self::Bgm => "bgm",
            Self::Voice => "voice",
            Self::Wav => "wav",
            Self::Mov => "mov",
        }
    }

    /// Corresponding `.xfl` archive filename, if backed by an archive.
    pub const fn xfl_filename(self) -> Option<&'static str> {
        match self {
            Self::Grpe => Some("grpe.xfl"),
            Self::Grpo => Some("grpo.xfl"),
            Self::Grpp => Some("grpp.xfl"),
            Self::Grps => Some("grps.xfl"),
            Self::Grpf => Some("grpf.xfl"),
            Self::GrpoEx => Some("grpo_ex.xfl"),
            Self::GrpoTp => Some("grpo_tp.xfl"),
            Self::Scr => Some("scr.xfl"),
            Self::Bgm => Some("bgm.xfl"),
            Self::Voice => Some("voice.xfl"),
            Self::Wav => Some("wav.xfl"),
            Self::Mov => None,
        }
    }

    /// Whether this archive was replaced in `补丁/原版备份/` by the CHS patch.
    pub const fn has_orig_backup_variant(self) -> bool {
        matches!(self, Self::Scr | Self::GrpoTp)
    }

    /// All categories backed by `.xfl` archives.
    pub const ALL_XFL: [Self; 11] = [
        Self::Grpe,
        Self::Grpo,
        Self::Grpp,
        Self::Grps,
        Self::Grpf,
        Self::GrpoEx,
        Self::GrpoTp,
        Self::Scr,
        Self::Bgm,
        Self::Voice,
        Self::Wav,
    ];
}

/// Configuration used to initialize [`AssetVfs`].
#[derive(Debug, Clone)]
pub struct VfsConfig {
    /// Base game directory containing `.xfl` files and `mov/`.
    pub game_dir: PathBuf,
    /// Optional hot-override directory (`Documents/override/`), checked first.
    pub override_dir: Option<PathBuf>,
    /// Optional explicit directory containing original unpatched `scr.xfl` and `grpo_tp.xfl`.
    /// When `None` in [`LanguageMode::Japanese`], `AssetVfs` auto-detects `<game_dir>/补丁/原版备份`
    /// if present, and falls back to `game_dir`.
    pub orig_backup_dir: Option<PathBuf>,
    /// Language mode controlling backup vs scattered-patch resolution.
    pub language_mode: LanguageMode,
}

impl VfsConfig {
    /// Create a configuration with default auto-detection for original backup paths.
    pub fn new(game_dir: impl Into<PathBuf>, language_mode: LanguageMode) -> Self {
        Self {
            game_dir: game_dir.into(),
            override_dir: None,
            orig_backup_dir: None,
            language_mode,
        }
    }

    /// Set a highest-priority override directory.
    pub fn with_override_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.override_dir = Some(dir.into());
        self
    }

    /// Set an explicit original-backup directory for `scr.xfl` and `grpo_tp.xfl`.
    pub fn with_orig_backup_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.orig_backup_dir = Some(dir.into());
        self
    }
}

/// Where an asset was resolved from (for diagnostics and testing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetSource {
    OverrideFile(PathBuf),
    ScatteredPatchFile(PathBuf),
    Archive {
        archive_path: PathBuf,
        entry_name: String,
    },
    MovieFile(PathBuf),
}

/// Asset bytes paired with provenance metadata.
#[derive(Debug, Clone)]
pub struct ResolvedAsset {
    pub bytes: Vec<u8>,
    pub source: AssetSource,
}

/// Layered virtual filesystem for original and patched game assets.
#[derive(Debug)]
pub struct AssetVfs {
    config: VfsConfig,
    archives: HashMap<AssetCategory, (PathBuf, XflArchive)>,
}

impl AssetVfs {
    /// Initialize the VFS and eagerly parse `.xfl` directory indices (without loading payloads).
    pub fn open(config: VfsConfig) -> Result<Self> {
        let mut archives = HashMap::new();
        let backup_dir = resolve_backup_dir(&config);

        for category in AssetCategory::ALL_XFL {
            let Some(xfl_name) = category.xfl_filename() else {
                continue;
            };
            let candidate = if config.language_mode == LanguageMode::Japanese
                && category.has_orig_backup_variant()
            {
                let in_backup = backup_dir
                    .as_ref()
                    .and_then(|dir| find_file_case_insensitive(dir, xfl_name));
                in_backup.or_else(|| find_file_case_insensitive(&config.game_dir, xfl_name))
            } else {
                find_file_case_insensitive(&config.game_dir, xfl_name)
            };

            if let Some(path) = candidate {
                let archive = XflArchive::open(&path)?;
                archives.insert(category, (path, archive));
            }
        }

        Ok(Self { config, archives })
    }

    /// Return the active [`VfsConfig`].
    pub fn config(&self) -> &VfsConfig {
        &self.config
    }

    /// Access an opened `.xfl` archive for a category, if present.
    pub fn archive(&self, category: AssetCategory) -> Option<&XflArchive> {
        self.archives.get(&category).map(|(_, a)| a)
    }

    /// Resolve an exact asset filename (case-insensitive) within `category`.
    pub fn resolve(&self, category: AssetCategory, name: &str) -> Result<ResolvedAsset> {
        // 1. Highest priority: `override_dir`
        if let Some(override_dir) = &self.config.override_dir {
            if let Some(sub) = find_dir_case_insensitive(override_dir, category.dir_name()) {
                if let Some(file_path) = find_file_case_insensitive(&sub, name) {
                    let bytes =
                        fs::read(&file_path).map_err(|e| FujihimeError::io(&file_path, e))?;
                    return Ok(ResolvedAsset {
                        bytes,
                        source: AssetSource::OverrideFile(file_path),
                    });
                }
            }
            if let Some(file_path) = find_file_case_insensitive(override_dir, name) {
                let bytes = fs::read(&file_path).map_err(|e| FujihimeError::io(&file_path, e))?;
                return Ok(ResolvedAsset {
                    bytes,
                    source: AssetSource::OverrideFile(file_path),
                });
            }
        }

        // 2. CHS V1.0 scattered directories (`grpe/`, `grpo_ex/`, `grps/`, etc.)
        if self.config.language_mode == LanguageMode::ChineseV1 && category != AssetCategory::Mov
        {
            if let Some(sub) =
                find_dir_case_insensitive(&self.config.game_dir, category.dir_name())
            {
                if let Some(file_path) = find_file_case_insensitive(&sub, name) {
                    let bytes =
                        fs::read(&file_path).map_err(|e| FujihimeError::io(&file_path, e))?;
                    return Ok(ResolvedAsset {
                        bytes,
                        source: AssetSource::ScatteredPatchFile(file_path),
                    });
                }
            }
        }

        // 3. `mov/` directory files
        if category == AssetCategory::Mov {
            if let Some(mov_dir) = find_dir_case_insensitive(&self.config.game_dir, "mov") {
                if let Some(file_path) = find_file_case_insensitive(&mov_dir, name) {
                    let bytes =
                        fs::read(&file_path).map_err(|e| FujihimeError::io(&file_path, e))?;
                    return Ok(ResolvedAsset {
                        bytes,
                        source: AssetSource::MovieFile(file_path),
                    });
                }
            }
            return Err(FujihimeError::AssetNotFound {
                name: format!("mov/{name}"),
            });
        }

        // 4. `.xfl` archive lookup
        if let Some((archive_path, archive)) = self.archives.get(&category) {
            if let Some(entry) = archive.find_entry(name) {
                let bytes = archive.read_entry(entry)?;
                return Ok(ResolvedAsset {
                    bytes,
                    source: AssetSource::Archive {
                        archive_path: archive_path.clone(),
                        entry_name: entry.name.clone(),
                    },
                });
            }
        }

        Err(FujihimeError::AssetNotFound {
            name: format!("{}/{name}", category.dir_name()),
        })
    }

    /// Resolve raw bytes for an exact asset filename in `category`.
    pub fn read(&self, category: AssetCategory, name: &str) -> Result<Vec<u8>> {
        self.resolve(category, name).map(|a| a.bytes)
    }

    /// Check whether `name` exists in `category` without reading its payload.
    pub fn exists(&self, category: AssetCategory, name: &str) -> bool {
        if let Some(override_dir) = &self.config.override_dir {
            if let Some(sub) = find_dir_case_insensitive(override_dir, category.dir_name()) {
                if find_file_case_insensitive(&sub, name).is_some() {
                    return true;
                }
            }
            if find_file_case_insensitive(override_dir, name).is_some() {
                return true;
            }
        }
        if self.config.language_mode == LanguageMode::ChineseV1 && category != AssetCategory::Mov {
            if let Some(sub) =
                find_dir_case_insensitive(&self.config.game_dir, category.dir_name())
            {
                if find_file_case_insensitive(&sub, name).is_some() {
                    return true;
                }
            }
        }
        if category == AssetCategory::Mov {
            return find_dir_case_insensitive(&self.config.game_dir, "mov")
                .and_then(|d| find_file_case_insensitive(&d, name))
                .is_some();
        }
        self.archives
            .get(&category)
            .and_then(|(_, archive)| archive.find_entry(name))
            .is_some()
    }

    /// Resolve a graphic asset by stem or filename, trying candidate extensions in order.
    ///
    /// For example, in `ChineseV1` mode `grpo_tp.xfl` replaces some `.wcg` files with `.bmp`;
    /// calling `resolve_with_extensions(AssetCategory::GrpoTp, "8000", &["wcg", "bmp"])`
    /// or passing `"8000.wcg"` will locate `"8000.wcg"` or fall back to `"8000.bmp"`.
    pub fn resolve_with_extensions(
        &self,
        category: AssetCategory,
        stem_or_name: &str,
        extensions: &[&str],
    ) -> Result<ResolvedAsset> {
        // First try exact name if it already contains a dot.
        if stem_or_name.contains('.') {
            if let Ok(found) = self.resolve(category, stem_or_name) {
                return Ok(found);
            }
        }
        let stem = match stem_or_name.rsplit_once('.') {
            Some((s, _)) => s,
            None => stem_or_name,
        };
        for ext in extensions {
            let ext = ext.trim_start_matches('.');
            let candidate = format!("{stem}.{ext}");
            if let Ok(found) = self.resolve(category, &candidate) {
                return Ok(found);
            }
        }
        Err(FujihimeError::AssetNotFound {
            name: format!("{}/{}", category.dir_name(), stem_or_name),
        })
    }

    /// Resolve an `ef%02d.msk` rule transition mask by effect id (`5..=99`).
    pub fn read_transition_mask(&self, effect_id: u16) -> Result<Vec<u8>> {
        let name = format!("ef{effect_id:02}.msk");
        self.read(AssetCategory::Grps, &name)
    }
}

fn resolve_backup_dir(config: &VfsConfig) -> Option<PathBuf> {
    if let Some(explicit) = &config.orig_backup_dir {
        if explicit.is_dir() {
            return Some(explicit.clone());
        }
        return None;
    }
    let patch = config.game_dir.join("补丁").join("原版备份");
    if patch.is_dir() {
        return Some(patch);
    }
    None
}

fn find_file_case_insensitive(dir: &Path, target: &str) -> Option<PathBuf> {
    let direct = dir.join(target);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if let Some(s) = name.to_str() {
            if s.eq_ignore_ascii_case(target) {
                let p = entry.path();
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn find_dir_case_insensitive(parent: &Path, target: &str) -> Option<PathBuf> {
    let direct = parent.join(target);
    if direct.is_dir() {
        return Some(direct);
    }
    let entries = fs::read_dir(parent).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if let Some(s) = name.to_str() {
            if s.eq_ignore_ascii_case(target) {
                let p = entry.path();
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    None
}
