#ifndef FUJIHIME_CORE_H
#define FUJIHIME_CORE_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * DirectSound minimum volume constant (`-100 dB`, hard mute `gain = 0.0`).
 */
#define DSBVOLUME_MIN -10000

/**
 * Minimum attenuated volume used by `0x45a200` and fade curves (`-50 dB`).
 */
#define FADE_FLOOR_MB -5000

/**
 * Fixed fade timer period (`100 ms` = `0x64`, `0x459ae9`).
 */
#define FADE_TICK_MS 100

/**
 * Number of stereo frames per `100 ms` fade tick at `44,100 Hz`.
 */
#define FADE_TICK_FRAMES (((uintptr_t)AUDIO_SAMPLE_RATE * (uintptr_t)FADE_TICK_MS) / 1000)

/**
 * Number of stereo frames per `50 ms` voice-delay unit (`param_4 * 50 ms`).
 */
#define VOICE_DELAY_UNIT_FRAMES (((uintptr_t)AUDIO_SAMPLE_RATE * 50) / 1000)

/**
 * Default fade step count (`150` steps = `15.0 s`, `0x459ba7`: `0x96`).
 */
#define DEFAULT_FADE_STEPS 150

/**
 * Standard audio sample rate used across all 3,084 `KhimeDL.exe` audio assets.
 */
#define AUDIO_SAMPLE_RATE 44100

/**
 * RIFF WAVE `wFormatTag` used by `vorbis.acm`.
 */
#define VORBIS_ACM_FORMAT_TAG 26481

/**
 * Required byte size of the `800x600` BGRA8 presentation buffer (`1,920,000` bytes).
 */
#define FUJIHIME_PRESENT_BGRA8_LEN (((uintptr_t)GAME_WIDTH * (uintptr_t)GAME_HEIGHT) * 4)

/**
 * Maximum UTF-8 byte buffer capacity for dialog title strings.
 */
#define FUJIHIME_DIALOG_TITLE_CAP 128

/**
 * Maximum UTF-8 byte buffer capacity for dialog message strings.
 */
#define FUJIHIME_DIALOG_MSG_CAP 512

/**
 * FFI status codes returned by engine functions.
 */
#define FUJIHIME_OK 0

#define FUJIHIME_ERR_INVALID_ARG -1

#define FUJIHIME_ERR_RUNTIME -2

#define FUJIHIME_ERR_PANIC -3

/**
 * Standard game logical canvas width.
 */
#define GAME_WIDTH 800

/**
 * Standard game logical canvas height.
 */
#define GAME_HEIGHT 600

/**
 * Size of the `DAT_00471378..0x471b8b` system config + global string block (`0x814` B).
 */
#define SAVE_CONFIG_SIZE 2068

/**
 * Byte offset of the 10x201B global string slots inside [`ConfigBlock2007`].
 */
#define GLOBAL_STR_OFFSET 58

/**
 * Number of global string slots (`0x9e` / `0x9f`).
 */
#define GLOBAL_STR_COUNT 10

/**
 * Fixed stride (in bytes, including NUL) per string slot (`0xC9`).
 */
#define STR_VAR_STRIDE 201

/**
 * Starting index of persistent global variables in `vars`.
 */
#define GLOBAL_VARS_START 7000

/**
 * Number of persistent global variables `vars[7000..10000]`.
 */
#define GLOBAL_VARS_COUNT 3000

/**
 * Number of local variables `vars[0..7000]`.
 */
#define LOCAL_VARS_COUNT 7000

/**
 * Exact byte length of one engine scene snapshot (`0x471b8c..0x477750`, `0x16f1` dwords).
 */
#define SCENE_SNAPSHOT_SIZE 23492

/**
 * Number of sprite/CG layers in the main scene (`0..=99`).
 */
#define LAYER_COUNT 100

/**
 * Byte stride per layer snapshot inside [`SceneSnapshot`] (`0x471c00..0x476570`).
 */
#define LAYER_SNAPSHOT_STRIDE 188

/**
 * Exact byte length of a `KhDLsave%02d.dat` slot header (`8*u16 + 26B`).
 */
