//! `.lwg`-backed interactive UI widgets & modal overlays (`compane`, `confscrn`, `savescrn`,
//! `select_6`, `selmap`, and `backlog`).

use std::path::Path;

use crate::gfx::compositor::{blit, filter_fill_argb, BlitParams};
use crate::gfx::decode::LwgImage;
use crate::gfx::surface::{Surface, GAME_HEIGHT, GAME_WIDTH};
use crate::save::{ConfigBlock2007, SlotSave2007, SlotSaveHeader2007};
use crate::text::caret::{parse_caret_bytes, TextEncoding, TextStyle};
use crate::text::freetype::{FontFamily, FontSet};
use crate::text::layout::{layout_text, render_layout_to_surface};
use crate::vfs::AssetVfs;
use crate::vm::{BacklogEntry, ChoicePrompt};

/// Default top-left origin of the `39x106` `compane.lwg` control strip on `800x600`.
pub const COMPANE_ORIGIN_X: i32 = 761;
pub const COMPANE_ORIGIN_Y: i32 = 494;

/// Axis-aligned hit rectangle in logical `800x600` coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl UiRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Blit a single named `.lwg` layer onto `dst` at `(origin_x + layer.x, origin_y + layer.y)`.
pub fn blit_lwg_layer_at(
    dst: &mut Surface,
    lwg: &LwgImage,
    layer_name: &str,
    origin_x: i32,
    origin_y: i32,
    global_alpha: i32,
) -> bool {
    let Some(layer) = lwg.find_layer(layer_name) else {
        return false;
    };
    let params = BlitParams {
        blend_mode: if global_alpha < 255 { 2 } else { 0 },
        alpha_channel_mode: 0,
        global_alpha,
        ..Default::default()
    };
    blit(
        dst,
        &layer.surface,
        &params,
        (origin_x + layer.x, origin_y + layer.y),
        None,
    );
    true
}

/// Look up layer bounding box from `.lwg`, falling back to `fallback`.
pub fn lwg_layer_rect(
    lwg: Option<&LwgImage>,
    layer_name: &str,
    origin_x: i32,
    origin_y: i32,
    fallback: UiRect,
) -> UiRect {
    if let Some(layer) = lwg.and_then(|img| img.find_layer(layer_name)) {
        UiRect::new(
            origin_x + layer.x,
            origin_y + layer.y,
            layer.surface.width() as i32,
            layer.surface.height() as i32,
        )
    } else {
        fallback
    }
}

/// Render raw CP932/GBK bytes onto `dst` at `(x, y)` inside `(max_w, max_h)` using `FontSet`.
#[allow(clippy::too_many_arguments)]
pub fn render_dbcs_text(
    dst: &mut Surface,
    fonts: &FontSet,
    vfs: Option<&AssetVfs>,
    gaiji: &mut crate::text::caret::GaijiBank,
    raw: &[u8],
    encoding: TextEncoding,
    font_size: u16,
    color_rgb: u32,
    x: i32,
    y: i32,
    max_w: i32,
    max_h: i32,
) {
    if raw.is_empty() || max_w <= 0 || max_h <= 0 {
        return;
    }
    let style = TextStyle {
        font_family: FontFamily::Gothic,
        base_height: font_size.max(8),
        color_rgb,
        ..TextStyle::default()
    };
    let parsed = parse_caret_bytes(raw, encoding, style, 0, None, &[color_rgb; 10]);
    if let Ok(layout) = layout_text(&parsed, fonts, vfs, gaiji, max_w, max_h, false) {
        if let Ok(mut text_surf) = Surface::filled(max_w as u32, max_h as u32, 0xFF00_0000) {
            render_layout_to_surface(&mut text_surf, &layout, None, (0, 0));
            let params = BlitParams {
                blend_mode: 0,
                alpha_channel_mode: 0,
                ..Default::default()
            };
            blit(dst, &text_surf, &params, (x, y), None);
        }
    }
}

/// Render an ASCII/UTF-8 label (encoded safely as ASCII/GBK/CP932 bytes) onto `dst`.
#[allow(clippy::too_many_arguments)]
pub fn render_ascii_or_utf8_label(
    dst: &mut Surface,
    fonts: &FontSet,
    vfs: Option<&AssetVfs>,
    gaiji: &mut crate::text::caret::GaijiBank,
    text: &str,
    encoding: TextEncoding,
    font_size: u16,
    color_rgb: u32,
    x: i32,
    y: i32,
    max_w: i32,
    max_h: i32,
) {
    let encoded = match encoding {
        TextEncoding::Cp932 => encoding_rs::SHIFT_JIS.encode(text).0.into_owned(),
        TextEncoding::Gbk => encoding_rs::GBK.encode(text).0.into_owned(),
    };
    render_dbcs_text(
        dst, fonts, vfs, gaiji, &encoded, encoding, font_size, color_rgb, x, y, max_w, max_h,
    );
}

