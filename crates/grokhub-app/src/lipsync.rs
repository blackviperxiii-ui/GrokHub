//! Lip-sync for the desktop companion: the spoken reply's mp3 is decoded
//! in this process into a loudness envelope before it plays, and while it
//! plays the mouth patch (closed, half, open) is picked by how loud the
//! voice is at the time since playback began. The decode is a few ms for
//! a sentence; the envelope is one `f32` per 20 ms.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// One envelope frame.
pub const FRAME_MS: u32 = 20;
/// Below this share of the loudest frame the mouth is closed, above
/// [`OPEN_LEVEL`] it is open, between the two half open.
pub const CLOSED_LEVEL: f32 = 0.18;
pub const OPEN_LEVEL: f32 = 0.5;
/// Frames louder than this many of the loudest are clipped so one shout
/// doesn't make the rest of the sentence mumble.
const LOUD_SHARE: f32 = 0.95;

/// How open the mouth is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouth {
    Closed,
    Half,
    Open,
}

/// Loudness per [`FRAME_MS`], 0 to 1, from the start of the clip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Envelope {
    pub levels: Vec<f32>,
}

impl Envelope {
    /// Loudness `at` into the clip; 0 after the end.
    pub fn level_at(&self, at: Duration) -> f32 {
        let i = (at.as_millis() / u128::from(FRAME_MS)) as usize;
        self.levels.get(i).copied().unwrap_or(0.0)
    }

    pub fn mouth_at(&self, at: Duration) -> Mouth {
        mouth_for(self.level_at(at))
    }
}

/// The mouth for a loudness level.
pub fn mouth_for(level: f32) -> Mouth {
    if level < CLOSED_LEVEL {
        Mouth::Closed
    } else if level < OPEN_LEVEL {
        Mouth::Half
    } else {
        Mouth::Open
    }
}

/// The envelope of mono PCM at `rate`: RMS per [`FRAME_MS`], scaled so
/// the 95th-percentile frame is 1 and anything louder clips to 1. Silence
/// (or an empty clip) is all zeros.
pub fn envelope_from_pcm(samples: &[f32], rate: u32) -> Envelope {
    let per_frame = ((rate as usize) * FRAME_MS as usize / 1000).max(1);
    let mut levels: Vec<f32> = samples
        .chunks(per_frame)
        .map(|w| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt())
        .collect();
    let mut sorted = levels.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let idx = ((sorted.len() as f32 * LOUD_SHARE) as usize).min(sorted.len().saturating_sub(1));
    let reference = sorted.get(idx).copied().unwrap_or(0.0);
    if reference <= 1e-6 {
        levels.iter_mut().for_each(|l| *l = 0.0);
    } else {
        levels
            .iter_mut()
            .for_each(|l| *l = (*l / reference).min(1.0));
    }
    Envelope { levels }
}

/// Decode an mp3 to mono PCM: (samples, rate). `None` when it isn't one.
pub fn decode_mp3(bytes: &[u8]) -> Option<(Vec<f32>, u32)> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;
    let track = format.default_track(TrackType::Audio)?;
    let track_id = track.id;
    let params = track.codec_params.as_ref()?.audio()?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .ok()?;
    let mut mono = Vec::new();
    let mut rate = 0u32;
    let mut buf: Vec<f32> = Vec::new();
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        let spec = decoded.spec();
        rate = spec.rate();
        let channels = spec.channels().count().max(1);
        decoded.copy_to_vec_interleaved(&mut buf);
        mono.extend(
            buf.chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
    }
    (rate > 0 && !mono.is_empty()).then_some((mono, rate))
}

/// The envelope of an mp3 clip, or `None` when it can't be decoded.
pub fn envelope_from_mp3(bytes: &[u8]) -> Option<Envelope> {
    let (pcm, rate) = decode_mp3(bytes)?;
    Some(envelope_from_pcm(&pcm, rate))
}

/// A reply being spoken right now: when playback began and its envelope.
#[derive(Clone, Debug)]
pub struct Speaking {
    pub started: Instant,
    pub env: Envelope,
}

impl Speaking {
    pub fn mouth_now(&self) -> Mouth {
        self.env.mouth_at(self.started.elapsed())
    }
}

/// Shared between the speaking thread and the UI.
pub type LipSlot = Arc<Mutex<Option<Speaking>>>;

