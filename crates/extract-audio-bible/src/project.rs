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
    bible::{Bible, BibleBook},
    error::{Error, Result},
    matcher::{Match, Run},
    scope::BookScope,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    /// What it's for: building an audio Bible, or finding every verse read
    #[serde(default)]
    pub kind: ProjectKind,
    /// The Bible it uses (for finding every verse read, the first of its translations, whose
    /// names are shown)
    pub bible_id: String,
    /// The book being built (most sermons preach through one book); none for the whole Bible
    #[serde(default)]
    pub book: Option<u8>,
    /// Only these verses of the book (2 Corinthians 2:14-17): exported as one file, or its verses
    /// as an album of their own
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passage: Option<Passage>,
    /// For finding every verse read: the Bibles searched, by id (each reading is of the one it
    /// fits best)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translations: Vec<String>,
    /// For finding every verse read: the books searched
    #[serde(default, skip_serializing_if = "BookScope::is_everything")]
    pub scope: BookScope,
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
    /// What's known about each sermon, by its file id ([`Source::file_id`]); kept apart from the
    /// sermons, so it can be given before they're added and outlives their removal
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, FileMetadata>,
}

/// What a project is for
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectKind {
    /// Building an audio Bible from the readings chosen for each verse
    #[default]
    AudioBible,
    /// Finding every verse read in the sermons, in any of several translations
    Readings,
}

/// What's known about a sermon: given by the user, or by its transcript
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct FileMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The passage it preaches ("1 Thessalonians 5:12-15")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passage: Option<String>,
    /// Who preached it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// The series it's part of ("1 Thessalonians")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
}

impl FileMetadata {
    /// This, with the fields `other` has set in its place
    pub fn merged(&self, other: &FileMetadata) -> FileMetadata {
        FileMetadata {
            title: other.title.clone().or_else(|| self.title.clone()),
            passage: other.passage.clone().or_else(|| self.passage.clone()),
            speaker: other.speaker.clone().or_else(|| self.speaker.clone()),
            series: other.series.clone().or_else(|| self.series.clone()),
        }
    }
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
    /// The sermon's id: the one its transcript gives (a Let's Church media id) once it's been
    /// searched, or its file's name. Empty in projects saved before there was one; see
    /// [`Source::file_id`]
    #[serde(default)]
    pub file_id: String,
}

impl Source {
    /// The sermon's id ([`Source::file_id`](field@Source::file_id)), or its file's name
    pub fn file_id(&self) -> String {
        if self.file_id.is_empty() {
            base_name(&self.transcript_path)
        } else {
            self.file_id.clone()
        }
    }
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
        std::iter::once(self.first())
            .chain(self.joined.iter().cloned())
            .collect()
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
    /**
    Where to download a sermon's audio when it has no file of its own, with `{id}` for its file
    id: `https://example.com/audio/{id}.mp3`. Empty for none. With one, transcripts without audio
    beside them are added as sermons too
    */
    pub audio_url: String,
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
            audio_url: String::new(),
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

/// Verses of a project's book from one to another: `2:14` to `2:17`, or across chapters, `7:24` to `8:2`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Passage {
    pub start_chapter: u16,
    pub start_verse: u16,
    pub end_chapter: u16,
    pub end_verse: u16,
}

impl Passage {
    /// Whether it has a verse
    pub fn contains(&self, chapter: u16, verse: u16) -> bool {
        (self.start_chapter, self.start_verse) <= (chapter, verse)
            && (chapter, verse) <= (self.end_chapter, self.end_verse)
    }

    /// Whether it has any of a chapter's verses
    pub fn has_chapter(&self, chapter: u16) -> bool {
        self.start_chapter <= chapter && chapter <= self.end_chapter
    }

    /// `2:14-17`, `7:24-8:2`, or `3:16`
    pub fn label(&self) -> String {
        let (sc, sv, ec, ev) = (self.start_chapter, self.start_verse, self.end_chapter, self.end_verse);
        if sc != ec {
            format!("{sc}:{sv}-{ec}:{ev}")
        } else if sv != ev {
            format!("{sc}:{sv}-{ev}")
        } else {
            format!("{sc}:{sv}")
        }
    }

