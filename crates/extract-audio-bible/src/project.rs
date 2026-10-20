//! A project: the sermons it draws from, the reading chosen for each verse, and how to export.
//! It's plain data, kept as JSON by whoever uses it (the app keeps it in its data folder, the CLI
//! in a project folder).

use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    bible::Bible,
    matcher::{Match, Run},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub bible_id: String,
    /// The book being built (most sermons preach through one book); none for the whole Bible
    #[serde(default)]
    pub book: Option<u8>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub sources: Vec<Source>,
    /// By verse key (`book.chapter.verse`, like `43.3.16`)
    #[serde(default)]
    pub selections: BTreeMap<String, Selection>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub metadata: Metadata,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub label: String,
    pub audio_path: String,
    pub transcript_path: String,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub word_count: usize,
    #[serde(default)]
    pub match_count: usize,
}

/// The reading used for a verse, and how to cut it
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub match_id: String,
    pub source_id: String,
    /// The reading's own cut points (seconds), kept so a selection outlives a new analysis
    pub start: f64,
    pub end: f64,
    /// Seconds added to each cut point (negative moves it earlier)
    #[serde(default)]
    pub trim_start: f64,
    #[serde(default)]
    pub trim_end: f64,
    /// Silence before and after this verse, in seconds
    #[serde(default)]
    pub pad_before: f64,
    #[serde(default)]
    pub pad_after: f64,
    /// Volume change for this verse, in dB
    #[serde(default)]
    pub gain_db: f64,
    /// Chosen, but still to be trimmed
    #[serde(default)]
    pub needs_trim: bool,
    #[serde(default)]
    pub note: String,
    /// More readings of the verse, played after this one in order: the rest of a verse that was
    /// read in pieces (its first half, then its second minutes later)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub joined: Vec<Part>,
}

/// One reading used for part of a verse, and how it's cut
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub match_id: String,
    pub source_id: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub trim_start: f64,
    #[serde(default)]
    pub trim_end: f64,
}

impl Part {
    /// Where it's cut, in seconds, with its trims
    pub fn clip(&self) -> (f64, f64) {
        let s = (self.start + self.trim_start).max(0.0);
        (s, (self.end + self.trim_end).max(s + 0.05))
    }
}

impl Selection {
    /// Where the first reading is cut, in seconds, with its trims
    pub fn clip(&self) -> (f64, f64) {
        self.first().clip()
    }

    /// The first reading
    pub fn first(&self) -> Part {
        Part {
            match_id: self.match_id.clone(),
            source_id: self.source_id.clone(),
            start: self.start,
            end: self.end,
            trim_start: self.trim_start,
            trim_end: self.trim_end,
        }
    }

    /// Every reading of the verse, in the order they're played
    pub fn parts(&self) -> Vec<Part> {
        std::iter::once(self.first()).chain(self.joined.iter().cloned()).collect()
    }

    /// Plays these readings, in this order (the first becomes the selection's own); none leaves it
    /// as it was
    pub fn set_parts(&mut self, parts: Vec<Part>) {
        let mut parts = parts.into_iter();
        let Some(first) = parts.next() else { return };
        self.match_id = first.match_id;
        self.source_id = first.source_id;
        self.start = first.start;
        self.end = first.end;
        self.trim_start = first.trim_start;
        self.trim_end = first.trim_end;
        self.joined = parts.collect();
    }

    /// Whether a reading is one of its parts
    pub fn uses(&self, match_id: &str) -> bool {
        self.match_id == match_id || self.joined.iter().any(|p| p.match_id == match_id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// How faithfully a verse's words must be read for a reading to count (0–1)
    pub min_accuracy: f32,
    /// Silence between every two verses
    pub gap_between_verses: f64,
    /// Silence at the start and end of each chapter's file
    pub chapter_lead_in: f64,
    pub chapter_tail: f64,
    pub leveling: Leveling,
    /// Speech level leveled to, dBFS RMS
    pub target_db: f64,
    /// Volume change for everything exported, in dB
    pub output_gain_db: f64,
    pub fade_ms: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            min_accuracy: 0.85,
            gap_between_verses: 0.0,
            chapter_lead_in: 0.5,
            chapter_tail: 1.0,
            leveling: Leveling::Each,
            target_db: -20.0,
            output_gain_db: 0.0,
            fade_ms: 8.0,
        }
    }
}

/// How loudness is evened out
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Leveling {
    /// Only the volume changes you make
    None,
    /// Every reading brought to the target level, so sermons recorded louder or softer match
    Each,
    /// One change for the whole export, so it reaches the target level overall while the readings
    /// keep their levels relative to each other
    Export,
}