// ---------------------------------------------------------------------------
// 1. `compane.lwg` Control Panel Widget
// ---------------------------------------------------------------------------

/// Buttons defined inside `compane.lwg` (`39x106`, 18 layers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompaneButton {
    Slide,
    Rev,
    Bak,
    Fow,
    Next,
    Auto,
    Hide,
    Voc,
}

/// High-level system menu actions exposed alongside `compane` / `MenuPause`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemPanelAction {
    Save,
    Load,
    Config,
    Backlog,
    Skip,
    Auto,
    HideTextbox,
    Rollback,
    Title,
    Exit,
    Advance,
    ReplayVoice,
}

/// Interactive `compane.lwg` floating/pinned control strip.
#[derive(Debug, Clone)]
pub struct CompaneWidget {
    pub visible: bool,
    pub pinned: bool,
    pub origin_x: i32,
    pub origin_y: i32,
    pub hovered: Option<CompaneButton>,
}

impl Default for CompaneWidget {
    fn default() -> Self {
        Self {
            visible: true,
            pinned: false,
            origin_x: COMPANE_ORIGIN_X,
            origin_y: COMPANE_ORIGIN_Y,
            hovered: None,
        }
    }
}

impl CompaneWidget {
    const BUTTONS: [(CompaneButton, &'static str, UiRect); 8] = [
        (CompaneButton::Slide, "slide", UiRect::new(4, 5, 15, 18)),
        (CompaneButton::Rev, "rev", UiRect::new(19, 3, 17, 18)),
        (CompaneButton::Bak, "bak", UiRect::new(19, 19, 17, 18)),
        (CompaneButton::Fow, "fow", UiRect::new(19, 35, 17, 18)),
        (CompaneButton::Next, "next", UiRect::new(19, 51, 17, 18)),
        (CompaneButton::Auto, "auto", UiRect::new(19, 67, 17, 18)),
        (CompaneButton::Hide, "hide", UiRect::new(3, 67, 17, 18)),
        (CompaneButton::Voc, "voc", UiRect::new(11, 84, 17, 18)),
    ];

    pub fn hit_test(&self, lwg: Option<&LwgImage>, px: i32, py: i32) -> Option<CompaneButton> {
        if !self.visible {
            return None;
        }
        // Check interactive buttons first (slide is handled after or within its rect)
        for &(btn, name, fb) in &Self::BUTTONS {
            let r = lwg_layer_rect(
                lwg,
                name,
                self.origin_x,
                self.origin_y,
                UiRect::new(self.origin_x + fb.x, self.origin_y + fb.y, fb.w, fb.h),
            );
            if r.contains(px, py) {
                return Some(btn);
            }
        }
        None
    }

    pub fn panel_rect(&self, lwg: Option<&LwgImage>) -> UiRect {
        let (w, h) = lwg
            .map(|i| (i.width as i32, i.height as i32))
            .unwrap_or((39, 106));
        UiRect::new(self.origin_x, self.origin_y, w, h)
    }

    pub fn render(
        &self,
        dst: &mut Surface,
        lwg: Option<&LwgImage>,
        auto_active: bool,
        skip_active: bool,
        hidden_active: bool,
    ) {
        if !self.visible {
            return;
        }
        let Some(img) = lwg else {
            return;
        };
        blit_lwg_layer_at(dst, img, "bg", self.origin_x, self.origin_y, 255);
        blit_lwg_layer_at(dst, img, "slide_lev", self.origin_x, self.origin_y, 255);

        let states = [
            (CompaneButton::Slide, "slide", "slide_f", self.pinned),
            (CompaneButton::Rev, "rev", "rev_f", false),
            (CompaneButton::Bak, "bak", "bak_f", false),
            (CompaneButton::Fow, "fow", "fow_f", skip_active),
            (CompaneButton::Next, "next", "next_f", false),
            (CompaneButton::Auto, "auto", "auto_f", auto_active),
            (CompaneButton::Hide, "hide", "hide_f", hidden_active),
            (CompaneButton::Voc, "voc", "voc_f", false),
        ];
        for (btn, norm, hov, active) in states {
            let layer_name = if active || self.hovered == Some(btn) {
                hov
            } else {
                norm
            };
            blit_lwg_layer_at(dst, img, layer_name, self.origin_x, self.origin_y, 255);
        }
    }
}

// ---------------------------------------------------------------------------
// 2. `confscrn.lwg` Settings Overlay (57 layers, JP + CHS)
// ---------------------------------------------------------------------------

/// Slider identifiers inside `confscrn.lwg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSliderKind {
    BgmVol,
    SeVol,
    VoiceVol,
    AutoWait,
}

