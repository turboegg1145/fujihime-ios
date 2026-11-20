//! Complete RScript `.gsc` bytecode virtual machine (`KhimeDL.exe` `0x41f420..0x425100`).

use std::collections::VecDeque;

use encoding_rs::{GBK, SHIFT_JIS};

use crate::error::{FujihimeError, Result};
use crate::save::{
    BitmapMap, GlobalSave2007, SceneSnapshot, SlotSave2007, SlotSaveHeader2007,
    dbcs_truncate_title_26b, GLOBAL_STR_COUNT, GLOBAL_VARS_COUNT, GLOBAL_VARS_START, LAYER_COUNT,
    LOCAL_VARS_COUNT, STR_VAR_STRIDE,
};
use crate::text::caret::{resolve_caret_color, TextEncoding};
use crate::text::rasterizer::{is_cp932_lead, is_cp932_trail, is_gbk_lead, is_gbk_trail};
use crate::vm::gsc::GscScript;
use crate::vm::opcodes::{
    decode_instruction, CommandOp, ExprOperand, HiOp, HiOpKind, InstructionKind, OperandMode,
};

/// Maximum `call_script` / `gosub_local` nesting depth (`0..=8`, overflow at `> 8`).
pub const MAX_CALL_DEPTH: usize = 8;
/// Total script variable count (`0..6999` local + `7000..9999` global).
pub const TOTAL_VARS: usize = LOCAL_VARS_COUNT + GLOBAL_VARS_COUNT;
/// Number of temporary expression registers `regs` (`DAT_00483270`).
pub const NUM_REGS: usize = 256;
/// Maximum backlog history entries (`0x414e40`: `200`).
pub const BACKLOG_CAPACITY: usize = 200;

/// Runtime layer state (`0..=99`, corresponding to `0x471088` + `0x471c00`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmLayerState {
    pub x: i32,
    pub y: i32,
    pub layer_type: u16,
    pub source_kind: u16, // 0 = numbered img, 1 = named, 2 = text, 3 = solid fill
    pub alpha_or_mode: u16,
    pub img_id: u16,
    pub fill_color: u32,
    pub z_mode: i16,
    pub blend_mode: u16,
    pub blend_param: u16,
    pub clip_rect: [i16; 4],
    pub visible: bool,
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub anim_active: bool,
    pub anim_kind: u8, // 0 = X (0x41b7a0), 1 = Y (0x41b800), 2 = Alpha (0x41b8f0)
    pub anim_x: [i16; 3],
    pub anim_y: [i16; 3],
    pub anim_alpha: [i16; 3],
    pub subframe_img_ids: [u16; 2],
    pub subframe_coords: [(i32, i32); 2],
    pub text_or_path: Vec<u8>,
    pub dir_prefix: String,
    pub z_order: i16,
    pub group_id: u16,
    pub extra_50: u16,
}

impl Default for VmLayerState {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            layer_type: 0,
            source_kind: 0,
            alpha_or_mode: 0,
            img_id: 0,
            fill_color: 0,
            z_mode: 0,
            blend_mode: 0,
            blend_param: 0,
            clip_rect: [0, 0, 800, 600],
            visible: true,
            anchor_x: 1,
            anchor_y: 1,
            anim_active: false,
            anim_kind: 0,
            anim_x: [0; 3],
            anim_y: [0; 3],
            anim_alpha: [0; 3],
            subframe_img_ids: [0; 2],
            subframe_coords: [(0, 0); 2],
            text_or_path: Vec::new(),
            dir_prefix: "grpo".to_string(),
            z_order: 0,
            group_id: 1,
            extra_50: 0x1e,
        }
    }
}

/// Gauge UI control state (`0x471040[0..10]`, opcodes `0x82..0x88`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmGaugeState {
    pub num_id: i16,
    pub bar_id: i16,
    pub fix_id: i16,
    pub min: i16,
    pub max: i16,
    pub p4: i16,
    pub p5: i16,
    pub visible: bool,
    pub x: i16,
    pub y: i16,
    pub num_x: i16,
    pub num_y: i16,
    pub bar_x: i16,
    pub bar_y: i16,
    pub value: i16,
    pub anim: i16,
}

/// Eyes overlay state (`DAT_0047106c`, opcodes `0x2B..0x2C`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EyesOverlayState {
    pub es_id: u16,
    pub blend_flag: u16,
    pub z_order: i16,
}

/// Fullscreen filter overlay state (`DAT_0047121c`, opcodes `0x2D..0x2E`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterOverlayState {
    pub alpha_pct: u16,
    pub mode_flag: u16,
    pub z_order: i16,
}

/// Single backlog history entry (`0x414e40`, capacity 200).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogEntry {
    pub raw_bytes: Vec<u8>,
    pub voice_id: u16,
    pub script_id: u16,
    pub text_str_idx: u32,
}

/// Textbox & backlog state (`0x471228`, opcodes `0x2A, 0x30, 0x38, 0x39, 0x50..0x69, 0xD2..0xD4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmTextboxState {
    pub visible: bool,
    /// Master enable flag set by `0x5E` (`0x415750` -> `*(tbox+0xac)`).
    pub master_enable: bool,
    pub active_slot: u16,
    /// Textbox window origin `(x, y)` on `800x600` (`0x5A` -> `0x4165a0`).
    pub pos: (i16, i16),
    pub rect: (i16, i16, i16, i16),
    /// Textbox skin index (`0x5C` -> `tbox%02d.lwg`, `0x4165e0`).
    pub skin_id: u16,
    /// Wait cursor skin index (`0x67` -> `wait%02d.lwg`, `0x416600`).
    pub wait_id: u16,
    /// Whether `0x5B tbox_rect` explicitly overrode `rect` after the last `0x5C` skin load.
    pub rect_custom: bool,
    /// Whether `0x64` explicitly overrode `icon_pos` after the last `0x5C` skin load.
    pub icon_pos_custom: bool,
    /// Font pixel height (`0x60` -> `0x440db0`, default `24`).
    pub font_size: u16,
    pub wrap_cfg: i16,
    pub layout_mode: u16, // 0=horz, nonzero=vert
    pub color_rgb: u32,
    pub custom_palette: [u32; 10],
    pub char_spacing: i16,
    pub line_spacing: i16,
    pub shadow_mode: u16,
    /// Paragraph alignment (`0x62` -> `0x440dd0`: 0=Left, 1=Center, 2=Right).
    pub text_align: u16,
    /// First-line & continuation-line horizontal indent (`0x63` -> `0x440e20`).
    pub indent_first: i16,
    pub indent_cont: i16,
    pub speed: u16,
    pub speed_p3: i16,
    pub face_id: i16,
    pub face_mode: i16,
    pub face_pos: (i16, i16),
    pub face_z: i16,
    pub icon_pos: (i16, i16),
    /// Optional `compane.lwg` origin override (`0x5D` -> `0x415700`).
    pub compane_pos: Option<(i16, i16)>,
    pub font_family_id: u16, // 0=Gothic, 1=Mincho (`0x61` -> `0x440dc0`)
    pub bg_color_rgb: u32,
    pub wait_glow_rgb: u32,
    pub bg_alpha: u8,
    pub style_p1: i16,
    pub style_p2: i32,
    pub style_p3: i16,
    pub style_p4: i16,
    pub wait_mode: i32,
    pub current_text_raw: Vec<u8>,
    pub current_voice_id: u16,
    pub backlog: VecDeque<BacklogEntry>,
    pub backlog_title_raw: Vec<u8>,
    pub backlog_extra: u32,
}

impl Default for VmTextboxState {
    fn default() -> Self {
        Self {
            visible: true,
            master_enable: true,
            active_slot: 0,
            pos: (0, 456),
            rect: (68, 26, 630, 108),
            skin_id: 0,
            wait_id: 0,
            rect_custom: false,
            icon_pos_custom: false,
            font_size: 24,
            wrap_cfg: 0,
            layout_mode: 0,
            color_rgb: 0x00FF_FFFF,
            custom_palette: [0x00FF_FFFF; 10],
            char_spacing: 0,
            line_spacing: 5,
            shadow_mode: 0,
            text_align: 0,
            indent_first: 0,
            indent_cont: 0,
            speed: 1,
            speed_p3: 0,
            face_id: 0,
            face_mode: 0,
            face_pos: (0, 0),
            face_z: 0,
            icon_pos: (728, 113),
            compane_pos: None,
            font_family_id: 0,
            bg_color_rgb: 0x0000_0000,
            wait_glow_rgb: 0x0064_645A,
            bg_alpha: 0x80,
            style_p1: 1,
            style_p2: 1,
            style_p3: 1,
            style_p4: 1,
            wait_mode: 0,
            current_text_raw: Vec::new(),
            current_voice_id: 0,
            backlog: VecDeque::with_capacity(BACKLOG_CAPACITY),
            backlog_title_raw: Vec::new(),
            backlog_extra: 0,
        }
    }
}

/// Active choice menu data produced by `0x0E select_6`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoicePrompt {
    pub prompt_raw: Vec<u8>,
    pub choices: Vec<(Vec<u8>, u32)>, // (expanded text bytes, target_pc)
    pub opts: [i16; 3],
}