#define SLOT_HEADER_SIZE 42

/**
 * Exact byte length of a 2007 DL slot save file `KhDLsave%02d.dat`.
 */
#define SLOT_SAVE_SIZE 75026

/**
 * Minimum byte length of a 2007 DL global save `KhDLsave.dat` (empty bitmaps).
 */
#define DL_2007_GLOBAL_MIN_SIZE ((SAVE_CONFIG_SIZE + (GLOBAL_VARS_COUNT * 2)) + 8)

/**
 * Exact byte length of a 2002 CD global save `KhDLsave.dat`.
 */
#define CD_2002_SAVE_SIZE 206024

/**
 * Number of script rows in the 2002 CD kidoku matrix.
 */
#define CD_2002_KIDOKU_ROWS 10000

/**
 * Bytes per row in the 2002 CD kidoku matrix.
 */
#define CD_2002_KIDOKU_ROW_BYTES 20

/**
 * Textbox line spacing added after each line/column (`0x442336`, `0x4425e5`: `+5 px`).
 */
#define LINE_SPACING_PX 5

/**
 * Core engine text timer period in milliseconds (`0x447a90`: `timeSetEvent(..., 50, ...)`).
 */
#define TEXT_TIMER_PERIOD_MS 50

#define CgGalleryState_TAB_COUNT 7

#define CgGalleryState_PAGES_PER_TAB 5

#define CgGalleryState_SLOTS_PER_PAGE 6

#define SceneReplayState_TAB_COUNT 7

#define SceneReplayState_PAGES_PER_TAB 4

#define SceneReplayState_SLOTS_PER_PAGE 6

/**
 * Default top-left origin of the `39x106` `compane.lwg` control strip on `800x600`.
 */
#define COMPANE_ORIGIN_X 761

#define COMPANE_ORIGIN_Y 494

/**
 * Win32 message ID posted to parent window when MPEG playback stops (`WM_USER + 0x1E`).
 */
#define WM_MPEG_NOTIFY_DONE 1054

/**
 * Maximum frames returned per MP2 audio packet in `pl_mpeg` (`PLM_AUDIO_SAMPLES_PER_FRAME`).
 */
#define PLM_AUDIO_SAMPLES_PER_FRAME 1152

/**
 * Maximum `call_script` / `gosub_local` nesting depth (`0..=8`, overflow at `> 8`).
 */
#define MAX_CALL_DEPTH 8

/**
 * Total script variable count (`0..6999` local + `7000..9999` global).
 */
#define TOTAL_VARS (LOCAL_VARS_COUNT + GLOBAL_VARS_COUNT)

/**
 * Number of temporary expression registers `regs` (`DAT_00483270`).
 */
#define NUM_REGS 256

/**
 * Maximum backlog history entries (`0x414e40`: `200`).
 */
#define BACKLOG_CAPACITY 200

/**
 * Active game language / patch mode.
 */
typedef enum FujihimeLanguageMode {
    FUJIHIME_LANGUAGE_MODE_JAPANESE = 0,
    FUJIHIME_LANGUAGE_MODE_CHINESE_V1 = 1,
} FujihimeLanguageMode;

/**
 * Font slot role for [`fujihime_engine_load_font`].
 */
typedef enum FujihimeFontRole {
    FUJIHIME_FONT_ROLE_GOTHIC = 0,
    FUJIHIME_FONT_ROLE_MINCHO = 1,
    FUJIHIME_FONT_ROLE_FALLBACK = 2,
} FujihimeFontRole;

/**
 * Pointer / touch event kind (`800x600` logical coordinate space).
 */
typedef enum FujihimePointerKind {
    FUJIHIME_POINTER_KIND_MOVE = 0,
    FUJIHIME_POINTER_KIND_DOWN = 1,
    FUJIHIME_POINTER_KIND_UP = 2,
    FUJIHIME_POINTER_KIND_RIGHT_UP = 3,
    FUJIHIME_POINTER_KIND_MIDDLE_UP = 4,
    FUJIHIME_POINTER_KIND_WHEEL_UP = 5,
    FUJIHIME_POINTER_KIND_WHEEL_DOWN = 6,
} FujihimePointerKind;

