//! Native MPEG-1 video & MP2 audio playback via MIT `pl_mpeg`, plus `CMpegWindow` state machine.
//!
//! Matches `KhimeDL.exe` `CMpegWindow` (`0x4505f0`..`0x450a60`):
//! - Stretches decoded video frames to logical `800x600` (`MCI_DGV_PUT_DESTINATION`).
//! - Strictly detects natural end-of-stream via `plm_has_ended()` (never `plm_get_duration()`).
//! - Stops only on `WM_LBUTTONUP (0x202)`, natural EOS (`MM_MCINOTIFY`), or internal stop (`0x403`);
//!   keyboard events and right-clicks are ignored.

use std::ffi::c_void;
use std::fs;
use std::path::Path;

use crate::error::{FujihimeError, Result};
use crate::gfx::compositor::stretch_blit_srccopy;
use crate::gfx::surface::{Surface, GAME_HEIGHT, GAME_WIDTH};

/// Win32 message ID posted to parent window when MPEG playback stops (`WM_USER + 0x1E`).
pub const WM_MPEG_NOTIFY_DONE: u32 = 0x041E;

/// Maximum frames returned per MP2 audio packet in `pl_mpeg` (`PLM_AUDIO_SAMPLES_PER_FRAME`).
pub const PLM_AUDIO_SAMPLES_PER_FRAME: usize = 1152;

#[repr(C)]
struct PlmOpaque {
    _private: [u8; 0],
}

extern "C" {
    fn fujihime_plm_create_with_memory(bytes: *const u8, length: usize) -> *mut PlmOpaque;
    fn fujihime_plm_destroy(plm: *mut PlmOpaque);
    fn fujihime_plm_has_headers(plm: *mut PlmOpaque) -> i32;
    fn fujihime_plm_get_width(plm: *mut PlmOpaque) -> i32;
    fn fujihime_plm_get_height(plm: *mut PlmOpaque) -> i32;
    fn fujihime_plm_get_framerate(plm: *mut PlmOpaque) -> f64;
    fn fujihime_plm_get_samplerate(plm: *mut PlmOpaque) -> i32;
    fn fujihime_plm_get_num_video_streams(plm: *mut PlmOpaque) -> i32;
    fn fujihime_plm_get_num_audio_streams(plm: *mut PlmOpaque) -> i32;
    fn fujihime_plm_has_ended(plm: *mut PlmOpaque) -> i32;
    fn fujihime_plm_set_video_enabled(plm: *mut PlmOpaque, enabled: i32);
    fn fujihime_plm_set_audio_enabled(plm: *mut PlmOpaque, enabled: i32);
    fn fujihime_plm_set_loop(plm: *mut PlmOpaque, looping: i32);
    fn fujihime_plm_rewind(plm: *mut PlmOpaque);
    fn fujihime_plm_decode_video_bgra(
        plm: *mut PlmOpaque,
        out_bgra: *mut u8,
        stride_bytes: i32,
        out_time: *mut f64,
    ) -> i32;
    fn fujihime_plm_decode_audio(
        plm: *mut PlmOpaque,
        out_interleaved: *mut f32,
        max_frames: u32,
        out_time: *mut f64,
    ) -> u32;
}

/// Decoded MPEG-1 video frame in top-down 32-bit BGRA (`A = 0x00` opaque, inverted alpha).
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub surface: Surface,
    pub pts_sec: f64,
}

/// Decoded MPEG-1 Layer II stereo audio packet (`44,100 Hz` interleaved `f32`).
#[derive(Debug, Clone)]
pub struct VideoAudioPacket {
    pub samples_interleaved: Vec<f32>,
    pub frames: usize,
    pub pts_sec: f64,
}

/// Native MPEG-1 stream decoder backed by `pl_mpeg`.
pub struct MpegPlayer {
    // Drop order: destroy `plm` before releasing `_backing`.
    plm: *mut PlmOpaque,
    _backing: Box<[u8]>,
    width: u32,
    height: u32,
    framerate: f64,
    samplerate: u32,
    num_video_streams: u32,
    num_audio_streams: u32,
}

// Each `plm_t` owns completely independent demuxer/decoder state with no global mutable state.
unsafe impl Send for MpegPlayer {}