/// The mouth right now, if a reply is being spoken.
pub fn mouth_now(slot: &LipSlot) -> Option<Mouth> {
    slot.lock().ok()?.as_ref().map(Speaking::mouth_now)
}

/// Speak an mp3 reply: write it to the temp file the player reads,
/// decode its envelope, publish it in `slot` as playback starts, and clear
/// the slot when the player returns. A clip that won't decode still plays,
/// with the mouth closed.
pub fn speak_mp3(bytes: Vec<u8>, slot: &LipSlot) {
    let Ok(path) = crate::desktop::private_temp("speak.mp3") else {
        return;
    };
    if std::fs::write(&path, &bytes).is_err() {
        return;
    }
    let env = envelope_from_mp3(&bytes).unwrap_or_default();
    if let Ok(mut s) = slot.lock() {
        *s = Some(Speaking {
            started: Instant::now(),
            env,
        });
    }
    let _ = crate::desktop::play_audio(&path);
    if let Ok(mut s) = slot.lock() {
        *s = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, secs: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| amp * (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin())
            .collect()
    }

    #[test]
    fn the_mouth_opens_with_loudness() {
        assert_eq!(mouth_for(0.0), Mouth::Closed);
        assert_eq!(mouth_for(0.17), Mouth::Closed);
        assert_eq!(mouth_for(0.18), Mouth::Half);
        assert_eq!(mouth_for(0.49), Mouth::Half);
        assert_eq!(mouth_for(0.5), Mouth::Open);
        assert_eq!(mouth_for(1.0), Mouth::Open);
    }

    #[test]
    fn the_envelope_has_one_frame_per_20_ms_scaled_to_the_loud_part() {
        let rate = 16_000;
        // 200 ms silence, 400 ms loud, 200 ms quiet.
        let mut pcm = vec![0.0; (rate as f32 * 0.2) as usize];
        pcm.extend(tone(rate, 0.4, 0.8));
        pcm.extend(tone(rate, 0.2, 0.2));
        let env = envelope_from_pcm(&pcm, rate);
        assert_eq!(env.levels.len(), 40, "800 ms / 20 ms");
        assert_eq!(env.mouth_at(Duration::from_millis(100)), Mouth::Closed);
        assert_eq!(env.mouth_at(Duration::from_millis(400)), Mouth::Open);
        assert_eq!(env.mouth_at(Duration::from_millis(700)), Mouth::Half);
        assert_eq!(
            env.mouth_at(Duration::from_millis(900)),
            Mouth::Closed,
            "after the end"
        );
        let loud = env.level_at(Duration::from_millis(400));
        assert!(
            (0.9..=1.0).contains(&loud),
            "the loud part sits at the top: {loud}"
        );
        let quiet = env.level_at(Duration::from_millis(700));
        assert!(
            (quiet - 0.25).abs() < 0.02,
            "a quarter of the loud part: {quiet}"
        );
    }

    #[test]
    fn silence_and_nothing_give_a_closed_mouth() {
        let env = envelope_from_pcm(&vec![0.0; 16_000], 16_000);
        assert_eq!(env.levels.len(), 50);
        assert!(env.levels.iter().all(|l| *l == 0.0));
        assert_eq!(envelope_from_pcm(&[], 16_000).levels.len(), 0);
        assert_eq!(Envelope::default().mouth_at(Duration::ZERO), Mouth::Closed);
    }

    #[test]
    fn garbage_is_not_an_mp3() {
        assert_eq!(decode_mp3(b"not audio at all"), None);
        assert_eq!(envelope_from_mp3(&[]), None);
    }

    #[test]
    fn the_slot_reads_the_mouth_only_while_speaking() {
        let slot: LipSlot = Arc::default();
        assert_eq!(mouth_now(&slot), None);
        let mut levels = vec![0.0; 10];
        levels.extend(vec![1.0; 10]);
        *slot.lock().unwrap() = Some(Speaking {
            started: Instant::now(),
            env: Envelope { levels },
        });
        assert_eq!(
            mouth_now(&slot),
            Some(Mouth::Closed),
            "first 200 ms are quiet"
        );
        *slot.lock().unwrap() = None;
        assert_eq!(mouth_now(&slot), None);
    }
}
