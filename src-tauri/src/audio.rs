//! Reading sermons' audio (any part of it, as mono samples), drawing it, and writing MP3 and WAV.

use std::{
    fs::File,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
};

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

use crate::error::{AppError, Result};

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

fn open(path: &Path) -> Result<Opened> {
    let file = File::open(path).map_err(|e| AppError::msg(format!("{}: {e}", path.display())))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions { enable_gapless: true, ..Default::default() },
        &MetadataOptions::default(),
    )?;
    let format = probed.format;
    let track = (format.tracks().iter())
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AppError::msg(format!("{} has no audio", path.display())))?;
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
        mono.extend(buf.samples().chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
        if !each(time, &mono) {
            break;
        }
    }
    Ok(())
}

/// The audio from `start` to `end` seconds, mono, at the file's own rate
pub fn decode_range(path: &Path, start: f64, end: f64) -> Result<Pcm> {
    let mut opened = open(path)?;
    let rate = opened.rate;
    let start = start.max(0.0);
    let seek = (start - 1.0).max(0.0);
    if seek > 0.0 {
        let track_id = opened.track_id;
        // A file that can't seek is read from the start
        if opened
            .format
            .seek(SeekMode::Accurate, SeekTo::Time { time: Time::from(seek), track_id: Some(track_id) })
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

/// Loudest sample of every 1/50 s of the whole file, and its length in seconds (kept in
/// `cache_dir` by the file's path, size, and time)
pub fn peaks(path: &Path, cache_dir: &Path) -> Result<(Vec<f32>, f64)> {
    let meta = std::fs::metadata(path)?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok()).hash(&mut h);
    let cache = cache_dir.join(format!("{:016x}.peaks", h.finish()));
    if let Ok(bytes) = std::fs::read(&cache) {
        if bytes.len() >= 8 {
            let duration = f64::from_le_bytes(bytes[..8].try_into().unwrap());
            let peaks = bytes[8..].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            return Ok((peaks, duration));
        }
    }
    let mut opened = open(path)?;
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
    std::fs::create_dir_all(cache_dir)?;
    let mut bytes = duration.to_le_bytes().to_vec();
    bytes.extend(peaks.iter().flat_map(|p| p.to_le_bytes()));
    std::fs::write(&cache, bytes)?;
    Ok((peaks, duration))
}

/// `samples` at another rate (windowed sinc, low-passed when lowering the rate)
pub fn resample(pcm: &Pcm, rate: u32) -> Pcm {
    if pcm.rate == rate || pcm.samples.is_empty() {
        return Pcm { rate, samples: pcm.samples.clone() };
    }
    const TAPS: isize = 16;
    let ratio = pcm.rate as f64 / rate as f64;
    let cutoff = (1.0 / ratio).min(1.0) * 0.95;
    let out_len = (pcm.samples.len() as f64 / ratio).round() as usize;
    let x = &pcm.samples;
    let mut out = Vec::with_capacity(out_len);
    let reach = (TAPS as f64 / cutoff).ceil() as isize;
    for k in 0..out_len {
        let t = k as f64 * ratio;
        let center = t.floor() as isize;
        let mut acc = 0.0f64;
        let mut norm = 0.0f64;
        for j in center - reach + 1..=center + reach {
            if j < 0 || j as usize >= x.len() {
                continue;
            }
            let d = t - j as f64;
            let arg = d * cutoff;
            let sinc = if arg.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg) };
            // Hann window over the filter's reach
            let w = 0.5 * (1.0 + (std::f64::consts::PI * d / reach as f64).cos());
            let c = sinc * w;
            acc += x[j as usize] as f64 * c;
            norm += c;
        }
        out.push(if norm.abs() > 1e-9 { (acc / norm) as f32 } else { 0.0 });
    }
    Pcm { rate, samples: out }
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