    /**
    Verses of `book` written as `2:14-17`, `7:24-8:2`, `3:16`, a chapter (`23`), or chapters
    (`23-24`); a dash may be an en or em dash. An error if it isn't one, or the book hasn't those
    verses
    */
    pub fn parse(text: &str, book: &BibleBook) -> Result<Self> {
        let text: String = text.trim().replace(['–', '—'], "-").split_whitespace().collect();
        let bad = || Error::invalid(format!("{text}: not verses of {} (like 2:14-17)", book.name));
        let number = |s: &str| s.parse::<u16>().ok().filter(|&n| n > 0);
        let verses_in = |chapter: u16| book.chapters.get(chapter as usize - 1).map(|c| c.len() as u16);
        let (from, to) = text
            .split_once('-')
            .map_or((text.as_str(), None), |(a, b)| (a, Some(b)));
        let (start_chapter, start_verse) = match from.split_once(':') {
            Some((c, v)) => (number(c).ok_or_else(bad)?, Some(number(v).ok_or_else(bad)?)),
            None => (number(from).ok_or_else(bad)?, None),
        };
        let (end_chapter, end_verse) = match to {
            None => (start_chapter, start_verse),
            Some(to) => match to.split_once(':') {
                Some((c, v)) => (number(c).ok_or_else(bad)?, Some(number(v).ok_or_else(bad)?)),
                // `2:14-17` is verses of one chapter; `23-24` is chapters
                None if start_verse.is_some() => (start_chapter, Some(number(to).ok_or_else(bad)?)),
                None => (number(to).ok_or_else(bad)?, None),
            },
        };
        let start_count = verses_in(start_chapter).ok_or_else(bad)?;
        let end_count = verses_in(end_chapter).ok_or_else(bad)?;
        let passage = Passage {
            start_chapter,
            start_verse: start_verse.unwrap_or(1),
            end_chapter,
            end_verse: end_verse.unwrap_or(end_count),
        };
        if passage.start_verse > start_count
            || passage.end_verse > end_count
            || (passage.start_chapter, passage.start_verse) > (passage.end_chapter, passage.end_verse)
        {
            return Err(Error::invalid(format!(
                "{} {text}: {} hasn't those verses",
                book.name, book.name
            )));
        }
        Ok(passage)
    }
}

/**
A book, or verses of one, of a Bible: `2 Corinthians 2:14-17`, `Romans 7:24-8:2`, `Psalm 23`, or
`1 Thessalonians` (the whole book, no passage)
*/
pub fn parse_reference(text: &str, bible: &Bible) -> Result<(u8, Option<Passage>)> {
    let text = text.trim();
    let find = |name: &str| {
        let id = crate::bible::find_book(name)?.0;
        bible.book(id).map(|b| (id, b))
    };
    // The verses are what follows the book's name: its last word, if that's numbers
    if let Some((name, verses)) = text.rsplit_once(' ')
        && verses.starts_with(|c: char| c.is_ascii_digit())
        && let Some((id, book)) = find(name)
    {
        return Ok((id, Some(Passage::parse(verses, book)?)));
    }
    find(text)
        .map(|(id, _)| (id, None))
        .ok_or_else(|| Error::not_found(format!("{text}: no such book in {}", bible.name)))
}

/**
Changes to a project, as an editor sends them while it's edited (rather than all of it, which with
thousands of sermons is megabytes): each part given replaces the project's, and its chosen verses
and what's known of its sermons change entry by entry
*/
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectChanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ProjectKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bible_id: Option<String>,
    /// `Some(None)` for the whole Bible (`null`), `None` (left out) for no change
    #[serde(deserialize_with = "given", skip_serializing_if = "Option::is_none")]
    pub book: Option<Option<u8>>,
    /// `Some(None)` for the whole book (`null`), `None` (left out) for no change
    #[serde(deserialize_with = "given", skip_serializing_if = "Option::is_none")]
    pub passage: Option<Option<Passage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<BookScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<Source>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selections: Option<Keyed<Selection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Keyed<FileMetadata>>,
}

