//! Stable C11 ABI FFI layer for Swift (iOS 14.4+) and native hosts.

use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::slice;

use crate::engine::{DialogKind, Engine, EngineConfig, FontRole, PointerKind, TopScene};
use crate::gfx::surface::{GAME_HEIGHT, GAME_WIDTH};
use crate::ui::SystemPanelAction;
use crate::vfs::LanguageMode;

/// Required byte size of the `800x600` BGRA8 presentation buffer (`1,920,000` bytes).
pub const FUJIHIME_PRESENT_BGRA8_LEN: usize = (GAME_WIDTH as usize) * (GAME_HEIGHT as usize) * 4;

/// Maximum UTF-8 byte buffer capacity for dialog title strings.
pub const FUJIHIME_DIALOG_TITLE_CAP: usize = 128;
/// Maximum UTF-8 byte buffer capacity for dialog message strings.
pub const FUJIHIME_DIALOG_MSG_CAP: usize = 512;

/// FFI status codes returned by engine functions.
pub const FUJIHIME_OK: i32 = 0;
pub const FUJIHIME_ERR_INVALID_ARG: i32 = -1;
pub const FUJIHIME_ERR_RUNTIME: i32 = -2;
pub const FUJIHIME_ERR_PANIC: i32 = -3;

/// Active game language / patch mode.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FujihimeLanguageMode {
    #[default]
    Japanese = 0,
    ChineseV1 = 1,
}

impl From<FujihimeLanguageMode> for LanguageMode {
    fn from(m: FujihimeLanguageMode) -> Self {
        match m {
            FujihimeLanguageMode::Japanese => LanguageMode::Japanese,
            FujihimeLanguageMode::ChineseV1 => LanguageMode::ChineseV1,
        }
    }
}

impl From<LanguageMode> for FujihimeLanguageMode {
    fn from(m: LanguageMode) -> Self {
        match m {
            LanguageMode::Japanese => FujihimeLanguageMode::Japanese,
            LanguageMode::ChineseV1 => FujihimeLanguageMode::ChineseV1,
        }
    }
}

/// Font slot role for [`fujihime_engine_load_font`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FujihimeFontRole {
    Gothic = 0,
    Mincho = 1,
    Fallback = 2,
}

impl From<FujihimeFontRole> for FontRole {
    fn from(r: FujihimeFontRole) -> Self {
        match r {
            FujihimeFontRole::Gothic => FontRole::Gothic,
            FujihimeFontRole::Mincho => FontRole::Mincho,
            FujihimeFontRole::Fallback => FontRole::Fallback,
        }
    }
}

/// Pointer / touch event kind (`800x600` logical coordinate space).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FujihimePointerKind {
    Move = 0,
    Down = 1,
    Up = 2,
    RightUp = 3,
    MiddleUp = 4,
    WheelUp = 5,
    WheelDown = 6,
}

impl From<FujihimePointerKind> for PointerKind {
    fn from(k: FujihimePointerKind) -> Self {
        match k {
            FujihimePointerKind::Move => PointerKind::Move,
            FujihimePointerKind::Down => PointerKind::Down,
            FujihimePointerKind::Up => PointerKind::Up,
            FujihimePointerKind::RightUp => PointerKind::RightUp,
            FujihimePointerKind::MiddleUp => PointerKind::MiddleUp,
            FujihimePointerKind::WheelUp => PointerKind::WheelUp,
            FujihimePointerKind::WheelDown => PointerKind::WheelDown,
        }
    }
}

/// Top-level UI scene identifier (`0x41d010`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FujihimeSceneKind {
    #[default]
    Title = 1,
    MainGame = 2,
    LoadMenu = 3,
    ConfigMenu = 4,
    CgGallery = 5,
    SceneReplay = 6,
    MusicMode = 7,
    ExtraMenu = 8,
    SubsceneDll = 9,
}

impl From<TopScene> for FujihimeSceneKind {
    fn from(s: TopScene) -> Self {
        match s {
            TopScene::Title => FujihimeSceneKind::Title,
            TopScene::MainGame => FujihimeSceneKind::MainGame,
            TopScene::LoadMenu => FujihimeSceneKind::LoadMenu,
            TopScene::ConfigMenu => FujihimeSceneKind::ConfigMenu,
            TopScene::CgGallery => FujihimeSceneKind::CgGallery,
            TopScene::SceneReplay => FujihimeSceneKind::SceneReplay,
            TopScene::MusicMode => FujihimeSceneKind::MusicMode,
            TopScene::ExtraMenu => FujihimeSceneKind::ExtraMenu,
            TopScene::SubsceneDll => FujihimeSceneKind::SubsceneDll,
        }
    }
}

