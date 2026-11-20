//! State, hit-testing, and rendering for top-level UI scenes (`Title`, `CgGallery`,
//! `SceneReplay`, `MusicMode`, `ExtraMenu`, `SubsceneDll`).

use crate::gfx::compositor::{blit, filter_fill_argb, BlitParams};
use crate::gfx::surface::{Surface, GAME_HEIGHT, GAME_WIDTH};
use crate::ui::tables::{
    CgGalleryEntry, Recall0399Route, SceneReplayEntry, CG_GALLERY_TABLE, DL2007_BGM_TRACK_RANGE,
    EXTRA_BTN_COORDS, RECALL_0399_ROUTES, SCENE_REPLAY_TABLE, TITLE_ME_COORDS,
};
use crate::ui::widgets::UiRect;
use crate::vfs::{AssetCategory, AssetVfs};

/// Action triggered by clicking a Title screen button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAction {
    StartNewGame,
    ContinueScript1,
    OpenLoad,
    OpenConfig,
    OpenExtra,
    StartOmake7000,
    RequestExit,
}

/// Title screen state (Scene 1 `0x4712c0`).
#[derive(Debug, Clone, Default)]
pub struct TitleSceneState {
    pub script_driven: bool,
    pub hovered_exe_btn: Option<usize>,
    pub hovered_vm_layer: Option<usize>,
}

impl TitleSceneState {
    pub fn hit_test_exe(&self, vars_7000: i16, px: i32, py: i32) -> Option<usize> {
        for (idx, &(bx, by)) in TITLE_ME_COORDS.iter().enumerate() {
            if idx == 5 && vars_7000 == 0 {
                continue;
            }
            if UiRect::new(bx, by, 180, 22).contains(px, py) {
                return Some(idx);
            }
        }
        None
    }

    pub fn exe_button_action(idx: usize) -> Option<TitleAction> {
        match idx {
            0 => Some(TitleAction::StartNewGame),
            1 => Some(TitleAction::ContinueScript1),
            2 => Some(TitleAction::OpenLoad),
            3 => Some(TitleAction::OpenConfig),
            4 => Some(TitleAction::OpenExtra),
            5 => Some(TitleAction::StartOmake7000),
            6 => Some(TitleAction::RequestExit),
            _ => None,
        }
    }
}

/// Active fullscreen CG viewer state inside `CgGallery`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgFullscreenView {
    pub entry_index: usize,
    pub variant_cursor: usize,
    pub active_variants: Vec<u16>,
    pub current_img_id: u16,
    pub category: AssetCategory,
    pub last_image_missing_in_dl: bool,
}

/// CG Gallery state (Scene 5 `0x4712c8`, `0x41bb50`, 102 sets / 266 images).
#[derive(Debug, Clone, Default)]
pub struct CgGalleryState {
    pub tab: u8,  // 0..=6
    pub page: u8, // 0..=4 (6 slots/page)
    pub hovered_tab: Option<u8>,
    pub hovered_slot: Option<u8>,
    pub fullscreen: Option<CgFullscreenView>,
}

impl CgGalleryState {
    pub const TAB_COUNT: u8 = 7;
    pub const PAGES_PER_TAB: u8 = 5;
    pub const SLOTS_PER_PAGE: u8 = 6;

    pub fn tab_rect(tab: u8) -> UiRect {
        UiRect::new(35 + 84 * i32::from(tab), 47, 76, 28)
    }

    pub fn slot_rect(slot_on_page: u8) -> UiRect {
        let s = i32::from(slot_on_page);
        UiRect::new(39 + (s % 2) * 174, 39 + (s / 2) * 127, 160, 120)
    }

    pub fn prev_rect() -> UiRect {
        UiRect::new(430, 439, 32, 32)
    }

    pub fn next_rect() -> UiRect {
        UiRect::new(497, 439, 32, 32)
    }

    pub fn return_rect() -> UiRect {
        UiRect::new(573, 443, 96, 36)
    }

    pub fn current_entry(&self, slot_on_page: u8) -> Option<(usize, &'static CgGalleryEntry)> {
        let abs_slot = self.page.checked_mul(Self::SLOTS_PER_PAGE)?.checked_add(slot_on_page)?;
        CG_GALLERY_TABLE
            .iter()
            .enumerate()
            .find(|(_, e)| e.tab == self.tab && e.slot == abs_slot)
    }

    pub fn open_entry(&mut self, entry_idx: usize, entry: &CgGalleryEntry) -> &CgFullscreenView {
        let active: Vec<u16> = entry.active_variants().collect();
        let first = active.first().copied().unwrap_or(0);
        self.fullscreen = Some(CgFullscreenView {
            entry_index: entry_idx,
            variant_cursor: 0,
            active_variants: active,
            current_img_id: first,
            category: entry.full_image_category(),
            last_image_missing_in_dl: false,
        });
        self.fullscreen.as_ref().unwrap()
    }

