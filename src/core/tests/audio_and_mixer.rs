use std::sync::Arc;

use fujihime_core::audio::{
    bgm_vol_to_mb, decode_vorbis_acm_wav, fade_in_mb, fade_out_mb, mb_to_linear_gain,
    se_voice_vol_to_mb, AudioMixer, DecodedAudio, LoopMode, AUDIO_SAMPLE_RATE, DEFAULT_FADE_STEPS,
    DSBVOLUME_MIN, FADE_FLOOR_MB, FADE_TICK_FRAMES, VOICE_DELAY_UNIT_FRAMES,
};
use fujihime_core::vfs::{AssetCategory, AssetVfs, LanguageMode, VfsConfig};

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
fn real_all_3084_audio_assets_decode_and_strip_dummy_pages() {
    if !require_real_assets_or_skip("real_all_3084_audio_assets_decode_and_strip_dummy_pages") {
        return;
    }
    let vfs = AssetVfs::open(VfsConfig::new(GAME_DIR, LanguageMode::Japanese)).unwrap();

    let expected = [
        (AssetCategory::Bgm, 21usize, 2u16, 144_435_732usize, 511_039usize),
        (AssetCategory::Voice, 3015usize, 1u16, 627_223_074usize, 1_442_201usize),
        (AssetCategory::Wav, 48usize, 1u16, 47_488_904usize, 115_241usize),
    ];

    let mut grand_entries = 0usize;
    let mut grand_samples = 0usize;
    let mut grand_dummy = 0usize;

    for (cat, exp_count, exp_ch, exp_samples, exp_dummy) in expected {
        let arc = vfs.archive(cat).expect("audio archive must exist");
        assert_eq!(arc.len(), exp_count, "unexpected entry count in {cat:?}");

        let mut cat_samples = 0usize;
        let mut cat_dummy = 0usize;

        for entry in arc.entries() {
            let raw = arc.read_entry(entry).unwrap();
            let decoded = decode_vorbis_acm_wav(&raw).unwrap_or_else(|e| {
                panic!("failed decoding {cat:?}/{}: {e}", entry.name);
            });
            assert_eq!(decoded.channels, exp_ch, "channels mismatch on {}", entry.name);
            assert_eq!(
                decoded.sample_rate, AUDIO_SAMPLE_RATE,
                "sample rate mismatch on {}",
                entry.name
            );
            assert!(decoded.eos_seen, "missing EOS on {}", entry.name);
            if let Some(fact) = decoded.fact_samples {
                assert_eq!(
                    fact as usize, decoded.samples_per_channel,
                    "fact mismatch on {}",
                    entry.name
                );
            }
            cat_samples += decoded.samples_per_channel;
            cat_dummy += decoded.dummy_pages_stripped;
        }

        assert_eq!(cat_samples, exp_samples, "total samples mismatch for {cat:?}");
        assert_eq!(cat_dummy, exp_dummy, "dummy pages mismatch for {cat:?}");
        grand_entries += arc.len();
        grand_samples += cat_samples;
        grand_dummy += cat_dummy;
    }

    assert_eq!(grand_entries, 3084);
    assert_eq!(grand_samples, 819_147_710);
    assert_eq!(grand_dummy, 2_068_481);
}

#[test]
fn directsound_volume_curves_and_fade_math() {
    // SE / Voice curve (0x448d00): ((vol - 255) * 5300) / 255 clamped to [-5000, 0]
    assert_eq!(se_voice_vol_to_mb(255), 0);
    assert_eq!(se_voice_vol_to_mb(0), -5000);
    assert_eq!(se_voice_vol_to_mb(128), ((128 - 255) * 5300) / 255);

    // BGM curve (0x448fd0): ((vol - 255) * 3000) / 255 clamped to [-5000, 0]
    assert_eq!(bgm_vol_to_mb(255), 0);
    assert_eq!(bgm_vol_to_mb(0), -3000);
    assert!(
        mb_to_linear_gain(bgm_vol_to_mb(0)) > 0.03,
        "BGM vol=0 (-30 dB) must still have faint audible gain, not 0.0"
    );
    assert_eq!(mb_to_linear_gain(DSBVOLUME_MIN), 0.0);
    assert!((mb_to_linear_gain(0) - 1.0).abs() < 1e-6);

    // 100ms fade curves (0x45a7f0)
    assert_eq!(fade_in_mb(0, 0, DEFAULT_FADE_STEPS), FADE_FLOOR_MB);
    assert_eq!(fade_in_mb(0, DEFAULT_FADE_STEPS, DEFAULT_FADE_STEPS), 0);
    assert_eq!(fade_in_mb(0, 75, 150), -2500);
    assert_eq!(fade_out_mb(0, 0, DEFAULT_FADE_STEPS), 0);
    assert_eq!(fade_out_mb(0, 75, 150), -2500);
    assert_eq!(fade_out_mb(0, DEFAULT_FADE_STEPS, DEFAULT_FADE_STEPS), FADE_FLOOR_MB);
}