/// Reason the VM yielded execution to the outer frame loop / host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmYield {
    /// Waiting for player click/key (`0x0A`, `0x0B`, `0x51`, `0x52`, `0x53`).
    WaitClick {
        slot: u16,
        clear_after: bool,
        is_dialogue: bool,
        voice_id: u16,
    },
    /// Timed wait (`0x0D`: `ticks * 100 ms`).
    WaitSleepMs(u32),
    /// Transition effect in progress (`0x14`, `0x15`, `0x1C`, `0x1D`, `0x0E`).
    WaitTransition {
        effect_id: u16,
        steps: u16,
        delay_ms: u16,
    },
    /// Branch selection menu (`0x0E`).
    WaitSelect(ChoicePrompt),
    /// Map/special selector (`0xD5`).
    WaitMapSelect { mode: i16, param2: i16, unused: i16 },
    /// MPEG-1 video playback (`0x41`).
    WaitVideo { mpg_id: u16 },
    /// Wait for SE0 (`0x44`) or Voice (`0x45`) completion.
    WaitAudio { is_voice: bool },
    /// Screen shake (`0x17`) or flash (`0x18`).
    WaitEffect,
    /// Interactive layer hotspot wait loop (`0x49 timer_wait_loop`).
    WaitTimerLoop { var_idx: i16, mode: i16, p3: i16 },
    /// Save/pause menu requested (`0x11`).
    MenuPause,
    /// VM halted (`0x08`, `0x19`, `0x3A`, EOF, or error).
    Halted { reason: HaltReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltReason {
    ExitOpcode,
    EndOfScript,
    AbortFlag,
    SubsceneDll,
}

/// Host callbacks invoked by [`VmRuntime`] so the VM remains decoupled from concrete media backends.
pub trait VmHost {
    /// Load parsed `.gsc` script by numeric ID (`%04d.gsc`).
    fn load_script(&mut self, script_id: u16) -> Result<GscScript>;

    /// Refresh/recompose layers to `new_scene` (`0x421860` / `0x425100`).
    fn refresh_scene(&mut self, state: &VmState) -> Result<()>;

    /// Start a screen transition (`0x422060`). If `effect_id == 0` or effects disabled, commits immediately.
    fn start_transition(&mut self, state: &VmState, effect_id: u16, steps: u16, delay_ms: u16) -> Result<()>;

    /// Trigger screen shake (`0x17`).
    fn trigger_shake(&mut self, _count: i16, _dir: i16, _u3: i16, _u4: i16) {}

    /// Trigger screen flash (`0x18`).
    fn trigger_flash(&mut self, _count: i16, _mode: i16) {}

    /// Display text in textbox (`0x51` / `0x52` / `0x50`).
    fn display_text(&mut self, state: &VmState, slot: u16, raw_expanded: &[u8]) -> Result<()>;

    /// Play BGM (`0x3C`).
    fn play_bgm(&mut self, bgm_id: u16, loop_flag: bool, fade_ms: u16);

    /// Stop BGM (`0x3D`).
    fn stop_bgm(&mut self, fade_ms: u16);

    /// Preload SE0 (`0x3E`).
    fn load_se(&mut self, se_id: u16);

    /// Play preloaded SE0 (`0x3F`).
    fn play_se(&mut self, loop_cnt: i16, fade_flag: u16, fade_50ms: i16);

    /// Stop SE0 (`0x40`).
    fn stop_se(&mut self, fade_ms: u16);

    /// Play Voice (`0x42` / `0x51`).
    fn play_voice(&mut self, voice_id: u16, loop_cnt: i16, fade_flag: u16, delay_50ms: i16);

    /// Stop Voice (`0x43`).
    fn stop_voice(&mut self, fade_ms: u16);

    /// Start MPEG-1 video (`0x41`).
    fn start_video(&mut self, mpg_id: u16);

    /// Query whether SE0 or Voice is currently playing (`0x44` / `0x45`).
    fn is_audio_playing(&self, is_voice: bool) -> bool;

    /// Query whether `voice_cut_on_click` (`0x471396`) is enabled.
    fn voice_cut_on_click(&self) -> bool;

    /// Query whether `skip_unread` (`0x47138c`) is enabled.
    fn skip_unread(&self) -> bool;

    /// Notify host of global flag / CG unlock change (`0x14`, `0x72`) or kidoku update (`0x51`).
    fn on_persistent_state_changed(&mut self, _state: &VmState) {}
}

/// Complete mutable state of the `.gsc` virtual machine.
#[derive(Debug, Clone)]
pub struct VmState {
    pub encoding: TextEncoding,
    pub regs: [i16; NUM_REGS],
    pub vars: [i16; TOTAL_VARS],
    pub str_vars: [[u8; STR_VAR_STRIDE]; GLOBAL_STR_COUNT],
    pub global_str_vars: [[u8; STR_VAR_STRIDE]; GLOBAL_STR_COUNT],
    pub script_ids: [u16; 11],
    pub return_pcs: [u32; 10],
    pub call_depth: u16,
    pub pc: u32,
    pub layers: Vec<VmLayerState>, // always length 100
    pub gauges: Vec<VmGaugeState>, // always length 10
    pub eyes_overlay: EyesOverlayState,
    pub filter_overlay: FilterOverlayState,
    pub tbox: VmTextboxState,
    pub coord_scale: (i16, i16),
    pub batch_mode: bool,
    pub auto_snapshot_enable: bool, // DAT_00471bf0
    pub rollback_enable: bool,      // DAT_00471bf4
    pub auto_clear_anim: bool,      // DAT_00471bf8
    pub recall_flag: bool,          // DAT_0047135c
    pub save_disable_flag: bool,    // DAT_0047135e
    pub fast_skip_active: bool,     // Ctrl / scene skip
    pub active_bgm_id: u16,
    pub active_se_id: u16,
    pub active_se_loop: bool,
    pub active_se_param: i16,
    pub richedit_cfg: [i32; 16],
    pub kidoku: BitmapMap,
    pub global_flags: BitmapMap,
    pub rng_state: u32,
    // Dual snapshots (`0x477750` primary + `0x47d314` rollback)
    pub primary_snapshot: SceneSnapshot,
    pub primary_vars: [i16; LOCAL_VARS_COUNT],
    pub rollback_snapshot: SceneSnapshot,
    pub rollback_vars: [i16; LOCAL_VARS_COUNT],
}

impl VmState {
    /// Initialize clean state matching `FUN_0041daa0`.
    pub fn new(encoding: TextEncoding) -> Self {
        let mut layers = vec![VmLayerState::default(); LAYER_COUNT];
        for (i, layer) in layers.iter_mut().enumerate() {
            layer.z_order = i as i16;
            layer.group_id = 1;
            layer.dir_prefix = "grpo".to_string();
        }
        layers[0].img_id = 999;
        layers[0].anchor_x = 0;
        layers[0].anchor_y = 0;
        layers[0].blend_mode = 99;
        layers[0].group_id = 0;
        layers[0].dir_prefix = "grpe".to_string();

        let snap = SceneSnapshot::default();
        Self {
            encoding,
            regs: [0; NUM_REGS],
            vars: [0; TOTAL_VARS],
            str_vars: [[0; STR_VAR_STRIDE]; GLOBAL_STR_COUNT],
            global_str_vars: [[0; STR_VAR_STRIDE]; GLOBAL_STR_COUNT],
            script_ids: [0; 11],
            return_pcs: [0; 10],
            call_depth: 0,
            pc: 0,
            layers,
            gauges: vec![VmGaugeState::default(); 10],
            eyes_overlay: EyesOverlayState::default(),
            filter_overlay: FilterOverlayState::default(),
            tbox: VmTextboxState::default(),
            coord_scale: (1, 1),
            batch_mode: false,
            auto_snapshot_enable: true,
            rollback_enable: true,
            auto_clear_anim: true,
            recall_flag: false,
            save_disable_flag: false,
            fast_skip_active: false,
            active_bgm_id: 0,
            active_se_id: 0,
            active_se_loop: false,
            active_se_param: 0,
            richedit_cfg: [4; 16],
            kidoku: BitmapMap::new(),
            global_flags: BitmapMap::new(),
            rng_state: 0x1234_5678,
            primary_snapshot: snap.clone(),
            primary_vars: [0; LOCAL_VARS_COUNT],
            rollback_snapshot: snap,
            rollback_vars: [0; LOCAL_VARS_COUNT],
        }
    }

    /// Current running script ID (`script_ids[call_depth]`).
    #[inline]
    pub fn current_script_id(&self) -> u16 {
        self.script_ids[self.call_depth as usize]
    }

    /// Sync global variables, global strings, kidoku, and global flags from [`GlobalSave2007`].
    pub fn load_from_global_save(&mut self, gs: &GlobalSave2007) {
        self.vars[GLOBAL_VARS_START..TOTAL_VARS].copy_from_slice(&gs.vars_7000_9999);
        for (slot, dst) in self.global_str_vars.iter_mut().enumerate() {
            dst.copy_from_slice(gs.config.global_str_slot(slot));
        }
        self.kidoku = gs.kidoku.clone();
        self.global_flags = gs.global_flags.clone();
        self.tbox.speed = gs.config.text_speed();
    }

    /// Write global variables, global strings, kidoku, and global flags back into [`GlobalSave2007`].
    pub fn save_to_global_save(&self, gs: &mut GlobalSave2007) {
        gs.vars_7000_9999
            .copy_from_slice(&self.vars[GLOBAL_VARS_START..TOTAL_VARS]);
        for (slot, src) in self.global_str_vars.iter().enumerate() {
            gs.config.global_str_slot_mut(slot).copy_from_slice(src);
        }
        gs.kidoku = self.kidoku.clone();
        gs.global_flags = self.global_flags.clone();
    }

    /// Read variable safely (`vars[idx]`, returning `0` if out of range).
    #[inline]
    pub fn read_var(&self, idx: i16) -> i16 {
        if let Ok(u) = usize::try_from(idx) {
            self.vars.get(u).copied().unwrap_or(0)
        } else {
            0
        }
    }

    /// Write variable safely (`vars[idx] = val`).
    #[inline]
    pub fn write_var(&mut self, idx: i16, val: i16) {
        if let Ok(u) = usize::try_from(idx) {
            if let Some(slot) = self.vars.get_mut(u) {
                *slot = val;
            }
        }
    }

    /// `FUN_0041f420` / `FUN_00421350`: dereference variable index `depth` times.
    #[inline]
    pub fn deref_index(&self, depth: i16, mut val: i16) -> i16 {
        for _ in 0..depth.max(0) {
            val = self.read_var(val);
        }
        val
    }

    /// Evaluate 4-byte `ExprOperand` (`0x421350`).
    #[inline]
    pub fn eval_expr(&self, op: ExprOperand) -> i16 {
        self.deref_index(op.depth, op.raw_val)
    }

    /// Deterministic PRNG matching MSVC `rand()` (for `0x09 rand_reg`).
    pub fn next_rand_i16(&mut self) -> i16 {
        self.rng_state = self.rng_state.wrapping_mul(214_013).wrapping_add(2_531_011);
        ((self.rng_state >> 16) & 0x7FFF) as i16
    }

    /// Export current live state into a 23,492-byte [`SceneSnapshot`] (`0x41de30`).
    pub fn encode_scene_snapshot(&self) -> SceneSnapshot {
        let mut s = SceneSnapshot::default();
        s.set_bgm_id(self.active_bgm_id);
        s.set_se_id(self.active_se_id);
        s.set_se_loop(self.active_se_loop);
        s.set_se_param(self.active_se_param);
        for (i, &id) in self.script_ids.iter().enumerate() {
            s.set_script_id(i, id);
        }
        for (i, &pc) in self.return_pcs.iter().enumerate() {
            s.set_return_pc(i, pc);
        }
        // Store current PC in return_pcs[call_depth] so slot saves/restores resume at exact PC.
        s.set_return_pc(self.call_depth as usize, self.pc);
        s.set_call_depth(self.call_depth);
        s.set_eyes_es_id(self.eyes_overlay.es_id);
        s.set_eyes_blend_flag(self.eyes_overlay.blend_flag);
        s.set_eyes_z(self.eyes_overlay.z_order);
        s.set_filter_alpha_pct(self.filter_overlay.alpha_pct);
        s.set_filter_mode_flag(self.filter_overlay.mode_flag);
        s.set_filter_z(self.filter_overlay.z_order);
        s.set_tbox_style_p2(self.tbox.style_p2);
        s.set_auto_snapshot_enable(self.auto_snapshot_enable);
        s.set_rollback_enable(self.rollback_enable);
        s.set_auto_clear_anim(self.auto_clear_anim);
        s.set_text_voice_wait_mode(self.tbox.wait_mode);

        for (i, l) in self.layers.iter().enumerate() {
            s.set_layer_xy(i, l.x, l.y);
            s.set_layer_type(i, l.layer_type);
            s.set_layer_source_kind(i, l.source_kind);
            s.set_layer_alpha_or_mode(i, l.alpha_or_mode);
            s.set_layer_img_id(i, l.img_id);
            s.set_layer_fill_color(i, l.fill_color);
            s.set_layer_z_mode(i, l.z_mode);
            s.set_layer_blend_mode(i, l.blend_mode);
            s.set_layer_blend_param(i, l.blend_param);
            s.set_layer_clip_rect(
                i,
                l.clip_rect[0],
                l.clip_rect[1],
                l.clip_rect[2],
                l.clip_rect[3],
            );
            s.set_layer_visible(i, l.visible);
            s.set_layer_anchors(i, l.anchor_x, l.anchor_y);
            s.set_layer_anim_active(i, l.anim_active);
            s.set_layer_extra_50(i, l.extra_50);
            s.set_layer_text_or_path(i, &l.text_or_path);
            let dir_fmt = format!(".\\{}", l.dir_prefix);
            s.set_layer_dir_prefix(i, dir_fmt.as_bytes());
            s.set_layer_z(i, l.z_order);
            s.set_layer_group(i, l.group_id);
        }
        s.set_coord_scale(self.coord_scale.0, self.coord_scale.1);
        for (i, slot) in self.str_vars.iter().enumerate() {
            s.set_str_var(i, slot);
        }
        s
    }

    /// Restore live state from a 23,492-byte [`SceneSnapshot`] + 7,000 local vars (`0x424060` / `0x41dff0`).
    pub fn apply_scene_snapshot(&mut self, s: &SceneSnapshot, local_vars: &[i16; LOCAL_VARS_COUNT]) {
        self.vars[..LOCAL_VARS_COUNT].copy_from_slice(local_vars);
        self.active_bgm_id = s.bgm_id();
        self.active_se_id = s.se_id();
        self.active_se_loop = s.se_loop();
        self.active_se_param = s.se_param();
        for (i, slot) in self.script_ids.iter_mut().enumerate() {
            *slot = s.script_id(i);
        }
        for (i, slot) in self.return_pcs.iter_mut().enumerate() {
            *slot = s.return_pc(i);
        }
        self.call_depth = s.call_depth().min(MAX_CALL_DEPTH as u16);
        self.pc = self.return_pcs[self.call_depth as usize];
        self.eyes_overlay = EyesOverlayState {
            es_id: s.eyes_es_id(),
            blend_flag: s.eyes_blend_flag(),
            z_order: s.eyes_z(),
        };
        self.filter_overlay = FilterOverlayState {
            alpha_pct: s.filter_alpha_pct(),
            mode_flag: s.filter_mode_flag(),
            z_order: s.filter_z(),
        };
        self.tbox.style_p2 = s.tbox_style_p2();
        self.auto_snapshot_enable = s.auto_snapshot_enable();
        self.rollback_enable = s.rollback_enable();
        self.auto_clear_anim = s.auto_clear_anim();
        self.tbox.wait_mode = s.text_voice_wait_mode();

        for (i, l) in self.layers.iter_mut().enumerate() {
            let lv = s.layer(i);
            let dir_raw = String::from_utf8_lossy(lv.dir_prefix_bytes());
            let dir_clean = dir_raw
                .trim_start_matches(".\\")
                .trim_start_matches("./")
                .trim_start_matches('\\')
                .to_string();
            l.x = lv.x();
            l.y = lv.y();
            l.layer_type = lv.layer_type();
            l.source_kind = lv.source_kind();
            l.alpha_or_mode = lv.alpha_or_mode();
            l.img_id = lv.img_id();
            l.fill_color = lv.fill_color();
            l.z_mode = lv.z_mode();
            l.blend_mode = lv.blend_mode();
            l.blend_param = lv.blend_param();
            l.clip_rect = lv.clip_rect();
            l.visible = lv.visible();
            l.anchor_x = lv.anchor_x();
            l.anchor_y = lv.anchor_y();
            l.anim_active = lv.anim_active();
            l.text_or_path = lv.text_or_path_bytes().to_vec();
            if !dir_clean.is_empty() {
                l.dir_prefix = dir_clean;
            }
            l.z_order = s.layer_z(i);
            l.group_id = s.layer_group(i);
        }
        self.coord_scale = s.coord_scale();
        for (i, slot) in self.str_vars.iter_mut().enumerate() {
            slot.copy_from_slice(s.str_var(i));
        }
    }

    /// `0x37 save_snapshot` (`0x41de30`): capture live state into `primary_snapshot`.
    pub fn save_snapshot(&mut self) {
        if !self.save_disable_flag {
            self.primary_snapshot = self.encode_scene_snapshot();
            self.primary_vars
                .copy_from_slice(&self.vars[..LOCAL_VARS_COUNT]);
        }
    }

    /// `0x32 rollback_save` (`0x41e160`): copy primary snapshot to rollback snapshot.
    pub fn rollback_save(&mut self) {
        if !self.save_disable_flag {
            self.rollback_snapshot = self.primary_snapshot.clone();
            self.rollback_vars.copy_from_slice(&self.primary_vars);
        }
    }

    /// Build a 75,026-byte [`SlotSave2007`] (`0x41de70`) from current snapshots.
    pub fn export_slot_save(
        &self,
        year: u16,
        month: u16,
        day: u16,
        hour: u16,
        minute: u16,
    ) -> SlotSave2007 {
        SlotSave2007 {
            header: SlotSaveHeader2007 {
                year,
                month,
                day,
                hour,
                minute,
                var1: self.vars[1] as u16,
                var2: self.vars[2] as u16,
                var3: self.vars[3] as u16,
                title_raw: dbcs_truncate_title_26b(&self.tbox.current_text_raw, self.encoding),
            },
            primary_snapshot: self.primary_snapshot.clone(),
            primary_vars: self.primary_vars,
            rollback_snapshot: self.rollback_snapshot.clone(),
            rollback_vars: self.rollback_vars,
        }
    }

    /// Load a 75,026-byte [`SlotSave2007`] (`0x41dff0`) into primary/live and rollback slots.
    pub fn import_slot_save(&mut self, slot: &SlotSave2007) {
        self.primary_snapshot = slot.primary_snapshot.clone();
        self.primary_vars = slot.primary_vars;
        self.rollback_snapshot = slot.rollback_snapshot.clone();
        self.rollback_vars = slot.rollback_vars;
        self.apply_scene_snapshot(&slot.primary_snapshot, &slot.primary_vars);
    }
}

/// Runtime `.gsc` interpreter driving `VmState` + active `GscScript`.
#[derive(Debug, Clone)]
pub struct VmRuntime {
    pub state: VmState,
    pub current_script: Option<GscScript>,
    pub pending_clear_slot: Option<u16>,
    pub pending_wait_click: bool,
}

impl VmRuntime {
    pub fn new(encoding: TextEncoding) -> Self {
        Self {
            state: VmState::new(encoding),
            current_script: None,
            pending_clear_slot: None,
            pending_wait_click: false,
        }
    }

    /// Start execution at `(script_id, pc = 0)` and initialize primary + rollback snapshots (`0x41d7b0`).
    pub fn start_script<H: VmHost>(&mut self, host: &mut H, script_id: u16, pc: u32) -> Result<()> {
        let script = host.load_script(script_id)?;
        self.state.call_depth = 0;
        self.state.script_ids[0] = script_id;
        self.state.pc = pc;
        self.current_script = Some(script);
        self.pending_clear_slot = None;
        self.pending_wait_click = false;
        self.state.save_snapshot();
        self.state.rollback_save();
        Ok(())
    }

    /// Reload script matching `self.state.current_script_id()` after a load or rollback (`0x424060`).
    pub fn sync_loaded_script_and_host<H: VmHost>(&mut self, host: &mut H) -> Result<()> {
        let sid = self.state.current_script_id();
        self.current_script = Some(host.load_script(sid)?);
        if self.state.active_bgm_id != 0 {
            host.play_bgm(self.state.active_bgm_id, true, 0);
        } else {
            host.stop_bgm(0);
        }
        if self.state.active_se_id != 0 {
            host.load_se(self.state.active_se_id);
            if self.state.active_se_loop {
                host.play_se(999, 1, self.state.active_se_param);
            }
        } else {
            host.stop_se(0);
        }
        host.refresh_scene(&self.state)?;
        Ok(())
    }

    /// Perform post-click cleanup (`0x422830`) and advance rollback checkpoint if enabled.
    pub fn complete_wait_click<H: VmHost>(&mut self, host: &mut H) -> Result<()> {
        self.pending_wait_click = false;
        if let Some(slot) = self.pending_clear_slot.take() {
            let _ = slot;
            self.state.tbox.current_text_raw.clear();
            host.refresh_scene(&self.state)?;
        }
        if self.state.rollback_enable {
            self.state.rollback_save();
        }
        Ok(())
    }

    /// Complete a `0x0E select_6` choice selection (`choice_idx` in `0..choices.len()`).
    pub fn complete_select<H: VmHost>(
        &mut self,
        host: &mut H,
        targets: &[u32],
        choice_idx: usize,
    ) -> Result<()> {
        if self.state.rollback_enable {
            self.state.rollback_save();
        }
        if let Some(&target) = targets.get(choice_idx) {
            self.jump_to(host, target)?;
        }
        Ok(())
    }

    /// Complete an interactive `0x49 timer_wait_loop` with `result` written to `vars[var_idx]`.
    pub fn complete_timer_wait_loop(&mut self, var_idx: i16, result: i16) {
        self.state.write_var(var_idx, result);
        if self.state.auto_clear_anim {
            for l in &mut self.state.layers {
                l.anim_active = false;
            }
        }
    }

    /// Trigger rollback restore (`0x33`, `0x424060`).
    pub fn rollback_restore<H: VmHost>(&mut self, host: &mut H) -> Result<()> {
        let snap = self.state.rollback_snapshot.clone();
        let vars = self.state.rollback_vars;
        self.state.apply_scene_snapshot(&snap, &vars);
        self.pending_clear_slot = None;
        self.pending_wait_click = false;
        self.sync_loaded_script_and_host(host)?;
        Ok(())
    }

    /// Step instructions until the VM yields (wait state or halt) or exceeds `max_steps` instructions.
    pub fn run_until_yield<H: VmHost>(
        &mut self,
        host: &mut H,
        max_steps: usize,
    ) -> Result<Option<VmYield>> {
        for _ in 0..max_steps {
            if let Some(y) = self.step_one(host)? {
                return Ok(Some(y));
            }
        }
        Ok(None)
    }

    /// Execute a single bytecode instruction. Returns `Some(VmYield)` if execution must pause/yield.
    pub fn step_one<H: VmHost>(&mut self, host: &mut H) -> Result<Option<VmYield>> {
        let script = match &self.current_script {
            Some(s) => s,
            None => {
                return Ok(Some(VmYield::Halted {
                    reason: HaltReason::EndOfScript,
                }))
            }
        };

        let pc = self.state.pc as usize;
        if pc >= script.code.len() {
            if self.state.call_depth > 0 {
                self.do_ret(host, 0)?;
                return Ok(None);
            }
            return Ok(Some(VmYield::Halted {
                reason: HaltReason::EndOfScript,
            }));
        }

        let insn = decode_instruction(&script.code, pc)?;
        let next_pc = self.state.pc + insn.byte_len;
        self.state.pc = next_pc;

        match insn.kind {
            InstructionKind::Hi(hi) => {
                self.exec_hi_op(hi);
                Ok(None)
            }
            InstructionKind::Cmd(cmd) => self.exec_cmd(host, cmd, next_pc),
        }
    }

    fn read_hi_operand(&self, mode: OperandMode, dep: u8, raw: i16) -> i16 {
        match mode {
            OperandMode::Immediate => raw,
            OperandMode::Reg => self.state.regs.get(raw as u16 as usize).copied().unwrap_or(0),
            OperandMode::Var => {
                let idx = self.state.deref_index(i16::from(dep), raw);
                self.state.read_var(idx)
            }
            OperandMode::Unknown(_) => 0,
        }
    }

    fn write_hi_lhs(&mut self, mode: OperandMode, dep: u8, raw: i16, val: i16) {
        match mode {
            OperandMode::Immediate | OperandMode::Unknown(_) => {}
            OperandMode::Reg => {
                if let Some(r) = self.state.regs.get_mut(raw as u16 as usize) {
                    *r = val;
                }
            }
            OperandMode::Var => {
                let idx = self.state.deref_index(i16::from(dep), raw);
                self.state.write_var(idx, val);
            }
        }
    }

    fn exec_hi_op(&mut self, op: HiOp) {
        let dst = op.reg_dst as usize;
        let out = match op.kind {
            HiOpKind::Assign => {
                let val = self.read_hi_operand(op.rhs_mode, op.rhs_dep, op.rhs_raw);
                self.write_hi_lhs(op.lhs_mode, op.lhs_dep, op.lhs_raw, val);
                val
            }
            HiOpKind::LoadReg => self.read_hi_operand(op.rhs_mode, op.rhs_dep, op.rhs_raw),
            HiOpKind::LogicalOr => {
                let l = self.read_hi_operand(op.lhs_mode, op.lhs_dep, op.lhs_raw);
                if l != 0 {
                    1
                } else {
                    let r = self.read_hi_operand(op.rhs_mode, op.rhs_dep, op.rhs_raw);
                    i16::from(r != 0)
                }
            }
            HiOpKind::LogicalAnd => {
                let l = self.read_hi_operand(op.lhs_mode, op.lhs_dep, op.lhs_raw);
                if l == 0 {
                    0
                } else {
                    let r = self.read_hi_operand(op.rhs_mode, op.rhs_dep, op.rhs_raw);
                    i16::from(r != 0)
                }
            }
            other => {
                let l = self.read_hi_operand(op.lhs_mode, op.lhs_dep, op.lhs_raw);
                let r = self.read_hi_operand(op.rhs_mode, op.rhs_dep, op.rhs_raw);
                match other {
                    HiOpKind::Eq => i16::from(l == r),
                    HiOpKind::Ge => i16::from(l >= r),
                    HiOpKind::Gt => i16::from(l > r),
                    HiOpKind::Le => i16::from(l <= r),
                    HiOpKind::Lt => i16::from(l < r),
                    HiOpKind::Ne => i16::from(l != r),
                    HiOpKind::Add => l.wrapping_add(r),
                    HiOpKind::Sub => l.wrapping_sub(r),
                    HiOpKind::Mul => l.wrapping_mul(r),
                    HiOpKind::Div => {
                        if r == 0 {
                            0
                        } else {
                            l.wrapping_div(r)
                        }
                    }
                    HiOpKind::Mod => {
                        if r == 0 {
                            0
                        } else {
                            l.wrapping_rem(r)
                        }
                    }
                    HiOpKind::Assign
                    | HiOpKind::LoadReg
                    | HiOpKind::LogicalOr
                    | HiOpKind::LogicalAnd => unreachable!(),
                }
            }
        };
        if let Some(slot) = self.state.regs.get_mut(dst) {
            *slot = out;
        }
    }

    fn jump_to<H: VmHost>(&mut self, host: &mut H, target: u32) -> Result<Option<VmYield>> {
        let code_len = self
            .current_script
            .as_ref()
            .map(|s| s.code.len() as u32)
            .unwrap_or(0);
        if target < code_len {
            self.state.pc = target;
            Ok(None)
        } else if target == code_len {
            if self.state.call_depth > 0 {
                self.do_ret(host, 0)?;
                Ok(None)
            } else {
                self.state.pc = code_len;
                Ok(Some(VmYield::Halted {
                    reason: HaltReason::EndOfScript,
                }))
            }
        } else {
            Err(FujihimeError::VmError {
                reason: format!(
                    "jump target {target} out of bounds (code_len={code_len}) in script {:04}",
                    self.state.current_script_id()
                ),
            })
        }
    }

    fn do_ret<H: VmHost>(&mut self, host: &mut H, ret_val: i16) -> Result<()> {
        if self.state.call_depth == 0 {
            return Err(FujihimeError::VmError {
                reason: "script nest underflow (スクリプトネストアンダーフロー)".to_string(),
            });
        }
        self.state.call_depth -= 1;
        let caller_id = self.state.script_ids[self.state.call_depth as usize];
        let ret_pc = self.state.return_pcs[self.state.call_depth as usize];
        self.current_script = Some(host.load_script(caller_id)?);
        self.state.pc = ret_pc;
        self.state.vars[0] = ret_val;
        Ok(())
    }

    fn expand_str_by_idx(&self, str_idx: u32) -> Vec<u8> {
        let raw = self
            .current_script
            .as_ref()
            .and_then(|s| s.raw_string(str_idx))
            .unwrap_or(&[]);
        expand_script_string(
            raw,
            self.state.encoding,
            &self.state.vars,
            &self.state.str_vars,
        )
    }

    fn target_layers(&self, spec: i16) -> Vec<usize> {
        if spec == 0 {
            (1..LAYER_COUNT).collect()
        } else if spec > 100 {
            let group = (spec as u16) % 100;
            (1..LAYER_COUNT)
                .filter(|&i| self.state.layers[i].group_id == group)
                .collect()
        } else if spec > 0 && (spec as usize) < LAYER_COUNT {
            vec![spec as usize]
        } else {
            Vec::new()
        }
    }

    fn validate_single_layer(&self, layer: i16) -> Result<usize> {
        if layer < 0 || (layer as usize) >= LAYER_COUNT {
            return Err(FujihimeError::VmError {
                reason: format!("invalid layer {layer} (レイヤー指定違反)"),
            });
        }
        Ok(layer as usize)
    }

    fn exec_cmd<H: VmHost>(
        &mut self,
        host: &mut H,
        cmd: CommandOp,
        next_pc: u32,
    ) -> Result<Option<VmYield>> {
        match cmd {
            // 4.1 Control flow
            CommandOp::Jz { target } => {
                if self.state.regs[0] == 0 {
                    return self.jump_to(host, target);
                }
            }
            CommandOp::Jnz { target } => {
                if self.state.regs[0] != 0 {
                    return self.jump_to(host, target);
                }
            }
            CommandOp::Jmp { target } => {
                return self.jump_to(host, target);
            }
            CommandOp::Exit => {
                return Ok(Some(VmYield::Halted {
                    reason: HaltReason::ExitOpcode,
                }));
            }
            CommandOp::RandReg { reg_dst } => {
                let r = self.state.next_rand_i16();
                if let Some(slot) = self.state.regs.get_mut(reg_dst as usize) {
                    *slot = r;
                }
            }
            CommandOp::WaitClick => {
                if !self.state.fast_skip_active {
                    self.pending_clear_slot = None;
                    self.pending_wait_click = true;
                    return Ok(Some(VmYield::WaitClick {
                        slot: 0,
                        clear_after: false,
                        is_dialogue: false,
                        voice_id: 0,
                    }));
                }
            }
            CommandOp::WaitClickClear => {
                if !self.state.fast_skip_active {
                    self.pending_clear_slot = Some(0);
                    self.pending_wait_click = true;
                    return Ok(Some(VmYield::WaitClick {
                        slot: 0,
                        clear_after: true,
                        is_dialogue: false,
                        voice_id: 0,
                    }));
                } else {
                    self.state.tbox.current_text_raw.clear();
                }
            }
            CommandOp::GotoScript {
                script_id,
                label_str,
            } => {
                let sid = self.state.eval_expr(script_id) as u16;
                let label_bytes = if label_str != 0 {
                    Some(self.expand_str_by_idx(label_str))
                } else {
                    None
                };
                let script = host.load_script(sid)?;
                let target_pc = label_bytes
                    .as_deref()
                    .and_then(|lbl| script.find_label(lbl))
                    .unwrap_or(0);
                self.state.script_ids[self.state.call_depth as usize] = sid;
                self.state.pc = target_pc;
                self.current_script = Some(script);
            }
            CommandOp::Sleep100ms { ticks } => {
                let t = self.state.eval_expr(ticks);
                if t > 0 && !self.state.fast_skip_active {
                    return Ok(Some(VmYield::WaitSleepMs((t as u32) * 100)));
                }
            }
            CommandOp::CallScript {
                script_id,
                label_str,
                args,
            } => {
                if (self.state.call_depth as usize) > MAX_CALL_DEPTH {
                    return Err(FujihimeError::VmError {
                        reason: "script nest overflow (スクリプトネストオーバーフロー)".to_string(),
                    });
                }
                let sid = self.state.eval_expr(script_id) as u16;
                let label_bytes = if label_str != 0 {
                    Some(self.expand_str_by_idx(label_str))
                } else {
                    None
                };
                let mut eval_args = [0i16; 10];
                for (i, a) in args.iter().enumerate() {
                    eval_args[i] = self.state.eval_expr(*a);
                }
                let depth = self.state.call_depth as usize;
                self.state.return_pcs[depth] = next_pc;
                self.state.call_depth += 1;
                self.state.script_ids[depth + 1] = sid;
                let script = host.load_script(sid)?;
                let target_pc = label_bytes
                    .as_deref()
                    .and_then(|lbl| script.find_label(lbl))
                    .unwrap_or(0);
                self.state.pc = target_pc;
                self.current_script = Some(script);
                self.state.vars[10..20].copy_from_slice(&eval_args);
            }
            CommandOp::Ret { ret_val } => {
                let val = self.state.eval_expr(ret_val);
                self.do_ret(host, val)?;
            }
            CommandOp::SubVmCall { script_id } => {
                // Full backup/nested execution semantics (0 uses in 341 scripts).
                let sid = self.state.eval_expr(script_id) as u16;
                let backup_vars = self.state.vars;
                let backup_tbox = self.state.tbox.clone();
                let backup_script = self.current_script.clone();
                let backup_depth = self.state.call_depth;
                let backup_ids = self.state.script_ids;
                let backup_pcs = self.state.return_pcs;
                let backup_pc = self.state.pc;

                self.start_script(host, sid, 0)?;
                let _ = self.run_until_yield(host, 10_000)?;

                self.state.vars = backup_vars;
                self.state.tbox = backup_tbox;
                self.current_script = backup_script;
                self.state.call_depth = backup_depth;
                self.state.script_ids = backup_ids;
                self.state.return_pcs = backup_pcs;
                self.state.pc = backup_pc;
            }
            CommandOp::GosubLocal { target_pc, args } => {
                if (self.state.call_depth as usize) > MAX_CALL_DEPTH {
                    return Err(FujihimeError::VmError {
                        reason: "script nest overflow (スクリプトネストオーバーフロー)".to_string(),
                    });
                }
                let mut eval_args = [0i16; 10];
                for (i, a) in args.iter().enumerate() {
                    eval_args[i] = self.state.eval_expr(*a);
                }
                let depth = self.state.call_depth as usize;
                self.state.return_pcs[depth] = next_pc;
                self.state.script_ids[depth + 1] = self.state.script_ids[depth];
                self.state.call_depth += 1;
                self.state.vars[10..20].copy_from_slice(&eval_args);
                return self.jump_to(host, target_pc);
            }

            // 4.2 Choices, menus, rollback & snapshots
            CommandOp::Select6 {
                count,
                prompt_str,
                targets,
                choice_strs,
                opt0,
                opt1,
                opt2,
            } => {
                if self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                    self.state.batch_mode = false;
                }
                if self.state.fast_skip_active {
                    self.state.fast_skip_active = false;
                    host.refresh_scene(&self.state)?;
                }
                if self.state.auto_snapshot_enable {
                    self.state.save_snapshot();
                }
                let prompt_raw = self.expand_str_by_idx(prompt_str);
                self.state.tbox.current_text_raw = prompt_raw.clone();
                let n = (count as usize).min(5);
                let mut choices = Vec::with_capacity(n);
                for i in 0..n {
                    choices.push((self.expand_str_by_idx(choice_strs[i]), targets[i]));
                }
                let opts = [
                    self.state.eval_expr(opt0),
                    self.state.eval_expr(opt1),
                    self.state.eval_expr(opt2),
                ];
                return Ok(Some(VmYield::WaitSelect(ChoicePrompt {
                    prompt_raw,
                    choices,
                    opts,
                })));
            }
            CommandOp::MenuPause => {
                return Ok(Some(VmYield::MenuPause));
            }
            CommandOp::CallSubsceneDll { str_idx, arg0 } => {
                let _path = self.expand_str_by_idx(str_idx);
                self.state.vars[0] = self.state.eval_expr(arg0);
                host.stop_bgm(0);
                host.stop_se(0);
                host.stop_voice(0);
                return Ok(Some(VmYield::Halted {
                    reason: HaltReason::SubsceneDll,
                }));
            }
            CommandOp::RollbackSave => {
                self.state.rollback_save();
            }
            CommandOp::RollbackRestore => {
                self.rollback_restore(host)?;
            }
            CommandOp::SkipStop => {
                self.state.fast_skip_active = false;
                host.refresh_scene(&self.state)?;
            }
            CommandOp::RollbackFlag { enable } => {
                self.state.rollback_enable = self.state.eval_expr(enable) != 0;
            }
            CommandOp::SaveSnapshot => {
                self.state.save_snapshot();
            }
            CommandOp::AbortIfFlag => {
                if self.state.recall_flag {
                    return Ok(Some(VmYield::Halted {
                        reason: HaltReason::AbortFlag,
                    }));
                }
            }
            CommandOp::MapSelect {
                mode,
                param2,
                unused,
            } => {
                return Ok(Some(VmYield::WaitMapSelect {
                    mode: self.state.eval_expr(mode),
                    param2: self.state.eval_expr(param2),
                    unused: self.state.eval_expr(unused),
                }));
            }

            // 4.3 Text display, styling & backlog
            CommandOp::TboxShow { slot, show } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.visible = self.state.eval_expr(show) != 0;
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::PortraitFace { face_id, mode } => {
                self.state.tbox.face_id = self.state.eval_expr(face_id);
                self.state.tbox.face_mode = self.state.eval_expr(mode);
            }
            CommandOp::TboxStyle { p1, p2, p3, p4, p5 } => {
                self.state.tbox.style_p1 = self.state.eval_expr(p1);
                self.state.tbox.style_p2 = i32::from(self.state.eval_expr(p2));
                self.state.tbox.style_p3 = self.state.eval_expr(p3);
                self.state.tbox.style_p4 = self.state.eval_expr(p4);
                self.state.auto_snapshot_enable = self.state.eval_expr(p5) != 0;
            }
            CommandOp::TboxReset => {
                self.state.tbox = VmTextboxState::default();
            }
            CommandOp::TextClear { slot } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.current_text_raw.clear();
                host.refresh_scene(&self.state)?;
            }
            CommandOp::TextVoice {
                slot,
                voice_id,
                loop_cnt,
                delay_50ms,
                unused_str: _,
                text_str,
                clear,
            } => {
                self.state.batch_mode = false;
                let slot_u16 = self.state.eval_expr(slot) as u16;
                let vid = self.state.eval_expr(voice_id) as u16;
                let lcnt = self.state.eval_expr(loop_cnt);
                let d50 = self.state.eval_expr(delay_50ms);
                let clr = self.state.eval_expr(clear) != 0;

                let cur_sid = self.state.current_script_id();
                let was_read = self.state.kidoku.get_bit(cur_sid, text_str);
                if self.state.fast_skip_active && !host.skip_unread() && !was_read {
                    self.state.fast_skip_active = false;
                    host.refresh_scene(&self.state)?;
                }
                self.state.kidoku.set_bit(cur_sid, text_str);
                host.on_persistent_state_changed(&self.state);

                if self.state.auto_snapshot_enable {
                    self.state.save_snapshot();
                }

                let expanded = self.expand_str_by_idx(text_str);
                self.state.tbox.visible = self.state.tbox.master_enable;
                self.state.tbox.active_slot = slot_u16;
                self.state.tbox.current_text_raw = expanded.clone();
                self.state.tbox.current_voice_id = vid;
                if self.state.tbox.backlog.len() >= BACKLOG_CAPACITY {
                    self.state.tbox.backlog.pop_front();
                }
                self.state.tbox.backlog.push_back(BacklogEntry {
                    raw_bytes: expanded.clone(),
                    voice_id: vid,
                    script_id: cur_sid,
                    text_str_idx: text_str,
                });

                host.display_text(&self.state, slot_u16, &expanded)?;

                if self.state.fast_skip_active {
                    if clr && self.state.tbox.wait_mode == 0 {
                        self.state.tbox.current_text_raw.clear();
                    }
                    return Ok(None);
                }

                if vid != 0 {
                    host.play_voice(vid, lcnt, 0, d50);
                }

                self.pending_clear_slot = if clr { Some(slot_u16) } else { None };
                self.pending_wait_click = true;
                return Ok(Some(VmYield::WaitClick {
                    slot: slot_u16,
                    clear_after: clr,
                    is_dialogue: true,
                    voice_id: vid,
                }));
            }
            CommandOp::TextSay {
                slot,
                p2: _,
                p3: _,
                p4: _,
                text_str,
                clear,
            } => {
                self.state.batch_mode = false;
                let slot_u16 = self.state.eval_expr(slot) as u16;
                let clr = self.state.eval_expr(clear) != 0;
                let cur_sid = self.state.current_script_id();
                self.state.kidoku.set_bit(cur_sid, text_str);
                host.on_persistent_state_changed(&self.state);

                if self.state.auto_snapshot_enable {
                    self.state.save_snapshot();
                }

                let expanded = self.expand_str_by_idx(text_str);
                self.state.tbox.visible = self.state.tbox.master_enable;
                self.state.tbox.active_slot = slot_u16;
                self.state.tbox.current_text_raw.extend_from_slice(&expanded);
                let combined = self.state.tbox.current_text_raw.clone();
                host.display_text(&self.state, slot_u16, &combined)?;

                if self.state.fast_skip_active {
                    if clr {
                        self.state.tbox.current_text_raw.clear();
                    }
                    return Ok(None);
                }
                self.pending_clear_slot = if clr { Some(slot_u16) } else { None };
                self.pending_wait_click = true;
                return Ok(Some(VmYield::WaitClick {
                    slot: slot_u16,
                    clear_after: clr,
                    is_dialogue: true,
                    voice_id: 0,
                }));
            }
            CommandOp::TextClearWait { slot } => {
                let slot_u16 = self.state.eval_expr(slot) as u16;
                self.state.tbox.current_text_raw.clear();
                host.refresh_scene(&self.state)?;
                if !self.state.fast_skip_active {
                    self.pending_clear_slot = None;
                    self.pending_wait_click = true;
                    return Ok(Some(VmYield::WaitClick {
                        slot: slot_u16,
                        clear_after: false,
                        is_dialogue: false,
                        voice_id: 0,
                    }));
                }
            }
            // `0x5A` (`0x422640` -> `0x4159e0` -> `0x4165a0`): sets textbox window origin `(x, y)`.
            CommandOp::TboxTextPos { slot, x, y } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.pos = (self.state.eval_expr(x), self.state.eval_expr(y));
            }
            // `0x5B` (`0x4225f0` -> `0x415a10` + `0x415b40`): sets text region `(x, y, w, h)`.
            CommandOp::TboxRect {
                slot,
                x1,
                y1,
                x2,
                y2,
            } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.rect = (
                    self.state.eval_expr(x1),
                    self.state.eval_expr(y1),
                    self.state.eval_expr(x2),
                    self.state.eval_expr(y2),
                );
                self.state.tbox.rect_custom = true;
            }
            // `0x5C` (`0x422540` -> `0x415980` -> `0x4165e0` -> `0x416a20`): loads `tbox%02d.lwg`.
            CommandOp::TboxFontSize { slot, size } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.skin_id = self.state.eval_expr(size).max(0) as u16;
                self.state.tbox.rect_custom = false;
                self.state.tbox.icon_pos_custom = false;
            }
            // `0x5D` (`0x4226d0` -> `0x415700`): sets `compane.lwg` container position `(x, y)`.
            CommandOp::TboxWrapCfg { slot, val } => {
                let x = self.state.eval_expr(slot);
                let y = self.state.eval_expr(val);
                self.state.tbox.compane_pos = Some((x, y));
            }
            // `0x5E` (`0x422670` -> `0x415750` -> `0x4157d0`): sets textbox master enable flag.
            CommandOp::TboxMode { mode } => {
                let enabled = self.state.eval_expr(mode) != 0;
                self.state.tbox.master_enable = enabled;
                if !enabled {
                    self.state.tbox.visible = false;
                } else if !self.state.tbox.current_text_raw.is_empty() {
                    self.state.tbox.visible = true;
                }
            }
            // `0x5F` (`0x4223b0`): text color preset.
            CommandOp::TboxColorPreset { slot, preset } => {
                let _s = self.state.eval_expr(slot);
                let p = self.state.eval_expr(preset);
                let rgb = match p {
                    1 => 0x00FF_FFFF,
                    2 => 0x0079_F1F2,
                    3 => 0x00B7_3333,
                    4 => 0x00FF_DE00,
                    5 => 0x00F8_B1EF,
                    6 => 0x007F_DFA5,
                    7 => 0x00C1_87F6,
                    8 => 0x00FA_A25A,
                    _ => 0x0000_0000,
                };
                self.state.tbox.color_rgb = rgb;
            }
            // `0x60` (`0x4224c0` -> `0x415c00` -> `0x416790` -> `0x440db0`): sets font pixel height.
            CommandOp::TboxCharSpacing { slot, px } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.font_size = self.state.eval_expr(px).max(1) as u16;
            }
            // `0x61` (`0x4224e0` -> `0x415c50` -> `0x4167f0` -> `0x440dc0`): sets font family (`0=Gothic, 1=Mincho`).
            CommandOp::TboxLineSpacing { slot, px } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.font_family_id = self.state.eval_expr(px).max(0) as u16;
            }
            // `0x62` (`0x422500` -> `0x415ca0` -> `0x416820` -> `0x440dd0`): sets alignment (`0=Left, 1=Center, 2=Right`).
            CommandOp::TboxShadow { slot, mode } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.text_align = self.state.eval_expr(mode).max(0) as u16;
            }
            // `0x63` (`0x422520` -> `0x415ba0` -> `0x416750` -> `0x440e20`): sets first/continuation indent `(p2, p3)`.
            CommandOp::TboxSpeed { slot, speed, p3 } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.indent_first = self.state.eval_expr(speed);
                self.state.tbox.indent_cont = self.state.eval_expr(p3);
            }
            // `0x64` (`0x422560` -> `0x415a70` -> `0x416660`): sets wait cursor offset `(x, y)`.
            CommandOp::TboxFacePos { slot, x, y } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.icon_pos = (self.state.eval_expr(x), self.state.eval_expr(y));
                self.state.tbox.icon_pos_custom = true;
            }
            // `0x65` (`0x422690` -> `0x415640`): sets portrait widget position `(x, y)`.
            CommandOp::TboxIconPos { x, y } => {
                self.state.tbox.face_pos = (self.state.eval_expr(x), self.state.eval_expr(y));
            }
            CommandOp::TextVoiceWaitMode { mode } => {
                self.state.tbox.wait_mode = i32::from(self.state.eval_expr(mode));
            }
            // `0x67` (`0x422590` -> `0x4159b0` -> `0x416600` -> `0x416db0`): loads `wait%02d.lwg`.
            CommandOp::TboxFontType { slot, font_id } => {
                let _s = self.state.eval_expr(slot);
                self.state.tbox.wait_id = self.state.eval_expr(font_id).max(0) as u16;
            }
            // `0x68` (`0x4225b0` -> `0x415ad0` -> `0x4166a0`): sets wait cursor glow RGB.
            CommandOp::TboxBgColor { slot, r, g, b } => {
                let _s = self.state.eval_expr(slot);
                let rr = (self.state.eval_expr(r) as u32) & 0xFF;
                let gg = (self.state.eval_expr(g) as u32) & 0xFF;
                let bb = (self.state.eval_expr(b) as u32) & 0xFF;
                self.state.tbox.wait_glow_rgb = (rr << 16) | (gg << 8) | bb;
            }
            // `0x69` (`0x4226b0` -> `0x415690`): sets portrait Z-order (`0 -> 10, nonzero -> 0`).
            CommandOp::TboxBgAlpha { alpha } => {
                let z_flag = self.state.eval_expr(alpha);
                self.state.tbox.face_z = if z_flag == 0 { 10 } else { 0 };
            }
            CommandOp::BacklogSetTitle { str_idx, reset } => {
                if self.state.eval_expr(reset) != 0 {
                    self.state.tbox.backlog_title_raw.clear();
                } else {
                    self.state.tbox.backlog_title_raw = self.expand_str_by_idx(str_idx);
                }
            }
            CommandOp::BacklogAddEntry {
                str_idx,
                p2: _,
                p3: _,
                p4: _,
            } => {
                let expanded = self.expand_str_by_idx(str_idx);
                if self.state.tbox.backlog.len() >= BACKLOG_CAPACITY {
                    self.state.tbox.backlog.pop_front();
                }
                self.state.tbox.backlog.push_back(BacklogEntry {
                    raw_bytes: expanded,
                    voice_id: 0,
                    script_id: self.state.current_script_id(),
                    text_str_idx: str_idx,
                });
            }
            CommandOp::BacklogSetExtra { val } => {
                self.state.tbox.backlog_extra = val;
            }

            // 4.4 Graphics, CG, sprite layers, animations & effects
            CommandOp::BgLoad { cg_id, mode } => {
                let cid = self.state.eval_expr(cg_id) as u16;
                let m = self.state.eval_expr(mode) as u16;
                if cid < 1000 {
                    self.state.vars[9000 + cid as usize] = 1;
                }
                self.state.global_flags.set_bit(0, u32::from(cid));
                host.on_persistent_state_changed(&self.state);

                if cid == 0 {
                    self.state.layers[0].source_kind = 3;
                    self.state.layers[0].img_id = 0;
                    self.state.layers[0].fill_color = 0x0000_0000;
                    self.state.layers[0].visible = true;
                } else {
                    self.state.layers[0].source_kind = 0;
                    self.state.layers[0].img_id = cid;
                    self.state.layers[0].x = 0;
                    self.state.layers[0].y = 0;
                    self.state.layers[0].alpha_or_mode = m;
                    self.state.layers[0].visible = true;
                }
                if !self.state.batch_mode {
                    host.start_transition(&self.state, 0, 15, 0)?;
                }
            }
            CommandOp::BgClear { white_flag } => {
                let wf = self.state.eval_expr(white_flag);
                self.state.layers[0].source_kind = 3;
                self.state.layers[0].img_id = 0;
                self.state.layers[0].fill_color = if wf == 1 { 0x00FF_FFFF } else { 0x0000_0000 };
                self.state.layers[0].visible = true;
                if !self.state.batch_mode {
                    if self.state.fast_skip_active {
                        host.refresh_scene(&self.state)?;
                    } else {
                        host.start_transition(&self.state, 1, 15, 0)?;
                        return Ok(Some(VmYield::WaitTransition {
                            effect_id: 1,
                            steps: 15,
                            delay_ms: 0,
                        }));
                    }
                }
            }
            CommandOp::SprLoadLayer0 { cg_id, x, y, alpha } => {
                let cid = self.state.eval_expr(cg_id) as u16;
                self.state.layers[0].source_kind = 0;
                self.state.layers[0].img_id = cid;
                self.state.layers[0].x = i32::from(self.state.eval_expr(x));
                self.state.layers[0].y = i32::from(self.state.eval_expr(y));
                self.state.layers[0].alpha_or_mode = self.state.eval_expr(alpha) as u16;
                self.state.layers[0].visible = true;
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::Shake { count, dir, u3, u4 } => {
                let c = self.state.eval_expr(count);
                let d = self.state.eval_expr(dir);
                let p3 = self.state.eval_expr(u3);
                let p4 = self.state.eval_expr(u4);
                if !self.state.fast_skip_active {
                    host.trigger_shake(c, d, p3, p4);
                }
            }
            CommandOp::Flash { count, mode } => {
                let c = self.state.eval_expr(count);
                let m = self.state.eval_expr(mode);
                if !self.state.fast_skip_active {
                    host.trigger_flash(c, m);
                }
            }
            CommandOp::BatchBegin => {
                self.state.batch_mode = true;
            }
            CommandOp::BatchEnd => {
                self.state.batch_mode = false;
                host.refresh_scene(&self.state)?;
            }
            CommandOp::Transition {
                effect_id,
                steps,
                delay_ms,
            } => {
                self.state.batch_mode = false;
                let eid = self.state.eval_expr(effect_id) as u16;
                let st = self.state.eval_expr(steps) as u16;
                let dly = self.state.eval_expr(delay_ms) as u16;
                if self.state.fast_skip_active || eid == 0 || eid >= 300 {
                    host.refresh_scene(&self.state)?;
                } else {
                    host.start_transition(&self.state, eid, st, dly)?;
                    return Ok(Some(VmYield::WaitTransition {
                        effect_id: eid,
                        steps: if st == 0 { 10 } else { st },
                        delay_ms: dly,
                    }));
                }
            }
            CommandOp::ZoomTransition {
                out_flag,
                focus_digit,
            } => {
                self.state.batch_mode = false;
                let base = if self.state.eval_expr(out_flag) != 0 {
                    100u16
                } else {
                    200u16
                };
                let digit = (self.state.eval_expr(focus_digit).clamp(0, 9)) as u16;
                let eid = base + digit;
                if self.state.fast_skip_active {
                    host.refresh_scene(&self.state)?;
                } else {
                    host.start_transition(&self.state, eid, 0, 0)?;
                    return Ok(Some(VmYield::WaitTransition {
                        effect_id: eid,
                        steps: 0,
                        delay_ms: 0,
                    }));
                }
            }
            CommandOp::SprLoad {
                layer,
                img_id,
                x,
                y,
                z_mode,
                alpha,
            } => {
                let l_raw = self.state.eval_expr(layer);
                if l_raw != 0 {
                    let li = self.validate_single_layer(l_raw)?;
                    let (sx, sy) = self.state.coord_scale;
                    let px = i32::from(self.state.eval_expr(x)) * i32::from(sx);
                    let py = i32::from(self.state.eval_expr(y)) * i32::from(sy);
                    let iid = self.state.eval_expr(img_id) as u16;
                    let zm = self.state.eval_expr(z_mode);
                    let al = self.state.eval_expr(alpha) as u16;
                    let l = &mut self.state.layers[li];
                    l.source_kind = 0;
                    l.img_id = iid;
                    l.x = px;
                    l.y = py;
                    l.z_mode = zm;
                    l.alpha_or_mode = al;
                    l.visible = true;
                    l.anim_active = false;
                    if !self.state.batch_mode {
                        host.refresh_scene(&self.state)?;
                    }
                }
            }
            CommandOp::SprLoadStr {
                layer,
                x,
                y,
                z_mode,
                alpha,
                str_idx,
            } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                let (sx, sy) = self.state.coord_scale;
                let px = i32::from(self.state.eval_expr(x)) * i32::from(sx);
                let py = i32::from(self.state.eval_expr(y)) * i32::from(sy);
                let zm = self.state.eval_expr(z_mode);
                let al = self.state.eval_expr(alpha) as u16;
                let expanded = self.expand_str_by_idx(str_idx);
                let l = &mut self.state.layers[li];
                l.source_kind = 2;
                l.text_or_path = expanded;
                l.x = px;
                l.y = py;
                l.z_mode = zm;
                l.alpha_or_mode = al;
                l.visible = true;
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprLoadGrp {
                layer,
                x,
                y,
                img_id,
                alpha,
            } => {
                let l_spec = self.state.eval_expr(layer);
                let (sx, sy) = self.state.coord_scale;
                let px = i32::from(self.state.eval_expr(x)) * i32::from(sx);
                let py = i32::from(self.state.eval_expr(y)) * i32::from(sy);
                let iid = self.state.eval_expr(img_id) as u16;
                let al = self.state.eval_expr(alpha) as u16;
                for li in self.target_layers(l_spec) {
                    let l = &mut self.state.layers[li];
                    l.source_kind = 0;
                    l.img_id = iid;
                    l.x = px;
                    l.y = py;
                    l.alpha_or_mode = al;
                    l.visible = true;
                }
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprLoadRel {
                layer,
                dx,
                dy,
                img_id,
                alpha,
            } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                let (sx, sy) = self.state.coord_scale;
                let dxv = i32::from(self.state.eval_expr(dx)) * i32::from(sx);
                let dyv = i32::from(self.state.eval_expr(dy)) * i32::from(sy);
                let iid = self.state.eval_expr(img_id) as u16;
                let al = self.state.eval_expr(alpha) as u16;
                let l = &mut self.state.layers[li];
                l.source_kind = 0;
                l.img_id = iid;
                l.x += dxv;
                l.y += dyv;
                l.alpha_or_mode = al;
                l.visible = true;
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprShowMode { layer, mode } => {
                let l_spec = self.state.eval_expr(layer);
                let m = self.state.eval_expr(mode) as u16;
                for li in self.target_layers(l_spec) {
                    self.state.layers[li].alpha_or_mode = m;
                }
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprClear { layer, mode } => {
                let l_spec = self.state.eval_expr(layer);
                let _m = self.state.eval_expr(mode);
                for li in self.target_layers(l_spec) {
                    let l = &mut self.state.layers[li];
                    l.img_id = 0;
                    l.text_or_path.clear();
                    l.source_kind = 0;
                    l.anim_active = false;
                }
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprVisible { layer, vis } => {
                let l_spec = self.state.eval_expr(layer);
                let v = self.state.eval_expr(vis) != 0;
                for li in self.target_layers(l_spec) {
                    self.state.layers[li].visible = v;
                }
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            // `0x26` (`0x423230` -> `0x406300`): sets sprite centering flags `(anchor_x, anchor_y)`.
            CommandOp::SprPos { layer, x, y } => {
                let l_spec = self.state.eval_expr(layer);
                let ax = i32::from(self.state.eval_expr(x));
                let ay = i32::from(self.state.eval_expr(y));
                for li in self.target_layers(l_spec) {
                    self.state.layers[li].anchor_x = ax;
                    self.state.layers[li].anchor_y = ay;
                }
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprBlend { layer, mode, param } => {
                let l_spec = self.state.eval_expr(layer);
                let m = self.state.eval_expr(mode) as u16;
                let p = self.state.eval_expr(param) as u16;
                for li in self.target_layers(l_spec) {
                    self.state.layers[li].blend_mode = m;
                    self.state.layers[li].blend_param = p;
                }
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprZ { layer, z } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                self.state.layers[li].z_order = self.state.eval_expr(z);
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprGroup { layer, group_id } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                self.state.layers[li].group_id = self.state.eval_expr(group_id) as u16;
            }
            CommandOp::EyesOverlay { es_id, blend_flag } => {
                self.state.eyes_overlay.es_id = self.state.eval_expr(es_id) as u16;
                self.state.eyes_overlay.blend_flag = self.state.eval_expr(blend_flag) as u16;
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::EyesZ { z } => {
                self.state.eyes_overlay.z_order = self.state.eval_expr(z);
            }
            CommandOp::FilterOverlay {
                alpha_pct,
                mode_flag,
            } => {
                self.state.filter_overlay.alpha_pct = self.state.eval_expr(alpha_pct) as u16;
                self.state.filter_overlay.mode_flag = self.state.eval_expr(mode_flag) as u16;
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::FilterZ { z } => {
                self.state.filter_overlay.z_order = self.state.eval_expr(z);
            }
            CommandOp::CoordScale { sx, sy } => {
                self.state.coord_scale = (self.state.eval_expr(sx), self.state.eval_expr(sy));
            }
            CommandOp::SprType { layer, type_id } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                self.state.layers[li].layer_type = self.state.eval_expr(type_id) as u16;
            }
            CommandOp::SprLoadCentered {
                layer,
                img_id,
                mode,
                alpha,
            } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                let iid = self.state.eval_expr(img_id) as u16;
                let _m = self.state.eval_expr(mode);
                let al = self.state.eval_expr(alpha) as u16;
                let l = &mut self.state.layers[li];
                l.source_kind = 0;
                l.img_id = iid;
                l.alpha_or_mode = al;
                l.anchor_x = 1;
                l.anchor_y = 1;
                l.visible = true;
                if !self.state.batch_mode {
                    host.refresh_scene(&self.state)?;
                }
            }
            CommandOp::SprAnimX { layer, p2, p3, p4 } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                let vals = [
                    self.state.eval_expr(p2),
                    self.state.eval_expr(p3),
                    self.state.eval_expr(p4),
                ];
                self.state.layers[li].anim_x = vals;
                self.state.layers[li].anim_kind = 0;
                self.state.layers[li].anim_active = true;
            }
            CommandOp::SprAnimY { layer, p2, p3, p4 } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                let vals = [
                    self.state.eval_expr(p2),
                    self.state.eval_expr(p3),
                    self.state.eval_expr(p4),
                ];
                self.state.layers[li].anim_y = vals;
                self.state.layers[li].anim_kind = 1;
                self.state.layers[li].anim_active = true;
            }
            CommandOp::SprAnimStop { layer } => {
                let l_spec = self.state.eval_expr(layer);
                for li in self.target_layers(l_spec) {
                    self.state.layers[li].anim_active = false;
                }
            }
            CommandOp::TimerWaitLoop { var_idx, mode, p3 } => {
                let vi = self.state.eval_expr(var_idx);
                let m = self.state.eval_expr(mode);
                let p = self.state.eval_expr(p3);
                self.state.write_var(vi, 0);
                if self.state.layers.iter().any(|l| l.anim_active) {
                    return Ok(Some(VmYield::WaitTimerLoop {
                        var_idx: vi,
                        mode: m,
                        p3: p,
                    }));
                }
                if self.state.auto_clear_anim {
                    for l in &mut self.state.layers {
                        l.anim_active = false;
                    }
                }
            }
            CommandOp::AutoClearAnim { enable } => {
                self.state.auto_clear_anim = self.state.eval_expr(enable) != 0;
            }
            CommandOp::SprAnimFrame {
                layer,
                img_id,
                x,
                y,
                slot,
            } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                let iid = self.state.eval_expr(img_id) as u16;
                let px = i32::from(self.state.eval_expr(x));
                let py = i32::from(self.state.eval_expr(y));
                let sl = (self.state.eval_expr(slot) as usize).min(1);
                self.state.layers[li].subframe_img_ids[sl] = iid;
                self.state.layers[li].subframe_coords[sl] = (px, py);
            }
            CommandOp::SprAnimAlpha { layer, p2, p3, p4 } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                let vals = [
                    self.state.eval_expr(p2),
                    self.state.eval_expr(p3),
                    self.state.eval_expr(p4),
                ];
                self.state.layers[li].anim_alpha = vals;
                self.state.layers[li].anim_kind = 2;
                self.state.layers[li].anim_active = true;
            }
            CommandOp::SprAnchor { layer, val } => {
                let l_spec = self.state.eval_expr(layer);
                let v = self.state.eval_expr(val) as u16;
                for li in self.target_layers(l_spec) {
                    self.state.layers[li].extra_50 = v;
                }
            }
            // `0x79` (`0x4234f0`): when `layer == 0`, updates only layers `1..=99`, preserving layer 0 (`grpe`).
            CommandOp::SprDir { layer, str_idx } => {
                let l_spec = self.state.eval_expr(layer);
                let raw = self.expand_str_by_idx(str_idx);
                let dir = String::from_utf8_lossy(&raw)
                    .trim_start_matches(".\\")
                    .trim_start_matches("./")
                    .trim_start_matches('\\')
                    .to_string();
                for li in self.target_layers(l_spec) {
                    self.state.layers[li].dir_prefix = dir.clone();
                }
            }
            CommandOp::SprClipRect { layer, l, t, r, b } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                self.state.layers[li].clip_rect = [
                    self.state.eval_expr(l),
                    self.state.eval_expr(t),
                    self.state.eval_expr(r),
                    self.state.eval_expr(b),
                ];
            }

            // 4.5 Audio & video control
            CommandOp::BgmPlay {
                bgm_id,
                loop_flag,
                fade_ms,
            } => {
                let id = self.state.eval_expr(bgm_id) as u16;
                let lf = self.state.eval_expr(loop_flag) != 0;
                let fm = self.state.eval_expr(fade_ms) as u16;
                self.state.active_bgm_id = id;
                if !self.state.fast_skip_active {
                    host.play_bgm(id, lf, fm);
                }
            }
            CommandOp::BgmStop { unused: _, fade_ms } => {
                let fm = self.state.eval_expr(fade_ms) as u16;
                self.state.active_bgm_id = 0;
                if !self.state.fast_skip_active {
                    host.stop_bgm(fm);
                }
            }
            CommandOp::SeLoad { se_id } => {
                let id = self.state.eval_expr(se_id) as u16;
                self.state.active_se_id = id;
                if id != 0 && !self.state.fast_skip_active {
                    host.load_se(id);
                }
            }
            CommandOp::SePlay {
                loop_cnt,
                fade_flag,
                fade_50ms,
            } => {
                let lcnt = self.state.eval_expr(loop_cnt);
                let ff = self.state.eval_expr(fade_flag) as u16;
                let f50 = self.state.eval_expr(fade_50ms);
                if lcnt == 999 {
                    self.state.active_se_loop = true;
                }
                self.state.active_se_param = f50;
                if !self.state.fast_skip_active {
                    host.play_se(lcnt, ff, f50);
                }
            }
            CommandOp::SeStop { fade_ms } => {
                let fm = self.state.eval_expr(fade_ms) as u16;
                self.state.active_se_loop = false;
                if !self.state.fast_skip_active {
                    host.stop_se(fm);
                }
            }
            CommandOp::MpgPlay { mpg_id } => {
                let id = self.state.eval_expr(mpg_id) as u16;
                host.start_video(id);
                return Ok(Some(VmYield::WaitVideo { mpg_id: id }));
            }
            CommandOp::VoicePlay {
                voice_id,
                loop_cnt,
                fade_flag,
                delay_50ms,
            } => {
                let vid = self.state.eval_expr(voice_id) as u16;
                let lcnt = self.state.eval_expr(loop_cnt);
                let ff = self.state.eval_expr(fade_flag) as u16;
                let d50 = self.state.eval_expr(delay_50ms);
                if vid != 0 && !self.state.fast_skip_active {
                    host.play_voice(vid, lcnt, ff, d50);
                }
            }
            CommandOp::VoiceStop { fade_ms } => {
                let fm = self.state.eval_expr(fade_ms) as u16;
                if !self.state.fast_skip_active {
                    host.stop_voice(fm);
                }
            }
            CommandOp::SeWait => {
                if !self.state.fast_skip_active && host.is_audio_playing(false) {
                    return Ok(Some(VmYield::WaitAudio { is_voice: false }));
                }
            }
            CommandOp::VoiceWait => {
                if !self.state.fast_skip_active && host.is_audio_playing(true) {
                    return Ok(Some(VmYield::WaitAudio { is_voice: true }));
                }
            }

            // 4.6 Math, arrays, global flags, UI gauges & string vars
            CommandOp::LoadArray { var_idx, arr_idx } => {
                let start = self.state.eval_expr(var_idx);
                if let Some(elems) = self
                    .current_script
                    .as_ref()
                    .and_then(|s| s.get_array(arr_idx))
                    .map(|slice| slice.to_vec())
                {
                    for (offset, val) in elems.into_iter().enumerate() {
                        self.state.write_var(start.wrapping_add(offset as i16), val as i16);
                    }
                }
            }
            CommandOp::GetSprPos {
                layer,
                out_x_var,
                out_y_var,
            } => {
                let li = self.validate_single_layer(self.state.eval_expr(layer))?;
                let (sx, sy) = self.state.coord_scale;
                let ox = self.state.eval_expr(out_x_var);
                let oy = self.state.eval_expr(out_y_var);
                let x = if sx != 0 {
                    (self.state.layers[li].x / i32::from(sx)) as i16
                } else {
                    0
                };
                let y = if sy != 0 {
                    (self.state.layers[li].y / i32::from(sy)) as i16
                } else {
                    0
                };
                self.state.write_var(ox, x);
                self.state.write_var(oy, y);
            }
            CommandOp::Muldiv { a, b, c } => {
                let av = i32::from(self.state.eval_expr(a));
                let bv = i32::from(self.state.eval_expr(b));
                let cv = i32::from(self.state.eval_expr(c));
                self.state.vars[0] = if cv != 0 { ((av * bv) / cv) as i16 } else { 0 };
            }
            CommandOp::FtolNoop { val } => {
                let _ = self.state.eval_expr(val);
            }
            CommandOp::PowI16 { base, exp } => {
                let b = i32::from(self.state.eval_expr(base));
                let e = self.state.eval_expr(exp);
                let res = if e <= 1 {
                    b as i16
                } else {
                    let mut acc = 1i32;
                    for _ in 0..e {
                        acc = acc.wrapping_mul(b);
                    }
                    acc as i16
                };
                self.state.vars[0] = res;
            }
            CommandOp::GlobalFlag { flag_id, is_set } => {
                let fid = self.state.eval_expr(flag_id) as u16;
                if self.state.eval_expr(is_set) != 0 {
                    self.state.global_flags.set_bit(0, u32::from(fid));
                    host.on_persistent_state_changed(&self.state);
                } else {
                    self.state.vars[0] = i16::from(self.state.global_flags.get_bit(0, u32::from(fid)));
                }
            }
            CommandOp::GaugeSetParts {
                idx,
                num_id,
                bar_id,
                fix_id,
            } => {
                let gi = (self.state.eval_expr(idx) as usize).min(9);
                self.state.gauges[gi].num_id = self.state.eval_expr(num_id);
                self.state.gauges[gi].bar_id = self.state.eval_expr(bar_id);
                self.state.gauges[gi].fix_id = self.state.eval_expr(fix_id);
            }
            CommandOp::GaugeSetRange {
                idx,
                min,
                max,
                p4,
                p5,
            } => {
                let gi = (self.state.eval_expr(idx) as usize).min(9);
                self.state.gauges[gi].min = self.state.eval_expr(min);
                self.state.gauges[gi].max = self.state.eval_expr(max);
                self.state.gauges[gi].p4 = self.state.eval_expr(p4);
                self.state.gauges[gi].p5 = self.state.eval_expr(p5);
            }
            CommandOp::GaugeVisible { idx, vis } => {
                let gi = (self.state.eval_expr(idx) as usize).min(9);
                self.state.gauges[gi].visible = self.state.eval_expr(vis) != 0;
            }
            CommandOp::GaugePos { idx, x, y } => {
                let gi = (self.state.eval_expr(idx) as usize).min(9);
                self.state.gauges[gi].x = self.state.eval_expr(x);
                self.state.gauges[gi].y = self.state.eval_expr(y);
            }
            CommandOp::GaugePartPos {
                idx,
                nx,
                ny,
                bx,
                by,
            } => {
                let gi = (self.state.eval_expr(idx) as usize).min(9);
                self.state.gauges[gi].num_x = self.state.eval_expr(nx);
                self.state.gauges[gi].num_y = self.state.eval_expr(ny);
                self.state.gauges[gi].bar_x = self.state.eval_expr(bx);
                self.state.gauges[gi].bar_y = self.state.eval_expr(by);
            }
            CommandOp::GaugeValue { idx, val, anim } => {
                let gi = (self.state.eval_expr(idx) as usize).min(9);
                self.state.gauges[gi].value = self.state.eval_expr(val);
                self.state.gauges[gi].anim = self.state.eval_expr(anim);
            }
            CommandOp::StrAssignLit { str_var, str_idx } => {
                let sv = (self.state.eval_expr(str_var) as usize).min(9);
                let expanded = self.expand_str_by_idx(str_idx);
                write_cstr_slot(&mut self.state.str_vars[sv], &expanded);
            }
            CommandOp::StrConcatLit { str_var, str_idx } => {
                let sv = (self.state.eval_expr(str_var) as usize).min(9);
                let expanded = self.expand_str_by_idx(str_idx);
                append_cstr_slot(&mut self.state.str_vars[sv], &expanded);
            }
            CommandOp::StrFromIntFw { str_var, int_var } => {
                let sv = (self.state.eval_expr(str_var) as usize).min(9);
                let iv = self.state.eval_expr(int_var);
                let val = self.state.read_var(iv);
                let fw = format_fullwidth_i16(val, self.state.encoding);
                write_cstr_slot(&mut self.state.str_vars[sv], &fw);
            }
            CommandOp::StrToInt { out_var, str_var } => {
                let ov = self.state.eval_expr(out_var);
                let sv = (self.state.eval_expr(str_var) as usize).min(9);
                let parsed = parse_int_from_cstr(&self.state.str_vars[sv], self.state.encoding);
                self.state.write_var(ov, parsed);
            }
            CommandOp::StrCopy { dst, src } => {
                let d = (self.state.eval_expr(dst) as usize).min(9);
                let s = (self.state.eval_expr(src) as usize).min(9);
                let copy = self.state.str_vars[s];
                self.state.str_vars[d] = copy;
            }
            CommandOp::StrConcat { dst, src } => {
                let d = (self.state.eval_expr(dst) as usize).min(9);
                let s = (self.state.eval_expr(src) as usize).min(9);
                let src_bytes = read_cstr_slice(&self.state.str_vars[s]).to_vec();
                append_cstr_slot(&mut self.state.str_vars[d], &src_bytes);
            }
            CommandOp::RicheditOpen { mode: _, u2: _, u3: _ } => {
                // Phase 1B verified unreachable across all 341+341 scripts (`0` occurrences).
                // Safe explicit no-op on iOS / headless port.
            }
            CommandOp::RicheditCfg {
                idx,
                val,
                u3: _,
                u4: _,
                u5: _,
            } => {
                let i = (self.state.eval_expr(idx) as usize).min(15);
                self.state.richedit_cfg[i] = i32::from(self.state.eval_expr(val));
            }
            CommandOp::StrSaveGlobal { str_var, slot } => {
                let sv = (self.state.eval_expr(str_var) as usize).min(9);
                let gs = (self.state.eval_expr(slot) as usize).min(9);
                self.state.global_str_vars[gs] = self.state.str_vars[sv];
                host.on_persistent_state_changed(&self.state);
            }
            CommandOp::StrLoadGlobal { str_var, slot } => {
                let sv = (self.state.eval_expr(str_var) as usize).min(9);
                let gs = (self.state.eval_expr(slot) as usize).min(9);
                self.state.str_vars[sv] = self.state.global_str_vars[gs];
            }
            CommandOp::MemsetVars { start, end, val } => {
                let s = self.state.eval_expr(start).max(0) as usize;
                let e = self.state.eval_expr(end).clamp(0, 9999) as usize;
                let v = self.state.eval_expr(val);
                if s <= e {
                    self.state.vars[s..=e].fill(v);
                }
            }
        }
        Ok(None)
    }
}