/// Modality for native confirmation alerts (`UIAlertController`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FujihimeDialogKind {
    OkAlert = 0,
    YesNoConfirm = 1,
}

/// High-level iOS touch/gesture & system panel actions (`compane` + `WndProc` parity).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FujihimeSystemAction {
    Save = 0,
    Load = 1,
    Config = 2,
    Backlog = 3,
    Skip = 4,
    Auto = 5,
    HideTextbox = 6,
    Rollback = 7,
    Title = 8,
    Exit = 9,
    Advance = 10,
    ReplayVoice = 11,
}

impl From<FujihimeSystemAction> for SystemPanelAction {
    fn from(a: FujihimeSystemAction) -> Self {
        match a {
            FujihimeSystemAction::Save => SystemPanelAction::Save,
            FujihimeSystemAction::Load => SystemPanelAction::Load,
            FujihimeSystemAction::Config => SystemPanelAction::Config,
            FujihimeSystemAction::Backlog => SystemPanelAction::Backlog,
            FujihimeSystemAction::Skip => SystemPanelAction::Skip,
            FujihimeSystemAction::Auto => SystemPanelAction::Auto,
            FujihimeSystemAction::HideTextbox => SystemPanelAction::HideTextbox,
            FujihimeSystemAction::Rollback => SystemPanelAction::Rollback,
            FujihimeSystemAction::Title => SystemPanelAction::Title,
            FujihimeSystemAction::Exit => SystemPanelAction::Exit,
            FujihimeSystemAction::Advance => SystemPanelAction::Advance,
            FujihimeSystemAction::ReplayVoice => SystemPanelAction::ReplayVoice,
        }
    }
}

impl From<FujihimeSceneKind> for TopScene {
    fn from(s: FujihimeSceneKind) -> Self {
        match s {
            FujihimeSceneKind::Title => TopScene::Title,
            FujihimeSceneKind::MainGame => TopScene::MainGame,
            FujihimeSceneKind::LoadMenu => TopScene::LoadMenu,
            FujihimeSceneKind::ConfigMenu => TopScene::ConfigMenu,
            FujihimeSceneKind::CgGallery => TopScene::CgGallery,
            FujihimeSceneKind::SceneReplay => TopScene::SceneReplay,
            FujihimeSceneKind::MusicMode => TopScene::MusicMode,
            FujihimeSceneKind::ExtraMenu => TopScene::ExtraMenu,
            FujihimeSceneKind::SubsceneDll => TopScene::SubsceneDll,
        }
    }
}

/// Engine creation configuration passed by C/Swift caller.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FujihimeEngineConfig {
    pub game_root: *const c_char,
    pub save_dir: *const c_char,
    pub override_dir: *const c_char,
    pub orig_backup_dir: *const c_char,
    pub language_mode: FujihimeLanguageMode,
    pub repair_chs_recall_labels: bool,
}

/// Pending native dialog description copied safely into caller-owned C buffers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FujihimePendingDialog {
    pub dialog_id: u64,
    pub kind: FujihimeDialogKind,
    pub title_utf8: [u8; FUJIHIME_DIALOG_TITLE_CAP],
    pub message_utf8: [u8; FUJIHIME_DIALOG_MSG_CAP],
}

impl Default for FujihimePendingDialog {
    fn default() -> Self {
        Self {
            dialog_id: 0,
            kind: FujihimeDialogKind::OkAlert,
            title_utf8: [0; FUJIHIME_DIALOG_TITLE_CAP],
            message_utf8: [0; FUJIHIME_DIALOG_MSG_CAP],
        }
    }
}

/// Engine runtime state snapshot returned by [`fujihime_engine_get_state`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FujihimeEngineState {
    pub scene: FujihimeSceneKind,
    pub language_mode: FujihimeLanguageMode,
    pub is_playing_video: bool,
    pub is_waiting_click: bool,
    pub is_waiting_select: bool,
    pub is_transitioning: bool,
    pub exit_requested: bool,
    pub current_script_id: u16,
    pub current_pc: u32,
}

