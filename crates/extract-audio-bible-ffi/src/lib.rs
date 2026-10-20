//! The API that every language binding sees: owned data, a few objects, no lifetimes.
//!
//! Hosts start from [`ExtractAudioBible`]. Files come from a [`Files`] object: the disk
//! natively, or bytes the host adds (WebAssembly has no files).

mod project;
mod records;
#[cfg(test)]
mod tests;

use std::sync::{Arc, OnceLock, RwLock};

use boltffi::*;
use eab::{MatchSet, scripture_quotes::Engine};
pub use project::*;
pub use records::*;

/// What went wrong, by kind
#[error]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtractAudioBibleError {
    /// A file, book, chapter, sermon, or reading that isn't there
    NotFound { message: String },
    /// Reading or writing failed (or a method that needs files was called in WebAssembly)
    Io { message: String },
    /// Input that can't be used: a transcript that isn't JSON, an unknown book, bad settings
    Invalid { message: String },
    /// Audio that can't be decoded or encoded
    Audio { message: String },
    /// Audio given by a URL that couldn't be downloaded
    Network { message: String },
}

impl From<eab::Error> for ExtractAudioBibleError {
    fn from(error: eab::Error) -> Self {
        use eab::ErrorKind as K;
        let message = error.to_string();
        match error.kind() {
            K::NotFound => Self::NotFound { message },
            K::Io => Self::Io { message },
            K::Audio => Self::Audio { message },
            K::Network => Self::Network { message },
            // `Invalid`, and any kind a newer library adds
            _ => Self::Invalid { message },
        }
    }
}

fn invalid(message: impl Into<String>) -> ExtractAudioBibleError {
    ExtractAudioBibleError::Invalid {
        message: message.into(),
    }
}

fn not_found(message: impl Into<String>) -> ExtractAudioBibleError {
    ExtractAudioBibleError::NotFound {
        message: message.into(),
    }
}

/// A book by any of its names (`1 Thess`, `1Th`, `52`), or none
fn book_named(name: Option<String>) -> Result<Option<u8>, ExtractAudioBibleError> {
    match name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => eab::bible::find_book(name)
            .map(|b| Some(b.0))
            .ok_or_else(|| invalid(format!("{name}: not a book of the Bible"))),
        None => Ok(None),
    }
}

/**
Where everything starts: read Bibles and transcripts, start or open projects.

```ts
import { ExtractAudioBible, Files } from "extract-audio-bible";

const eab = new ExtractAudioBible();
const bible = eab.importBible([{ path: "JHN.usfm", bytes }], "King James Version", "KJV");
const project = eab.newProject(bible, "", "John");
```
*/
#[derive(Default)]
pub struct ExtractAudioBible;

#[export]
impl ExtractAudioBible {
    pub fn new() -> Self {
        Self
    }

    /// This library's version
    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    /**
    A Bible from its files, given as bytes: USFM, USX, OSIS, ThML, GBF, JSON, CSV/TSV
    (`book, chapter, verse, text`), or lines like `John 3:16 For God so loved…`. Each file's
    name says its format. With no abbreviation, the name is used
    */
    pub fn import_bible(
        &self,
        files: Vec<InputFile>,
        name: String,
        abbreviation: String,
    ) -> Result<Bible, ExtractAudioBibleError> {
        let imported = eab::import(files.into_iter().map(|f| (f.path, f.bytes)).collect())?;
        Ok(Bible::new(named_bible(&name, &abbreviation, imported)))
    }

    /// [`ExtractAudioBible::import_bible`] from files and folders on disk (native only; in
    /// WebAssembly, read the files and use `importBible`)
    pub fn import_bible_from_paths(
        &self,
        paths: Vec<String>,
        name: String,
        abbreviation: String,
    ) -> Result<Bible, ExtractAudioBibleError> {
        import_paths(paths, name, abbreviation)
    }

    /// A Bible saved with `Bible.toJson` (or by the Extract Audio Bible app)
    pub fn read_bible(&self, json: String) -> Result<Bible, ExtractAudioBibleError> {
        let bible: eab::Bible = serde_json::from_str(&json).map_err(|e| invalid(format!("not a Bible's JSON: {e}")))?;
        Ok(Bible::new(bible))
    }