/// Resolve caret color preset index (`0x5F`) via shared `caret` helper.
pub fn resolve_color_preset(preset: i16, custom_palette: &[u32; 10]) -> u32 {
    let ch = match preset {
        1 => b'w',
        2 => b's',
        3 => b'r',
        4 => b'y',
        5 => b'p',
        6 => b'g',
        7 => b'v',
        8 => b'o',
        _ => return 0x0000_0000,
    };
    resolve_caret_color(ch, custom_palette).unwrap_or(0x0000_0000)
}

#[inline]
fn is_dbcs_pair(encoding: TextEncoding, b0: u8, b1: u8) -> bool {
    match encoding {
        TextEncoding::Cp932 => is_cp932_lead(b0) && is_cp932_trail(b1),
        TextEncoding::Gbk => is_gbk_lead(b0) && is_gbk_trail(b1),
    }
}

/// Two-pass DBCS-safe script string interpolation (`0x421310` = `0x421200` `@var` + `0x421130` `$0..$9`).
pub fn expand_script_string(
    raw: &[u8],
    encoding: TextEncoding,
    vars: &[i16; TOTAL_VARS],
    str_vars: &[[u8; STR_VAR_STRIDE]; GLOBAL_STR_COUNT],
) -> Vec<u8> {
    // Pass 1: `@` integer variable expansion (`0x421200`)
    let mut pass1 = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while i < raw.len() {
        let b = raw[i];
        if i + 1 < raw.len() && is_dbcs_pair(encoding, b, raw[i + 1]) {
            pass1.push(b);
            pass1.push(raw[i + 1]);
            i += 2;
            continue;
        }
        if b == b'@' {
            let mut depth = 0usize;
            let mut j = i;
            while j < raw.len() && raw[j] == b'@' {
                depth += 1;
                j += 1;
            }
            let num_start = j;
            if j < raw.len() && raw[j] == b'-' {
                j += 1;
            }
            let digit_start = j;
            while j < raw.len() && raw[j].is_ascii_digit() {
                j += 1;
            }
            if j > digit_start {
                let s = std::str::from_utf8(&raw[num_start..j]).unwrap_or("0");
                let mut val = s.parse::<i32>().unwrap_or(0) as i16;
                for _ in 0..depth {
                    val = if let Ok(u) = usize::try_from(val) {
                        vars.get(u).copied().unwrap_or(0)
                    } else {
                        0
                    };
                }
                pass1.extend_from_slice(val.to_string().as_bytes());
                i = j;
                continue;
            } else {
                pass1.extend(std::iter::repeat_n(b'@', depth));
                i = j;
                continue;
            }
        }
        pass1.push(b);
        i += 1;
    }

    // Pass 2: `$0..$9` string variable expansion (`0x421130`)
    let mut pass2 = Vec::with_capacity(pass1.len());
    let mut k = 0usize;
    while k < pass1.len() {
        let b = pass1[k];
        if k + 1 < pass1.len() && is_dbcs_pair(encoding, b, pass1[k + 1]) {
            pass2.push(b);
            pass2.push(pass1[k + 1]);
            k += 2;
            continue;
        }
        if b == b'$' && k + 1 < pass1.len() && pass1[k + 1].is_ascii_digit() {
            let idx = (pass1[k + 1] - b'0') as usize;
            pass2.extend_from_slice(read_cstr_slice(&str_vars[idx]));
            k += 2;
            continue;
        }
        pass2.push(b);
        k += 1;
    }
    pass2
}

