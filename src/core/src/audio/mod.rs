//! Audio subsystem: RIFF `0x6771` (`vorbis.acm`) unpacking, `0xFFFFFFFF` dummy-page stripping,
//! pure-Rust Ogg Vorbis decoding, and DirectSound 4-slot software mixer.

pub mod mixer;
pub mod vorbis;

pub use mixer::{
    bgm_vol_to_mb, fade_in_mb, fade_out_mb, mb_to_linear_gain, se_voice_vol_to_mb, AudioMixer,
    LoopMode, MixerChannel, DEFAULT_FADE_STEPS, DSBVOLUME_MIN, FADE_FLOOR_MB, FADE_TICK_FRAMES,
    FADE_TICK_MS, VOICE_DELAY_UNIT_FRAMES,
};
pub use vorbis::{
    decode_vorbis_acm_wav, parse_riff_vorbis, strip_vorbis_acm_dummy_pages, DecodedAudio,
    RiffVorbisContainer, StrippedOggStream, AUDIO_SAMPLE_RATE, VORBIS_ACM_FORMAT_TAG,
};