/// Actions emitted when clicking a control inside `confscrn.lwg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfScrnAction {
    UpdatedConfig,
    OpenLoad,
    OpenSave,
    RequestExit,
    RequestTitle,
    Close,
}

/// Interactive state for `confscrn.lwg` (`0x40df80`).
#[derive(Debug, Clone, Default)]
pub struct ConfScrnOverlay {
    pub active: bool,
    pub return_to_main_game: bool,
    pub hovered_control: Option<&'static str>,
    pub dragging_slider: Option<ConfigSliderKind>,
}

impl ConfScrnOverlay {
    const SLIDER_TRACK_X: i32 = 463;
    const SLIDER_TRACK_W: i32 = 242;
    const SLIDER_KNOB_HALF: i32 = 11;
    const SLIDER_TRAVEL: i32 = 220;

    fn slider_rect(y: i32) -> UiRect {
        UiRect::new(Self::SLIDER_TRACK_X, y, Self::SLIDER_TRACK_W, 22)
    }

    fn slider_value_from_x(px: i32, max_val: u32) -> u32 {
        let rel = (px - Self::SLIDER_TRACK_X - Self::SLIDER_KNOB_HALF)
            .clamp(0, Self::SLIDER_TRAVEL) as u32;
        ((rel * max_val + (Self::SLIDER_TRAVEL as u32 / 2)) / Self::SLIDER_TRAVEL as u32)
            .min(max_val)
    }

    fn slider_knob_x(val: u32, max_val: u32) -> i32 {
        Self::SLIDER_TRACK_X
            + ((val.min(max_val) * Self::SLIDER_TRAVEL as u32) / max_val.max(1)) as i32
    }