    /// Advance to next variant in fullscreen view; returns `false` if viewer closed at end.
    pub fn advance_fullscreen_variant(&mut self) -> bool {
        let Some(fs) = &mut self.fullscreen else {
            return false;
        };
        if fs.variant_cursor + 1 < fs.active_variants.len() {
            fs.variant_cursor += 1;
            fs.current_img_id = fs.active_variants[fs.variant_cursor];
            true
        } else {
            self.fullscreen = None;
            false
        }
    }
}

/// Scene Replay state (Scene 6 `0x4712ca`, `0x425410`, 79 EXE entries + 0399.gsc dual-track).
#[derive(Debug, Clone, Default)]
pub struct SceneReplayState {
    pub tab: u8,  // 0..=6
    pub page: u8, // 0..=3 (6 slots/page)
    pub last_launched_script_id: Option<u16>,
    pub last_launched_route_id: Option<u16>,
    pub last_missing_legacy_script: Option<u16>,
}

impl SceneReplayState {
    pub const TAB_COUNT: u8 = 7;
    pub const PAGES_PER_TAB: u8 = 4;
    pub const SLOTS_PER_PAGE: u8 = 6;

    pub fn current_entry(&self, slot_on_page: u8) -> Option<&'static SceneReplayEntry> {
        let abs_slot = self.page.checked_mul(Self::SLOTS_PER_PAGE)?.checked_add(slot_on_page)?;
        SCENE_REPLAY_TABLE
            .iter()
            .find(|e| e.tab == self.tab && e.slot == abs_slot)
    }

    pub fn available_0399_routes() -> &'static [Recall0399Route] {
        RECALL_0399_ROUTES
    }
}

/// Music Mode state (Scene 7 `0x4712cc`, `0x41e590`).
#[derive(Debug, Clone, Default)]
pub struct MusicModeState {
    /// Whether `grps/muscmenu.lwg` exists (`false` in stock 2007 DL `grps.xfl`).
    pub muscmenu_lwg_present: bool,
    /// Available `Track%02d.wav` IDs discovered in `bgm.xfl` (`3..=23` in 2007 DL).
    pub available_tracks: Vec<u16>,
    pub current_track_id: Option<u16>,
    pub is_playing: bool,
    pub last_missing_track_id: Option<u16>,
}

impl MusicModeState {
    pub fn probe_from_vfs(&mut self, vfs: &AssetVfs) {
        self.muscmenu_lwg_present = vfs.exists(AssetCategory::Grps, "muscmenu.lwg");
        self.available_tracks.clear();
        for id in DL2007_BGM_TRACK_RANGE.0..=DL2007_BGM_TRACK_RANGE.1 {
            if vfs.exists(AssetCategory::Bgm, &format!("Track{id:02}.wav")) {
                self.available_tracks.push(id);
            }
        }
    }
}

/// Extra Menu action (Scene 8 `0x4712c4`, `0x419210`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraAction {
    CgGallery,
    SceneReplay,
    MusicMode,
    Omake9000,
    ReturnToTitle,
}

#[derive(Debug, Clone, Default)]
pub struct ExtraMenuState {
    pub hovered_btn: Option<usize>,
    pub omake_missing_9000: bool,
}

impl ExtraMenuState {
    pub fn hit_test(&self, px: i32, py: i32) -> Option<usize> {
        for (i, &(bx, by)) in EXTRA_BTN_COORDS.iter().enumerate() {
            if UiRect::new(bx, by, 180, 32).contains(px, py) {
                return Some(i);
            }
        }
        None
    }

    pub fn action_for_btn(idx: usize) -> Option<ExtraAction> {
        match idx {
            0 => Some(ExtraAction::CgGallery),
            1 => Some(ExtraAction::SceneReplay),
            2 => Some(ExtraAction::MusicMode),
            3 => Some(ExtraAction::Omake9000),
            4 => Some(ExtraAction::ReturnToTitle),
            _ => None,
        }
    }
}

/// Render a fullscreen surface stretched/centered cleanly on `800x600` (`SRCCOPY`).
pub fn blit_fullscreen_srccopy(dst: &mut Surface, src: &Surface) {
    if src.width() == GAME_WIDTH && src.height() == GAME_HEIGHT {
        let params = BlitParams {
            blend_mode: 0,
            alpha_channel_mode: 6,
            ..Default::default()
        };
        blit(dst, src, &params, (0, 0), None);
        return;
    }
    filter_fill_argb(dst, 0x0000_0000);
    let dx = (GAME_WIDTH as i32 - src.width() as i32) / 2;
    let dy = (GAME_HEIGHT as i32 - src.height() as i32) / 2;
    let params = BlitParams {
        blend_mode: 0,
        alpha_channel_mode: 6,
        ..Default::default()
    };
    blit(dst, src, &params, (dx, dy), None);
}