impl std::fmt::Debug for MpegPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MpegPlayer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("framerate", &self.framerate)
            .field("samplerate", &self.samplerate)
            .field("num_video_streams", &self.num_video_streams)
            .field("num_audio_streams", &self.num_audio_streams)
            .finish()
    }
}

impl Drop for MpegPlayer {
    fn drop(&mut self) {
        if !self.plm.is_null() {
            unsafe {
                fujihime_plm_destroy(self.plm);
            }
            self.plm = std::ptr::null_mut();
        }
    }
}

impl MpegPlayer {
    /// Create a decoder from an in-memory `.mpg` byte buffer.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() {
            return Err(FujihimeError::InvalidVideo {
                reason: "empty MPEG-1 buffer".to_string(),
            });
        }
        let backing = bytes.into_boxed_slice();
        let plm = unsafe { fujihime_plm_create_with_memory(backing.as_ptr(), backing.len()) };
        if plm.is_null() {
            return Err(FujihimeError::InvalidVideo {
                reason: "plm_create_with_memory failed".to_string(),
            });
        }

        let has_headers = unsafe { fujihime_plm_has_headers(plm) };
        if has_headers == 0 {
            unsafe { fujihime_plm_destroy(plm) };
            return Err(FujihimeError::InvalidVideo {
                reason: "MPEG-1 headers not found".to_string(),
            });
        }

        let w = unsafe { fujihime_plm_get_width(plm) };
        let h = unsafe { fujihime_plm_get_height(plm) };
        if w <= 0 || h <= 0 {
            unsafe { fujihime_plm_destroy(plm) };
            return Err(FujihimeError::InvalidVideo {
                reason: format!("invalid video dimensions {w}x{h}"),
            });
        }

        let framerate = unsafe { fujihime_plm_get_framerate(plm) };
        let samplerate = unsafe { fujihime_plm_get_samplerate(plm) }.max(0) as u32;
        let num_video_streams = unsafe { fujihime_plm_get_num_video_streams(plm) }.max(0) as u32;
        let num_audio_streams = unsafe { fujihime_plm_get_num_audio_streams(plm) }.max(0) as u32;

        // Ensure unused opaque pointer type import doesn't cause warnings.
        let _check: *mut c_void = plm.cast();
        let _ = _check;

        Ok(Self {
            plm,
            _backing: backing,
            width: w as u32,
            height: h as u32,
            framerate,
            samplerate,
            num_video_streams,
            num_audio_streams,
        })
    }

    /// Load and open an `.mpg` file from disk.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|e| FujihimeError::io(path, e))?;
        Self::from_bytes(bytes)
    }

    /// Native video frame width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Native video frame height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Video frame rate in FPS.
    #[inline]
    pub fn framerate(&self) -> f64 {
        self.framerate
    }

    /// Audio sample rate in Hz (typically `44100`).
    #[inline]
    pub fn samplerate(&self) -> u32 {
        self.samplerate
    }

    /// Number of MPEG-1 video streams detected.
    #[inline]
    pub fn num_video_streams(&self) -> u32 {
        self.num_video_streams
    }

    /// Number of MPEG-1 audio streams detected.
    #[inline]
    pub fn num_audio_streams(&self) -> u32 {
        self.num_audio_streams
    }

    /// Authoritative end-of-stream query (`plm_has_ended()`).
    #[inline]
    pub fn has_ended(&self) -> bool {
        unsafe { fujihime_plm_has_ended(self.plm) != 0 }
    }

    /// Enable or disable video stream decoding.
    pub fn set_video_enabled(&mut self, enabled: bool) {
        unsafe { fujihime_plm_set_video_enabled(self.plm, i32::from(enabled)) };
    }

    /// Enable or disable audio stream decoding.
    pub fn set_audio_enabled(&mut self, enabled: bool) {
        unsafe { fujihime_plm_set_audio_enabled(self.plm, i32::from(enabled)) };
    }

    /// Enable or disable looping playback.
    pub fn set_loop(&mut self, looping: bool) {
        unsafe { fujihime_plm_set_loop(self.plm, i32::from(looping)) };
    }

    /// Rewind stream back to the beginning.
    pub fn rewind(&mut self) {
        unsafe { fujihime_plm_rewind(self.plm) };
    }

    /// Decode the next video frame into a [`Surface`] (`A = 0x00` opaque, inverted alpha).
    pub fn decode_video_frame(&mut self) -> Result<Option<VideoFrame>> {
        let mut surface = Surface::new(self.width, self.height)?;
        // `Surface::new` zeroes all bytes so alpha is already `0x00` (opaque in inverted convention),
        // and `plm_frame_to_bgra` leaves byte +3 untouched.
        let stride = (self.width * 4) as i32;
        let mut pts_sec = 0.0f64;
        let got = unsafe {
            fujihime_plm_decode_video_bgra(
                self.plm,
                surface.as_bytes_mut().as_mut_ptr(),
                stride,
                &mut pts_sec,
            )
        };
        if got == 0 {
            return Ok(None);
        }
        Ok(Some(VideoFrame { surface, pts_sec }))
    }

    /// Decode the next video frame and stretch it to an `800x600` target [`Surface`]
    /// (`MCI_DGV_PUT_DESTINATION`, `0x450720`).
    pub fn decode_video_frame_to_800x600(&mut self, dst: &mut Surface) -> Result<Option<f64>> {
        if dst.width() != GAME_WIDTH || dst.height() != GAME_HEIGHT {
            return Err(FujihimeError::InvalidSurface {
                reason: format!(
                    "expected {}x{} destination surface, got {}x{}",
                    GAME_WIDTH,
                    GAME_HEIGHT,
                    dst.width(),
                    dst.height()
                ),
            });
        }
        let Some(frame) = self.decode_video_frame()? else {
            return Ok(None);
        };
        stretch_blit_srccopy(
            dst,
            (0, 0, GAME_WIDTH as i32, GAME_HEIGHT as i32),
            &frame.surface,
            (0, 0, frame.surface.width() as i32, frame.surface.height() as i32),
        );
        Ok(Some(frame.pts_sec))
    }

    /// Decode the next MP2 stereo audio packet (`1152` frames interleaved `f32`).
    pub fn decode_audio_packet(&mut self) -> Option<VideoAudioPacket> {
        let mut buf = vec![0.0f32; PLM_AUDIO_SAMPLES_PER_FRAME * 2];
        let mut pts_sec = 0.0f64;
        let frames = unsafe {
            fujihime_plm_decode_audio(
                self.plm,
                buf.as_mut_ptr(),
                PLM_AUDIO_SAMPLES_PER_FRAME as u32,
                &mut pts_sec,
            )
        } as usize;
        if frames == 0 {
            return None;
        }
        buf.truncate(frames * 2);
        Some(VideoAudioPacket {
            samples_interleaved: buf,
            frames,
            pts_sec,
        })
    }
}

