//! DirectSound 4-slot software audio mixer (`KhimeDL.exe` `0x445aa0`, `0x448d00`..`0x449056`, `0x459a80`..`0x45a8a3`).
//!
//! Slots:
//! - BGM dual-slot ping-pong (`+0x1a8`, 2 alternating channels for crossfading and seamless looping)
//! - SE0 (`+0x1ac`, ambient/loopable sound effect)
//! - Voice (`+0x1b0`, delayed start `param_4 * 50 ms`, click-cut support)
//! - SE1 (`+0x1b4`, UI / transient sound effects)

use std::sync::Arc;

use crate::audio::vorbis::{DecodedAudio, AUDIO_SAMPLE_RATE};

/// DirectSound minimum volume constant (`-100 dB`, hard mute `gain = 0.0`).
pub const DSBVOLUME_MIN: i32 = -10_000;

/// Minimum attenuated volume used by `0x45a200` and fade curves (`-50 dB`).
pub const FADE_FLOOR_MB: i32 = -5_000;

/// Fixed fade timer period (`100 ms` = `0x64`, `0x459ae9`).
pub const FADE_TICK_MS: u32 = 100;

/// Number of stereo frames per `100 ms` fade tick at `44,100 Hz`.
pub const FADE_TICK_FRAMES: usize = (AUDIO_SAMPLE_RATE as usize * FADE_TICK_MS as usize) / 1000;

/// Number of stereo frames per `50 ms` voice-delay unit (`param_4 * 50 ms`).
pub const VOICE_DELAY_UNIT_FRAMES: usize = (AUDIO_SAMPLE_RATE as usize * 50) / 1000;

/// Default fade step count (`150` steps = `15.0 s`, `0x459ba7`: `0x96`).
pub const DEFAULT_FADE_STEPS: u32 = 150;

/// Convert `0..=255` SE/Voice volume to DirectSound millibels (`0x448d00` + `0x45a200` clamp).
///
/// Formula: `clamp(((vol - 255) * 5300) / 255, -5000, 0)`.
#[inline]
pub fn se_voice_vol_to_mb(vol: u8) -> i32 {
    let raw = ((i32::from(vol) - 255) * 5300) / 255;
    raw.clamp(FADE_FLOOR_MB, 0)
}

/// Convert `0..=255` BGM volume to DirectSound millibels (`0x448fd0` + `0x45a200` clamp).
///
/// Formula: `clamp(((vol - 255) * 3000) / 255, -5000, 0)`.
#[inline]
pub fn bgm_vol_to_mb(vol: u8) -> i32 {
    let raw = ((i32::from(vol) - 255) * 3000) / 255;
    raw.clamp(FADE_FLOOR_MB, 0)
}

/// Convert DirectSound millibels `mb ∈ [-10000, 0]` to linear amplitude gain.
///
/// - `mb <= -10000` (muted): `0.0`
/// - Otherwise: `10^(mb / 2000)`
#[inline]
pub fn mb_to_linear_gain(mb: i32) -> f32 {
    if mb <= DSBVOLUME_MIN {
        0.0
    } else {
        10.0f32.powf(mb as f32 / 2000.0)
    }
}

/// Compute fade-in millibels at `step` (`0..=total_steps`) toward `target_mb` (`0x45a809`).
#[inline]
pub fn fade_in_mb(target_mb: i32, step: u32, total_steps: u32) -> i32 {
    if total_steps == 0 || step >= total_steps {
        return target_mb;
    }
    ((target_mb - FADE_FLOOR_MB) * (step as i32)) / (total_steps as i32) + FADE_FLOOR_MB
}

