//! RIFF `0x6771` (`vorbis.acm`) demuxer, `0xFFFFFFFF` dummy-page stripper, and Ogg Vorbis PCM decoder.

use std::io::Cursor;

use lewton::inside_ogg::OggStreamReader;
use lewton::samples::InterleavedSamples;

use crate::error::{FujihimeError, Result};

/// Standard audio sample rate used across all 3,084 `KhimeDL.exe` audio assets.
pub const AUDIO_SAMPLE_RATE: u32 = 44_100;

/// RIFF WAVE `wFormatTag` used by `vorbis.acm`.
pub const VORBIS_ACM_FORMAT_TAG: u16 = 0x6771;

/// Parsed RIFF `0x6771` container fields.
#[derive(Debug, Clone)]
pub struct RiffVorbisContainer<'a> {
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub fact_samples: Option<u32>,
    pub data: &'a [u8],
}

/// Output of stripping `vorbis.acm` `serial = 0xFFFFFFFF` dummy OggS pages.
#[derive(Debug, Clone)]
pub struct StrippedOggStream {
    pub clean_ogg: Vec<u8>,
    pub dummy_pages_stripped: usize,
    pub real_pages: usize,
    pub eos_seen: bool,
}

/// Decoded PCM audio buffer (native channels, `44,100 Hz`, normalized `f32`).
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples_per_channel: usize,
    pub samples_interleaved: Vec<f32>,
    pub fact_samples: Option<u32>,
    pub dummy_pages_stripped: usize,
    pub real_pages: usize,
    pub eos_seen: bool,
}

impl DecodedAudio {
    /// Duration in seconds.
    pub fn duration_sec(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.samples_per_channel as f64 / f64::from(self.sample_rate)
        }
    }
}

/// Parse a RIFF `WAVE` buffer containing `wFormatTag = 0x6771` (`vorbis.acm`).
pub fn parse_riff_vorbis(blob: &[u8]) -> Result<RiffVorbisContainer<'_>> {
    if blob.len() < 12 || &blob[0..4] != b"RIFF" || &blob[8..12] != b"WAVE" {
        return Err(FujihimeError::InvalidAudio {
            reason: "not a RIFF WAVE container".to_string(),
        });
    }

    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32)> = None;
    let mut fact_samples: Option<u32> = None;
    let mut data_slice: Option<&[u8]> = None;

    while pos + 8 <= blob.len() {
        let cid = &blob[pos..pos + 4];
        let csz = u32::from_le_bytes([blob[pos + 4], blob[pos + 5], blob[pos + 6], blob[pos + 7]])
            as usize;
        let payload_start = pos + 8;
        let payload_end = payload_start.checked_add(csz).ok_or_else(|| {
            FujihimeError::InvalidAudio {
                reason: "RIFF chunk size overflow".to_string(),
            }
        })?;
        if payload_end > blob.len() {
            return Err(FujihimeError::InvalidAudio {
                reason: format!("truncated RIFF chunk {:?}", cid),
            });
        }
        let cdata = &blob[payload_start..payload_end];

        if cid == b"fmt " && cdata.len() >= 16 {
            let format_tag = u16::from_le_bytes([cdata[0], cdata[1]]);
            let channels = u16::from_le_bytes([cdata[2], cdata[3]]);
            let sample_rate = u32::from_le_bytes([cdata[4], cdata[5], cdata[6], cdata[7]]);
            fmt = Some((format_tag, channels, sample_rate));
        } else if cid == b"fact" && cdata.len() >= 4 {
            fact_samples = Some(u32::from_le_bytes([cdata[0], cdata[1], cdata[2], cdata[3]]));
        } else if cid == b"data" {
            data_slice = Some(cdata);
        }

        // RIFF chunks are padded to even byte boundaries.
        pos = payload_end + (csz & 1);
    }

    let (format_tag, channels, sample_rate) = fmt.ok_or_else(|| FujihimeError::InvalidAudio {
        reason: "missing RIFF fmt chunk".to_string(),
    })?;
    let data = data_slice.ok_or_else(|| FujihimeError::InvalidAudio {
        reason: "missing RIFF data chunk".to_string(),
    })?;

    if format_tag != VORBIS_ACM_FORMAT_TAG {
        return Err(FujihimeError::InvalidAudio {
            reason: format!("unexpected WAVE format tag 0x{format_tag:04x}, expected 0x6771"),
        });
    }

    Ok(RiffVorbisContainer {
        format_tag,
        channels,
        sample_rate,
        fact_samples,
        data,
    })
}

