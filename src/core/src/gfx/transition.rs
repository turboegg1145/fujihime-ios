//! Full screen transition dispatcher (`KhimeDL.exe` `0x422060`, `0x448170`..`0x448c70`).
//!
//! Implements all transition IDs `0..=299` (and `>= 300` no-op):
//! - `0`: Instant cut (`0x4461b0`)
//! - `1`: Crossfade (`0x4483f0`)
//! - `2`: 4x4 Ordered Dither (`0x4482c0`)
//! - `3`: Fade through white (`0x448c70`)
//! - `4`: Fade through black (`0x448a00`)
//! - `5..=99`: `.msk` rule wipe (`0x448170`, assets `ef14..19.msk`, `ef21..26.msk`)
//! - `100..=199`: Focal zoom-in 50% blend (`0x448540`)
//! - `200..=299`: Focal zoom-out to black (`0x4486c0`)
//! - `>= 300`: No-op (`0x422132 -> 0x422286`)

use crate::error::Result;
use crate::gfx::compositor::{blit, filter_fill_argb, stretch_blit_srccopy, BlitParams, Canvas800x600};
use crate::gfx::decode::AlphaMask;
use crate::gfx::surface::{GAME_HEIGHT, GAME_WIDTH};
use crate::vfs::AssetVfs;

/// Decoded transition family for an `effect_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Instant,
    Crossfade,
    OrderedDither,
    FadeThroughWhite,
    FadeThroughBlack,
    RuleMask { mask_id: u16 },
    ZoomHalfBlend { p1: u8, p2: u8 },
    ZoomBlack { p1: u8, p2: u8 },
    NoOp,
}

impl TransitionKind {
    /// Classify an `effect_id` matching `FUN_00422060`.
    pub const fn from_id(effect_id: u16) -> Self {
        match effect_id {
            0 => Self::Instant,
            1 => Self::Crossfade,
            2 => Self::OrderedDither,
            3 => Self::FadeThroughWhite,
            4 => Self::FadeThroughBlack,
            5..=99 => Self::RuleMask { mask_id: effect_id },
            100..=199 => Self::ZoomHalfBlend {
                p1: ((effect_id % 100) / 10) as u8,
                p2: (effect_id % 10) as u8,
            },
            200..=299 => Self::ZoomBlack {
                p1: ((effect_id % 100) / 10) as u8,
                p2: (effect_id % 10) as u8,
            },
            _ => Self::NoOp,
        }
    }
}

/// Map a numpad-layout focal digit (`0..=9`) to screen coordinates (`FUN_00421740`).
pub const fn focal_grid_point(digit: u8, width: i32, height: i32) -> (i32, i32) {
    match digit {
        7 => (0, 0),
        8 => (width / 2, 0),
        9 => (width, 0),
        4 => (0, height / 2),
        6 => (width, height / 2),
        1 => (0, height),
        2 => (width / 2, height),
        3 => (width, height),
        _ => (width / 2, height / 2), // 5, 0, or fallback
    }
}

/// Compute clamped zoom rectangle `(x0, y0, crop_w, crop_h)` matching `0x446440` / `0x446590`.
pub fn zoom_crop_rect(z: u32, cx: i32, cy: i32, width: i32, height: i32) -> (i32, i32, i32, i32) {
    let denom = (z as i32) + 100;
    let crop_w = (width * 100) / denom;
    let crop_h = (height * 100) / denom;
    let x0 = (cx - crop_w / 2).clamp(0, width - crop_w);
    let y0 = (cy - crop_h / 2).clamp(0, height - crop_h);
    (x0, y0, crop_w, crop_h)
}

#[derive(Debug, Clone)]
enum RunnerState {
    InstantPending,
    Crossfade {
        step: u32,
        total_steps: u32,
        delay_ms: u16,
        final_pending: bool,
    },
    Dither {
        step: u32,
        total_steps: u32,
        delay_ms: u16,
        final_pending: bool,
    },
    FadeTwoPhase {
        white: bool,
        phase: u8,
        step: u32,
        total_steps: u32,
        delay_ms: u16,
        final_pending: bool,
    },
    RuleMask {
        step: u32,
        total_n: u32,
        delay_ms: u16,
        mask: Option<AlphaMask>,
        prepared: bool,
        final_pending: bool,
    },
    ZoomHalfBlend {
        p1: (i32, i32),
        p2: (i32, i32),
        phase: u8,
        z: u32,
        prepared: bool,
        final_pending: bool,
    },
    ZoomBlack {
        p1: (i32, i32),
        p2: (i32, i32),
        phase: u8,
        z: u32,
        prepared: bool,
        final_pending: bool,
    },
    Done,
}