/// Compute fade-out millibels at `step` (`0..=total_steps`) from `target_mb` (`0x45a840`).
#[inline]
pub fn fade_out_mb(target_mb: i32, step: u32, total_steps: u32) -> i32 {
    if total_steps == 0 || step >= total_steps {
        return FADE_FLOOR_MB;
    }
    let rem = (total_steps - step) as i32;
    ((target_mb - FADE_FLOOR_MB) * rem) / (total_steps as i32) + FADE_FLOOR_MB
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeDirection {
    FadeIn,
    FadeOut { stop_on_end: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FadeState {
    direction: FadeDirection,
    step: u32,
    total_steps: u32,
}

/// Playback mode for a single DirectSound-equivalent software channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMode {
    /// Play once (`loop_cnt == 0`).
    Once,
    /// Infinite loop (`999` or `-1` in `KhimeDL.exe`).
    Infinite,
    /// Play a fixed total number of times (`>= 1`).
    Repeat(u32),
}

impl LoopMode {
    /// Convert script `se_play` `loop_cnt` (`999` -> infinite, `0` -> once, `n` -> repeat `n`).
    pub fn from_script_loop_cnt(loop_cnt: i32) -> Self {
        if loop_cnt == 999 || loop_cnt < 0 {
            Self::Infinite
        } else if loop_cnt <= 1 {
            Self::Once
        } else {
            Self::Repeat(loop_cnt as u32)
        }
    }
}

/// Single software mixer channel modeling a DirectSound buffer + `100 ms` fade timer (`0x459a80`).
#[derive(Debug, Clone)]
pub struct MixerChannel {
    audio: Option<Arc<DecodedAudio>>,
    frame_cursor: usize,
    delay_frames_remaining: usize,
    loop_mode: LoopMode,
    remaining_plays: u32,
    enabled: bool,
    target_mb: i32,
    current_mb: i32,
    fade: Option<FadeState>,
    default_fade_steps: u32,
}

impl MixerChannel {
    fn new(target_mb: i32, enabled: bool) -> Self {
        Self {
            audio: None,
            frame_cursor: 0,
            delay_frames_remaining: 0,
            loop_mode: LoopMode::Once,
            remaining_plays: 0,
            enabled,
            target_mb,
            current_mb: if enabled { target_mb } else { DSBVOLUME_MIN },
            fade: None,
            default_fade_steps: DEFAULT_FADE_STEPS,
        }
    }

    /// Whether audio is currently active (playing or waiting on start delay).
    pub fn is_playing(&self) -> bool {
        self.audio.is_some()
    }

    /// Whether a fade-in or fade-out envelope is currently running.
    pub fn is_fading(&self) -> bool {
        self.fade.is_some()
    }

    /// Current effective DirectSound millibels (`-10000` when muted).
    pub fn current_mb(&self) -> i32 {
        if !self.enabled {
            DSBVOLUME_MIN
        } else {
            self.current_mb
        }
    }

    /// Current linear amplitude multiplier.
    pub fn current_gain(&self) -> f32 {
        mb_to_linear_gain(self.current_mb())
    }

    /// Target unmuted millibels.
    pub fn target_mb(&self) -> i32 {
        self.target_mb
    }

    /// Remaining start delay in frames (at 44,100 Hz).
    pub fn delay_frames_remaining(&self) -> usize {
        self.delay_frames_remaining
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.current_mb = DSBVOLUME_MIN;
        } else if self.fade.is_none() {
            self.current_mb = self.target_mb;
        }
    }

    fn set_target_mb(&mut self, mb: i32) {
        self.target_mb = mb.clamp(FADE_FLOOR_MB, 0);
        if self.enabled && self.fade.is_none() {
            self.current_mb = self.target_mb;
        }
    }

    fn play(
        &mut self,
        audio: Arc<DecodedAudio>,
        loop_mode: LoopMode,
        fade_in: bool,
        fade_steps: Option<u32>,
        delay_frames: usize,
    ) {
        let steps = match fade_steps {
            Some(s) if s > 0 => s,
            _ => self.default_fade_steps,
        };
        self.audio = Some(audio);
        self.frame_cursor = 0;
        self.delay_frames_remaining = delay_frames;
        self.loop_mode = loop_mode;
        self.remaining_plays = match loop_mode {
            LoopMode::Once => 1,
            LoopMode::Infinite => u32::MAX,
            LoopMode::Repeat(n) => n.max(1),
        };

        if fade_in && steps > 0 {
            self.fade = Some(FadeState {
                direction: FadeDirection::FadeIn,
                step: 0,
                total_steps: steps,
            });
            self.current_mb = if self.enabled {
                FADE_FLOOR_MB
            } else {
                DSBVOLUME_MIN
            };
        } else {
            self.fade = None;
            self.current_mb = if self.enabled {
                self.target_mb
            } else {
                DSBVOLUME_MIN
            };
        }
    }

    fn start_fade_out(&mut self, fade_steps: Option<u32>, stop_on_end: bool) {
        if self.audio.is_none() {
            return;
        }
        let steps = match fade_steps {
            Some(0) => {
                if stop_on_end {
                    self.stop();
                } else {
                    self.current_mb = if self.enabled {
                        FADE_FLOOR_MB
                    } else {
                        DSBVOLUME_MIN
                    };
                }
                return;
            }
            Some(s) => s,
            None => self.default_fade_steps,
        };
        self.fade = Some(FadeState {
            direction: FadeDirection::FadeOut { stop_on_end },
            step: 0,
            total_steps: steps,
        });
    }

    fn stop(&mut self) {
        self.audio = None;
        self.frame_cursor = 0;
        self.delay_frames_remaining = 0;
        self.fade = None;
    }

    /// Advance one `100 ms` fade timer tick (`0x45a7f0`).
    fn tick_fade_100ms(&mut self) {
        let Some(mut fade) = self.fade else {
            return;
        };
        fade.step += 1;
        let mb = match fade.direction {
            FadeDirection::FadeIn => fade_in_mb(self.target_mb, fade.step, fade.total_steps),
            FadeDirection::FadeOut { .. } => {
                fade_out_mb(self.target_mb, fade.step, fade.total_steps)
            }
        };
        if self.enabled {
            self.current_mb = mb;
        } else {
            self.current_mb = DSBVOLUME_MIN;
        }

        if fade.step >= fade.total_steps {
            match fade.direction {
                FadeDirection::FadeIn => {
                    self.fade = None;
                    if self.enabled {
                        self.current_mb = self.target_mb;
                    }
                }
                FadeDirection::FadeOut { stop_on_end } => {
                    self.fade = None;
                    if stop_on_end {
                        self.stop();
                    }
                }
            }
        } else {
            self.fade = Some(fade);
        }
    }

    /// Mix up to `frames` stereo frames into `out_interleaved` (length `>= frames * 2`).
    fn mix_into(&mut self, out_interleaved: &mut [f32], frames: usize) {
        let gain = self.current_gain();
        let mut out_idx = 0usize;

        while out_idx < frames {
            let Some(audio) = self.audio.as_ref() else {
                break;
            };
            if self.delay_frames_remaining > 0 {
                let skip = self.delay_frames_remaining.min(frames - out_idx);
                self.delay_frames_remaining -= skip;
                out_idx += skip;
                continue;
            }

            let total_src = audio.samples_per_channel;
            if total_src == 0 || self.frame_cursor >= total_src {
                self.advance_loop_or_stop();
                continue;
            }

            let avail = total_src - self.frame_cursor;
            let chunk = avail.min(frames - out_idx);

            if gain > 0.0 {
                if audio.channels == 1 {
                    let src = &audio.samples_interleaved[self.frame_cursor..self.frame_cursor + chunk];
                    for (i, &mono) in src.iter().enumerate() {
                        let s = mono * gain;
                        let dst_pos = (out_idx + i) * 2;
                        out_interleaved[dst_pos] += s;
                        out_interleaved[dst_pos + 1] += s;
                    }
                } else {
                    let src_start = self.frame_cursor * 2;
                    let src = &audio.samples_interleaved[src_start..src_start + chunk * 2];
                    for i in 0..chunk {
                        let dst_pos = (out_idx + i) * 2;
                        out_interleaved[dst_pos] += src[i * 2] * gain;
                        out_interleaved[dst_pos + 1] += src[i * 2 + 1] * gain;
                    }
                }
            }

            self.frame_cursor += chunk;
            out_idx += chunk;

            if self.frame_cursor >= total_src {
                self.advance_loop_or_stop();
            }
        }
    }

    fn advance_loop_or_stop(&mut self) {
        match self.loop_mode {
            LoopMode::Infinite => {
                self.frame_cursor = 0;
            }
            LoopMode::Once => {
                self.stop();
            }
            LoopMode::Repeat(_) => {
                if self.remaining_plays > 1 {
                    self.remaining_plays -= 1;
                    self.frame_cursor = 0;
                } else {
                    self.stop();
                }
            }
        }
    }
}

/// 4-slot DirectSound-equivalent software audio mixer for `AVAudioEngine` pull rendering.
#[derive(Debug, Clone)]
pub struct AudioMixer {
    bgm_slots: [MixerChannel; 2],
    active_bgm_slot: usize,
    current_bgm_id: Option<u16>,
    se0: MixerChannel,
    voice: MixerChannel,
    se1: MixerChannel,
    bgm_vol: u8,
    se_vol: u8,
    voice_vol: u8,
    bgm_enabled: bool,
    se_enabled: bool,
    voice_enabled: bool,
    fade_sample_accum: usize,
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMixer {
    /// Construct a mixer initialized to `KhimeDL.exe` defaults (all enabled, volume `255`).
    pub fn new() -> Self {
        let bgm_mb = bgm_vol_to_mb(255);
        let se_mb = se_voice_vol_to_mb(255);
        Self {
            bgm_slots: [MixerChannel::new(bgm_mb, true), MixerChannel::new(bgm_mb, true)],
            active_bgm_slot: 0,
            current_bgm_id: None,
            se0: MixerChannel::new(se_mb, true),
            voice: MixerChannel::new(se_mb, true),
            se1: MixerChannel::new(se_mb, true),
            bgm_vol: 255,
            se_vol: 255,
            voice_vol: 255,
            bgm_enabled: true,
            se_enabled: true,
            voice_enabled: true,
            fade_sample_accum: 0,
        }
    }

    /// Set BGM volume `0..=255` (`0x448fd0`).
    pub fn set_bgm_volume(&mut self, vol: u8) {
        self.bgm_vol = vol;
        let mb = bgm_vol_to_mb(vol);
        self.bgm_slots[0].set_target_mb(mb);
        self.bgm_slots[1].set_target_mb(mb);
    }

    /// Set BGM enable/mute switch (`0x47137c`).
    pub fn set_bgm_enabled(&mut self, enabled: bool) {
        self.bgm_enabled = enabled;
        self.bgm_slots[0].set_enabled(enabled);
        self.bgm_slots[1].set_enabled(enabled);
    }

    /// Set SE volume `0..=255` for both SE0 and SE1 (`0x448d00`, `0x40df80`).
    pub fn set_se_volume(&mut self, vol: u8) {
        self.se_vol = vol;
        let mb = se_voice_vol_to_mb(vol);
        self.se0.set_target_mb(mb);
        self.se1.set_target_mb(mb);
    }

    /// Set SE enable/mute switch (`0x471380`).
    pub fn set_se_enabled(&mut self, enabled: bool) {
        self.se_enabled = enabled;
        self.se0.set_enabled(enabled);
        self.se1.set_enabled(enabled);
    }

    /// Set Voice volume `0..=255` (`0x448d00`).
    pub fn set_voice_volume(&mut self, vol: u8) {
        self.voice_vol = vol;
        let mb = se_voice_vol_to_mb(vol);
        self.voice.set_target_mb(mb);
    }

    /// Set Voice enable/mute switch (`0x47137e`).
    pub fn set_voice_enabled(&mut self, enabled: bool) {
        self.voice_enabled = enabled;
        self.voice.set_enabled(enabled);
    }

    /// Currently active BGM track ID, if any.
    pub fn current_bgm_id(&self) -> Option<u16> {
        self.current_bgm_id
    }

    /// Active BGM ping-pong slot index (`0` or `1`).
    pub fn active_bgm_slot_index(&self) -> usize {
        self.active_bgm_slot
    }

    /// Access both BGM ping-pong slots.
    pub fn bgm_slots(&self) -> &[MixerChannel; 2] {
        &self.bgm_slots
    }

    /// Access SE0 channel (`+0x1ac`).
    pub fn se0(&self) -> &MixerChannel {
        &self.se0
    }

    /// Access Voice channel (`+0x1b0`).
    pub fn voice(&self) -> &MixerChannel {
        &self.voice
    }

    /// Access SE1 channel (`+0x1b4`).
    pub fn se1(&self) -> &MixerChannel {
        &self.se1
    }

    /// Play BGM with dual-slot ping-pong crossfading (`0x448d90` / `0x448e60`).
    ///
    /// - If `track_id == 0` or `self.current_bgm_id == Some(track_id)`, no-op (`0x448dab..0x448db4`).
    /// - When switching tracks while one is playing, fades out the old slot and swaps `active_bgm_slot ^= 1`.
    pub fn play_bgm(
        &mut self,
        track_id: u16,
        audio: Arc<DecodedAudio>,
        loop_flag: bool,
        crossfade: bool,
        fade_steps: Option<u32>,
    ) {
        if track_id == 0 || self.current_bgm_id == Some(track_id) {
            return;
        }
        if self.current_bgm_id.is_some() {
            self.bgm_slots[self.active_bgm_slot].start_fade_out(fade_steps, true);
            self.active_bgm_slot ^= 1;
        }
        self.current_bgm_id = Some(track_id);
        let loop_mode = if loop_flag {
            LoopMode::Infinite
        } else {
            LoopMode::Once
        };
        let do_fade_in = crossfade && fade_steps != Some(0);
        self.bgm_slots[self.active_bgm_slot].play(audio, loop_mode, do_fade_in, fade_steps, 0);
    }

    /// Stop active BGM (`0x448f40`) with optional fade-out steps (`Some(0)` = immediate).
    pub fn stop_bgm(&mut self, fade_steps: Option<u32>) {
        if self.current_bgm_id.is_none() {
            return;
        }
        self.bgm_slots[self.active_bgm_slot].start_fade_out(fade_steps, true);
        self.current_bgm_id = None;
    }

    /// Play sound effect on SE0 (`+0x1ac`, `0x41e300` / `0x424c00`).
    pub fn play_se0(
        &mut self,
        audio: Arc<DecodedAudio>,
        loop_mode: LoopMode,
        fade_steps: Option<u32>,
    ) {
        let do_fade = matches!(fade_steps, Some(s) if s > 0);
        self.se0.play(audio, loop_mode, do_fade, fade_steps, 0);
    }

    /// Stop SE0 (`0x424c90`).
    pub fn stop_se0(&mut self, fade_steps: Option<u32>) {
        self.se0.start_fade_out(fade_steps, true);
    }

    /// Play transient/UI sound effect on SE1 (`+0x1b4`, `0x41e300`).
    pub fn play_se1(&mut self, audio: Arc<DecodedAudio>) {
        self.se1.play(audio, LoopMode::Once, false, None, 0);
    }

    /// Play voice on the Voice slot (`+0x1b0`, `0x41e370`), interrupting any previous voice.
    ///
    /// `delay_50ms` schedules playback after `delay_50ms * 50 ms` (`param_4 * 50 ms`).
    pub fn play_voice(
        &mut self,
        audio: Arc<DecodedAudio>,
        loop_flag: bool,
        delay_50ms: u32,
        fade_steps: Option<u32>,
    ) {
        let delay_frames = (delay_50ms as usize).saturating_mul(VOICE_DELAY_UNIT_FRAMES);
        let loop_mode = if loop_flag {
            LoopMode::Infinite
        } else {
            LoopMode::Once
        };
        let do_fade = matches!(fade_steps, Some(s) if s > 0);
        self.voice
            .play(audio, loop_mode, do_fade, fade_steps, delay_frames);
    }

    /// Stop voice playback (`0x424d10`), or cut on click when `voice_cut_enabled == true` (`0x471398`).
    pub fn stop_voice(&mut self, fade_steps: Option<u32>) {
        self.voice.start_fade_out(fade_steps, true);
    }

    /// Handle click-cut voice rule (`DAT_00471398`).
    pub fn on_dialogue_advance(&mut self, voice_cut_on_click: bool) {
        if voice_cut_on_click {
            self.voice.stop();
        }
    }

    /// Stop all channels immediately (`0x449020`).
    pub fn stop_all(&mut self) {
        self.bgm_slots[0].stop();
        self.bgm_slots[1].stop();
        self.current_bgm_id = None;
        self.se0.stop();
        self.voice.stop();
        self.se1.stop();
    }

    /// Manually advance `ticks` of the `100 ms` fade timer (`0x45a7f0`).
    pub fn step_fade_ticks(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.bgm_slots[0].tick_fade_100ms();
            self.bgm_slots[1].tick_fade_100ms();
            self.se0.tick_fade_100ms();
            self.voice.tick_fade_100ms();
            self.se1.tick_fade_100ms();
        }
    }

    /// Render interleaved stereo `f32` PCM (`44,100 Hz`) into `out_interleaved` with hard clipping.
    ///
    /// Automatically steps the `100 ms` DirectSound fade timers every `4,410` frames.
    pub fn render_stereo_f32(&mut self, out_interleaved: &mut [f32]) {
        out_interleaved.fill(0.0);
        let total_frames = out_interleaved.len() / 2;
        if total_frames == 0 {
            return;
        }

        let mut frame_off = 0usize;
        while frame_off < total_frames {
            let frames_until_tick = FADE_TICK_FRAMES - self.fade_sample_accum;
            let chunk = (total_frames - frame_off).min(frames_until_tick);
            let slice = &mut out_interleaved[frame_off * 2..(frame_off + chunk) * 2];

            self.bgm_slots[0].mix_into(slice, chunk);
            self.bgm_slots[1].mix_into(slice, chunk);
            self.se0.mix_into(slice, chunk);
            self.voice.mix_into(slice, chunk);
            self.se1.mix_into(slice, chunk);

            for sample in slice.iter_mut() {
                if !sample.is_finite() {
                    *sample = 0.0;
                } else {
                    *sample = sample.clamp(-1.0, 1.0);
                }
            }

            frame_off += chunk;
            self.fade_sample_accum += chunk;
            if self.fade_sample_accum >= FADE_TICK_FRAMES {
                self.fade_sample_accum = 0;
                self.step_fade_ticks(1);
            }
        }
    }
}
