/*!
Audio streamed as HLS: an `.m3u8` playlist of fragmented MP4 segments (an init segment, then a few
seconds of audio each), as Let's Church serves a sermon's audio. A stretch of it is read by
fetching only the segments it's in (through [`Files`], which downloads and keeps them when given
URLs), not the whole sermon.
*/

use crate::{
    audio::{self, Pcm},
    error::{Error, Result},
    files::{Files, Memory},
};

/// A playlist's segments: where each is, and when it starts and how long it is, in seconds
#[derive(Clone, Debug, PartialEq)]
pub struct Playlist {
    /// The segment every other needs first (`#EXT-X-MAP`), if it has one
    pub init: Option<String>,
    pub segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub path: String,
    pub start: f64,
    pub duration: f64,
}

impl Playlist {
    /// How long it all is, in seconds
    pub fn duration(&self) -> f64 {
        self.segments.last().map_or(0.0, |s| s.start + s.duration)
    }
}

/// Whether a path (or URL) is an HLS playlist
pub fn is_playlist(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path.to_ascii_lowercase().ends_with(".m3u8")
}

/// A playlist read from its text, its segments' paths made whole from where it is (`path`)
pub fn parse(text: &str, path: &str) -> Result<Playlist> {
    let base = path.split(['?', '#']).next().unwrap_or(path);
    let base = base.rsplit_once('/').map_or("", |(dir, _)| dir);
    let whole = |uri: &str| {
        if uri.contains("://") || uri.starts_with('/') || base.is_empty() {
            uri.to_string()
        } else {
            format!("{base}/{uri}")
        }
    };
    let mut init = None;
    let mut segments = vec![];
    let mut duration = None;
    let mut at = 0.0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(map) = line.strip_prefix("#EXT-X-MAP:") {
            let uri = (map.split(',').find_map(|attr| attr.trim().strip_prefix("URI=")))
                .map(|uri| uri.trim_matches('"'))
                .ok_or_else(|| Error::invalid(format!("{path}: an init segment without its URI")))?;
            init = Some(whole(uri));
        } else if let Some(info) = line.strip_prefix("#EXTINF:") {
            let seconds = info.split(',').next().unwrap_or("").trim();
            duration = Some(
                seconds
                    .parse::<f64>()
                    .map_err(|_| Error::invalid(format!("{path}: a segment's length isn't a number: {seconds}")))?,
            );
        } else if !line.starts_with('#') {
            let duration = duration
                .take()
                .ok_or_else(|| Error::invalid(format!("{path}: a segment without its length")))?;
            segments.push(Segment {
                path: whole(line),
                start: at,
                duration,
            });
            at += duration;
        }
    }
    if segments.is_empty() {
        return Err(Error::invalid(format!("{path}: not an HLS playlist of segments")));
    }
    Ok(Playlist { init, segments })
}

/// A playlist, read through `files`
pub fn read(files: &dyn Files, path: &str) -> Result<Playlist> {
    parse(&String::from_utf8_lossy(&files.read(path)?), path)
}

/**
The audio from `start` to `end` seconds of an HLS stream, mono, at its own rate: the segments it's
in (and the one before, so the first is decoded cleanly) fetched through `files`, decoded, and cut
*/
pub fn decode_range(files: &dyn Files, path: &str, start: f64, end: f64) -> Result<Pcm> {
    let playlist = read(files, path)?;
    let start = start.max(0.0);
    let segments = &playlist.segments;
    let first = (segments.iter())
        .position(|s| s.start + s.duration > start)
        .unwrap_or(segments.len() - 1)
        .saturating_sub(1);
    let last = (segments.iter())
        .rposition(|s| s.start < end)
        .unwrap_or(first)
        .max(first);
    let mut bytes = match &playlist.init {
        Some(init) => files.read(init)?,
        None => vec![],
    };
    for segment in &segments[first..=last] {
        bytes.extend(files.read(&segment.path)?);
    }
    // The segments joined are a stream of their own, starting when the first of them does
    let offset = segments[first].start;
    let mut joined = Memory::new();
    joined.insert("segments.mp4", bytes);
    audio::decode_range(&joined, "segments.mp4", start - offset, end - offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYLIST: &str = "#EXTM3U
#EXT-X-VERSION:7
#EXT-X-TARGETDURATION:7
#EXT-X-PLAYLIST-TYPE:VOD
#EXT-X-MAP:URI=\"AUDIO_init.mp4\"
#EXTINF:7.018667,
AUDIO_0000.m4s
#EXTINF:6.997333,
AUDIO_0001.m4s
#EXTINF:3.5,
AUDIO_0002.m4s
#EXT-X-ENDLIST
";

    #[test]
    fn a_playlist_read() {
        let url = "https://media.example/abc/AUDIO.m3u8";
        assert!(is_playlist(url) && is_playlist("x.M3U8?token=1") && !is_playlist("x.mp3"));
        let p = parse(PLAYLIST, url).unwrap();
        assert_eq!(p.init.as_deref(), Some("https://media.example/abc/AUDIO_init.mp4"));
        assert_eq!(p.segments.len(), 3);
        assert_eq!(p.segments[1].path, "https://media.example/abc/AUDIO_0001.m4s");
        assert!((p.segments[2].start - 14.016).abs() < 1e-9);
        assert!((p.duration() - 17.516).abs() < 1e-9);
        assert!(parse("#EXTM3U\n", url).is_err());
        assert!(parse("#EXTINF:abc,\nx.m4s\n", url).is_err());
    }

    #[test]
    fn a_stretch_read_from_its_segments_alone() {
        // Five seconds of audio, every other second silent, split as HLS splits it: its header as
        // the init segment, then a second of samples in each segment
        let rate = 8_000u32;
        let samples: Vec<f32> = (0..rate * 5)
            .map(|i| {
                if (i / rate) % 2 == 0 {
                    (i as f32 * 0.35).sin() * 0.5
                } else {
                    0.0
                }
            })
            .collect();
        let whole = audio::wav(&Pcm { rate, samples });
        let (header, data) = whole.split_at(44);
        let mut files = Memory::new();
        files.insert("dir/init.wav", header.to_vec());
        let mut playlist = String::from("#EXTM3U\n#EXT-X-MAP:URI=\"init.wav\"\n");
        for (k, second) in data.chunks(rate as usize * 2).enumerate() {
            files.insert(format!("dir/s{k}.wav"), second.to_vec());
            playlist += &format!("#EXTINF:1.0,\ns{k}.wav\n");
        }
        files.insert("dir/AUDIO.m3u8", playlist.into_bytes());

        let loud = |pcm: &Pcm| pcm.samples.iter().any(|s| s.abs() > 0.1);
        let pcm = decode_range(&files, "dir/AUDIO.m3u8", 2.25, 2.75).unwrap();
        assert_eq!(pcm.samples.len(), (rate / 2) as usize);
        assert!(loud(&pcm), "second 2 has sound");
        assert!(
            !loud(&decode_range(&files, "dir/AUDIO.m3u8", 3.25, 3.75).unwrap()),
            "second 3 is silent"
        );
        assert!(
            loud(&decode_range(&files, "dir/AUDIO.m3u8", 4.1, 4.9).unwrap()),
            "second 4 has sound"
        );
        // Only the segments it's in, and the one before, were needed
        files.remove("dir/s0.wav");
        files.remove("dir/s1.wav");
        assert!(decode_range(&files, "dir/AUDIO.m3u8", 3.25, 3.75).is_ok());
        assert!(decode_range(&files, "missing.m3u8", 0.0, 1.0).is_err());
    }
}