/// Entries of a map changed: those set (added or changed), and those taken out, by key
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Keyed<T> {
    pub set: BTreeMap<String, T>,
    pub removed: Vec<String>,
}

impl<T> Default for Keyed<T> {
    fn default() -> Self {
        Keyed {
            set: BTreeMap::new(),
            removed: vec![],
        }
    }
}

impl<T> Keyed<T> {
    fn apply(self, map: &mut BTreeMap<String, T>) {
        for key in &self.removed {
            map.remove(key);
        }
        map.extend(self.set);
    }
}

/// A field given (even as `null`) is `Some`, so `null` can be told from left out
fn given<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

impl Project {
    /// The project with `changes` made (see [`ProjectChanges`])
    pub fn apply(&mut self, changes: ProjectChanges) {
        let ProjectChanges {
            name,
            kind,
            bible_id,
            book,
            passage,
            translations,
            scope,
            sources,
            selections,
            settings,
            metadata,
            files,
        } = changes;
        fn set<T>(field: &mut T, value: Option<T>) {
            if let Some(value) = value {
                *field = value;
            }
        }
        set(&mut self.name, name);
        set(&mut self.kind, kind);
        set(&mut self.bible_id, bible_id);
        set(&mut self.book, book);
        set(&mut self.passage, passage);
        set(&mut self.translations, translations);
        set(&mut self.scope, scope);
        set(&mut self.sources, sources);
        set(&mut self.settings, settings);
        set(&mut self.metadata, metadata);
        if let Some(selections) = selections {
            selections.apply(&mut self.selections);
        }
        if let Some(files) = files {
            files.apply(&mut self.files);
        }
    }

    /**
    A new project drawing on `bible`, for one `book` or the whole Bible. With no name, it's
    named after the book and translation ("John (ESV)")
    */
    pub fn new(id: impl Into<String>, name: &str, bible: &Bible, book: Option<u8>) -> Self {
        let book_name = book.and_then(|b| bible.book(b)).map(|b| b.name.clone());
        let metadata = Metadata {
            language: bible.language.clone().unwrap_or_default(),
            ..Default::default()
        };
        let translation = if bible.abbreviation.is_empty() {
            &bible.name
        } else {
            &bible.abbreviation
        };
        Project {
            id: id.into(),
            name: match (name.trim(), book_name) {
                ("", Some(book)) => format!("{book} ({translation})"),
                ("", None) => format!("{} audio Bible", bible.name),
                (name, _) => name.into(),
            },
            kind: ProjectKind::AudioBible,
            bible_id: bible.id.clone(),
            book,
            passage: None,
            translations: vec![],
            scope: BookScope::default(),
            created_at: now(),
            updated_at: now(),
            sources: vec![],
            selections: BTreeMap::new(),
            settings: Default::default(),
            metadata,
            files: BTreeMap::new(),
        }
    }

    /**
    A project finding every verse read in its sermons, in any of `bibles` (the first's names
    shown), in the books of `scope`. With no name, it's named after the translations. With no
    Bibles, it's an error
    */
    pub fn readings(id: impl Into<String>, name: &str, bibles: &[&Bible], scope: BookScope) -> Result<Self> {
        let first = bibles
            .first()
            .ok_or_else(|| crate::Error::invalid("choose at least one translation"))?;
        let abbreviations: Vec<&str> = bibles
            .iter()
            .map(|b| {
                if b.abbreviation.is_empty() {
                    b.name.as_str()
                } else {
                    b.abbreviation.as_str()
                }
            })
            .collect();
        let mut project = Project::new(id, name, first, None);
        if name.trim().is_empty() {
            project.name = format!("Verses read ({})", abbreviations.join(", "));
        }
        project.kind = ProjectKind::Readings;
        project.translations = bibles.iter().map(|b| b.id.clone()).collect();
        project.scope = scope;
        Ok(project)
    }

    /// The Bibles its sermons are searched in, by id
    pub fn bibles_searched(&self) -> Vec<String> {
        match self.kind {
            ProjectKind::Readings if !self.translations.is_empty() => self.translations.clone(),
            _ => vec![self.bible_id.clone()],
        }
    }