/// Step-by-step deterministic screen transition runner for [`Canvas800x600`].
///
/// Each call to [`TransitionRunner::step`] renders one frame into `canvas.front` and
/// returns `Some(delay_ms)` for that frame, or `None` once the transition is complete.
#[derive(Debug, Clone)]
pub struct TransitionRunner {
    kind: TransitionKind,
    state: RunnerState,
}

impl TransitionRunner {
    /// Create a transition runner with an optional pre-decoded `.msk` rule mask.
    ///
    /// Matches `FUN_00422060(effect_id, steps, delay_ms)` (`steps == 0` defaults to `10`).
    pub fn new(effect_id: u16, steps: u16, delay_ms: u16, mask: Option<AlphaMask>) -> Self {
        let kind = TransitionKind::from_id(effect_id);
        let steps_u32 = if steps == 0 { 10 } else { steps as u32 };
        let w = GAME_WIDTH as i32;
        let h = GAME_HEIGHT as i32;

        let state = match kind {
            TransitionKind::Instant => RunnerState::InstantPending,
            TransitionKind::Crossfade => RunnerState::Crossfade {
                step: 0,
                total_steps: steps_u32,
                delay_ms,
                final_pending: true,
            },
            TransitionKind::OrderedDither => RunnerState::Dither {
                step: 0,
                total_steps: steps_u32,
                delay_ms,
                final_pending: true,
            },
            TransitionKind::FadeThroughWhite => RunnerState::FadeTwoPhase {
                white: true,
                phase: 1,
                step: 0,
                total_steps: steps_u32,
                delay_ms,
                final_pending: true,
            },
            TransitionKind::FadeThroughBlack => RunnerState::FadeTwoPhase {
                white: false,
                phase: 1,
                step: 0,
                total_steps: steps_u32,
                delay_ms,
                final_pending: true,
            },
            TransitionKind::RuleMask { .. } => RunnerState::RuleMask {
                step: 1,
                total_n: steps_u32 * 2,
                delay_ms,
                mask,
                prepared: false,
                final_pending: true,
            },
            TransitionKind::ZoomHalfBlend { p1, p2 } => RunnerState::ZoomHalfBlend {
                p1: focal_grid_point(p1, w, h),
                p2: focal_grid_point(p2, w, h),
                phase: 1,
                z: 0,
                prepared: false,
                final_pending: true,
            },
            TransitionKind::ZoomBlack { p1, p2 } => RunnerState::ZoomBlack {
                p1: focal_grid_point(p1, w, h),
                p2: focal_grid_point(p2, w, h),
                phase: 1,
                z: 0,
                prepared: false,
                final_pending: true,
            },
            TransitionKind::NoOp => RunnerState::Done,
        };

        Self { kind, state }
    }

    /// Create a transition runner, loading `ef%02d.msk` from [`AssetVfs`] when `effect_id` is in `5..=99`.
    ///
    /// If the specific mask ID does not exist in `grps`, matches `KhimeDL.exe` by falling back
    /// cleanly to `mask = None` (finishes with `new_scene` commit).
    pub fn with_vfs(
        effect_id: u16,
        steps: u16,
        delay_ms: u16,
        vfs: &AssetVfs,
    ) -> Result<Self> {
        let mask = if (5..=99).contains(&effect_id) {
            match vfs.read_transition_mask(effect_id) {
                Ok(raw) => Some(crate::gfx::decode::decode_msk(&raw)?),
                Err(_) => None,
            }
        } else {
            None
        };
        Ok(Self::new(effect_id, steps, delay_ms, mask))
    }

    /// Classified [`TransitionKind`] for this runner.
    pub fn kind(&self) -> TransitionKind {
        self.kind
    }

    /// Whether all transition frames have been rendered.
    pub fn is_done(&self) -> bool {
        matches!(self.state, RunnerState::Done)
    }