    /// A transcript's JSON: WhisperX output, or a Let's Church transcript (paragraphs with or
    /// without word times)
    pub fn read_transcript(&self, json: String) -> Result<Transcript, ExtractAudioBibleError> {
        Ok(Transcript {
            inner: Arc::new(eab::Transcript::parse(&json)?),
        })
    }

    /// A transcript file through `files`
    pub fn open_transcript(&self, files: &Files, path: String) -> Result<Transcript, ExtractAudioBibleError> {
        Ok(Transcript {
            inner: Arc::new(eab::Transcript::open(files, &path)?),
        })
    }

    /**
    A new project drawing on `bible`, for one book (any of its names: `1 Thess`, `52`) or, with
    none, the whole Bible. With no name it's named after the book and translation
    */
    pub fn new_project(
        &self,
        bible: &Bible,
        name: String,
        book: Option<String>,
    ) -> Result<Project, ExtractAudioBibleError> {
        let book = book_named(book)?;
        if let Some(b) = book
            && bible.inner.book(b).is_none()
        {
            return Err(not_found(format!("{} doesn't have book {b}", bible.inner.name)));
        }
        let id = eab::project::stable_id(&format!("{}{}{}", bible.inner.id, name, eab::project::now()));
        Ok(Project::new(
            eab::Project::new(id, &name, &bible.inner, book),
            MatchSet::default(),
        ))
    }

    /// A project saved with `Project.toJson`, and its readings from `Project.readingsJson` if
    /// kept (search again otherwise)
    pub fn read_project(
        &self,
        project_json: String,
        readings_json: Option<String>,
    ) -> Result<Project, ExtractAudioBibleError> {
        let project = serde_json::from_str(&project_json).map_err(|e| invalid(format!("not a project's JSON: {e}")))?;
        let matches = match readings_json {
            Some(json) => serde_json::from_str(&json).map_err(|e| invalid(format!("not a project's readings: {e}")))?,
            None => MatchSet::default(),
        };
        Ok(Project::new(project, matches))
    }

    /**
    Finds verses in transcripts without a project: one book's (any of its names) or, with none,
    the whole Bible's, read at least `minAccuracy` faithfully (0–1; 0.85 is the default)
    */
    pub fn searcher(
        &self,
        bible: &Bible,
        book: Option<String>,
        min_accuracy: f64,
    ) -> Result<Searcher, ExtractAudioBibleError> {
        let book = book_named(book)?;
        let engine = bible.engine();
        eab::matcher::searcher(&engine, book).map_err(|e| invalid(e.to_string()))?;
        Ok(Searcher {
            engine,
            book,
            min_accuracy: min_accuracy as f32,
        })
    }

    /// `start` to `end` seconds of an audio file as WAV (mono, at the file's own rate unless
    /// processed)
    pub fn render_clip(
        &self,
        files: &Files,
        path: String,
        start: f64,
        end: f64,
        options: ClipOptions,
    ) -> Result<Vec<u8>, ExtractAudioBibleError> {
        let pcm = eab::render_clip(files, &path, start, end, &(&options).into())?;
        Ok(eab::audio::wav(&pcm))
    }

    /// A book's number (1 for Genesis) from any of its names (`1 Thess`, `1Th`, `52`)
    pub fn book_id(&self, name: String) -> Option<u8> {
        eab::bible::find_book(&name).map(|b| b.0)
    }
}

fn named_bible(name: &str, abbreviation: &str, imported: eab::Imported) -> eab::Bible {
    let name = if name.trim().is_empty() {
        "Untitled translation"
    } else {
        name.trim()
    };
    let abbreviation = if abbreviation.trim().is_empty() {
        name
    } else {
        abbreviation.trim()
    };
    eab::Bible::new(eab::project::stable_id(name), name, abbreviation, imported)
}

#[cfg(not(target_arch = "wasm32"))]
fn import_paths(paths: Vec<String>, name: String, abbreviation: String) -> Result<Bible, ExtractAudioBibleError> {
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(Into::into).collect();
    Ok(Bible::new(named_bible(
        &name,
        &abbreviation,
        eab::import_paths(&paths)?,
    )))
}

#[cfg(target_arch = "wasm32")]
fn import_paths(paths: Vec<String>, name: String, abbreviation: String) -> Result<Bible, ExtractAudioBibleError> {
    let _ = (paths, name, abbreviation);
    Err(ExtractAudioBibleError::Io {
        message: "no file access in WebAssembly: read the files and use importBible".into(),
    })
}