/// Strip `vorbis.acm`-injected dummy `OggS` pages (`serial == 0xFFFFFFFF && granule == 0 && body_len == 0`).
pub fn strip_vorbis_acm_dummy_pages(ogg_data: &[u8]) -> Result<StrippedOggStream> {
    let mut pos = 0usize;
    let n = ogg_data.len();
    let mut clean_ogg = Vec::with_capacity(n);
    let mut dummy_pages_stripped = 0usize;
    let mut real_pages = 0usize;
    let mut eos_seen = false;

    while pos < n {
        if pos + 27 > n || &ogg_data[pos..pos + 4] != b"OggS" {
            return Err(FujihimeError::InvalidAudio {
                reason: format!("OggS sync lost at offset {pos}/{n}"),
            });
        }
        let header_type = ogg_data[pos + 5];
        let granule = u64::from_le_bytes(
            ogg_data[pos + 6..pos + 14]
                .try_into()
                .expect("8-byte granule"),
        );
        let serial = u32::from_le_bytes(
            ogg_data[pos + 14..pos + 18]
                .try_into()
                .expect("4-byte serial"),
        );
        let nseg = ogg_data[pos + 26] as usize;
        let seg_end = pos + 27 + nseg;
        if seg_end > n {
            return Err(FujihimeError::InvalidAudio {
                reason: format!("truncated Ogg segment table at {pos}"),
            });
        }
        let body_len: usize = ogg_data[pos + 27..seg_end].iter().map(|&b| b as usize).sum();
        let page_end = seg_end + body_len;
        if page_end > n {
            return Err(FujihimeError::InvalidAudio {
                reason: format!("truncated Ogg page body at {pos}"),
            });
        }

        if serial == 0xFFFF_FFFF && body_len == 0 && granule == 0 {
            dummy_pages_stripped += 1;
        } else {
            clean_ogg.extend_from_slice(&ogg_data[pos..page_end]);
            real_pages += 1;
            if (header_type & 0x04) != 0 {
                eos_seen = true;
            }
        }
        pos = page_end;
    }

    Ok(StrippedOggStream {
        clean_ogg,
        dummy_pages_stripped,
        real_pages,
        eos_seen,
    })
}

/// Decode a `KhimeDL.exe` RIFF `0x6771` `.wav` asset into normalized `f32` PCM samples.
pub fn decode_vorbis_acm_wav(blob: &[u8]) -> Result<DecodedAudio> {
    let riff = parse_riff_vorbis(blob)?;
    let stripped = strip_vorbis_acm_dummy_pages(riff.data)?;

    let mut reader = OggStreamReader::new(Cursor::new(stripped.clean_ogg.as_slice())).map_err(|e| {
        FujihimeError::InvalidAudio {
            reason: format!("Vorbis header parse failed: {e}"),
        }
    })?;

    let channels = reader.ident_hdr.audio_channels as u16;
    let sample_rate = reader.ident_hdr.audio_sample_rate;
    if channels == 0 || channels > 2 || sample_rate == 0 {
        return Err(FujihimeError::InvalidAudio {
            reason: format!("unsupported Vorbis stream: {channels}ch @ {sample_rate}Hz"),
        });
    }
    if channels != riff.channels || sample_rate != riff.sample_rate {
        return Err(FujihimeError::InvalidAudio {
            reason: format!(
                "RIFF fmt ({}ch/{}Hz) mismatches Vorbis header ({}ch/{}Hz)",
                riff.channels, riff.sample_rate, channels, sample_rate
            ),
        });
    }

    let mut samples_interleaved = Vec::new();
    if let Some(fact) = riff.fact_samples {
        samples_interleaved.reserve((fact as usize) * (channels as usize));
    }

    while let Some(pkt) = reader
        .read_dec_packet_generic::<InterleavedSamples<f32>>()
        .map_err(|e| FujihimeError::InvalidAudio {
            reason: format!("Vorbis packet decode failed: {e}"),
        })?
    {
        for &s in &pkt.samples {
            if !s.is_finite() {
                return Err(FujihimeError::InvalidAudio {
                    reason: "non-finite PCM sample decoded".to_string(),
                });
            }
        }
        samples_interleaved.extend_from_slice(&pkt.samples);
    }

    let samples_per_channel = samples_interleaved.len() / (channels as usize);
    if samples_per_channel == 0 {
        return Err(FujihimeError::InvalidAudio {
            reason: "decoded 0 PCM samples".to_string(),
        });
    }
    if let Some(fact) = riff.fact_samples {
        if fact as usize != samples_per_channel {
            return Err(FujihimeError::InvalidAudio {
                reason: format!(
                    "fact sample count ({fact}) != decoded samples ({samples_per_channel})"
                ),
            });
        }
    }

    Ok(DecodedAudio {
        channels,
        sample_rate,
        samples_per_channel,
        samples_interleaved,
        fact_samples: riff.fact_samples,
        dummy_pages_stripped: stripped.dummy_pages_stripped,
        real_pages: stripped.real_pages,
        eos_seen: stripped.eos_seen,
    })
}