fn synthetic_mono_tone(val: f32, frames: usize) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: 1,
        sample_rate: AUDIO_SAMPLE_RATE,
        samples_per_channel: frames,
        samples_interleaved: vec![val; frames],
        fact_samples: Some(frames as u32),
        dummy_pages_stripped: 0,
        real_pages: 1,
        eos_seen: true,
    })
}

fn synthetic_stereo_tone(l: f32, r: f32, frames: usize) -> Arc<DecodedAudio> {
    let mut samples = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        samples.push(l);
        samples.push(r);
    }
    Arc::new(DecodedAudio {
        channels: 2,
        sample_rate: AUDIO_SAMPLE_RATE,
        samples_per_channel: frames,
        samples_interleaved: samples,
        fact_samples: Some(frames as u32),
        dummy_pages_stripped: 0,
        real_pages: 1,
        eos_seen: true,
    })
}

#[test]
fn mixer_bgm_crossfade_voice_delay_and_hard_clip() {
    let mut mixer = AudioMixer::new();
    let track1 = synthetic_stereo_tone(0.25, -0.25, 10_000);
    let track2 = synthetic_stereo_tone(0.50, 0.50, 10_000);

    // Start Track 3 instantly on slot 0
    mixer.play_bgm(3, track1, true, false, Some(0));
    assert_eq!(mixer.current_bgm_id(), Some(3));
    assert_eq!(mixer.active_bgm_slot_index(), 0);
    assert!(mixer.bgm_slots()[0].is_playing());
    assert!(!mixer.bgm_slots()[1].is_playing());

    // Calling play_bgm with same track_id is a no-op (0x448dab)
    mixer.play_bgm(3, track2.clone(), true, true, Some(4));
    assert_eq!(mixer.active_bgm_slot_index(), 0);

    // Switch to Track 4 with 4-step (400ms) crossfade -> ping-pongs to slot 1
    mixer.play_bgm(4, track2, true, true, Some(4));
    assert_eq!(mixer.current_bgm_id(), Some(4));
    assert_eq!(mixer.active_bgm_slot_index(), 1);
    assert!(mixer.bgm_slots()[0].is_fading());
    assert!(mixer.bgm_slots()[1].is_fading());

    // Render 4 * 100ms (4 * 4410 frames) and verify slot 0 finishes fade-out and stops
    let mut pcm = vec![0.0f32; FADE_TICK_FRAMES * 4 * 2];
    mixer.render_stereo_f32(&mut pcm);
    assert!(!mixer.bgm_slots()[0].is_playing());
    assert!(mixer.bgm_slots()[1].is_playing());
    assert!(!mixer.bgm_slots()[1].is_fading());
    for &s in &pcm {
        assert!(s.is_finite());
        assert!((-1.0..=1.0).contains(&s));
    }

    // Mute BGM -> gain becomes 0.0; unmute restores
    mixer.set_bgm_enabled(false);
    assert_eq!(mixer.bgm_slots()[1].current_gain(), 0.0);
    mixer.set_bgm_enabled(true);
    assert!((mixer.bgm_slots()[1].current_gain() - 1.0).abs() < 1e-6);

    // Voice delayed start: param_4 = 2 -> 2 * 50ms = 4410 silent frames
    mixer.stop_all();
    let voice = synthetic_mono_tone(0.4, 2000);
    mixer.play_voice(voice, false, 2, Some(0));
    assert_eq!(
        mixer.voice().delay_frames_remaining(),
        2 * VOICE_DELAY_UNIT_FRAMES
    );

    let mut vbuf = vec![0.0f32; (2 * VOICE_DELAY_UNIT_FRAMES + 100) * 2];
    mixer.render_stereo_f32(&mut vbuf);
    // Delay prefix must be strictly silent
    for &s in &vbuf[..2 * VOICE_DELAY_UNIT_FRAMES * 2] {
        assert_eq!(s, 0.0);
    }
    // After delay, mono voice is duplicated to L and R
    assert!((vbuf[2 * VOICE_DELAY_UNIT_FRAMES * 2] - 0.4).abs() < 1e-6);
    assert!((vbuf[2 * VOICE_DELAY_UNIT_FRAMES * 2 + 1] - 0.4).abs() < 1e-6);

    // Click cut stops voice when enabled
    mixer.on_dialogue_advance(false);
    assert!(mixer.voice().is_playing());
    mixer.on_dialogue_advance(true);
    assert!(!mixer.voice().is_playing());

    // Hard-clip test across SE0 + SE1 + BGM simultaneously
    let loud = synthetic_mono_tone(0.8, 1024);
    mixer.play_bgm(5, loud.clone(), true, false, Some(0));
    mixer.play_se0(loud.clone(), LoopMode::Repeat(2), None);
    mixer.play_se1(loud);
    let mut clip_buf = vec![0.0f32; 512 * 2];
    mixer.render_stereo_f32(&mut clip_buf);
    assert!(clip_buf.iter().all(|&s| (s - 1.0).abs() < 1e-6));
}
