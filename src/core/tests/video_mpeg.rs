use fujihime_core::gfx::surface::{Surface, GAME_HEIGHT, GAME_WIDTH};
use fujihime_core::vfs::{AssetCategory, AssetVfs, LanguageMode, VfsConfig};
use fujihime_core::video::{
    MpegCompletionEvent, MpegPlayer, MpegStopReason, MpegWindowController, MpegWindowEvent,
    MpegWindowPhase, WM_MPEG_NOTIFY_DONE,
};

const GAME_DIR: &str = "/workspace/game/腐姫ＤＬ";

fn require_real_assets_or_skip(test_name: &str) -> bool {
    if std::path::Path::new(GAME_DIR).is_dir() {
        return true;
    }
    if std::env::var("FUJIHIME_REQUIRE_REAL_ASSETS").ok().as_deref() == Some("1") {
        panic!("FUJIHIME_REQUIRE_REAL_ASSETS=1 but real assets missing at {GAME_DIR}");
    }
    eprintln!("[skip] {test_name}: real game assets not found at {GAME_DIR}");
    false
}

#[test]
fn real_mpg_0001_and_0002_full_decode_and_eof() {
    if !require_real_assets_or_skip("real_mpg_0001_and_0002_full_decode_and_eof") {
        return;
    }
    let vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap();

    // 1. mov/0001.mpg: 320x240, 30fps, 3,217 video frames, 4,106 audio packets (4,730,112 samples/ch)
    let mpg1_bytes = vfs.read(AssetCategory::Mov, "0001.mpg").unwrap();
    let mut p1 = MpegPlayer::from_bytes(mpg1_bytes).unwrap();
    assert_eq!((p1.width(), p1.height()), (320, 240));
    assert!((p1.framerate() - 30.0).abs() < 1e-3);
    assert_eq!(p1.samplerate(), 44_100);
    assert_eq!(p1.num_video_streams(), 1);
    assert_eq!(p1.num_audio_streams(), 1);
    assert!(!p1.has_ended());

    let mut vframes_1 = 0usize;
    let mut last_vpts_1 = 0.0f64;
    while let Some(frame) = p1.decode_video_frame().unwrap() {
        assert_eq!((frame.surface.width(), frame.surface.height()), (320, 240));
        // Inverted alpha must be 0x00 (opaque) across decoded video surface.
        assert_eq!(frame.surface.get_pixel(0, 0)[3], 0x00);
        last_vpts_1 = frame.pts_sec;
        vframes_1 += 1;
    }
    assert_eq!(vframes_1, 3217);
    assert!(last_vpts_1 > 107.0, "must not truncate at header duration 106.433s");

    p1.rewind();
    let mut apackets_1 = 0usize;
    let mut asamples_1 = 0usize;
    let mut max_abs_1 = 0.0f32;
    while let Some(pkt) = p1.decode_audio_packet() {
        apackets_1 += 1;
        asamples_1 += pkt.frames;
        for &s in &pkt.samples_interleaved {
            assert!(s.is_finite(), "NaN/Inf in 0001.mpg audio");
            max_abs_1 = max_abs_1.max(s.abs());
        }
    }
    assert_eq!(apackets_1, 4106);
    assert_eq!(asamples_1, 4_730_112);
    assert!(max_abs_1 > 0.5 && max_abs_1 < 1.0);

    // 2. mov/0002.mpg: 640x480, 30fps, 131 video frames, 168 audio packets (193,536 samples/ch)
    let mpg2_bytes = vfs.read(AssetCategory::Mov, "0002.mpg").unwrap();
    let mut p2 = MpegPlayer::from_bytes(mpg2_bytes).unwrap();
    assert_eq!((p2.width(), p2.height()), (640, 480));
    assert!((p2.framerate() - 30.0).abs() < 1e-3);
    assert_eq!(p2.samplerate(), 44_100);

    let mut vframes_2 = 0usize;
    while let Some(frame) = p2.decode_video_frame().unwrap() {
        assert_eq!((frame.surface.width(), frame.surface.height()), (640, 480));
        vframes_2 += 1;
    }
    assert_eq!(vframes_2, 131);

    p2.rewind();
    let mut apackets_2 = 0usize;
    let mut asamples_2 = 0usize;
    while let Some(pkt) = p2.decode_audio_packet() {
        apackets_2 += 1;
        asamples_2 += pkt.frames;
        for &s in &pkt.samples_interleaved {
            assert!(s.is_finite(), "NaN/Inf in 0002.mpg audio");
        }
    }
    assert_eq!(apackets_2, 168);
    assert_eq!(asamples_2, 193_536);
}

#[test]
fn cmpeg_window_input_and_stretch_state_machine() {
    if !require_real_assets_or_skip("cmpeg_window_input_and_stretch_state_machine") {
        return;
    }
    let vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap();
    let mpg2_bytes = vfs.read(AssetCategory::Mov, "0002.mpg").unwrap();

    // 1. Natural EOF triggers 0x41E after decoding all 131 frames stretched to 800x600.
    let mut win = MpegWindowController::new();
    assert_eq!(win.phase(), MpegWindowPhase::Idle);
    assert!(!win.is_visible());

    assert!(win.start_bytes(mpg2_bytes.clone()).is_none());
    assert_eq!(win.phase(), MpegWindowPhase::Playing);
    assert!(win.is_visible());

    // Keyboard & right-click must be ignored during MPEG playback (0x450900).
    assert!(win.handle_event(MpegWindowEvent::KeyDown(0x1B)).is_none());
    assert!(win.handle_event(MpegWindowEvent::KeyDown(0x20)).is_none());
    assert!(win.handle_event(MpegWindowEvent::RightButtonUp).is_none());
    assert_eq!(win.phase(), MpegWindowPhase::Playing);

    let mut screen = Surface::new(GAME_WIDTH, GAME_HEIGHT).unwrap();
    let mut frame_count = 0usize;
    let mut completion: Option<MpegCompletionEvent> = None;
    while completion.is_none() {
        let (pts, done) = win.step_video_800x600(&mut screen).unwrap();
        if pts.is_some() {
            frame_count += 1;
        }
        completion = done;
    }
    assert_eq!(frame_count, 131);
    assert_eq!(
        completion,
        Some(MpegCompletionEvent {
            message_id: WM_MPEG_NOTIFY_DONE,
            reason: MpegStopReason::EndOfStream,
        })
    );
    assert!(!win.is_visible());
    assert_eq!(
        win.phase(),
        MpegWindowPhase::Completed(MpegStopReason::EndOfStream)
    );

    // 2. WM_LBUTTONUP skips playback immediately and emits 0x41E.
    assert!(win.start_bytes(mpg2_bytes).is_none());
    let (pts, done) = win.step_video_800x600(&mut screen).unwrap();
    assert!(pts.is_some());
    assert!(done.is_none());

    let skip_ev = win.handle_event(MpegWindowEvent::LeftButtonUp);
    assert_eq!(
        skip_ev,
        Some(MpegCompletionEvent {
            message_id: WM_MPEG_NOTIFY_DONE,
            reason: MpegStopReason::LeftButtonUp,
        })
    );
    assert!(!win.is_visible());
    assert_eq!(
        win.phase(),
        MpegWindowPhase::Completed(MpegStopReason::LeftButtonUp)
    );
}