    fn hit_control(lwg: Option<&LwgImage>, px: i32, py: i32) -> Option<&'static str> {
        let controls: [(&str, UiRect); 24] = [
            ("scm_wnd", UiRect::new(382, 57, 120, 26)),
            ("scm_ful", UiRect::new(582, 51, 178, 38)),
            ("bgm_on", UiRect::new(224, 96, 47, 38)),
            ("bgm_off", UiRect::new(297, 96, 48, 38)),
            ("sef_on", UiRect::new(224, 141, 47, 38)),
            ("sef_off", UiRect::new(297, 141, 48, 38)),
            ("voc_on", UiRect::new(224, 186, 47, 38)),
            ("voc_off", UiRect::new(297, 186, 48, 38)),
            ("gef_on", UiRect::new(224, 231, 47, 38)),
            ("gef_off", UiRect::new(297, 231, 48, 38)),
            ("vocst_on", UiRect::new(595, 231, 71, 38)),
            ("vocst_off", UiRect::new(714, 231, 49, 38)),
            ("msp_slw", UiRect::new(370, 276, 99, 38)),
            ("msp_nom", UiRect::new(521, 276, 50, 38)),
            ("msp_now", UiRect::new(614, 276, 151, 38)),
            ("msk_on", UiRect::new(327, 321, 206, 38)),
            ("msk_off", UiRect::new(560, 321, 204, 38)),
            ("load", UiRect::new(444, 517, 62, 66)),
            ("save", UiRect::new(508, 517, 62, 66)),
            ("exit", UiRect::new(572, 517, 62, 66)),
            ("title", UiRect::new(636, 517, 62, 66)),
            ("close", UiRect::new(723, 517, 62, 66)),
            ("bgm_lev", Self::slider_rect(104)),
            ("sef_lev", Self::slider_rect(149)),
        ];
        for (name, fb) in controls {
            let r = lwg_layer_rect(lwg, name, 0, 0, fb);
            if r.contains(px, py) {
                return Some(name);
            }
        }
        if lwg_layer_rect(lwg, "voc_lev", 0, 0, Self::slider_rect(194)).contains(px, py) {
            return Some("voc_lev");
        }
        if lwg_layer_rect(lwg, "auto_lev", 0, 0, Self::slider_rect(374)).contains(px, py) {
            return Some("auto_lev");
        }
        None
    }

    pub fn handle_move(
        &mut self,
        lwg: Option<&LwgImage>,
        cfg: &mut ConfigBlock2007,
        px: i32,
        py: i32,
    ) -> bool {
        if let Some(sl) = self.dragging_slider {
            Self::apply_slider(sl, px, cfg);
            return true;
        }
        self.hovered_control = Self::hit_control(lwg, px, py);
        false
    }

    fn apply_slider(sl: ConfigSliderKind, px: i32, cfg: &mut ConfigBlock2007) {
        match sl {
            ConfigSliderKind::BgmVol => {
                cfg.set_bgm_volume(Self::slider_value_from_x(px, 255) as u8);
            }
            ConfigSliderKind::SeVol => {
                cfg.set_se_volume(Self::slider_value_from_x(px, 255) as u8);
            }
            ConfigSliderKind::VoiceVol => {
                cfg.set_voice_volume(Self::slider_value_from_x(px, 255) as u8);
            }
            ConfigSliderKind::AutoWait => {
                cfg.set_auto_wait_ms(Self::slider_value_from_x(px, 3000) as u16);
            }
        }
    }

    pub fn handle_down(&mut self, lwg: Option<&LwgImage>, cfg: &mut ConfigBlock2007, px: i32, py: i32) -> bool {
        let hit = Self::hit_control(lwg, px, py);
        self.hovered_control = hit;
        let sl = match hit {
            Some("bgm_lev") => Some(ConfigSliderKind::BgmVol),
            Some("sef_lev") => Some(ConfigSliderKind::SeVol),
            Some("voc_lev") => Some(ConfigSliderKind::VoiceVol),
            Some("auto_lev") => Some(ConfigSliderKind::AutoWait),
            _ => None,
        };
        if let Some(s) = sl {
            self.dragging_slider = Some(s);
            Self::apply_slider(s, px, cfg);
            return true;
        }
        false
    }

    pub fn handle_up(
        &mut self,
        lwg: Option<&LwgImage>,
        cfg: &mut ConfigBlock2007,
        px: i32,
        py: i32,
    ) -> Option<ConfScrnAction> {
        if let Some(sl) = self.dragging_slider.take() {
            Self::apply_slider(sl, px, cfg);
            return Some(ConfScrnAction::UpdatedConfig);
        }
        let hit = Self::hit_control(lwg, px, py)?;
        match hit {
            "scm_wnd" => cfg.set_fullscreen(false),
            "scm_ful" => cfg.set_fullscreen(true),
            "bgm_on" => cfg.set_bgm_enable(true),
            "bgm_off" => cfg.set_bgm_enable(false),
            "sef_on" => cfg.set_se_enable(true),
            "sef_off" => cfg.set_se_enable(false),
            "voc_on" => cfg.set_voice_enable(true),
            "voc_off" => cfg.set_voice_enable(false),
            "gef_on" => cfg.set_effect_enable(true),
            "gef_off" => cfg.set_effect_enable(false),
            "vocst_on" => cfg.set_voice_cut_on_click(true),
            "vocst_off" => cfg.set_voice_cut_on_click(false),
            "msp_slw" => cfg.set_text_speed(4),
            "msp_nom" => cfg.set_text_speed(1),
            "msp_now" => cfg.set_text_speed(0),
            "msk_on" => cfg.set_skip_unread(true),
            "msk_off" => cfg.set_skip_unread(false),
            "bgm_lev" => Self::apply_slider(ConfigSliderKind::BgmVol, px, cfg),
            "sef_lev" => Self::apply_slider(ConfigSliderKind::SeVol, px, cfg),
            "voc_lev" => Self::apply_slider(ConfigSliderKind::VoiceVol, px, cfg),
            "auto_lev" => Self::apply_slider(ConfigSliderKind::AutoWait, px, cfg),
            "load" => return Some(ConfScrnAction::OpenLoad),
            "save" => return Some(ConfScrnAction::OpenSave),
            "exit" => return Some(ConfScrnAction::RequestExit),
            "title" => return Some(ConfScrnAction::RequestTitle),
            "close" => return Some(ConfScrnAction::Close),
            _ => return None,
        }
        Some(ConfScrnAction::UpdatedConfig)
    }

    pub fn render(&self, dst: &mut Surface, lwg: Option<&LwgImage>, cfg: &ConfigBlock2007) {
        let Some(img) = lwg else {
            filter_fill_argb(dst, 0xFF10_141C);
            return;
        };
        blit_lwg_layer_at(dst, img, "bg", 0, 0, 255);

        let toggles: [(&str, &str, bool); 15] = [
            ("scm_wnd", "scm_wnd_f", !cfg.fullscreen()),
            ("scm_ful", "scm_ful_f", cfg.fullscreen()),
            ("bgm_on", "bgm_on_f", cfg.bgm_enable()),
            ("bgm_off", "bgm_off_f", !cfg.bgm_enable()),
            ("sef_on", "sef_on_f", cfg.se_enable()),
            ("sef_off", "sef_off_f", !cfg.se_enable()),
            ("voc_on", "voc_on_f", cfg.voice_enable()),
            ("voc_off", "voc_off_f", !cfg.voice_enable()),
            ("gef_on", "gef_on_f", cfg.effect_enable()),
            ("gef_off", "gef_off_f", !cfg.effect_enable()),
            ("vocst_on", "vocst_on_f", cfg.voice_cut_on_click()),
            ("vocst_off", "vocst_off_f", !cfg.voice_cut_on_click()),
            ("msp_slw", "msp_slw_f", cfg.text_speed() == 4),
            ("msp_nom", "msp_nom_f", cfg.text_speed() == 1),
            ("msp_now", "msp_now_f", cfg.text_speed() == 0),
        ];
        for (norm, focus, selected) in toggles {
            let hov = self.hovered_control == Some(norm);
            let name = if selected || hov { focus } else { norm };
            blit_lwg_layer_at(dst, img, name, 0, 0, 255);
        }
        for (norm, focus, selected) in [
            ("msk_on", "msk_on_f", cfg.skip_unread()),
            ("msk_off", "msk_off_f", !cfg.skip_unread()),
        ] {
            let name = if selected || self.hovered_control == Some(norm) {
                focus
            } else {
                norm
            };
            blit_lwg_layer_at(dst, img, name, 0, 0, 255);
        }

        // Tracks & volume knobs
        for (track, knob, knob_f, val, max_v, sl_kind) in [
            ("bgm_lev", "bgm_vol", "bgm_vol_f", u32::from(cfg.bgm_volume()), 255u32, ConfigSliderKind::BgmVol),
            ("sef_lev", "sef_vol", "sef_vol_f", u32::from(cfg.se_volume()), 255u32, ConfigSliderKind::SeVol),
            ("voc_lev", "voc_vol", "voc_vol_f", u32::from(cfg.voice_volume()), 255u32, ConfigSliderKind::VoiceVol),
            ("auto_lev", "auto_vol", "auto_vol_f", u32::from(cfg.auto_wait_ms()), 3000u32, ConfigSliderKind::AutoWait),
        ] {
            blit_lwg_layer_at(dst, img, track, 0, 0, 255);
            let k_name = if self.dragging_slider == Some(sl_kind) || self.hovered_control == Some(track) {
                knob_f
            } else {
                knob
            };
            if let Some(layer) = img.find_layer(k_name) {
                let kx = Self::slider_knob_x(val, max_v);
                let params = BlitParams {
                    blend_mode: 0,
                    alpha_channel_mode: 0,
                    ..Default::default()
                };
                blit(dst, &layer.surface, &params, (kx, layer.y), None);
            }
        }

        // Bottom navigation buttons
        for (norm, focus) in [
            ("load", "load_f"),
            ("save", "save_f"),
            ("exit", "exit_f"),
            ("title", "title_f"),
            ("close", "close_f"),
        ] {
            let disabled = norm == "save" && !self.return_to_main_game;
            let name = if !disabled && self.hovered_control == Some(norm) {
                focus
            } else {
                norm
            };
            blit_lwg_layer_at(dst, img, name, 0, 0, if disabled { 110 } else { 255 });
        }
    }
}

