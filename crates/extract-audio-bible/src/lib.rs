#![doc = include_str!("../README.md")]

pub mod analyze;
pub mod audio;
pub mod bible;
pub mod choose;
pub mod error;
pub mod export;
pub mod files;
pub mod matcher;
pub mod project;
pub mod transcript;
#[cfg(test)]
mod e2e_test;

pub use analyze::{Analysis, analyze, analyze_with};
pub use bible::{Bible, BibleBook, Imported, import, import_paths};
pub use choose::ChapterStatus;
pub use error::{Error, ErrorKind, Result};
pub use export::{
    ChapterRef, ClipOptions, ExportProgress, ExportedFile, VerseTime, chapter_timeline, chapters_to_export, export, export_to_dir,
    render_chapter, render_clip, render_verse,
};
pub use files::{Disk, Files, Memory};
pub use matcher::{Match, MatchKind, Run};
pub use project::{Added, ExportMode, Leveling, MatchSet, Metadata, Part, Project, Selection, Settings, Source};
pub use scripture_quotes;
pub use transcript::{Transcript, Word};