    /// The books its sermons are searched for, by number, or none for every book: its book (for
    /// an audio Bible), or its scope (for finding every verse read)
    /**
    An audio Bible of a passage of a book (2 Corinthians 2:14-17): named after it unless `name` is
    given, its file exported for the whole passage named after it, and its verses' files an album
    of their own
    */
    pub fn passage(id: impl Into<String>, name: &str, bible: &Bible, book: u8, passage: Passage) -> Self {
        let reference = bible
            .book(book)
            .map_or_else(|| passage.label(), |b| format!("{} {}", b.name, passage.label()));
        let translation = if bible.abbreviation.is_empty() {
            &bible.name
        } else {
            &bible.abbreviation
        };
        let name = if name.trim().is_empty() {
            format!("{reference} ({translation})")
        } else {
            name.trim().to_string()
        };
        let mut project = Project::new(id, &name, bible, Some(book));
        project.passage = Some(passage);
        project.metadata.title_template = "{passage}".into();
        project.metadata.filename_template = "{passage}".into();
        project.metadata.album_template = "{passage} ({abbreviation})".into();
        project
    }

    /// Whether a verse is one the project is for: of its book (if it has one), in its passage (if it has one)
    pub fn covers(&self, book: u8, chapter: u16, verse: u16) -> bool {
        self.book.is_none_or(|b| b == book) && self.passage.is_none_or(|p| p.contains(chapter, verse))
    }

    /// What it's of: "2 Corinthians 2:14-17", "1 Thessalonians", or none for the whole Bible
    pub fn reference(&self, bible: &Bible) -> Option<String> {
        let book = bible.book(self.book?)?;
        Some(match self.passage {
            Some(p) => format!("{} {}", book.name, p.label()),
            None => book.name.clone(),
        })
    }

    pub fn books_searched(&self) -> Result<Option<std::collections::BTreeSet<u8>>> {
        match self.kind {
            ProjectKind::AudioBible => Ok(self.book.map(|b| std::collections::BTreeSet::from([b]))),
            ProjectKind::Readings => self.scope.books(),
        }
    }

    /**
    Where a sermon's audio is: its own file, or (when it has none) the project's audio URL filled
    in with its file id, for `Downloading` files (the `download` feature) or a host to fetch.
    Empty when it has neither
    */
    pub fn audio_path(&self, source: &Source) -> String {
        if !source.audio_path.is_empty() {
            return source.audio_path.clone();
        }
        let template = self.settings.audio_url.trim();
        if template.is_empty() {
            String::new()
        } else {
            template.replace("{id}", &url_encode(&source.file_id()))
        }
    }

    /// What's known about a sermon
    pub fn file_metadata(&self, source: &Source) -> FileMetadata {
        self.files.get(&source.file_id()).cloned().unwrap_or_default()
    }

    /// A sermon's title, or its file's name when it has none
    pub fn source_title(&self, source: &Source) -> String {
        self.file_metadata(source).title.unwrap_or_else(|| source.label.clone())
    }

    /**
    Sets what's known about sermons by their file ids, field by field (a field left out is kept),
    from JSON like `{"1qUVQV7mkmdh3TxbEwHHtA": {"title": "Encourage One Another", "passage":
    "1 Thess 5:12-15", "speaker": "Paul Washer", "series": "1 Thessalonians"}}`. Returns how many
    ids were given
    */
    pub fn apply_file_metadata(&mut self, given: BTreeMap<String, FileMetadata>) -> usize {
        let count = given.len();
        for (id, meta) in given {
            let merged = self.files.get(&id).cloned().unwrap_or_default().merged(&meta);
            self.files.insert(id, merged);
        }
        self.updated_at = now();
        count
    }