/**
 * Modality for native confirmation alerts (`UIAlertController`).
 */
typedef enum FujihimeDialogKind {
    FUJIHIME_DIALOG_KIND_OK_ALERT = 0,
    FUJIHIME_DIALOG_KIND_YES_NO_CONFIRM = 1,
} FujihimeDialogKind;

/**
 * Top-level UI scene identifier (`0x41d010`).
 */
typedef enum FujihimeSceneKind {
    FUJIHIME_SCENE_KIND_TITLE = 1,
    FUJIHIME_SCENE_KIND_MAIN_GAME = 2,
    FUJIHIME_SCENE_KIND_LOAD_MENU = 3,
    FUJIHIME_SCENE_KIND_CONFIG_MENU = 4,
    FUJIHIME_SCENE_KIND_CG_GALLERY = 5,
    FUJIHIME_SCENE_KIND_SCENE_REPLAY = 6,
    FUJIHIME_SCENE_KIND_MUSIC_MODE = 7,
    FUJIHIME_SCENE_KIND_EXTRA_MENU = 8,
    FUJIHIME_SCENE_KIND_SUBSCENE_DLL = 9,
} FujihimeSceneKind;

/**
 * High-level iOS touch/gesture & system panel actions (`compane` + `WndProc` parity).
 */
typedef enum FujihimeSystemAction {
    FUJIHIME_SYSTEM_ACTION_SAVE = 0,
    FUJIHIME_SYSTEM_ACTION_LOAD = 1,
    FUJIHIME_SYSTEM_ACTION_CONFIG = 2,
    FUJIHIME_SYSTEM_ACTION_BACKLOG = 3,
    FUJIHIME_SYSTEM_ACTION_SKIP = 4,
    FUJIHIME_SYSTEM_ACTION_AUTO = 5,
    FUJIHIME_SYSTEM_ACTION_HIDE_TEXTBOX = 6,
    FUJIHIME_SYSTEM_ACTION_ROLLBACK = 7,
    FUJIHIME_SYSTEM_ACTION_TITLE = 8,
    FUJIHIME_SYSTEM_ACTION_EXIT = 9,
    FUJIHIME_SYSTEM_ACTION_ADVANCE = 10,
    FUJIHIME_SYSTEM_ACTION_REPLAY_VOICE = 11,
} FujihimeSystemAction;

/**
 * All 7 `0x445e90` confirmation dialog callsites in `KhimeDL.exe` / `KhimeDL_CHS.exe`.
 */
typedef struct ConfirmCallsite ConfirmCallsite;

/**
 * Opaque engine handle owned by the C/Swift host.
 */
typedef struct FujihimeEngine FujihimeEngine;

/**
 * All 10 direct `MessageBoxA` callsites in `KhimeDL.exe` / `KhimeDL_CHS.exe`.
 */
typedef struct MessageBoxCallsite MessageBoxCallsite;

/**
 * Engine creation configuration passed by C/Swift caller.
 */
typedef struct FujihimeEngineConfig {
    const char *game_root;
    const char *save_dir;
    const char *override_dir;
    const char *orig_backup_dir;
    enum FujihimeLanguageMode language_mode;
    bool repair_chs_recall_labels;
} FujihimeEngineConfig;

/**
 * Pending native dialog description copied safely into caller-owned C buffers.
 */
typedef struct FujihimePendingDialog {
    uint64_t dialog_id;
    enum FujihimeDialogKind kind;
    uint8_t title_utf8[FUJIHIME_DIALOG_TITLE_CAP];
    uint8_t message_utf8[FUJIHIME_DIALOG_MSG_CAP];
} FujihimePendingDialog;

/**
 * Engine runtime state snapshot returned by [`fujihime_engine_get_state`].
 */
typedef struct FujihimeEngineState {
    enum FujihimeSceneKind scene;
    enum FujihimeLanguageMode language_mode;
    bool is_playing_video;
    bool is_waiting_click;
    bool is_waiting_select;
    bool is_transitioning;
    bool exit_requested;
    uint16_t current_script_id;
    uint32_t current_pc;
} FujihimeEngineState;