/// Bring speech to about `target_db` dBFS RMS (measured over its louder parts, so pauses
/// don't count), without letting peaks pass -1 dBFS
pub fn normalize(samples: &mut [f32], rate: u32, target_db: f64) {
    let window = (rate / 20).max(1) as usize;
    let mut levels: Vec<f64> = samples
        .chunks(window)
        .map(|c| (c.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / c.len() as f64).sqrt())
        .filter(|r| *r > 1e-4)
        .collect();
    if levels.is_empty() {
        return;
    }
    levels.sort_by(f64::total_cmp);
    // The louder half is speech
    let speech = &levels[levels.len() / 2..];
    let rms = (speech.iter().map(|r| r * r).sum::<f64>() / speech.len() as f64).sqrt();
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs())) as f64;
    let mut gain = 10f64.powf(target_db / 20.0) / rms;
    gain = gain.clamp(0.25, 6.0).min(0.89 / peak.max(1e-6));
    for s in samples {
        *s = (*s as f64 * gain) as f32;
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

/// MP3, mono, at a constant bitrate
pub fn write_mp3(pcm: &Pcm, kbps: u16, path: &PathBuf) -> Result<()> {
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};
    let bitrate = match kbps {
        0..=40 => Bitrate::Kbps32,
        41..=56 => Bitrate::Kbps48,
        57..=72 => Bitrate::Kbps64,
        73..=88 => Bitrate::Kbps80,
        89..=104 => Bitrate::Kbps96,
        105..=120 => Bitrate::Kbps112,
        121..=144 => Bitrate::Kbps128,
        145..=176 => Bitrate::Kbps160,
        177..=208 => Bitrate::Kbps192,
        209..=240 => Bitrate::Kbps224,
        241..=288 => Bitrate::Kbps256,
        _ => Bitrate::Kbps320,
    };
    let lame = |e: &dyn std::fmt::Display| AppError::msg(format!("MP3 encoder: {e}"));
    let mut builder = Builder::new().ok_or_else(|| AppError::msg("MP3 encoder could not start"))?;
    builder.set_num_channels(1).map_err(|e| lame(&e))?;
    builder.set_sample_rate(pcm.rate).map_err(|e| lame(&e))?;
    builder.set_brate(bitrate).map_err(|e| lame(&e))?;
    builder.set_quality(Quality::Best).map_err(|e| lame(&e))?;
    let mut encoder = builder.build().map_err(|e| lame(&e))?;
    let mut out = Vec::with_capacity(pcm.samples.len() / 4);
    for chunk in pcm.samples.chunks(1 << 16) {
        out.reserve(mp3lame_encoder::max_required_buffer_size(chunk.len()));
        encoder.encode_to_vec(MonoPcm(chunk), &mut out).map_err(|e| lame(&e))?;
    }
    out.reserve(7200);
    encoder.flush_to_vec::<FlushNoGap>(&mut out).map_err(|e| lame(&e))?;
    let mut file = File::create(path)?;
    file.write_all(&out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_length_and_tone() {
        let rate = 48_000;
        let samples: Vec<f32> = (0..rate).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5).collect();
        let out = resample(&Pcm { rate, samples }, 44_100);
        assert_eq!(out.samples.len(), 44_100);
        let peak = out.samples[1000..43000].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "{peak}");
    }

    #[test]
    fn encodes_and_decodes() {
        let rate = 44_100;
        let samples: Vec<f32> = (0..rate * 2).map(|i| (i as f32 * 300.0 * std::f32::consts::TAU / rate as f32).sin() * 0.3).collect();
        let path = std::env::temp_dir().join(format!("cab-{}.mp3", std::process::id()));
        write_mp3(&Pcm { rate, samples }, 64, &path).unwrap();
        let back = decode_range(&path, 0.5, 1.5).unwrap();
        assert_eq!(back.rate, rate);
        assert_eq!(back.samples.len(), rate as usize);
        let peak = back.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.2 && peak < 0.4, "{peak}");
        let (peaks, duration) = peaks(&path, &std::env::temp_dir().join("cab-peaks-test")).unwrap();
        assert!((duration - 2.0).abs() < 0.1, "{duration}");
        assert!(peaks.len() >= 99);
    }
}
