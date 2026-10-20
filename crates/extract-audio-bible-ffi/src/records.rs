//! The plain data that crosses into other languages, converted from the library's own types.

use boltffi::*;

/// A 32-bit score as the 64-bit number hosts get, without float noise (`0.85`, not
/// `0.8500000238418579`)
fn wide(x: f32) -> f64 {
    (x as f64 * 1e6).round() / 1e6
}

/// A file given as bytes (in WebAssembly, or whenever the host reads files itself)
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputFile {
    /// Its name or path; the extension says what it is (`JHN.usx`, `sermon.mp3`)
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A book of a Bible
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookInfo {
    /// 1 for Genesis through 66 for Revelation
    pub id: u8,
    pub name: String,
    /// `1Thess`
    pub osis: String,
    /// How many verses each chapter has (verses with no text of their own included)
    pub verse_counts: Vec<u16>,
}

/// A word of a transcript and when it was said
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    /// Seconds
    pub start: f64,
    pub end: f64,
    /// The transcriber's confidence, when it gave one
    pub score: Option<f64>,
    pub speaker: Option<String>,
    /// Whether the transcriber gave this word its own time (others are spread over their
    /// segment)
    pub timed: bool,
}

impl From<&eab::Word> for Word {
    fn from(w: &eab::Word) -> Self {
        Word {
            text: w.text.clone(),
            start: w.start,
            end: w.end,
            score: w.score,
            speaker: w.speaker.clone(),
            timed: w.timed,
        }
    }
}

#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingKind {
    /// Every word, in order, with next to nothing added
    Exact,
    /// Most of the verse, or all of it with some words changed or added
    Close,
    /// Only part of the verse
    Partial,
}

/// A verse read in a sermon
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// `sourceId:book.chapter.verse@word`
    pub id: String,
    pub source_id: String,
    pub book: u8,
    pub chapter: u16,
    pub verse: u16,
    /// The transcript's words read (inclusive)
    pub word_start: u32,
    pub word_end: u32,
    /// Where to cut, in seconds: halfway into the pauses around it
    pub start: f64,
    pub end: f64,
    /// How much of the verse was read, discounted by how far it strays (0–1); ranks readings
    pub score: f64,
    /// How faithfully the verse's words were said, from the first read to the last (0–1)
    pub accuracy: f64,
    /// How much of the verse was read (0–1)
    pub coverage: f64,
    /// How much of what was said belongs to the verse (0–1)
    pub precision: f64,
    pub kind: ReadingKind,
    /// Per word of the verse's text (split on spaces): whether it was heard
    pub verse_words_heard: Vec<bool>,
    /// The transcript's words in the reading that aren't the verse's
    pub extra_words: Vec<u32>,
    /// The run it's part of, if verses around it were read straight through
    pub run_id: Option<String>,
}

impl From<&eab::Match> for Reading {
    fn from(m: &eab::Match) -> Self {
        Reading {
            id: m.id.clone(),
            source_id: m.source_id.clone(),
            book: m.book,
            chapter: m.chapter,
            verse: m.verse,
            word_start: m.word_start,
            word_end: m.word_end,
            start: m.start,
            end: m.end,
            score: wide(m.score),
            accuracy: wide(m.accuracy),
            coverage: wide(m.coverage),
            precision: wide(m.precision),
            kind: match m.kind {
                eab::MatchKind::Exact => ReadingKind::Exact,
                eab::MatchKind::Close => ReadingKind::Close,
                eab::MatchKind::Partial => ReadingKind::Partial,
            },
            verse_words_heard: m.verse_words_heard.clone(),
            extra_words: m.extra_words.clone(),
            run_id: m.run_id.clone(),
        }
    }
}

/// Verses read one after another
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct ReadingRun {
    pub id: String,
    pub source_id: String,
    pub book: u8,
    pub start_chapter: u16,
    pub start_verse: u16,
    pub end_chapter: u16,
    pub end_verse: u16,
    /// Its readings, in order
    pub reading_ids: Vec<String>,
    pub start: f64,
    pub end: f64,
}

impl From<&eab::Run> for ReadingRun {
    fn from(r: &eab::Run) -> Self {
        ReadingRun {
            id: r.id.clone(),
            source_id: r.source_id.clone(),
            book: r.book,
            start_chapter: r.start_chapter,
            start_verse: r.start_verse,
            end_chapter: r.end_chapter,
            end_verse: r.end_verse,
            reading_ids: r.match_ids.clone(),
            start: r.start,
            end: r.end,
        }
    }
}

/// Every reading found in a transcript
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct Readings {
    pub readings: Vec<Reading>,
    pub runs: Vec<ReadingRun>,
}

