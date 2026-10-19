//! A project: the sermons it draws from, the reading chosen for each verse, and how to export.
//! Everything is kept as JSON in the app's data folder.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    bible::{Bible, BibleSummary},
    error::{AppError, Result},
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
    /// Chosen, but still to be trimmed
    /// Volume change for this verse, in dB
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub needs_trim: bool,
    #[serde(default)]
    pub note: String,
}

impl Selection {
    pub fn clip(&self) -> (f64, f64) {
        let s = (self.start + self.trim_start).max(0.0);
        (s, (self.end + self.trim_end).max(s + 0.05))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub min_score: f32,
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
            min_score: 0.45,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub bible_id: String,
    pub book: Option<u8>,
    pub updated_at: u64,
    pub source_count: usize,
    pub selection_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSet {
    pub analyzed_at: u64,
    pub matches: Vec<Match>,
    pub runs: Vec<Run>,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Where everything is kept
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written beside, then moved, so a crash never leaves half a file
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(value)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path).map_err(|e| AppError::msg(format!("{}: {e}", path.display())))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn safe_id(id: &str) -> Result<&str> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::msg(format!("Bad id: {id}")));
    }
    Ok(id)
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    fn bible_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self.root.join("bibles").join(format!("{}.json", safe_id(id)?)))
    }

    fn project_dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.root.join("projects").join(safe_id(id)?))
    }

    pub fn save_bible(&self, bible: &Bible) -> Result<()> {
        write_json(&self.bible_path(&bible.id)?, bible)?;
        write_json(&self.root.join("bibles").join(format!("{}.summary", bible.id)), &bible.summary())
    }

    pub fn bible(&self, id: &str) -> Result<Bible> {
        read_json(&self.bible_path(id)?)
    }

    pub fn delete_bible(&self, id: &str) -> Result<()> {
        let path = self.bible_path(id)?;
        let _ = std::fs::remove_file(path.with_extension("summary"));
        Ok(std::fs::remove_file(path)?)
    }

    pub fn bibles(&self) -> Result<Vec<BibleSummary>> {
        let dir = self.root.join("bibles");
        let mut out = vec![];
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "summary") {
                if let Ok(s) = read_json::<BibleSummary>(&path) {
                    out.push(s);
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn save_project(&self, project: &Project) -> Result<()> {
        write_json(&self.project_dir(&project.id)?.join("project.json"), project)
    }

    pub fn project(&self, id: &str) -> Result<Project> {
        read_json(&self.project_dir(id)?.join("project.json"))
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        Ok(std::fs::remove_dir_all(self.project_dir(id)?)?)
    }

    pub fn projects(&self) -> Result<Vec<ProjectSummary>> {
        let mut out = vec![];
        for entry in std::fs::read_dir(self.root.join("projects")).into_iter().flatten().flatten() {
            if let Ok(p) = read_json::<Project>(&entry.path().join("project.json")) {
                out.push(ProjectSummary {
                    id: p.id,
                    name: p.name,
                    bible_id: p.bible_id,
                    book: p.book,
                    updated_at: p.updated_at,
                    source_count: p.sources.len(),
                    selection_count: p.selections.len(),
                });
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub fn save_matches(&self, project_id: &str, matches: &MatchSet) -> Result<()> {
        write_json(&self.project_dir(project_id)?.join("matches.json"), matches)
    }

    pub fn matches(&self, project_id: &str) -> Result<MatchSet> {
        let path = self.project_dir(project_id)?.join("matches.json");
        if !path.exists() {
            return Ok(MatchSet::default());
        }
        read_json(&path)
    }
}