/**
A translation's text, ready to search.

```ts
for (const book of bible.books()) console.log(book.name, book.verseCounts.length);
bible.verse(43, 11, 35); // → "Jesus wept."
```
*/
pub struct Bible {
    inner: Arc<eab::Bible>,
    /// Indexed for searching the first time it's needed (about a fifth of a second for a whole
    /// Bible), then kept
    engine: OnceLock<Arc<Engine>>,
}

impl Bible {
    fn new(bible: eab::Bible) -> Self {
        Bible {
            inner: Arc::new(bible),
            engine: OnceLock::new(),
        }
    }

    fn engine(&self) -> Arc<Engine> {
        self.engine
            .get_or_init(|| Arc::new(eab::matcher::engine(&self.inner)))
            .clone()
    }
}

#[export]
impl Bible {
    pub fn id(&self) -> String {
        self.inner.id.clone()
    }

    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    pub fn abbreviation(&self) -> String {
        self.inner.abbreviation.clone()
    }

    pub fn books(&self) -> Vec<BookInfo> {
        (self.inner.books.iter())
            .map(|b| BookInfo {
                id: b.id,
                name: b.name.clone(),
                osis: b.osis.clone(),
                verse_counts: b.chapters.iter().map(|c| c.len() as u16).collect(),
            })
            .collect()
    }

    /// A verse's text (empty for a verse whose words are in the one before it, like the second
    /// of a bridge `4-5`)
    pub fn verse(&self, book: u8, chapter: u16, verse: u16) -> Option<String> {
        let b = self.inner.book(book)?;
        b.chapters
            .get(chapter.checked_sub(1)? as usize)?
            .get(verse.checked_sub(1)? as usize)
            .cloned()
    }

    /// A chapter's verses' text, in order
    pub fn chapter(&self, book: u8, chapter: u16) -> Vec<String> {
        (self.inner.book(book))
            .and_then(|b| b.chapters.get(chapter.checked_sub(1)? as usize))
            .cloned()
            .unwrap_or_default()
    }

    /// The Bible as JSON, to read again with `readBible` (the same as the app keeps)
    pub fn to_json(&self) -> String {
        serde_json::to_string(&*self.inner).unwrap_or_default()
    }
}

/// A sermon's words, with when each was said
pub struct Transcript {
    inner: Arc<eab::Transcript>,
}

#[export]
impl Transcript {
    pub fn words(&self) -> Vec<Word> {
        self.inner.words.iter().map(Word::from).collect()
    }

    pub fn word_count(&self) -> u32 {
        self.inner.words.len() as u32
    }

    /// Seconds, to the end of the last word
    pub fn duration(&self) -> f64 {
        self.inner.duration()
    }

    /**
    Where each paragraph begins, by word: the transcript's segments (a Let's Church transcript's
    paragraphs), or its long pauses when it has none. Paragraph `k` is words
    `paragraphStarts()[k]` up to the next one's start
    */
    pub fn paragraph_starts(&self) -> Vec<u32> {
        self.inner.paragraphs.iter().map(|&i| i as u32).collect()
    }

    /// The id it gives its sermon (a Let's Church transcript's media id), if any
    pub fn id(&self) -> Option<String> {
        self.inner.id.clone()
    }

    /// The title it gives its sermon, if any
    pub fn title(&self) -> Option<String> {
        self.inner.title.clone()
    }
}

/**
Finds the verses read in transcripts (made by `ExtractAudioBible.searcher`).

```ts
const searcher = eab.searcher(bible, "John", 0.85);
const { readings, runs } = searcher.find(eab.readTranscript(json), "sunday");
```
*/
pub struct Searcher {
    engine: Arc<Engine>,
    book: Option<u8>,
    min_accuracy: f32,
}

#[export]
impl Searcher {
    /// Every reading of a verse in a transcript; `sourceId` begins each reading's id. A few
    /// milliseconds for an hour-long sermon
    pub fn find(&self, transcript: &Transcript, source_id: String) -> Readings {
        let options = eab::scripture_quotes::Options {
            min_accuracy: self.min_accuracy,
            ..Default::default()
        };
        let Ok(searcher) = eab::matcher::searcher(&self.engine, self.book) else {
            return Readings {
                readings: vec![],
                runs: vec![],
            };
        };
        let (matches, runs) = eab::matcher::find_matches(&searcher, &transcript.inner, &source_id, &options);
        Readings {
            readings: matches.iter().map(Reading::from).collect(),
            runs: runs.iter().map(ReadingRun::from).collect(),
        }
    }
}