/// A sermon of a project
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceInfo {
    pub id: String,
    /// Its id for metadata: the one its transcript gives (a Let's Church media id) once
    /// searched, or its file's name
    pub file_id: String,
    /// Its file's name
    pub label: String,
    /// Its title, given or from its transcript
    pub title: Option<String>,
    /// The passage it preaches, if given
    pub passage: Option<String>,
    pub audio_path: String,
    pub transcript_path: String,
    /// Seconds, once searched
    pub duration: Option<f64>,
    pub word_count: u32,
    pub reading_count: u32,
}

impl SourceInfo {
    pub(crate) fn of(project: &eab::Project, s: &eab::Source) -> Self {
        let meta = project.file_metadata(s);
        SourceInfo {
            id: s.id.clone(),
            file_id: s.file_id(),
            label: s.label.clone(),
            title: meta.title,
            passage: meta.passage,
            audio_path: s.audio_path.clone(),
            transcript_path: s.transcript_path.clone(),
            duration: s.duration,
            word_count: s.word_count as u32,
            reading_count: s.match_count as u32,
        }
    }
}

/// What's known about a sermon, by its file id
#[data]
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FileMetadata {
    pub title: Option<String>,
    /// The passage it preaches ("1 Thessalonians 5:12-15")
    pub passage: Option<String>,
}

impl From<eab::FileMetadata> for FileMetadata {
    fn from(m: eab::FileMetadata) -> Self {
        FileMetadata {
            title: m.title,
            passage: m.passage,
        }
    }
}

impl From<FileMetadata> for eab::FileMetadata {
    fn from(m: FileMetadata) -> Self {
        eab::FileMetadata {
            title: m.title,
            passage: m.passage,
        }
    }
}

/// What adding sermons did
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub added: u32,
    /// Files that had no partner (audio without a transcript, or the reverse)
    pub unpaired: Vec<String>,
}

/// What searching a project's sermons found
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSummary {
    pub sermons: u32,
    pub readings: u32,
    /// Sermons that couldn't be searched, with why
    pub errors: Vec<String>,
}

/// The reading chosen for a verse, and how it's cut
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub book: u8,
    pub chapter: u16,
    pub verse: u16,
    pub reading_id: String,
    pub source_id: String,
    /// The reading's own cut points, in seconds
    pub start: f64,
    pub end: f64,
    /// Seconds added to each cut point (negative moves it earlier)
    pub trim_start: f64,
    pub trim_end: f64,
    /// Silence before and after the verse, in seconds
    pub pad_before: f64,
    pub pad_after: f64,
    /// Volume change, in dB
    pub gain_db: f64,
    /// Chosen, but still to be trimmed
    pub needs_trim: bool,
    pub note: String,
    /// More readings of the verse, played after this one in order: the rest of a verse read in
    /// pieces
    pub joined: Vec<SelectionPart>,
}

/// Another reading used for part of a verse, and how it's cut
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionPart {
    pub reading_id: String,
    pub source_id: String,
    pub start: f64,
    pub end: f64,
    pub trim_start: f64,
    pub trim_end: f64,
}

impl From<&eab::Part> for SelectionPart {
    fn from(p: &eab::Part) -> Self {
        SelectionPart {
            reading_id: p.match_id.clone(),
            source_id: p.source_id.clone(),
            start: p.start,
            end: p.end,
            trim_start: p.trim_start,
            trim_end: p.trim_end,
        }
    }
}

impl From<&SelectionPart> for eab::Part {
    fn from(p: &SelectionPart) -> Self {
        eab::Part {
            match_id: p.reading_id.clone(),
            source_id: p.source_id.clone(),
            start: p.start,
            end: p.end,
            trim_start: p.trim_start,
            trim_end: p.trim_end,
        }
    }
}

impl Selection {
    pub(crate) fn from_core(key: &str, s: &eab::Selection) -> Option<Self> {
        let mut parts = key.split('.').map(|p| p.parse::<u16>().ok());
        let (book, chapter, verse) = (parts.next()??, parts.next()??, parts.next()??);
        Some(Selection {
            book: book as u8,
            chapter,
            verse,
            reading_id: s.match_id.clone(),
            source_id: s.source_id.clone(),
            start: s.start,
            end: s.end,
            trim_start: s.trim_start,
            trim_end: s.trim_end,
            pad_before: s.pad_before,
            pad_after: s.pad_after,
            gain_db: s.gain_db,
            needs_trim: s.needs_trim,
            note: s.note.clone(),
            joined: s.joined.iter().map(SelectionPart::from).collect(),
        })
    }

    pub(crate) fn to_core(&self) -> (String, eab::Selection) {
        (
            eab::export::verse_key(self.book, self.chapter, self.verse),
            eab::Selection {
                match_id: self.reading_id.clone(),
                source_id: self.source_id.clone(),
                start: self.start,
                end: self.end,
                trim_start: self.trim_start,
                trim_end: self.trim_end,
                pad_before: self.pad_before,
                pad_after: self.pad_after,
                gain_db: self.gain_db,
                needs_trim: self.needs_trim,
                note: self.note.clone(),
                joined: self.joined.iter().map(eab::Part::from).collect(),
            },
        )
    }
}