/// Reason why `CMpegWindow` stopped playback and emitted `WM_USER + 0x1E (0x41E)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegStopReason {
    /// Natural end of stream (`plm_has_ended()` / `MM_MCINOTIFY`).
    EndOfStream,
    /// User left-click release (`WM_LBUTTONUP`, `0x202`).
    LeftButtonUp,
    /// Internal stop command (`0x403`).
    InternalStop,
    /// Failed to open / initialize stream (`0x450720` fail-safe notification).
    OpenFailed,
}

/// Input/control events routed to `CMpegWindow` (`0x450900`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegWindowEvent {
    /// `WM_LBUTTONUP (0x202)` — skips playback immediately.
    LeftButtonUp,
    /// `WM_RBUTTONUP (0x204)` — ignored by `0x450900`.
    RightButtonUp,
    /// `WM_KEYDOWN (0x100)` — ignored by `0x450900`.
    KeyDown(u32),
    /// Internal stop message (`0x403`).
    InternalStop,
}

/// Notification produced when `CMpegWindow` closes (`PostMessageA(parent, 0x41E, 0, 0)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpegCompletionEvent {
    pub message_id: u32,
    pub reason: MpegStopReason,
}

/// Playback phase of `CMpegWindow` (`0x4505f0`..`0x450a60`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegWindowPhase {
    Idle,
    Playing,
    Completed(MpegStopReason),
}

