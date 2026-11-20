//! Complete UI subsystem for all 9 original top-level scenes (`0x41d010`), `selmap`,
//! `.lwg`-driven overlays (`compane`, `confscrn`, `savescrn`, `select_6`, `backlog`),
//! bilingual (`Japanese` / `ChineseV1`) dialogs (`MessageBoxA` + `0x445e90`), and metadata tables.

pub mod dialogs;
pub mod scenes;
pub mod tables;
pub mod widgets;

pub use dialogs::{
    confirm_action_text, confirm_callsite_text, messagebox_text, ConfirmCallsite,
    MessageBoxCallsite, ALL_CONFIRM_CALLSITES, ALL_MESSAGEBOX_CALLSITES,
};
pub use scenes::{
    blit_fullscreen_srccopy, CgFullscreenView, CgGalleryState, ExtraAction, ExtraMenuState,
    MusicModeState, SceneReplayState, TitleAction, TitleSceneState,
};
pub use tables::{
    find_cg_entry, find_recall_0399_route, find_replay_entry, CgGalleryEntry, Recall0399Route,
    SceneReplayEntry, CG_GALLERY_TABLE, CG_TAB_COUNTS, DL2007_BGM_TRACK_RANGE,
    EXTRA_BTN_COORDS, RECALL_0399_ROUTES, RECALL_ZEROED_LABEL_SCRIPTS, REPLAY_TAB_COUNTS,
    SCENE_REPLAY_TABLE, TITLE_0101_BTN_COORDS, TITLE_ME_COORDS,
};
pub use widgets::{
    BacklogAction, BacklogOverlay, CompaneButton, CompaneWidget, ConfScrnAction, ConfScrnOverlay,
    ConfigSliderKind, SaveScrnAction, SaveScrnMode, SaveScrnOverlay, SelMapOverlay,
    Select6Overlay, SystemPanelAction, UiRect, COMPANE_ORIGIN_X, COMPANE_ORIGIN_Y,
};