#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChapterRef {
    pub book: u8,
    pub chapter: u16,
}

impl From<&eab::ChapterRef> for ChapterRef {
    fn from(c: &eab::ChapterRef) -> Self {
        ChapterRef {
            book: c.book,
            chapter: c.chapter,
        }
    }
}

impl From<&ChapterRef> for eab::ChapterRef {
    fn from(c: &ChapterRef) -> Self {
        eab::ChapterRef {
            book: c.book,
            chapter: c.chapter,
        }
    }
}

/// The verses given a reading in a chapter
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterChoice {
    pub book: u8,
    pub chapter: u16,
    pub verses: Vec<u16>,
}

/// How far along a chapter is
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterStatus {
    pub book: u8,
    pub chapter: u16,
    /// Verses with text of their own
    pub verses: u32,
    pub chosen: u32,
    pub with_readings: u32,
    /// Verses with text and no reading chosen
    pub missing: Vec<u16>,
}

/// Where a verse is heard in a chapter's audio, in seconds
#[data]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VerseTime {
    pub verse: u16,
    pub start: f64,
    pub end: f64,
}

/// How loudness is evened out
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leveling {
    /// Only the volume changes chosen per verse
    Off,
    /// Every reading brought to the target level, so sermons recorded louder or softer match
    EachReading,
    /// One change for the whole export, so it reaches the target level while readings keep their
    /// levels relative to each other
    WholeExport,
}

/// How a project searches and puts verses together
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// How faithfully a verse's words must be read for a reading to count (0–1; 0.85)
    pub min_accuracy: f64,
    /// Silence between every two verses, in seconds
    pub gap_between_verses: f64,
    /// Silence at the start and end of each chapter's file
    pub chapter_lead_in: f64,
    pub chapter_tail: f64,
    pub leveling: Leveling,
    /// The speech level leveled to, dBFS RMS (-20)
    pub target_db: f64,
    /// A volume change for everything exported, in dB
    pub output_gain_db: f64,
    /// Fades at every cut, in milliseconds
    pub fade_ms: f64,
}

impl From<&eab::Settings> for Settings {
    fn from(s: &eab::Settings) -> Self {
        Settings {
            min_accuracy: wide(s.min_accuracy),
            gap_between_verses: s.gap_between_verses,
            chapter_lead_in: s.chapter_lead_in,
            chapter_tail: s.chapter_tail,
            leveling: match s.leveling {
                eab::Leveling::None => Leveling::Off,
                eab::Leveling::Each => Leveling::EachReading,
                eab::Leveling::Export => Leveling::WholeExport,
            },
            target_db: s.target_db,
            output_gain_db: s.output_gain_db,
            fade_ms: s.fade_ms,
        }
    }
}

impl From<&Settings> for eab::Settings {
    fn from(s: &Settings) -> Self {
        eab::Settings {
            min_accuracy: s.min_accuracy as f32,
            gap_between_verses: s.gap_between_verses,
            chapter_lead_in: s.chapter_lead_in,
            chapter_tail: s.chapter_tail,
            leveling: match s.leveling {
                Leveling::Off => eab::Leveling::None,
                Leveling::EachReading => eab::Leveling::Each,
                Leveling::WholeExport => eab::Leveling::Export,
            },
            target_db: s.target_db,
            output_gain_db: s.output_gain_db,
            fade_ms: s.fade_ms,
        }
    }
}

/// What files an export writes
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportMode {
    /// One file per chapter
    Chapter,
    /// One file per verse, in a folder per chapter
    Verse,
    Both,
}

/// How chapter files are numbered as tracks
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackNumbering {
    /// The track is the chapter's number
    Chapter,
    /// The track is the file's place in the export
    Sequence,
}

/**
How an export is named and tagged. Templates fill in `{book}`, `{osis}`, `{bookNumber}`,
`{bookNumber2}`, `{chapter}`, `{chapter2}`, `{chapter3}`, `{chapters}`, `{verse}`, `{verse2}`,
`{verse3}`, `{verses}`, `{translation}`, and `{abbreviation}`
*/
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportMetadata {
    /// `{book} {chapter}`
    pub title_template: String,
    /// `{translation} – {book}`
    pub album_template: String,
    /// `{bookNumber2}_{osis}_{chapter3}`
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
    /// A JPEG or PNG, read through the export's files
    pub cover_path: Option<String>,
    /// MP3 kbps (96)
    pub bitrate: u16,
    /// Hz (44100)
    pub sample_rate: u32,
    /// Only chapters with every verse chosen
    pub only_complete: bool,
    /// Each book in its own folder
    pub folder_per_book: bool,
    pub track_numbering: TrackNumbering,
}