/// Opaque engine handle owned by the C/Swift host.
pub struct FujihimeEngine {
    inner: Engine,
}

unsafe fn opt_cstr_path(ptr: *const c_char) -> Option<PathBuf> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok().filter(|s| !s.is_empty()).map(PathBuf::from)
}

fn copy_utf8_cstr(dst: &mut [u8], src: &str) {
    dst.fill(0);
    let max_len = dst.len().saturating_sub(1);
    let mut end = src.len().min(max_len);
    while end > 0 && !src.is_char_boundary(end) {
        end -= 1;
    }
    dst[..end].copy_from_slice(&src.as_bytes()[..end]);
    dst[end] = 0;
}

/// Create and initialize a new [`FujihimeEngine`] instance. Returns `NULL` on error.
///
/// # Safety
/// `config` and its non-null C string fields must be valid NUL-terminated pointers.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_create(
    config: *const FujihimeEngineConfig,
) -> *mut FujihimeEngine {
    if config.is_null() {
        return std::ptr::null_mut();
    }
    let res = catch_unwind(AssertUnwindSafe(|| {
        let cfg = &*config;
        let Some(game_root) = opt_cstr_path(cfg.game_root) else {
            return std::ptr::null_mut();
        };
        let Some(save_dir) = opt_cstr_path(cfg.save_dir) else {
            return std::ptr::null_mut();
        };
        let mut engine_cfg = EngineConfig::new(game_root, save_dir, cfg.language_mode.into());
        engine_cfg.override_dir = opt_cstr_path(cfg.override_dir);
        engine_cfg.orig_backup_dir = opt_cstr_path(cfg.orig_backup_dir);
        engine_cfg.repair_chs_recall_labels = cfg.repair_chs_recall_labels;

        match Engine::create(engine_cfg) {
            Ok(inner) => Box::into_raw(Box::new(FujihimeEngine { inner })),
            Err(_) => std::ptr::null_mut(),
        }
    }));
    res.unwrap_or(std::ptr::null_mut())
}

/// Destroy an engine instance created by [`fujihime_engine_create`]. Passing `NULL` is a no-op.
///
/// # Safety
/// `engine` must either be `NULL` or a live pointer returned by [`fujihime_engine_create`].
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_destroy(engine: *mut FujihimeEngine) {
    if engine.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(Box::from_raw(engine));
    }));
}

