//! Reading sermons' audio (any part of it, as mono samples), drawing it, and writing MP3 and WAV.

use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error as SymError,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};

use crate::{
    error::{Error, Result},
    files::Files,
};

/// Mono samples at a rate
#[derive(Clone, Debug, Default)]
pub struct Pcm {
    pub rate: u32,
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / self.rate.max(1) as f64
    }
}

struct Opened {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    rate: u32,
    time_base: Option<symphonia::core::units::TimeBase>,
}

fn open(files: &dyn Files, path: &str) -> Result<Opened> {
    let mss = MediaSourceStream::new(files.open(path)?, Default::default());
    let mut hint = Hint::new();
    if let Some((_, ext)) = path.rsplit_once('.') {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions {
            enable_gapless: true,
            ..Default::default()
        },
        &MetadataOptions::default(),
    )?;
    let format = probed.format;
    let track = (format.tracks().iter())
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| Error::Audio(format!("{path} has no audio")))?;
    let decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    Ok(Opened {
        track_id: track.id,
        rate: track.codec_params.sample_rate.unwrap_or(44_100),
        time_base: track.codec_params.time_base,
        format,
        decoder,
    })
}

/// Every packet's samples, downmixed to mono, from wherever the reader is, with the time (in
/// seconds) each packet starts at; `each` returns false to stop
fn decode_from(opened: &mut Opened, mut each: impl FnMut(f64, &[f32]) -> bool) -> Result<()> {
    let mut buffer: Option<SampleBuffer<f32>> = None;
    let mut mono = vec![];
    loop {
        let packet = match opened.format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != opened.track_id {
            continue;
        }
        let time = match opened.time_base {
            Some(tb) => {
                let t = tb.calc_time(packet.ts());
                t.seconds as f64 + t.frac
            }
            None => packet.ts() as f64 / opened.rate as f64,
        };
        let decoded = match opened.decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let buf = match &mut buffer {
            Some(b) if b.capacity() >= decoded.capacity() * channels => b,
            _ => buffer.insert(SampleBuffer::new(decoded.capacity() as u64, spec)),
        };
        buf.copy_interleaved_ref(decoded);
        mono.clear();
        mono.extend(
            buf.samples()
                .chunks(channels)
                .map(|f| f.iter().sum::<f32>() / channels as f32),
        );
        if !each(time, &mono) {
            break;
        }
    }
    Ok(())
}

/// The audio from `start` to `end` seconds, mono, at the file's own rate (of an HLS stream, read
/// from the segments it's in)
pub fn decode_range(files: &dyn Files, path: &str, start: f64, end: f64) -> Result<Pcm> {
    if crate::hls::is_playlist(path) {
        return crate::hls::decode_range(files, path, start, end);
    }
    let mut opened = open(files, path)?;
    let rate = opened.rate;
    let start = start.max(0.0);
    let seek = (start - 1.0).max(0.0);
    if seek > 0.0 {
        let track_id = opened.track_id;
        // A file that can't seek is read from the start
        if opened
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(seek),
                    track_id: Some(track_id),
                },
            )
            .is_ok()
        {
            opened.decoder.reset();
        }
    }
    let first = (start * rate as f64).round() as i64;
    let last = (end * rate as f64).round() as i64;
    let mut samples = Vec::with_capacity((last - first).max(0) as usize);
    decode_from(&mut opened, |time, mono| {
        let at = (time * rate as f64).round() as i64;
        for (k, s) in mono.iter().enumerate() {
            let i = at + k as i64;
            if i >= first && i < last {
                samples.push(*s);
            }
        }
        at + (mono.len() as i64) < last
    })?;
    // Past the file's end, silence, so a clip is as long as asked
    samples.resize((last - first).max(0) as usize, 0.0);
    Ok(Pcm { rate, samples })
}

/// Peaks per second of audio drawn
pub const PEAKS_PER_SECOND: u32 = 50;

/// Loudest sample of every 1/50 s of some audio
pub fn peaks_of(pcm: &Pcm) -> Vec<f32> {
    let per = (pcm.rate / PEAKS_PER_SECOND).max(1) as usize;
    pcm.samples
        .chunks(per)
        .map(|c| c.iter().fold(0f32, |m, s| m.max(s.abs())))
        .collect()
}