/// What files are written
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportMode {
    /// One file per chapter
    Chapter,
    /// One file per verse
    Verse,
    Both,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Metadata {
    pub title_template: String,
    pub album_template: String,
    pub filename_template: String,
    pub verse_title_template: String,
    pub verse_filename_template: String,
    pub export_mode: ExportMode,
    pub artist: String,
    pub album_artist: String,
    pub narrator: String,
    pub genre: String,
    pub year: String,
    pub comment: String,
    pub copyright: String,
    pub publisher: String,
    pub language: String,
    pub cover_path: Option<String>,
    pub bitrate: u16,
    pub sample_rate: u32,
    pub output_dir: Option<String>,
    /// Only chapters with every verse chosen
    pub only_complete: bool,
    /// Put each book in its own folder
    pub folder_per_book: bool,
    /// "chapter" (track = chapter number) or "sequence" (track = order in the export)
    pub track_numbering: String,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            title_template: "{book} {chapter}".into(),
            album_template: "{translation} – {book}".into(),
            filename_template: "{bookNumber2}_{osis}_{chapter3}".into(),
            verse_title_template: "{book} {chapter}:{verse}".into(),
            verse_filename_template: "{bookNumber2}_{osis}_{chapter3}_{verse3}".into(),
            export_mode: ExportMode::Chapter,
            artist: String::new(),
            album_artist: String::new(),
            narrator: String::new(),
            genre: "Audiobook".into(),
            year: String::new(),
            comment: String::new(),
            copyright: String::new(),
            publisher: String::new(),
            language: String::new(),
            cover_path: None,
            bitrate: 96,
            sample_rate: 44_100,
            output_dir: None,
            only_complete: false,
            folder_per_book: true,
            track_numbering: "chapter".into(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSet {
    pub analyzed_at: u64,
    pub matches: Vec<Match>,
    pub runs: Vec<Run>,
}

/// Milliseconds since 1970 (0 in WebAssembly, which has no clock of its own)
pub fn now() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// A short id made from some text, the same every time (a source's id from its audio's path)
pub fn stable_id(text: &str) -> String {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:012x}", h.finish() & 0xffff_ffff_ffff)
}

impl Project {
    /**
    A new project drawing on `bible`, for one `book` or the whole Bible. With no name, it's
    named after the book and translation ("John (ESV)")
    */
    pub fn new(id: impl Into<String>, name: &str, bible: &Bible, book: Option<u8>) -> Self {
        let book_name = book.and_then(|b| bible.book(b)).map(|b| b.name.clone());
        let metadata = Metadata { language: bible.language.clone().unwrap_or_default(), ..Default::default() };
        let translation = if bible.abbreviation.is_empty() { &bible.name } else { &bible.abbreviation };
        Project {
            id: id.into(),
            name: match (name.trim(), book_name) {
                ("", Some(book)) => format!("{book} ({translation})"),
                ("", None) => format!("{} audio Bible", bible.name),
                (name, _) => name.into(),
            },
            bible_id: bible.id.clone(),
            book,
            created_at: now(),
            updated_at: now(),
            sources: vec![],
            selections: BTreeMap::new(),
            settings: Default::default(),
            metadata,
        }
    }

    /**
    Adds sermons from file paths: each audio file is paired with the transcript (`.json`) of
    the same name, in the same folder first, then anywhere among the paths. Audio already in the
    project is skipped. Returns how many were added and the files that had no partner
    */
    pub fn add_sources(&mut self, paths: &[String]) -> Added {
        let (transcripts, audios): (Vec<&String>, Vec<&String>) = (paths.iter())
            .filter(|f| ext(f) == "json" || AUDIO.contains(&ext(f).as_str()))
            .partition(|f| ext(f) == "json");
        let mut used = vec![false; transcripts.len()];
        let mut added = Added::default();
        let parent = |p: &str| Path::new(p).parent().map(Path::to_path_buf);
        for audio in audios {
            let base = base_name(audio);
            let pick = (0..transcripts.len())
                .filter(|&i| !used[i] && base_name(transcripts[i]) == base)
                .min_by_key(|&i| parent(transcripts[i]) != parent(audio));
            let Some(i) = pick else {
                added.unpaired.push(audio.clone());
                continue;
            };
            used[i] = true;
            if self.sources.iter().any(|s| &s.audio_path == audio) {
                continue;
            }
            self.sources.push(Source {
                id: stable_id(audio),
                label: base,
                audio_path: audio.clone(),
                transcript_path: transcripts[i].clone(),
                duration: None,
                word_count: 0,
                match_count: 0,
            });
            added.added += 1;
        }
        added.unpaired.extend(transcripts.iter().zip(&used).filter(|(_, u)| !**u).map(|(t, _)| (*t).clone()));
        self.updated_at = now();
        added
    }
}

/// What [`Project::add_sources`] did
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    pub added: usize,
    /// Files that had no partner (audio without a transcript, or the reverse)
    pub unpaired: Vec<String>,
}

/// The audio files a sermon can be
pub const AUDIO: [&str; 7] = ["mp3", "m4a", "wav", "flac", "ogg", "aac", "mp4"];

fn ext(p: &str) -> String {
    Path::new(p).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// A file's name without its extensions (`sermon.mp3.json` and `sermon.json` are `sermon`)
fn base_name(p: &str) -> String {
    let mut stem = Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    while let Some((s, ext)) = stem.rsplit_once('.') {
        if ext.eq_ignore_ascii_case("json") || AUDIO.contains(&ext.to_ascii_lowercase().as_str()) {
            stem = s.to_string();
        } else {
            break;
        }
    }
    stem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_audio_with_transcripts() {
        let bible = Bible {
            id: "b".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![],
        };
        let mut p = Project::new("p", "", &bible, None);
        let paths: Vec<String> = ["a/one.mp3", "b/one.json", "a/one.json", "a/two.m4a", "a/three.json", "notes.txt"]
            .map(String::from)
            .into();
        let added = p.add_sources(&paths);
        assert_eq!(added.added, 1);
        assert_eq!(p.sources[0].transcript_path, "a/one.json");
        assert_eq!(added.unpaired, ["a/two.m4a", "b/one.json", "a/three.json"]);
        assert_eq!(p.sources[0].id, stable_id("a/one.mp3"));
        assert_eq!(p.add_sources(&paths).added, 0);
    }
}