fn read_cstr_slice(slot: &[u8]) -> &[u8] {
    let end = slot.iter().position(|&b| b == 0).unwrap_or(slot.len());
    &slot[..end]
}

fn write_cstr_slot(slot: &mut [u8; STR_VAR_STRIDE], bytes: &[u8]) {
    slot.fill(0);
    let n = bytes.len().min(STR_VAR_STRIDE - 1);
    slot[..n].copy_from_slice(&bytes[..n]);
}

fn append_cstr_slot(slot: &mut [u8; STR_VAR_STRIDE], bytes: &[u8]) {
    let cur_len = read_cstr_slice(slot).len();
    let avail = (STR_VAR_STRIDE - 1).saturating_sub(cur_len);
    let n = bytes.len().min(avail);
    slot[cur_len..cur_len + n].copy_from_slice(&bytes[..n]);
    slot[cur_len + n] = 0;
}

fn format_fullwidth_i16(val: i16, encoding: TextEncoding) -> Vec<u8> {
    let ascii = val.to_string();
    let mut out = Vec::with_capacity(ascii.len() * 2);
    for b in ascii.bytes() {
        match (encoding, b) {
            (TextEncoding::Cp932, b'-') => out.extend_from_slice(&[0x81, 0x7C]),
            (TextEncoding::Cp932, b'0'..=b'9') => out.extend_from_slice(&[0x82, 0x4F + (b - b'0')]),
            (TextEncoding::Gbk, b'-') => out.extend_from_slice(&[0xA3, 0xAD]),
            (TextEncoding::Gbk, b'0'..=b'9') => out.extend_from_slice(&[0xA3, 0xB0 + (b - b'0')]),
            _ => out.push(b),
        }
    }
    out
}

fn parse_int_from_cstr(slot: &[u8], encoding: TextEncoding) -> i16 {
    let raw = read_cstr_slice(slot);
    let enc = match encoding {
        TextEncoding::Cp932 => SHIFT_JIS,
        TextEncoding::Gbk => GBK,
    };
    let (s, _, _) = enc.decode(raw);
    let mut ascii = String::new();
    for ch in s.chars() {
        match ch {
            '－' | '-' => ascii.push('-'),
            '０'..='９' => ascii.push(char::from(b'0' + ((ch as u32 - '０' as u32) as u8))),
            '0'..='9' => ascii.push(ch),
            _ => {}
        }
    }
    ascii.parse::<i32>().unwrap_or(0) as i16
}