/// Loudest sample of every 1/50 s of the whole file, and its length in seconds (it decodes the
/// whole file: about two seconds for an hour of audio)
pub fn peaks(files: &dyn Files, path: &str) -> Result<(Vec<f32>, f64)> {
    let mut opened = open(files, path)?;
    let per = (opened.rate / PEAKS_PER_SECOND).max(1) as usize;
    let mut peaks = vec![];
    let (mut current, mut count, mut total) = (0f32, 0usize, 0usize);
    decode_from(&mut opened, |_, mono| {
        for s in mono {
            current = current.max(s.abs());
            count += 1;
            if count == per {
                peaks.push(current);
                (current, count) = (0.0, 0);
            }
        }
        total += mono.len();
        true
    })?;
    if count > 0 {
        peaks.push(current);
    }
    let duration = total as f64 / opened.rate as f64;
    Ok((peaks, duration))
}

/// `samples` at another rate (windowed sinc, low-passed when lowering the rate)
pub fn resample(pcm: &Pcm, rate: u32) -> Pcm {
    if pcm.rate == rate || pcm.samples.is_empty() {
        return Pcm {
            rate,
            samples: pcm.samples.clone(),
        };
    }
    const TAPS: isize = 16;
    let ratio = pcm.rate as f64 / rate as f64;
    let cutoff = (1.0 / ratio).min(1.0) * 0.95;
    let out_len = (pcm.samples.len() as f64 / ratio).round() as usize;
    let x = &pcm.samples;
    let reach = (TAPS as f64 / cutoff).ceil() as isize;
    let width = (2 * reach) as usize;

    // Output sample k falls at input k·M/L: past input sample k·M div L by (k·M mod L)/L. There are
    // only L such offsets (L is 80 from 44.1 to 24 kHz), so each one's weights are worked out once;
    // rates with more are rounded to the nearest of 4096
    let g = gcd(pcm.rate as u64, rate as u64);
    let (m, l) = (pcm.rate as u64 / g, rate as u64 / g);
    let phases = l.min(4096);
    let weight = |d: f64| {
        let arg = d * cutoff;
        let sinc = if arg.abs() < 1e-9 {
            1.0
        } else {
            (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg)
        };
        // Hann window over the filter's reach
        sinc * 0.5 * (1.0 + (std::f64::consts::PI * d / reach as f64).cos())
    };
    // Weights of input samples center - reach + 1 ..= center + reach, for each offset
    let table: Vec<f64> = (0..=phases)
        .flat_map(|p| {
            let frac = p as f64 / phases as f64;
            (-reach + 1..=reach).map(move |j| weight(frac - j as f64))
        })
        .collect();

    let mut out = Vec::with_capacity(out_len);
    for k in 0..out_len as u64 {
        let (mut center, rest) = ((k * m / l) as isize, k * m % l);
        let mut phase = if phases == l {
            rest
        } else {
            (rest as f64 / l as f64 * phases as f64).round() as u64
        };
        if phase == phases {
            (center, phase) = (center + 1, 0);
        }
        let weights = &table[phase as usize * width..][..width];
        let first = center - reach + 1;
        let (mut acc, mut norm) = (0.0f64, 0.0f64);
        if first >= 0 && (first as usize + width) <= x.len() {
            for (s, c) in x[first as usize..][..width].iter().zip(weights) {
                acc += *s as f64 * c;
                norm += c;
            }
        } else {
            // At the ends, only the samples there are
            for (j, c) in (first..first + width as isize).zip(weights) {
                if j >= 0 && (j as usize) < x.len() {
                    acc += x[j as usize] as f64 * c;
                    norm += c;
                }
            }
        }
        out.push(if norm.abs() > 1e-9 { (acc / norm) as f32 } else { 0.0 });
    }
    Pcm { rate, samples: out }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Fade the first and last `ms` in and out, so cuts don't click
pub fn fade(samples: &mut [f32], rate: u32, ms: f64) {
    let n = ((rate as f64 * ms / 1000.0) as usize).min(samples.len() / 2);
    for i in 0..n {
        let g = i as f32 / n as f32;
        samples[i] *= g;
        let last = samples.len() - 1 - i;
        samples[last] *= g;
    }
}

/// The RMS of each 1/20 s of audible sound
pub fn levels(samples: &[f32], rate: u32) -> Vec<f64> {
    let window = (rate / 20).max(1) as usize;
    samples
        .chunks(window)
        .map(|c| (c.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / c.len() as f64).sqrt())
        .filter(|r| *r > 1e-4)
        .collect()
}

/// How loud speech is in these levels: the RMS of their louder half, so pauses don't count
pub fn speech_rms(mut levels: Vec<f64>) -> Option<f64> {
    if levels.is_empty() {
        return None;
    }
    levels.sort_by(f64::total_cmp);
    let speech = &levels[levels.len() / 2..];
    Some((speech.iter().map(|r| r * r).sum::<f64>() / speech.len() as f64).sqrt())
}

pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// The gain that brings speech to about `target_db` dBFS RMS (within 0.25–6×, and without
/// pushing peaks past -1 dBFS)
pub fn leveling_gain(samples: &[f32], rate: u32, target_db: f64) -> f64 {
    let Some(rms) = speech_rms(levels(samples, rate)) else {
        return 1.0;
    };
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs())) as f64;
    (db_to_gain(target_db) / rms)
        .clamp(0.25, 6.0)
        .min(0.89 / peak.max(1e-6))
}