/**
 * CG entry counts across the 7 tabs (`sum = 102`).
 */
#define CG_TAB_COUNTS { 12, 10, 17, 11, 12, 25, 15, }

/**
 * Scene replay entry counts across the 7 tabs (`sum = 79`).
 */
#define REPLAY_TAB_COUNTS { 12, 12, 15, 11, 11, 18, 0, }

/**
 * The 9 scripts whose `"8011".."8031"` recall labels are zeroed in CHS V1.0 `scr.xfl`.
 */
#define RECALL_ZEROED_LABEL_SCRIPTS { 1125, 2058, 2059, 2245, 3177, 3179, 3262, 4203, 5110, }



#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Create and initialize a new [`FujihimeEngine`] instance. Returns `NULL` on error.
 *
 * # Safety
 * `config` and its non-null C string fields must be valid NUL-terminated pointers.
 */
struct FujihimeEngine *fujihime_engine_create(const struct FujihimeEngineConfig *config);

/**
 * Destroy an engine instance created by [`fujihime_engine_create`]. Passing `NULL` is a no-op.
 *
 * # Safety
 * `engine` must either be `NULL` or a live pointer returned by [`fujihime_engine_create`].
 */
void fujihime_engine_destroy(struct FujihimeEngine *engine);

/**
 * Inject an in-memory TTF/OTF/TTC font buffer for the specified role (`AppBundle/Fonts/`).
 *
 * # Safety
 * `engine` must be a valid handle and `bytes` must point to `len` readable bytes.
 */
int32_t fujihime_engine_load_font(struct FujihimeEngine *engine,
                                  enum FujihimeFontRole role,
                                  const uint8_t *bytes,
                                  uintptr_t len,
                                  int32_t face_index);

/**
 * Start the original Title OP sequence (`0002.mpg` -> `0001.mpg` -> Title screen).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_start_title(struct FujihimeEngine *engine);

/**
 * Enter main ADV gameplay at `script_id` (e.g. `0` for New Game).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_start_game(struct FujihimeEngine *engine, uint16_t script_id);

/**
 * Advance the engine simulation by `delta_ms` milliseconds.
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_tick(struct FujihimeEngine *engine, uint32_t delta_ms);

/**
 * Dispatch a pointer/touch event in logical `800x600` coordinates.
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_pointer_event(struct FujihimeEngine *engine,
                                      enum FujihimePointerKind kind,
                                      int32_t x,
                                      int32_t y);

/**
 * Dispatch a Win32 `VK_*` key event (`0x0D` Return, `0x20` Space, `0x11` Ctrl, `0x10` Shift, `0x70` F1).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_key_event(struct FujihimeEngine *engine,
                                  uint16_t vk_code,
                                  bool is_down);

/**
 * Poll for a pending native dialog to display via `UIAlertController`. Returns `true` if populated.
 *
 * # Safety
 * `engine` and `out_dialog` must be non-null valid pointers.
 */
bool fujihime_engine_poll_pending_dialog(struct FujihimeEngine *engine,
                                         struct FujihimePendingDialog *out_dialog);

/**
 * Deliver user acceptance (`accepted = true` for `IDYES` / `OK`) for a pending dialog.
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_confirm_dialog_response(struct FujihimeEngine *engine,
                                                uint64_t dialog_id,
                                                bool accepted);

/**
 * Copy the current `800x600` front framebuffer in standard BGRA8 (`A = 0xFF` opaque) for Metal upload.
 *
 * `out_len` must be at least `800 * 600 * 4 = 1,920,000` bytes.
 *
 * # Safety
 * `engine` must be valid and `out_ptr` must point to `out_len` writable bytes.
 */
int32_t fujihime_engine_copy_present_bgra8(const struct FujihimeEngine *engine,
                                           uint8_t *out_ptr,
                                           uintptr_t out_len);