impl From<&eab::Metadata> for ExportMetadata {
    fn from(m: &eab::Metadata) -> Self {
        ExportMetadata {
            title_template: m.title_template.clone(),
            album_template: m.album_template.clone(),
            filename_template: m.filename_template.clone(),
            verse_title_template: m.verse_title_template.clone(),
            verse_filename_template: m.verse_filename_template.clone(),
            export_mode: match m.export_mode {
                eab::ExportMode::Chapter => ExportMode::Chapter,
                eab::ExportMode::Verse => ExportMode::Verse,
                eab::ExportMode::Both => ExportMode::Both,
            },
            artist: m.artist.clone(),
            album_artist: m.album_artist.clone(),
            narrator: m.narrator.clone(),
            genre: m.genre.clone(),
            year: m.year.clone(),
            comment: m.comment.clone(),
            copyright: m.copyright.clone(),
            publisher: m.publisher.clone(),
            language: m.language.clone(),
            cover_path: m.cover_path.clone(),
            bitrate: m.bitrate,
            sample_rate: m.sample_rate,
            only_complete: m.only_complete,
            folder_per_book: m.folder_per_book,
            track_numbering: if m.track_numbering == "sequence" {
                TrackNumbering::Sequence
            } else {
                TrackNumbering::Chapter
            },
        }
    }
}

impl ExportMetadata {
    /// Onto the project's own metadata (which keeps what this doesn't carry, like the app's
    /// export folder)
    pub(crate) fn apply(&self, m: &mut eab::Metadata) {
        m.title_template = self.title_template.clone();
        m.album_template = self.album_template.clone();
        m.filename_template = self.filename_template.clone();
        m.verse_title_template = self.verse_title_template.clone();
        m.verse_filename_template = self.verse_filename_template.clone();
        m.export_mode = match self.export_mode {
            ExportMode::Chapter => eab::ExportMode::Chapter,
            ExportMode::Verse => eab::ExportMode::Verse,
            ExportMode::Both => eab::ExportMode::Both,
        };
        m.artist = self.artist.clone();
        m.album_artist = self.album_artist.clone();
        m.narrator = self.narrator.clone();
        m.genre = self.genre.clone();
        m.year = self.year.clone();
        m.comment = self.comment.clone();
        m.copyright = self.copyright.clone();
        m.publisher = self.publisher.clone();
        m.language = self.language.clone();
        m.cover_path = self.cover_path.clone();
        m.bitrate = self.bitrate;
        m.sample_rate = self.sample_rate;
        m.only_complete = self.only_complete;
        m.folder_per_book = self.folder_per_book;
        m.track_numbering = match self.track_numbering {
            TrackNumbering::Chapter => "chapter".into(),
            TrackNumbering::Sequence => "sequence".into(),
        };
    }
}

/// How to render a stretch of a sermon to listen to
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct ClipOptions {
    /// Silence before and after, in seconds
    pub pad_before: f64,
    pub pad_after: f64,
    /// Level and fade it as an export will (at 44.1 kHz)
    pub processed: bool,
    /// When processed, bring it to this level first (dBFS RMS), as leveling each reading does
    pub target_db: Option<f64>,
    /// When processed, a volume change in dB
    pub gain_db: f64,
}

impl From<&ClipOptions> for eab::ClipOptions {
    fn from(o: &ClipOptions) -> Self {
        eab::ClipOptions {
            pad_before: o.pad_before,
            pad_after: o.pad_after,
            processed: o.processed,
            target_db: o.target_db,
            gain_db: o.gain_db,
        }
    }
}

/// A file an export made, with its bytes
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct ExportedFile {
    /// Where it goes under the export's folder (`52 1 Thessalonians/52_1Thess_005.mp3`)
    pub path: String,
    pub book: u8,
    pub chapter: u16,
    /// For a verse's own file
    pub verse: Option<u16>,
    /// Seconds
    pub duration: f64,
    /// For a chapter's file: its verses with no reading chosen
    pub missing: Vec<u16>,
    /// The tagged MP3
    pub bytes: Vec<u8>,
}

/// A file an export wrote to a folder
#[data]
#[derive(Clone, Debug, PartialEq)]
pub struct WrittenFile {
    /// Where it was written
    pub path: String,
    pub book: u8,
    pub chapter: u16,
    pub verse: Option<u16>,
    pub duration: f64,
    pub missing: Vec<u16>,
}

impl From<eab::ExportedFile> for WrittenFile {
    fn from(f: eab::ExportedFile) -> Self {
        WrittenFile {
            path: f.path,
            book: f.book,
            chapter: f.chapter,
            verse: f.verse,
            duration: f.duration,
            missing: f.missing,
        }
    }
}
