#![doc = include_str!("../README.md")]

pub mod analyze;
pub mod audio;
pub mod bible;
pub mod choose;
#[cfg(feature = "download")]
pub mod download;
#[cfg(test)]
mod e2e_test;
pub mod error;
pub mod export;
pub mod files;
pub mod matcher;
pub mod project;
pub mod readings;
pub mod said;
pub mod scope;
pub mod transcript;

pub use analyze::{Analysis, analyze, analyze_with};
pub use bible::{Bible, BibleBook, Imported, import, import_paths};
pub use choose::ChapterStatus;
#[cfg(feature = "download")]
pub use download::Downloading;
pub use error::{Error, ErrorKind, Result};
pub use export::{
    ChapterRef, ClipOptions, ExportProgress, ExportedFile, VerseTime, chapter_timeline, chapters_to_export, export,
    export_to_dir, render_chapter, render_clip, render_verse,
};
pub use files::{Disk, Files, Memory};
pub use matcher::{Match, MatchKind, Run};
pub use project::{
    Added, ExportMode, FileMetadata, Leveling, MatchSet, Metadata, Part, Project, ProjectKind, Selection, Settings,
    Source,
};
pub use readings::{ReadingRow, reading_rows};
pub use said::{FindOptions, Said, find_in, search_transcripts};
pub use scope::{BookScope, Genre, Testament};
pub use scripture_quotes;
pub use transcript::{Transcript, Word};