/**
 * Pull `frames` interleaved stereo `f32` PCM samples (`44,100 Hz`) for `AVAudioEngine`.
 *
 * Returns the number of stereo frames written.
 *
 * # Safety
 * `engine` must be valid and `out_interleaved` must point to `frames * 2` writable `f32`s.
 */
uintptr_t fujihime_engine_render_audio_f32(const struct FujihimeEngine *engine,
                                           float *out_interleaved,
                                           uintptr_t frames);

/**
 * Query current engine status snapshot.
 *
 * # Safety
 * `engine` and `out_state` must be non-null valid pointers.
 */
int32_t fujihime_engine_get_state(const struct FujihimeEngine *engine,
                                  struct FujihimeEngineState *out_state);

/**
 * Flush global save `KhDLsave.dat` to `save_dir`.
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_save_global(struct FujihimeEngine *engine);

/**
 * Save slot `KhDLsave%02d.dat` (`75,026 B`) to `save_dir`.
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_save_slot(struct FujihimeEngine *engine,
                                  uint16_t slot_num,
                                  uint16_t year,
                                  uint16_t month,
                                  uint16_t day,
                                  uint16_t hour,
                                  uint16_t minute);

/**
 * Load slot `KhDLsave%02d.dat` (`75,026 B`) from `save_dir`.
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_load_slot(struct FujihimeEngine *engine, uint16_t slot_num);

/**
 * Trigger one-step rollback restore (`0x424060`).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_rollback(struct FujihimeEngine *engine);

/**
 * Switch active language mode (`Japanese` vs `ChineseV1`) at runtime.
 *
 * # Safety
 * `engine` must be a valid handle and `orig_backup_dir` may be `NULL` or a valid NUL-terminated UTF-8 path.
 */
int32_t fujihime_engine_set_language_mode(struct FujihimeEngine *engine,
                                          enum FujihimeLanguageMode mode,
                                          const char *orig_backup_dir,
                                          bool repair_chs_recall_labels);

/**
 * Import a 206,024-byte 2002 CD `KhDLsave.dat` (e.g. Full CG save), migrate to 2007 DL,
 * apply to live state, and persist `KhDLsave.dat`.
 *
 * # Safety
 * `engine` must be valid and `bytes` must point to `len` readable bytes.
 */
int32_t fujihime_engine_import_2002_cd_save(struct FujihimeEngine *engine,
                                            const uint8_t *bytes,
                                            uintptr_t len);

/**
 * Execute an iOS gesture / `compane` system action (`Save`, `Load`, `Config`, `Backlog`, etc.).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_perform_action(struct FujihimeEngine *engine,
                                       enum FujihimeSystemAction action);

/**
 * Navigate directly to one of the 9 top-level UI scenes (`0x41d010`).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_enter_scene(struct FujihimeEngine *engine, enum FujihimeSceneKind scene);

/**
 * Convenience helper: toggle textbox visibility (`Shift` / `compane hide` / two-finger tap).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_toggle_textbox(struct FujihimeEngine *engine);

/**
 * Convenience helper: toggle Skip mode (`Tab` / `compane fow` / long-press).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_toggle_skip(struct FujihimeEngine *engine);

/**
 * Convenience helper: toggle Auto-play mode (`compane auto`).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_toggle_auto(struct FujihimeEngine *engine);

/**
 * Convenience helper: open Backlog (`Esc` / `WheelUp` / swipe-up).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_open_backlog(struct FujihimeEngine *engine);

/**
 * Convenience helper: open Config (`confscrn.lwg`).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_open_config(struct FujihimeEngine *engine);

/**
 * Convenience helper: open Save screen (`savescrn.lwg`).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_open_save_menu(struct FujihimeEngine *engine);

/**
 * Convenience helper: open Load screen (`savescrn.lwg`).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_open_load_menu(struct FujihimeEngine *engine);

/**
 * Convenience helper: trigger Exit confirmation dialog (`0x44753a`).
 *
 * # Safety
 * `engine` must be a valid handle.
 */
int32_t fujihime_engine_request_exit(struct FujihimeEngine *engine);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* FUJIHIME_CORE_H */