pub fn apply_gain(samples: &mut [f32], gain: f64) {
    if (gain - 1.0).abs() > 1e-9 {
        for s in samples {
            *s = (*s as f64 * gain) as f32;
        }
    }
}

/// Peaks over -1 dBFS rounded off smoothly, so turning a reading up never clips
pub fn limit(samples: &mut [f32]) {
    const KNEE: f32 = 0.89;
    for s in samples {
        let a = s.abs();
        if a > KNEE {
            *s = s.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh());
        }
    }
}

/// 16-bit mono WAV
pub fn wav(pcm: &Pcm) -> Vec<u8> {
    let data_len = pcm.samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend(b"RIFF");
    out.extend(((36 + data_len) as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(pcm.rate.to_le_bytes());
    out.extend((pcm.rate * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data_len as u32).to_le_bytes());
    for s in &pcm.samples {
        out.extend(((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

/// MP3, mono, at a constant bitrate (snapped to the nearest one MP3 has), with a header that
/// lets players skip the encoder's padding
pub fn mp3(pcm: &Pcm, kbps: u16) -> Result<Vec<u8>> {
    use rusty_mp3::{Error as Mp3Error, Mp3Encoder, Mp3EncoderConfig};
    let mut encoder = Mp3Encoder::new(Mp3EncoderConfig {
        bitrate_kbps: kbps as u32,
        vbr_quality: None,
    });
    encoder
        .push_pcm_f32(&pcm.samples, 1, pcm.rate)
        .map_err(|e| Error::Audio(format!("MP3 encoder: {e}")))?;
    encoder.finish();
    let mut out = Vec::with_capacity(pcm.samples.len() / 4);
    loop {
        match encoder.next_packet() {
            Ok(packet) => out.extend(packet),
            Err(Mp3Error::Again | Mp3Error::Eof) => break,
            Err(e) => return Err(Error::Audio(format!("MP3 encoder: {e}"))),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_length_and_tone() {
        let rate = 48_000;
        let samples: Vec<f32> = (0..rate)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5)
            .collect();
        let out = resample(&Pcm { rate, samples }, 44_100);
        assert_eq!(out.samples.len(), 44_100);
        let peak = out.samples[1000..43000].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "{peak}");
    }

    #[test]
    fn encodes_and_decodes() {
        let rate = 44_100;
        let samples: Vec<f32> = (0..rate * 2)
            .map(|i| (i as f32 * 300.0 * std::f32::consts::TAU / rate as f32).sin() * 0.3)
            .collect();
        let mut files = crate::files::Memory::new();
        files.insert("tone.mp3", mp3(&Pcm { rate, samples }, 64).unwrap());
        let back = decode_range(&files, "tone.mp3", 0.5, 1.5).unwrap();
        assert_eq!(back.rate, rate);
        assert_eq!(back.samples.len(), rate as usize);
        let peak = back.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.2 && peak < 0.4, "{peak}");
        let (peaks, duration) = peaks(&files, "tone.mp3").unwrap();
        assert!((duration - 2.0).abs() < 0.1, "{duration}");
        assert!(peaks.len() >= 99);
    }
}