// ---------------------------------------------------------------------------
// 3. `savescrn.lwg` Save / Load Overlay (`10 pages x 10 slots = 100 slots`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveScrnMode {
    Load,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveScrnAction {
    PageChanged(usize),
    SlotSelected { slot_num: u16, occupied: bool },
    Close,
}

#[derive(Debug, Clone)]
pub struct SaveScrnOverlay {
    pub active: bool,
    pub mode: SaveScrnMode,
    pub page: usize, // 0..10 (slots page*10 + 1 ..= page*10 + 10)
    pub hovered_btn: Option<&'static str>,
    pub cached_headers: [Option<SlotSaveHeader2007>; 10],
}

impl Default for SaveScrnOverlay {
    fn default() -> Self {
        Self {
            active: false,
            mode: SaveScrnMode::Load,
            page: 0,
            hovered_btn: None,
            cached_headers: std::array::from_fn(|_| None),
        }
    }
}

impl SaveScrnOverlay {
    const SLOT_NAMES: [&'static str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

    pub fn slot_rect_fallback(idx: usize) -> UiRect {
        let x = if idx < 5 { 54 } else { 425 };
        let y = 56 + 90 * (idx % 5);
        UiRect::new(x, y as i32, 321, 78)
    }

    pub fn refresh_headers(&mut self, save_dir: &Path) {
        for i in 0..10 {
            let slot_num = (self.page * 10 + i + 1) as u16;
            let path = save_dir.join(format!("KhDLsave{slot_num:02}.dat"));
            self.cached_headers[i] = if path.is_file() {
                SlotSave2007::load_file(&path).ok().map(|s| s.header)
            } else {
                None
            };
        }
    }

    fn hit_test(&self, lwg: Option<&LwgImage>, px: i32, py: i32) -> Option<&'static str> {
        for (name, fb) in [
            ("prev", UiRect::new(338, 551, 50, 32)),
            ("next", UiRect::new(412, 551, 50, 32)),
            ("exit", UiRect::new(723, 517, 62, 66)),
        ] {
            if lwg_layer_rect(lwg, name, 0, 0, fb).contains(px, py) {
                return Some(name);
            }
        }
        for (i, &name) in Self::SLOT_NAMES.iter().enumerate() {
            if lwg_layer_rect(lwg, name, 0, 0, Self::slot_rect_fallback(i)).contains(px, py) {
                return Some(name);
            }
        }
        None
    }