enum Store {
    /// With the folder audio given by URL is downloaded into, if any
    Disk(Option<String>),
    Memory(RwLock<eab::Memory>),
}

/**
Where audio, transcripts, and cover images are read from: the disk (native only), or files the
host adds as bytes, by the paths a project names them by.

A sermon whose audio comes from the project's audio URL has that URL as its path
(`SourceInfo.audioPath`): natively, `downloading` fetches it once into a folder; in WebAssembly,
fetch it and add it to memory files under that URL.

```ts
const files = Files.memory();
files.add("sermons/sunday.mp3", mp3Bytes);
```
*/
pub struct Files {
    store: Store,
}

#[export]
impl Files {
    /// Files read from disk (native only: in WebAssembly every read fails; use `memory`)
    pub fn disk() -> Self {
        Files {
            store: Store::Disk(None),
        }
    }

    /**
    Files read from disk, and audio given by a URL downloaded the first time it's read into
    `folder` and kept there (native only: in WebAssembly every read fails; use `memory`)
    */
    pub fn downloading(folder: String) -> Self {
        Files {
            store: Store::Disk(Some(folder)),
        }
    }

    /// Files the host adds
    pub fn memory() -> Self {
        Files {
            store: Store::Memory(RwLock::default()),
        }
    }

    /// Adds a file (replacing one at the same path); does nothing for disk files
    pub fn add(&self, path: String, bytes: Vec<u8>) {
        if let Store::Memory(m) = &self.store {
            m.write().unwrap_or_else(|e| e.into_inner()).insert(path, bytes);
        }
    }

    pub fn remove(&self, path: String) -> bool {
        match &self.store {
            Store::Memory(m) => m.write().unwrap_or_else(|e| e.into_inner()).remove(&path),
            Store::Disk(_) => false,
        }
    }

    pub fn has(&self, path: String) -> bool {
        match &self.store {
            Store::Memory(m) => m.read().unwrap_or_else(|e| e.into_inner()).contains(&path),
            Store::Disk(_) => std::path::Path::new(&path).exists(),
        }
    }

    /// The paths added (none for disk files)
    pub fn paths(&self) -> Vec<String> {
        match &self.store {
            Store::Memory(m) => {
                let mut paths: Vec<String> = m
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .paths()
                    .map(str::to_owned)
                    .collect();
                paths.sort();
                paths
            }
            Store::Disk(_) => vec![],
        }
    }
}

impl eab::Files for Files {
    fn read(&self, path: &str) -> eab::Result<Vec<u8>> {
        match &self.store {
            Store::Memory(m) => m.read().unwrap_or_else(|e| e.into_inner()).read(path),
            Store::Disk(downloads) => disk_read(downloads.as_deref(), path),
        }
    }

    fn open(&self, path: &str) -> eab::Result<Box<dyn eab::files::MediaSource>> {
        match &self.store {
            Store::Memory(m) => m.read().unwrap_or_else(|e| e.into_inner()).open(path),
            Store::Disk(downloads) => disk_open(downloads.as_deref(), path),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn disk_read(downloads: Option<&str>, path: &str) -> eab::Result<Vec<u8>> {
    match downloads {
        Some(folder) => eab::Files::read(&eab::Downloading::new(&eab::Disk, folder), path),
        None => eab::Files::read(&eab::Disk, path),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn disk_open(downloads: Option<&str>, path: &str) -> eab::Result<Box<dyn eab::files::MediaSource>> {
    match downloads {
        Some(folder) => eab::Files::open(&eab::Downloading::new(&eab::Disk, folder), path),
        None => eab::Files::open(&eab::Disk, path),
    }
}

#[cfg(target_arch = "wasm32")]
fn no_disk(path: &str) -> eab::Error {
    eab::Error::Io(std::io::Error::other(format!(
        "{path}: no file access in WebAssembly: use Files.memory() and add the file"
    )))
}

#[cfg(target_arch = "wasm32")]
fn disk_read(_downloads: Option<&str>, path: &str) -> eab::Result<Vec<u8>> {
    Err(no_disk(path))
}

#[cfg(target_arch = "wasm32")]
fn disk_open(_downloads: Option<&str>, path: &str) -> eab::Result<Box<dyn eab::files::MediaSource>> {
    Err(no_disk(path))
}