/// Rust state machine faithfully modeling `KhimeDL.exe` `CMpegWindow` (`0x4505f0`..`0x450a60`).
#[derive(Debug)]
pub struct MpegWindowController {
    player: Option<MpegPlayer>,
    phase: MpegWindowPhase,
    visible: bool,
}

impl Default for MpegWindowController {
    fn default() -> Self {
        Self::new()
    }
}

impl MpegWindowController {
    /// Construct an idle hidden `CMpegWindow` controller (`0x4505f0`).
    pub fn new() -> Self {
        Self {
            player: None,
            phase: MpegWindowPhase::Idle,
            visible: false,
        }
    }

    /// Current playback phase.
    #[inline]
    pub fn phase(&self) -> MpegWindowPhase {
        self.phase
    }

    /// Whether the video overlay window is visible (`WS_VISIBLE`).
    #[inline]
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Access the active [`MpegPlayer`], if playing.
    #[inline]
    pub fn player(&self) -> Option<&MpegPlayer> {
        self.player.as_ref()
    }

    /// Mutable access to the active [`MpegPlayer`], if playing.
    #[inline]
    pub fn player_mut(&mut self) -> Option<&mut MpegPlayer> {
        self.player.as_mut()
    }

    /// Start playing an `.mpg` byte stream (`0x450720` + `0x450820`).
    ///
    /// If opening fails, transitions immediately to `Completed(OpenFailed)` and returns the
    /// `0x41E` completion notification to avoid deadlocking the script wait (`0x426f10`).
    pub fn start_bytes(&mut self, bytes: Vec<u8>) -> Option<MpegCompletionEvent> {
        match MpegPlayer::from_bytes(bytes) {
            Ok(player) => {
                self.player = Some(player);
                self.phase = MpegWindowPhase::Playing;
                self.visible = true;
                None
            }
            Err(_) => {
                self.player = None;
                self.phase = MpegWindowPhase::Completed(MpegStopReason::OpenFailed);
                self.visible = false;
                Some(MpegCompletionEvent {
                    message_id: WM_MPEG_NOTIFY_DONE,
                    reason: MpegStopReason::OpenFailed,
                })
            }
        }
    }

    /// Decode the next video frame stretched to `800x600`, or emit `EndOfStream` when `plm_has_ended()`.
    pub fn step_video_800x600(
        &mut self,
        dst: &mut Surface,
    ) -> Result<(Option<f64>, Option<MpegCompletionEvent>)> {
        if self.phase != MpegWindowPhase::Playing {
            return Ok((None, None));
        }
        let Some(player) = self.player.as_mut() else {
            return Ok((None, None));
        };

        if let Some(pts) = player.decode_video_frame_to_800x600(dst)? {
            return Ok((Some(pts), None));
        }

        if player.has_ended() {
            let done = self.stop_with_reason(MpegStopReason::EndOfStream);
            return Ok((None, Some(done)));
        }
        Ok((None, None))
    }

    /// Handle a window input event per `CMpegWindow::WndProc` (`0x450900`).
    ///
    /// Only `WM_LBUTTONUP (0x202)` and `InternalStop (0x403)` stop playback;
    /// `WM_KEYDOWN` and `WM_RBUTTONUP` are ignored.
    pub fn handle_event(&mut self, event: MpegWindowEvent) -> Option<MpegCompletionEvent> {
        if self.phase != MpegWindowPhase::Playing {
            return None;
        }
        match event {
            MpegWindowEvent::LeftButtonUp => Some(self.stop_with_reason(MpegStopReason::LeftButtonUp)),
            MpegWindowEvent::InternalStop => Some(self.stop_with_reason(MpegStopReason::InternalStop)),
            MpegWindowEvent::RightButtonUp | MpegWindowEvent::KeyDown(_) => None,
        }
    }

    fn stop_with_reason(&mut self, reason: MpegStopReason) -> MpegCompletionEvent {
        self.visible = false;
        self.player = None;
        self.phase = MpegWindowPhase::Completed(reason);
        MpegCompletionEvent {
            message_id: WM_MPEG_NOTIFY_DONE,
            reason,
        }
    }
}