    pub fn handle_move(&mut self, lwg: Option<&LwgImage>, px: i32, py: i32) {
        self.hovered_btn = self.hit_test(lwg, px, py);
    }

    pub fn handle_up(&mut self, lwg: Option<&LwgImage>, px: i32, py: i32) -> Option<SaveScrnAction> {
        let hit = self.hit_test(lwg, px, py)?;
        match hit {
            "exit" => Some(SaveScrnAction::Close),
            "prev" => {
                self.page = (self.page + 9) % 10;
                Some(SaveScrnAction::PageChanged(self.page))
            }
            "next" => {
                self.page = (self.page + 1) % 10;
                Some(SaveScrnAction::PageChanged(self.page))
            }
            other => {
                let idx = other.parse::<usize>().ok()?;
                if idx < 10 {
                    let slot_num = (self.page * 10 + idx + 1) as u16;
                    let occupied = self.cached_headers[idx].is_some();
                    Some(SaveScrnAction::SlotSelected { slot_num, occupied })
                } else {
                    None
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        dst: &mut Surface,
        lwg: Option<&LwgImage>,
        saveconf: Option<&LwgImage>,
        nonbl: Option<&LwgImage>,
        fonts: Option<&FontSet>,
        vfs: Option<&AssetVfs>,
        gaiji: &mut crate::text::caret::GaijiBank,
        encoding: TextEncoding,
    ) {
        let Some(img) = lwg else {
            filter_fill_argb(dst, 0xFF10_141C);
            return;
        };
        let bg = match self.mode {
            SaveScrnMode::Load => "bg_load",
            SaveScrnMode::Save => "bg_save",
        };
        blit_lwg_layer_at(dst, img, bg, 0, 0, 255);

        for (norm, focus) in [("prev", "prev_f"), ("next", "next_f"), ("exit", "exit_f")] {
            let name = if self.hovered_btn == Some(norm) {
                focus
            } else {
                norm
            };
            blit_lwg_layer_at(dst, img, name, 0, 0, 255);
        }

        // Page digit (0..9 -> displays `nonbl` layer or digit)
        if let Some(nb) = nonbl {
            let num_rect = lwg_layer_rect(Some(img), "number", 0, 0, UiRect::new(386, 551, 28, 32));
            let digit_name = self.page.to_string();
            if let Some(dl) = nb.find_layer(&digit_name).or_else(|| nb.layers.get(self.page)) {
                let params = BlitParams {
                    blend_mode: 0,
                    alpha_channel_mode: 0,
                    ..Default::default()
                };
                blit(dst, &dl.surface, &params, (num_rect.x, num_rect.y), None);
            }
        }

        for (i, &slot_name) in Self::SLOT_NAMES.iter().enumerate() {
            let r = lwg_layer_rect(Some(img), slot_name, 0, 0, Self::slot_rect_fallback(i));
            blit_lwg_layer_at(dst, img, slot_name, 0, 0, 255);
            if let Some(hdr) = &self.cached_headers[i] {
                if let Some(sc) = saveconf {
                    if let Some(first) = sc.layers.first() {
                        let params = BlitParams {
                            blend_mode: 0,
                            alpha_channel_mode: 0,
                            ..Default::default()
                        };
                        blit(dst, &first.surface, &params, (r.x, r.y), None);
                    }
                }
                if let Some(fs) = fonts {
                    // Story preview (`19px`, DBCS-truncated 26 bytes inside slot card)
                    let title_bytes = hdr.title_bytes_trimmed();
                    if !title_bytes.is_empty() {
                        render_dbcs_text(
                            dst,
                            fs,
                            vfs,
                            gaiji,
                            title_bytes,
                            encoding,
                            19,
                            0x00FF_FFFF,
                            r.x + 16,
                            r.y + 14,
                            r.w - 32,
                            36,
                        );
                    }
                    // Timestamp (`%04d/%02d/%02d %02d:%02d` at `saveconf` date `(184, 54)`).
                    let date_pos = saveconf
                        .and_then(|sc| sc.find_layer("date"))
                        .map(|l| (r.x + l.x, r.y + l.y))
                        .unwrap_or((r.x + 184, r.y + 54));
                    let date_str = format!(
                        "{:04}/{:02}/{:02} {:02}:{:02}",
                        hdr.year, hdr.month, hdr.day, hdr.hour, hdr.minute
                    );
                    render_ascii_or_utf8_label(
                        dst,
                        fs,
                        vfs,
                        gaiji,
                        &date_str,
                        encoding,
                        13,
                        0x00E0_E8F0,
                        date_pos.0,
                        date_pos.1,
                        135,
                        22,
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 4. `select_6` (`sel_a01.lwg` / `sel_q01.lwg`) & `selmap` (`selmap01.lwg`)
// ---------------------------------------------------------------------------

/// `select_6` (`0x0E`) choice overlay state & hit-testing.
#[derive(Debug, Clone, Default)]
pub struct Select6Overlay {
    pub hovered: Option<usize>,
    pub keyboard_focus: usize,
}

impl Select6Overlay {
    pub fn choice_rect(count: usize, idx: usize) -> UiRect {
        let total_h = (count as i32) * 72;
        let top_y = ((GAME_HEIGHT as i32 - 144 - total_h) / 2).max(24);
        UiRect::new(75, top_y + (idx as i32) * 72, 650, 70)
    }

    pub fn body_hitbox(count: usize, idx: usize) -> UiRect {
        let base = Self::choice_rect(count, idx);
        UiRect::new(base.x + 18, base.y + 12, 614, 46)
    }

    pub fn hit_test(&self, count: usize, px: i32, py: i32) -> Option<usize> {
        (0..count).find(|&i| Self::body_hitbox(count, i).contains(px, py))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        dst: &mut Surface,
        prompt: &ChoicePrompt,
        sel_a01: Option<&LwgImage>,
        sel_q01: Option<&LwgImage>,
        fonts: Option<&FontSet>,
        vfs: Option<&AssetVfs>,
        gaiji: &mut crate::text::caret::GaijiBank,
        encoding: TextEncoding,
    ) {
        let count = prompt.choices.len();
        if let Some(q) = sel_q01 {
            if !prompt.prompt_raw.is_empty() {
                let qx = (GAME_WIDTH as i32 - q.width as i32) / 2;
                let qy = (Self::choice_rect(count, 0).y - q.height as i32 - 8).max(8);
                blit_lwg_layer_at(dst, q, "body", qx, qy, 255);
                if let Some(fs) = fonts {
                    let tr = lwg_layer_rect(Some(q), "text", qx, qy, UiRect::new(qx + 55, qy + 17, 540, 36));
                    render_dbcs_text(
                        dst, fs, vfs, gaiji, &prompt.prompt_raw, encoding, 22, 0x00FF_FFFF, tr.x, tr.y, tr.w, tr.h,
                    );
                }
            }
        }
        for (i, (text_bytes, _)) in prompt.choices.iter().enumerate() {
            let base = Self::choice_rect(count, i);
            let focused = self.hovered == Some(i) || (self.hovered.is_none() && self.keyboard_focus == i);
            if let Some(a) = sel_a01 {
                let layer_name = if focused { "body_f" } else { "body" };
                blit_lwg_layer_at(dst, a, layer_name, base.x, base.y, 255);
            }
            if let Some(fs) = fonts {
                let tr = lwg_layer_rect(sel_a01, "text", base.x, base.y, UiRect::new(base.x + 55, base.y + 17, 540, 36));
                render_dbcs_text(
                    dst, fs, vfs, gaiji, text_bytes, encoding, 22, 0x00FF_FFFF, tr.x, tr.y, tr.w, tr.h,
                );
            }
        }
    }
}

/// Interactive `0xD5` `selmap01.lwg` overlay (`header@(28,68)`, `select0..4@(119,142+63*i)` `614x46`).
#[derive(Debug, Clone, Default)]
pub struct SelMapOverlay {
    pub active: bool,
    pub mode: i16,
    pub param2: i16,
    pub unused: i16,
    pub hovered: Option<usize>,
    pub keyboard_focus: usize,
    pub last_selected: Option<usize>,
}

impl SelMapOverlay {
    const NAMES: [&'static str; 5] = ["select0", "select1", "select2", "select3", "select4"];

    pub fn slot_rect(lwg: Option<&LwgImage>, idx: usize) -> UiRect {
        let fb = UiRect::new(119, 142 + 63 * (idx as i32), 614, 46);
        lwg_layer_rect(lwg, Self::NAMES[idx], 0, 0, fb)
    }

    pub fn hit_test(&self, lwg: Option<&LwgImage>, px: i32, py: i32) -> Option<usize> {
        if !self.active {
            return None;
        }
        (0..5).find(|&i| Self::slot_rect(lwg, i).contains(px, py))
    }

    pub fn render(&self, dst: &mut Surface, lwg: Option<&LwgImage>) {
        if !self.active {
            return;
        }
        let Some(img) = lwg else {
            return;
        };
        blit_lwg_layer_at(dst, img, "header", 0, 0, 255);
        for (i, &name) in Self::NAMES.iter().enumerate() {
            let focused = self.hovered == Some(i) || (self.hovered.is_none() && self.keyboard_focus == i);
            let alpha = if focused { 255 } else { 200 };
            blit_lwg_layer_at(dst, img, name, 0, 0, alpha);
            if focused {
                let r = Self::slot_rect(Some(img), i);
                if let Ok(hl) = Surface::filled(r.w as u32, r.h as u32, 0xD038_6496) {
                    let params = BlitParams {
                        blend_mode: 2,
                        alpha_channel_mode: 6,
                        global_alpha: 64,
                        ..Default::default()
                    };
                    blit(dst, &hl, &params, (r.x, r.y), None);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Backlog Viewer Overlay (`0x414e40`, capacity 200)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklogAction {
    Close,
    ReplayVoice(u16),
    Scrolled,
}

#[derive(Debug, Clone, Default)]
pub struct BacklogOverlay {
    pub active: bool,
    /// Offset in entries from the bottom (newest). `0` = showing newest entries.
    pub scroll_from_bottom: usize,
    pub last_replayed_voice_id: Option<u16>,
}

impl BacklogOverlay {
    const ROWS_PER_VIEW: usize = 7;
    const ROW_X: i32 = 48;
    const ROW_Y0: i32 = 48;
    const ROW_W: i32 = 704;
    const ROW_H: i32 = 72;

    pub fn max_scroll(len: usize) -> usize {
        len.saturating_sub(Self::ROWS_PER_VIEW)
    }

    pub fn scroll_up(&mut self, len: usize) {
        self.scroll_from_bottom = (self.scroll_from_bottom + 1).min(Self::max_scroll(len));
    }

    pub fn scroll_down(&mut self) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(1);
    }

    fn visible_slice<'a>(
        &self,
        entries: &'a std::collections::VecDeque<BacklogEntry>,
    ) -> Vec<(usize, &'a BacklogEntry)> {
        let len = entries.len();
        let end = len.saturating_sub(self.scroll_from_bottom);
        let start = end.saturating_sub(Self::ROWS_PER_VIEW);
        (start..end).filter_map(|i| entries.get(i).map(|e| (i, e))).collect()
    }

    pub fn handle_up(
        &mut self,
        entries: &std::collections::VecDeque<BacklogEntry>,
        px: i32,
        py: i32,
    ) -> Option<BacklogAction> {
        let close_rect = UiRect::new(720, 16, 64, 32);
        if close_rect.contains(px, py) {
            return Some(BacklogAction::Close);
        }
        let rows = self.visible_slice(entries);
        for (row_idx, (_abs_idx, entry)) in rows.into_iter().enumerate() {
            let r = UiRect::new(
                Self::ROW_X,
                Self::ROW_Y0 + (row_idx as i32) * Self::ROW_H,
                Self::ROW_W,
                Self::ROW_H - 6,
            );
            if r.contains(px, py) && entry.voice_id != 0 {
                self.last_replayed_voice_id = Some(entry.voice_id);
                return Some(BacklogAction::ReplayVoice(entry.voice_id));
            }
        }
        None
    }

    pub fn render(
        &self,
        dst: &mut Surface,
        entries: &std::collections::VecDeque<BacklogEntry>,
        fonts: Option<&FontSet>,
        vfs: Option<&AssetVfs>,
        gaiji: &mut crate::text::caret::GaijiBank,
        encoding: TextEncoding,
    ) {
        if !self.active {
            return;
        }
        if let Ok(backdrop) = Surface::filled(GAME_WIDTH, GAME_HEIGHT, 0x000A_0D14) {
            let params = BlitParams {
                blend_mode: 2,
                alpha_channel_mode: 6,
                global_alpha: 210,
                ..Default::default()
            };
            blit(dst, &backdrop, &params, (0, 0), None);
        }
        let Some(fs) = fonts else {
            return;
        };
        let rows = self.visible_slice(entries);
        for (row_idx, (_abs_idx, entry)) in rows.into_iter().enumerate() {
            let ry = Self::ROW_Y0 + (row_idx as i32) * Self::ROW_H;
            if entry.voice_id != 0 {
                render_ascii_or_utf8_label(
                    dst, fs, vfs, gaiji, "[VOICE]", encoding, 14, 0x00FF_DE00, 52, ry, 72, 24,
                );
            }
            render_dbcs_text(
                dst,
                fs,
                vfs,
                gaiji,
                &entry.raw_bytes,
                encoding,
                20,
                0x00FF_FFFF,
                130,
                ry,
                610,
                Self::ROW_H - 8,
            );
        }
    }
}