/// Inject an in-memory TTF/OTF/TTC font buffer for the specified role (`AppBundle/Fonts/`).
///
/// # Safety
/// `engine` must be a valid handle and `bytes` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_load_font(
    engine: *mut FujihimeEngine,
    role: FujihimeFontRole,
    bytes: *const u8,
    len: usize,
    face_index: i32,
) -> i32 {
    if engine.is_null() || bytes.is_null() || len == 0 {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        let data = slice::from_raw_parts(bytes, len).to_vec();
        match eng.inner.load_font_bytes(role.into(), data, face_index) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Start the original Title OP sequence (`0002.mpg` -> `0001.mpg` -> Title screen).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_start_title(engine: *mut FujihimeEngine) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.start_title_sequence() {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Enter main ADV gameplay at `script_id` (e.g. `0` for New Game).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_start_game(
    engine: *mut FujihimeEngine,
    script_id: u16,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.start_game_script(script_id) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Advance the engine simulation by `delta_ms` milliseconds.
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_tick(
    engine: *mut FujihimeEngine,
    delta_ms: u32,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.tick(delta_ms) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Dispatch a pointer/touch event in logical `800x600` coordinates.
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_pointer_event(
    engine: *mut FujihimeEngine,
    kind: FujihimePointerKind,
    x: i32,
    y: i32,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.pointer_event(kind.into(), x, y) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Dispatch a Win32 `VK_*` key event (`0x0D` Return, `0x20` Space, `0x11` Ctrl, `0x10` Shift, `0x70` F1).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_key_event(
    engine: *mut FujihimeEngine,
    vk_code: u16,
    is_down: bool,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.key_event(vk_code, is_down) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Poll for a pending native dialog to display via `UIAlertController`. Returns `true` if populated.
///
/// # Safety
/// `engine` and `out_dialog` must be non-null valid pointers.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_poll_pending_dialog(
    engine: *mut FujihimeEngine,
    out_dialog: *mut FujihimePendingDialog,
) -> bool {
    if engine.is_null() || out_dialog.is_null() {
        return false;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        let Some(dlg) = eng.inner.poll_pending_dialog() else {
            return false;
        };
        let mut out = FujihimePendingDialog {
            dialog_id: dlg.id,
            kind: match dlg.kind {
                DialogKind::OkAlert => FujihimeDialogKind::OkAlert,
                DialogKind::YesNoConfirm => FujihimeDialogKind::YesNoConfirm,
            },
            title_utf8: [0; FUJIHIME_DIALOG_TITLE_CAP],
            message_utf8: [0; FUJIHIME_DIALOG_MSG_CAP],
        };
        copy_utf8_cstr(&mut out.title_utf8, &dlg.title);
        copy_utf8_cstr(&mut out.message_utf8, &dlg.message);
        *out_dialog = out;
        true
    }))
    .unwrap_or(false)
}

/// Deliver user acceptance (`accepted = true` for `IDYES` / `OK`) for a pending dialog.
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_confirm_dialog_response(
    engine: *mut FujihimeEngine,
    dialog_id: u64,
    accepted: bool,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.confirm_dialog_response(dialog_id, accepted) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Copy the current `800x600` front framebuffer in standard BGRA8 (`A = 0xFF` opaque) for Metal upload.
///
/// `out_len` must be at least `800 * 600 * 4 = 1,920,000` bytes.
///
/// # Safety
/// `engine` must be valid and `out_ptr` must point to `out_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_copy_present_bgra8(
    engine: *const FujihimeEngine,
    out_ptr: *mut u8,
    out_len: usize,
) -> i32 {
    if engine.is_null() || out_ptr.is_null() || out_len < FUJIHIME_PRESENT_BGRA8_LEN {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &*engine;
        let out_slice = slice::from_raw_parts_mut(out_ptr, FUJIHIME_PRESENT_BGRA8_LEN);
        match eng.inner.copy_present_bgra8(out_slice) {
            Ok(()) => FUJIHIME_OK,
            Err(_) => FUJIHIME_ERR_RUNTIME,
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Pull `frames` interleaved stereo `f32` PCM samples (`44,100 Hz`) for `AVAudioEngine`.
///
/// Returns the number of stereo frames written.
///
/// # Safety
/// `engine` must be valid and `out_interleaved` must point to `frames * 2` writable `f32`s.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_render_audio_f32(
    engine: *const FujihimeEngine,
    out_interleaved: *mut f32,
    frames: usize,
) -> usize {
    if engine.is_null() || out_interleaved.is_null() || frames == 0 {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &*engine;
        let out = slice::from_raw_parts_mut(out_interleaved, frames * 2);
        eng.inner.render_audio_f32(out)
    }))
    .unwrap_or(0)
}

/// Query current engine status snapshot.
///
/// # Safety
/// `engine` and `out_state` must be non-null valid pointers.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_get_state(
    engine: *const FujihimeEngine,
    out_state: *mut FujihimeEngineState,
) -> i32 {
    if engine.is_null() || out_state.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &*engine;
        let st = eng.inner.status();
        *out_state = FujihimeEngineState {
            scene: st.scene.into(),
            language_mode: st.language_mode.into(),
            is_playing_video: st.is_playing_video,
            is_waiting_click: st.is_waiting_click,
            is_waiting_select: st.is_waiting_select,
            is_transitioning: st.is_transitioning,
            exit_requested: st.exit_requested,
            current_script_id: st.current_script_id,
            current_pc: st.current_pc,
        };
        FUJIHIME_OK
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Flush global save `KhDLsave.dat` to `save_dir`.
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_save_global(engine: *mut FujihimeEngine) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.save_global() {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Save slot `KhDLsave%02d.dat` (`75,026 B`) to `save_dir`.
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_save_slot(
    engine: *mut FujihimeEngine,
    slot_num: u16,
    year: u16,
    month: u16,
    day: u16,
    hour: u16,
    minute: u16,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.save_slot(slot_num, year, month, day, hour, minute) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Load slot `KhDLsave%02d.dat` (`75,026 B`) from `save_dir`.
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_load_slot(
    engine: *mut FujihimeEngine,
    slot_num: u16,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.load_slot(slot_num) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Trigger one-step rollback restore (`0x424060`).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_rollback(engine: *mut FujihimeEngine) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.trigger_rollback() {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Switch active language mode (`Japanese` vs `ChineseV1`) at runtime.
///
/// # Safety
/// `engine` must be a valid handle and `orig_backup_dir` may be `NULL` or a valid NUL-terminated UTF-8 path.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_set_language_mode(
    engine: *mut FujihimeEngine,
    mode: FujihimeLanguageMode,
    orig_backup_dir: *const c_char,
    repair_chs_recall_labels: bool,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        let bak = opt_cstr_path(orig_backup_dir);
        match eng
            .inner
            .set_language_mode(mode.into(), bak, repair_chs_recall_labels)
        {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Import a 206,024-byte 2002 CD `KhDLsave.dat` (e.g. Full CG save), migrate to 2007 DL,
/// apply to live state, and persist `KhDLsave.dat`.
///
/// # Safety
/// `engine` must be valid and `bytes` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_import_2002_cd_save(
    engine: *mut FujihimeEngine,
    bytes: *const u8,
    len: usize,
) -> i32 {
    if engine.is_null() || bytes.is_null() || len == 0 {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        let slice = slice::from_raw_parts(bytes, len);
        match eng.inner.import_2002_cd_save(slice) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Execute an iOS gesture / `compane` system action (`Save`, `Load`, `Config`, `Backlog`, etc.).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_perform_action(
    engine: *mut FujihimeEngine,
    action: FujihimeSystemAction,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.perform_system_action(action.into()) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Navigate directly to one of the 9 top-level UI scenes (`0x41d010`).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_enter_scene(
    engine: *mut FujihimeEngine,
    scene: FujihimeSceneKind,
) -> i32 {
    if engine.is_null() {
        return FUJIHIME_ERR_INVALID_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let eng = &mut *engine;
        match eng.inner.enter_scene(scene.into()) {
            Ok(()) => FUJIHIME_OK,
            Err(e) => {
                eng.inner.set_last_error(e.to_string());
                FUJIHIME_ERR_RUNTIME
            }
        }
    }))
    .unwrap_or(FUJIHIME_ERR_PANIC)
}

/// Convenience helper: toggle textbox visibility (`Shift` / `compane hide` / two-finger tap).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_toggle_textbox(engine: *mut FujihimeEngine) -> i32 {
    fujihime_engine_perform_action(engine, FujihimeSystemAction::HideTextbox)
}

/// Convenience helper: toggle Skip mode (`Tab` / `compane fow` / long-press).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_toggle_skip(engine: *mut FujihimeEngine) -> i32 {
    fujihime_engine_perform_action(engine, FujihimeSystemAction::Skip)
}

/// Convenience helper: toggle Auto-play mode (`compane auto`).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_toggle_auto(engine: *mut FujihimeEngine) -> i32 {
    fujihime_engine_perform_action(engine, FujihimeSystemAction::Auto)
}

/// Convenience helper: open Backlog (`Esc` / `WheelUp` / swipe-up).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_open_backlog(engine: *mut FujihimeEngine) -> i32 {
    fujihime_engine_perform_action(engine, FujihimeSystemAction::Backlog)
}

/// Convenience helper: open Config (`confscrn.lwg`).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_open_config(engine: *mut FujihimeEngine) -> i32 {
    fujihime_engine_perform_action(engine, FujihimeSystemAction::Config)
}

/// Convenience helper: open Save screen (`savescrn.lwg`).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_open_save_menu(engine: *mut FujihimeEngine) -> i32 {
    fujihime_engine_perform_action(engine, FujihimeSystemAction::Save)
}

/// Convenience helper: open Load screen (`savescrn.lwg`).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_open_load_menu(engine: *mut FujihimeEngine) -> i32 {
    fujihime_engine_perform_action(engine, FujihimeSystemAction::Load)
}

/// Convenience helper: trigger Exit confirmation dialog (`0x44753a`).
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn fujihime_engine_request_exit(engine: *mut FujihimeEngine) -> i32 {
    fujihime_engine_perform_action(engine, FujihimeSystemAction::Exit)
}