    /**
    Records what a sermon's transcript says of it: its id becomes the sermon's (what was known
    by its old id moves with it), and its title is the sermon's if none was given
    */
    pub fn identify(&mut self, source_id: &str, id: Option<&str>, title: Option<&str>) {
        let Some(source) = self.sources.iter_mut().find(|s| s.id == source_id) else {
            return;
        };
        let old = source.file_id();
        if let Some(id) = id
            && id != old
        {
            source.file_id = id.to_string();
            if let Some(known) = self.files.remove(&old) {
                let merged = self.files.get(id).cloned().unwrap_or_default();
                self.files.insert(id.to_string(), known.merged(&merged));
            }
        } else if source.file_id.is_empty() {
            source.file_id = old;
        }
        let file_id = source.file_id.clone();
        if let Some(title) = title {
            let meta = self.files.entry(file_id).or_default();
            meta.title.get_or_insert_with(|| title.to_string());
        }
    }

    /**
    Adds sermons from file paths: each audio file is paired with the transcript (`.json`) of
    the same name, in the same folder first, then anywhere among the paths. A transcript without
    audio is a sermon too: it can be searched, and its readings found, and its audio is
    downloaded when it's needed if the project says where from ([`Settings::audio_url`]).
    Sermons already in the project are skipped. Returns how many were added and the audio files
    that had no transcript. To leave out JSON files that aren't transcripts, use
    [`Project::add_sources_from`]
    */
    pub fn add_sources(&mut self, paths: &[String]) -> Added {
        let mut added = self.pair_sources(paths);
        let (transcripts, rest): (Vec<String>, Vec<String>) = added.unpaired.drain(..).partition(|f| ext(f) == "json");
        added.unpaired = rest;
        let mut had: std::collections::HashSet<String> =
            self.sources.iter().map(|s| s.transcript_path.clone()).collect();
        for transcript in transcripts {
            if !had.insert(transcript.clone()) {
                continue;
            }
            let base = base_name(&transcript);
            self.sources.push(Source {
                id: stable_id(&transcript),
                label: base.clone(),
                audio_path: String::new(),
                transcript_path: transcript,
                duration: None,
                word_count: 0,
                match_count: 0,
                file_id: base,
            });
            added.added += 1;
        }
        added
    }

    /**
    [`Project::add_sources`], reading each JSON file through `files` first: one that isn't a
    transcript (a project's own files, say) isn't added, and is given back in
    [`Added::skipped`]
    */
    pub fn add_sources_from(&mut self, files: &dyn crate::Files, paths: &[String]) -> Added {
        self.add_sources_with(files, paths, &|_, _| {})
    }

    /**
    [`Project::add_sources_from`], telling `progress` how many JSON files have been read of how many
    (from several threads at once, natively). Transcripts already in the project aren't read again
    */
    pub fn add_sources_with(
        &mut self,
        files: &dyn crate::Files,
        paths: &[String],
        progress: &(dyn Fn(usize, usize) + Sync),
    ) -> Added {
        let had: std::collections::HashSet<&str> = self.sources.iter().map(|s| s.transcript_path.as_str()).collect();
        let to_read: Vec<&String> = (paths.iter())
            .filter(|p| ext(p) == "json" && !had.contains(p.as_str()))
            .collect();
        let total = to_read.len();
        let done = std::sync::atomic::AtomicUsize::new(0);
        let check = |p: &&String| {
            let ok = files
                .read(p)
                .is_ok_and(|bytes| crate::transcript::is_transcript(&bytes));
            progress(done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1, total);
            ok
        };
        // Read several at once: tens of thousands of gzipped transcripts take minutes one at a time
        #[cfg(not(target_arch = "wasm32"))]
        let transcripts: std::collections::HashSet<&String> = {
            use rayon::prelude::*;
            to_read.into_par_iter().filter(check).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let transcripts: std::collections::HashSet<&String> = to_read.into_iter().filter(check).collect();
        let (kept, skipped): (Vec<String>, Vec<String>) = (paths.iter().cloned())
            .partition(|p| ext(p) != "json" || had.contains(p.as_str()) || transcripts.contains(p));
        drop(had);
        let mut added = self.add_sources(&kept);
        added.skipped = skipped;
        added
    }

    fn pair_sources(&mut self, paths: &[String]) -> Added {
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
                file_id: base_name(audio),
            });
            added.added += 1;
        }
        added.unpaired.extend(
            transcripts
                .iter()
                .zip(&used)
                .filter(|(_, u)| !**u)
                .map(|(t, _)| (*t).clone()),
        );
        self.updated_at = now();
        added
    }
}