    /// Advance the transition by one frame on `canvas`.
    ///
    /// Returns `Some(delay_ms)` when a frame was produced in `canvas.front`, or `None` when done.
    pub fn step(&mut self, canvas: &mut Canvas800x600) -> Option<u16> {
        match &mut self.state {
            RunnerState::Done => None,

            RunnerState::InstantPending => {
                canvas.commit_new_scene();
                self.state = RunnerState::Done;
                Some(0)
            }

            RunnerState::Crossfade {
                step,
                total_steps,
                delay_ms,
                final_pending,
            } => {
                if *step == 0 {
                    canvas
                        .snapshot
                        .as_bytes_mut()
                        .copy_from_slice(canvas.front.as_bytes());
                    canvas
                        .staging
                        .as_bytes_mut()
                        .copy_from_slice(canvas.new_scene.as_bytes());
                }
                if *step <= *total_steps {
                    let ga = (((*total_steps - *step) * 255) / *total_steps) as i32;
                    canvas
                        .front
                        .as_bytes_mut()
                        .copy_from_slice(canvas.snapshot.as_bytes());
                    let params = BlitParams {
                        blend_mode: 2,
                        alpha_channel_mode: 6,
                        global_alpha: ga,
                        ..Default::default()
                    };
                    blit(&mut canvas.front, &canvas.staging, &params, (0, 0), None);
                    *step += 1;
                    Some(*delay_ms)
                } else if *final_pending {
                    canvas.commit_new_scene();
                    *final_pending = false;
                    self.state = RunnerState::Done;
                    Some(0)
                } else {
                    self.state = RunnerState::Done;
                    None
                }
            }

            RunnerState::Dither {
                step,
                total_steps,
                delay_ms,
                final_pending,
            } => {
                if *step == 0 {
                    canvas
                        .staging
                        .as_bytes_mut()
                        .copy_from_slice(canvas.new_scene.as_bytes());
                }
                let loop_frames = total_steps.saturating_sub(1);
                if *step < loop_frames {
                    let ga = (((*total_steps - *step) * 255) / *total_steps) as i32;
                    let params = BlitParams {
                        blend_mode: 0x12,
                        alpha_channel_mode: 6,
                        global_alpha: ga,
                        ..Default::default()
                    };
                    blit(&mut canvas.front, &canvas.staging, &params, (0, 0), None);
                    *step += 1;
                    Some(*delay_ms)
                } else if *final_pending {
                    canvas.commit_new_scene();
                    *final_pending = false;
                    self.state = RunnerState::Done;
                    Some(0)
                } else {
                    self.state = RunnerState::Done;
                    None
                }
            }

            RunnerState::FadeTwoPhase {
                white,
                phase,
                step,
                total_steps,
                delay_ms,
                final_pending,
            } => {
                let mode = if *white { 4 } else { 3 };
                if *phase == 1 {
                    if *step == 0 {
                        canvas
                            .staging
                            .as_bytes_mut()
                            .copy_from_slice(canvas.front.as_bytes());
                    }
                    let ga = ((*step * 255) / *total_steps) as i32;
                    let params = BlitParams {
                        blend_mode: mode,
                        alpha_channel_mode: 6,
                        global_alpha: ga,
                        ..Default::default()
                    };
                    blit(&mut canvas.front, &canvas.staging, &params, (0, 0), None);
                    if *step < *total_steps {
                        *step += 1;
                    } else {
                        *phase = 2;
                        *step = 0;
                    }
                    Some(*delay_ms)
                } else if *phase == 2 {
                    if *step == 0 {
                        canvas
                            .staging
                            .as_bytes_mut()
                            .copy_from_slice(canvas.new_scene.as_bytes());
                    }
                    let ga = (((*total_steps - *step) * 255) / *total_steps) as i32;
                    let params = BlitParams {
                        blend_mode: mode,
                        alpha_channel_mode: 6,
                        global_alpha: ga,
                        ..Default::default()
                    };
                    blit(&mut canvas.front, &canvas.staging, &params, (0, 0), None);
                    if *step < *total_steps {
                        *step += 1;
                    } else {
                        *phase = 3;
                    }
                    Some(*delay_ms)
                } else if *final_pending {
                    canvas.commit_new_scene();
                    *final_pending = false;
                    self.state = RunnerState::Done;
                    Some(0)
                } else {
                    self.state = RunnerState::Done;
                    None
                }
            }

            RunnerState::RuleMask {
                step,
                total_n,
                delay_ms,
                mask,
                prepared,
                final_pending,
            } => {
                if !*prepared {
                    canvas
                        .staging
                        .as_bytes_mut()
                        .copy_from_slice(canvas.new_scene.as_bytes());
                    if let Some(m) = mask.as_ref() {
                        let _ = m.apply_to_surface_alpha(&mut canvas.staging);
                    }
                    *prepared = true;
                }
                if *step < *total_n {
                    let threshold = ((*step * 255) / *total_n) as i32;
                    let params = BlitParams {
                        blend_mode: 0,
                        alpha_channel_mode: 2,
                        threshold,
                        ..Default::default()
                    };
                    blit(&mut canvas.front, &canvas.staging, &params, (0, 0), None);
                    *step += 1;
                    Some(*delay_ms)
                } else if *final_pending {
                    canvas.commit_new_scene();
                    *final_pending = false;
                    self.state = RunnerState::Done;
                    Some(0)
                } else {
                    self.state = RunnerState::Done;
                    None
                }
            }

            RunnerState::ZoomHalfBlend {
                p1,
                p2,
                phase,
                z,
                prepared,
                final_pending,
            } => {
                let w = GAME_WIDTH as i32;
                let h = GAME_HEIGHT as i32;
                if *phase == 1 {
                    if !*prepared {
                        canvas
                            .snapshot
                            .as_bytes_mut()
                            .copy_from_slice(canvas.front.as_bytes());
                        *prepared = true;
                    }
                    let src_rect = zoom_crop_rect(*z, p1.0, p1.1, w, h);
                    stretch_blit_srccopy(
                        &mut canvas.staging,
                        (0, 0, w, h),
                        &canvas.snapshot,
                        src_rect,
                    );
                    let params = BlitParams {
                        blend_mode: 8,
                        alpha_channel_mode: 6,
                        global_alpha: 255,
                        ..Default::default()
                    };
                    blit(&mut canvas.front, &canvas.staging, &params, (0, 0), None);
                    if *z < 600 {
                        *z += 20;
                    } else {
                        *phase = 2;
                        *z = 600;
                        *prepared = false;
                    }
                    Some(5)
                } else if *phase == 2 {
                    if !*prepared {
                        canvas
                            .snapshot
                            .as_bytes_mut()
                            .copy_from_slice(canvas.new_scene.as_bytes());
                        *prepared = true;
                    }
                    let src_rect = zoom_crop_rect(*z, p2.0, p2.1, w, h);
                    stretch_blit_srccopy(
                        &mut canvas.staging,
                        (0, 0, w, h),
                        &canvas.snapshot,
                        src_rect,
                    );
                    let params = BlitParams {
                        blend_mode: 8,
                        alpha_channel_mode: 6,
                        global_alpha: 255,
                        ..Default::default()
                    };
                    blit(&mut canvas.front, &canvas.staging, &params, (0, 0), None);
                    if *z > 0 {
                        *z -= 20;
                    } else {
                        *phase = 3;
                    }
                    Some(5)
                } else if *final_pending {
                    canvas.commit_new_scene();
                    *final_pending = false;
                    self.state = RunnerState::Done;
                    Some(0)
                } else {
                    self.state = RunnerState::Done;
                    None
                }
            }

            RunnerState::ZoomBlack {
                p1,
                p2,
                phase,
                z,
                prepared,
                final_pending,
            } => {
                let w = GAME_WIDTH as i32;
                let h = GAME_HEIGHT as i32;
                if *phase == 1 {
                    if !*prepared {
                        canvas
                            .snapshot
                            .as_bytes_mut()
                            .copy_from_slice(canvas.front.as_bytes());
                        *prepared = true;
                    }
                    filter_fill_argb(&mut canvas.front, 0);
                    let dst_rect = zoom_crop_rect(*z, p1.0, p1.1, w, h);
                    stretch_blit_srccopy(&mut canvas.front, dst_rect, &canvas.snapshot, (0, 0, w, h));
                    if *z < 2000 {
                        *z += 50;
                    } else {
                        *phase = 2;
                        *z = 2000;
                        *prepared = false;
                    }
                    Some(5)
                } else if *phase == 2 {
                    if !*prepared {
                        canvas
                            .snapshot
                            .as_bytes_mut()
                            .copy_from_slice(canvas.new_scene.as_bytes());
                        *prepared = true;
                    }
                    filter_fill_argb(&mut canvas.front, 0);
                    let dst_rect = zoom_crop_rect(*z, p2.0, p2.1, w, h);
                    stretch_blit_srccopy(&mut canvas.front, dst_rect, &canvas.snapshot, (0, 0, w, h));
                    if *z > 0 {
                        *z -= 50;
                    } else {
                        *phase = 3;
                    }
                    Some(5)
                } else if *final_pending {
                    canvas.commit_new_scene();
                    *final_pending = false;
                    self.state = RunnerState::Done;
                    Some(0)
                } else {
                    self.state = RunnerState::Done;
                    None
                }
            }
        }
    }

    /// Execute all remaining transition frames synchronously without sleeping.
    ///
    /// Returns the number of frames rendered.
    pub fn run_to_end(&mut self, canvas: &mut Canvas800x600) -> usize {
        let mut count = 0;
        while self.step(canvas).is_some() {
            count += 1;
        }
        count
    }
}