/// What [`Project::add_sources`] did
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    pub added: usize,
    /// Audio files that had no transcript
    pub unpaired: Vec<String>,
    /// JSON files that aren't transcripts (from [`Project::add_sources_from`])
    #[serde(default)]
    pub skipped: Vec<String>,
}

/// `text` safe in a URL: letters, digits, and `-._~` as they are, everything else percent-encoded
fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The audio files a sermon can be
pub const AUDIO: [&str; 7] = ["mp3", "m4a", "wav", "flac", "ogg", "aac", "mp4"];

/// A file's extension, lowercase (a gzipped transcript's, `.json.gz`, is `json`)
fn ext(p: &str) -> String {
    let ext = Path::new(p)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "gz" && p.to_ascii_lowercase().ends_with(".json.gz") {
        return "json".into();
    }
    ext
}

/// A file's name without its extensions (`sermon.mp3.json`, `sermon.json`, and `sermon.json.gz`
/// are `sermon`)
fn base_name(p: &str) -> String {
    let mut stem = Path::new(p)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if stem.to_ascii_lowercase().ends_with(".json.gz") {
        stem.truncate(stem.len() - ".gz".len());
    }
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

    /// Romans 7:1-25 and 8:1-39, and 2 Corinthians 2:1-17, as verse numbers
    fn bible_with_passages() -> Bible {
        let mut lines = String::new();
        for (book, chapter, verses) in [("Romans", 7, 25), ("Romans", 8, 39), ("2 Corinthians", 2, 17)] {
            for v in 1..=verses {
                lines += &format!("{book} {chapter}:{v} Verse {v}.\n");
            }
        }
        Bible::new(
            "b",
            "Test",
            "T",
            crate::import(vec![("b.txt".into(), lines.into_bytes())]).unwrap(),
        )
    }

    #[test]
    fn passages_read_and_named() {
        let bible = bible_with_passages();
        let (cor, rom) = (47, 45);
        let p = |text: &str| parse_reference(text, &bible).map(|(b, p)| (b, p.map(|p| p.label())));
        assert_eq!(p("2 Corinthians 2:14-17").unwrap(), (cor, Some("2:14-17".into())));
        assert_eq!(p("2 Cor 2:14–17").unwrap(), (cor, Some("2:14-17".into())));
        assert_eq!(p("Romans 7:24-8:2").unwrap(), (rom, Some("7:24-8:2".into())));
        assert_eq!(p("Romans 8").unwrap(), (rom, Some("8:1-39".into())));
        assert_eq!(p("Romans 7-8").unwrap(), (rom, Some("7:1-8:39".into())));
        assert_eq!(p("Romans 8:28").unwrap(), (rom, Some("8:28".into())));
        assert_eq!(p("Romans").unwrap(), (rom, None));
        // Verses the book hasn't, backwards, or not verses at all
        for bad in ["Romans 8:40", "Romans 9", "Romans 8:5-2", "Romans 8:x", "Hezekiah 3:1"] {
            assert!(p(bad).is_err(), "{bad}");
        }

        let romans = bible.book(rom).unwrap();
        let passage = Passage::parse("7:24-8:2", romans).unwrap();
        assert!(
            passage.contains(7, 25) && passage.contains(8, 1) && !passage.contains(7, 23) && !passage.contains(8, 3)
        );
        assert!(passage.has_chapter(8) && !passage.has_chapter(9));

        let project = Project::passage("p", "", &bible, rom, passage);
        assert_eq!(project.name, "Romans 7:24-8:2 (T)");
        assert_eq!(project.reference(&bible).as_deref(), Some("Romans 7:24-8:2"));
        assert!(project.covers(rom, 8, 2) && !project.covers(rom, 8, 3) && !project.covers(cor, 2, 14));
        // Kept as it was saved
        let saved: Project = serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
        assert_eq!(saved.passage, Some(passage));
    }

    #[test]
    fn changes_made_part_by_part() {
        let bible = Bible {
            id: "b".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![],
        };
        let mut p = Project::new("p", "", &bible, Some(43));
        p.add_sources(&["a.json".into(), "b.json".into()]);
        let selection = |id: &str| Selection {
            match_id: id.into(),
            source_id: "s".into(),
            start: 0.0,
            end: 1.0,
            trim_start: 0.0,
            trim_end: 0.0,
            pad_before: 0.0,
            pad_after: 0.0,
            gain_db: 0.0,
            needs_trim: false,
            note: String::new(),
            joined: vec![],
        };
        p.selections.insert("43.3.16".into(), selection("old"));
        p.selections.insert("43.3.17".into(), selection("kept"));
        let sources = p.sources.clone();

        // As an editor sends them: only what changed, `null` meaning none
        let changes: ProjectChanges = serde_json::from_str(
            r#"{"name": "John", "selections": {"set": {"43.3.18": {"matchId": "new", "sourceId": "s", "start": 2, "end": 3,
                "trimStart": 0, "trimEnd": 0, "padBefore": 0, "padAfter": 0, "gainDb": 0, "needsTrim": false, "note": ""}},
                "removed": ["43.3.16"]}, "files": {"set": {"a": {"speaker": "Paul"}}}, "book": null}"#,
        )
        .unwrap();
        p.apply(changes);
        assert_eq!((p.name.as_str(), p.book), ("John", None));
        assert_eq!(p.selections.keys().collect::<Vec<_>>(), ["43.3.17", "43.3.18"]);
        assert_eq!(p.files["a"].speaker.as_deref(), Some("Paul"));
        assert_eq!(p.sources.len(), sources.len());

        // Left out is left as it is
        p.apply(serde_json::from_str(r#"{"files": {"removed": ["a"]}}"#).unwrap());
        assert_eq!((p.name.as_str(), p.book, p.selections.len()), ("John", None, 2));
        assert!(p.files.is_empty());
        p.apply(serde_json::from_str(r#"{"book": 44}"#).unwrap());
        assert_eq!(p.book, Some(44));
    }

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
        let paths: Vec<String> = [
            "a/one.mp3",
            "b/one.json",
            "a/one.json",
            "a/two.m4a",
            "a/three.json",
            "notes.txt",
        ]
        .map(String::from)
        .into();
        let added = p.add_sources(&paths);
        // The pair, and the two transcripts without audio
        assert_eq!(added.added, 3);
        assert_eq!(p.sources[0].transcript_path, "a/one.json");
        assert_eq!(added.unpaired, ["a/two.m4a"]);
        assert_eq!(p.sources[0].id, stable_id("a/one.mp3"));
        assert_eq!(
            p.sources.iter().map(|s| s.audio_path.as_str()).collect::<Vec<_>>(),
            ["a/one.mp3", "", ""]
        );
        assert_eq!(p.add_sources(&paths).added, 0);
    }

    #[test]
    fn json_that_isnt_a_transcript_is_skipped() {
        let bible = Bible {
            id: "b".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![],
        };
        let mut p = Project::new("p", "", &bible, None);
        let mut files = crate::Memory::new();
        files.insert(
            "whisperx.json",
            br#"{"segments": [{"start": 0, "end": 1, "text": "Jesus wept."}]}"#.to_vec(),
        );
        files.insert(
            "lets-church.json",
            br#"{"mediaId": "m", "paragraphs": [{"start": 0, "end": 1, "text": "Amen."}]}"#.to_vec(),
        );
        files.insert(
            "words.json",
            br#"{"word_segments": [{"word": "Amen", "start": 0, "end": 1}]}"#.to_vec(),
        );
        files.insert("project.json", br#"{"id": "p", "sources": []}"#.to_vec());
        files.insert("broken.json", b"{".to_vec());
        let paths: Vec<String> = [
            "whisperx.json",
            "lets-church.json",
            "words.json",
            "project.json",
            "broken.json",
            "missing.json",
        ]
        .map(String::from)
        .into();
        let added = p.add_sources_from(&files, &paths);
        assert_eq!(added.added, 3);
        assert_eq!(added.skipped, ["project.json", "broken.json", "missing.json"]);
    }

    #[test]
    fn audio_from_a_url_by_file_id() {
        let bible = Bible {
            id: "b".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![],
        };
        let mut p = Project::new("p", "", &bible, None);
        let paths: Vec<String> = ["a/one.mp3", "a/one.json", "b/two words.json"].map(String::from).into();
        // Without an audio URL, a transcript alone is a sermon with no audio
        let mut without = p.clone();
        assert_eq!(without.add_sources(&paths).added, 2);
        assert_eq!(without.audio_path(&without.sources[1]), "");

        p.settings.audio_url = "https://example.com/audio/{id}.mp3".into();
        let added = p.add_sources(&paths);
        assert_eq!((added.added, added.unpaired.len()), (2, 0));
        assert_eq!(p.audio_path(&p.sources[0]), "a/one.mp3");
        assert_eq!(p.audio_path(&p.sources[1]), "https://example.com/audio/two%20words.mp3");
        // Its id from its transcript, once searched
        let id = p.sources[1].id.clone();
        p.identify(&id, Some("xyz"), None);
        assert_eq!(p.audio_path(&p.sources[1]), "https://example.com/audio/xyz.mp3");
        assert_eq!(p.add_sources(&paths).added, 0);
    }

    #[test]
    fn gzipped_transcripts() {
        let bible = Bible {
            id: "b".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![],
        };
        let mut p = Project::new("p", "", &bible, None);
        let paths: Vec<String> = ["a/one.mp3", "a/one.json.gz", "b/Two.JSON.GZ", "c/three.gz"]
            .map(String::from)
            .into();
        let added = p.add_sources(&paths);
        assert_eq!((added.added, added.unpaired.len()), (2, 0));
        assert_eq!(
            (p.sources[0].audio_path.as_str(), p.sources[0].transcript_path.as_str()),
            ("a/one.mp3", "a/one.json.gz")
        );
        assert_eq!(
            (p.sources[0].file_id(), p.sources[1].file_id()),
            ("one".into(), "Two".into())
        );
    }

    #[test]
    fn file_metadata_follows_the_transcript_id() {
        let bible = Bible {
            id: "b".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![],
        };
        let mut p = Project::new("p", "", &bible, None);
        p.add_sources(&["a/one.mp3".into(), "a/one.json".into()]);
        let source = p.sources[0].id.clone();
        let given = |title: Option<&str>, passage: Option<&str>| FileMetadata {
            title: title.map(Into::into),
            passage: passage.map(Into::into),
            ..Default::default()
        };
        p.apply_file_metadata([("one".into(), given(None, Some("John 11")))].into());
        assert_eq!(p.source_title(&p.sources[0]), "one");

        // Searching finds its transcript's id and title: what was given moves to the id
        p.identify(&source, Some("xyz"), Some("Jesus Wept"));
        assert_eq!(p.sources[0].file_id(), "xyz");
        assert_eq!(
            p.file_metadata(&p.sources[0]),
            given(Some("Jesus Wept"), Some("John 11"))
        );

        // A title given wins over the transcript's, and a field left out is kept
        p.apply_file_metadata([("xyz".into(), given(Some("Lazarus"), None))].into());
        p.identify(&source, Some("xyz"), Some("Jesus Wept"));
        assert_eq!(p.file_metadata(&p.sources[0]), given(Some("Lazarus"), Some("John 11")));
        assert_eq!(p.source_title(&p.sources[0]), "Lazarus");

        // Speaker and series too, from JSON
        let json = r#"{"xyz": {"speaker": "Sam Renihan", "series": "John"}}"#;
        p.apply_file_metadata(serde_json::from_str(json).unwrap());
        let meta = p.file_metadata(&p.sources[0]);
        assert_eq!(
            (meta.speaker.as_deref(), meta.series.as_deref()),
            (Some("Sam Renihan"), Some("John"))
        );
        assert_eq!(meta.title.as_deref(), Some("Lazarus"));
    }
}
