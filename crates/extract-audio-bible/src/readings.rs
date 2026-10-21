//! Every verse read in a project's sermons, as a table: to export as CSV or JSON.

use serde::{Deserialize, Serialize};

use crate::{
    bible::Bible,
    files::Files,
    matcher::{Match, MatchKind},
    project::{FileMetadata, MatchSet, Project},
    transcript::Transcript,
};

/// A verse read in a sermon
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingRow {
    /// The reading's id, and its sermon's in the project
    pub id: String,
    pub source_id: String,
    /// The sermon's title (or file's name), and its file id
    pub sermon: String,
    pub sermon_id: String,
    /// Who preached it, and the series it's in, if known
    pub speaker: Option<String>,
    pub series: Option<String>,
    /// "John 11:35", in the names of the project's first translation
    pub reference: String,
    pub book: u8,
    pub chapter: u16,
    pub verse: u16,
    /// The translation it fits best, by abbreviation
    pub translation: String,
    /// Other translations it was read from as faithfully, by abbreviation
    pub also_in: Vec<String>,
    /// Other verses worded the same, which it could as well be ("Psalm 14:1"), when the sermon
    /// doesn't say which
    pub also_at: Vec<String>,
    /// When it was read, in seconds
    pub start: f64,
    pub end: f64,
    pub kind: MatchKind,
    /// How faithfully it was read, and how much of the verse (0–1)
    pub accuracy: f32,
    pub coverage: f32,
    /// The words said, as the transcript has them
    pub said: String,
}

/// What readings' translations and books are called: by abbreviation, and in the names of the
/// project's first translation
pub struct Names {
    translations: std::collections::HashMap<String, String>,
    books: std::collections::HashMap<u8, String>,
}

impl Names {
    /// The names `bibles` give (the one `names_from` is, or the first, names the books)
    pub fn new(bibles: &[&Bible], names_from: &str) -> Self {
        let translations = (bibles.iter())
            .map(|b| {
                let name = if b.abbreviation.is_empty() {
                    &b.name
                } else {
                    &b.abbreviation
                };
                (b.id.clone(), name.clone())
            })
            .collect();
        let named = bibles.iter().find(|b| b.id == names_from).or(bibles.first());
        let books = named
            .map(|b| b.books.iter().map(|book| (book.id, book.name.clone())).collect())
            .unwrap_or_default();
        Names { translations, books }
    }

    /// Names given outright: translations' abbreviations by id, books' names by number
    pub fn from_parts(
        translations: std::collections::HashMap<String, String>,
        books: std::collections::HashMap<u8, String>,
    ) -> Self {
        Names { translations, books }
    }

    /// These names, with translations' abbreviations given anew
    pub fn with_translations(self, translations: std::collections::HashMap<String, String>) -> Self {
        Names { translations, ..self }
    }

    /// A translation's abbreviation (its id, if it isn't known)
    pub fn translation(&self, id: &str) -> String {
        self.translations.get(id).cloned().unwrap_or_else(|| id.to_string())
    }

    /// The translations an abbreviation is of, by id (an id not known is its own abbreviation)
    pub fn translation_ids(&self, abbreviation: &str) -> Vec<String> {
        let mut ids: Vec<String> = (self.translations.iter())
            .filter(|(_, a)| *a == abbreviation)
            .map(|(id, _)| id.clone())
            .collect();
        if !self.translations.contains_key(abbreviation) {
            ids.push(abbreviation.to_string());
        }
        ids
    }

    /// A book's name (its number, if it isn't known)
    pub fn book(&self, book: u8) -> String {
        self.books.get(&book).cloned().unwrap_or_else(|| book.to_string())
    }

    /// "John 11:35" (the book by its number, if it isn't known)
    pub fn reference(&self, book: u8, chapter: u16, verse: u16) -> String {
        format!("{} {chapter}:{verse}", self.book(book))
    }

    /// A reading as a row, in a sermon (`sermon`: its title, `sermon_id`: its file id), with the
    /// words said
    pub fn row(&self, m: &Match, sermon: &str, sermon_id: &str, meta: &FileMetadata, said: String) -> ReadingRow {
        ReadingRow {
            id: m.id.clone(),
            source_id: m.source_id.clone(),
            sermon: sermon.to_string(),
            sermon_id: sermon_id.to_string(),
            speaker: meta.speaker.clone(),
            series: meta.series.clone(),
            reference: self.reference(m.book, m.chapter, m.verse),
            book: m.book,
            chapter: m.chapter,
            verse: m.verse,
            translation: self.translation(&m.translation),
            also_in: m.also_in.iter().map(|t| self.translation(t)).collect(),
            also_at: (m.also_at.iter())
                .filter_map(|key| {
                    let mut parts = key.split('.').map(|p| p.parse::<u16>().ok());
                    Some(self.reference(u8::try_from(parts.next()??).ok()?, parts.next()??, parts.next()??))
                })
                .collect(),
            start: m.start,
            end: m.end,
            kind: m.kind,
            accuracy: m.accuracy,
            coverage: m.coverage,
            said,
        }
    }
}

/**
Every reading found in a project's sermons (`matches`), in the sermons' order and then as said.
`bibles` name the translations and books (the project's first translation names the books). The
words said are the readings' own; for readings found before they were kept, transcripts are read
through `files` for them, and a sermon whose transcript can't be read has them empty
*/
pub fn reading_rows(project: &Project, matches: &MatchSet, bibles: &[&Bible], files: &dyn Files) -> Vec<ReadingRow> {
    let names = Names::new(bibles, &project.bible_id);
    // Each sermon's readings, gathered once (a sermon at a time would go through them all for each)
    let mut by_source: std::collections::HashMap<&str, Vec<&Match>> = Default::default();
    for m in &matches.matches {
        by_source.entry(m.source_id.as_str()).or_default().push(m);
    }
    let rows_of = |source: &crate::project::Source| {
        let Some(found) = by_source.get(source.id.as_str()) else {
            return vec![];
        };
        let mut found = found.clone();
        found.sort_by_key(|m| m.word_start);
        let transcript = (found.iter().any(|m| m.said.is_empty()))
            .then(|| Transcript::open(files, &source.transcript_path).ok())
            .flatten();
        let sermon = project.source_title(source);
        let meta = project.file_metadata(source);
        let file_id = source.file_id();
        (found.into_iter())
            .map(|m| {
                let said = if m.said.is_empty() {
                    (transcript.as_ref())
                        .and_then(|t| t.words.get(m.word_start as usize..=m.word_end as usize))
                        .map(|words| words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
                        .unwrap_or_default()
                } else {
                    m.said.clone()
                };
                names.row(m, &sermon, &file_id, &meta, said)
            })
            .collect()
    };
    // Transcripts read for the words said several at a time, natively
    #[cfg(not(target_arch = "wasm32"))]
    let rows: Vec<Vec<ReadingRow>> = {
        use rayon::prelude::*;
        project.sources.par_iter().map(rows_of).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let rows: Vec<Vec<ReadingRow>> = project.sources.iter().map(rows_of).collect();
    rows.into_iter().flatten().collect()
}

/// The header row of readings as CSV
pub const CSV_HEADER: &str = "sermon,sermon id,speaker,series,reference,book,chapter,verse,also at,translation,also in,start,end,kind,accuracy,coverage,said\n";

/// A reading as a CSV row, ending with a newline (times in seconds, accuracy and coverage 0–1; the
/// ids within the project left out)
pub fn csv_line(r: &ReadingRow) -> String {
    let field = |s: &str| {
        if s.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let kind = match r.kind {
        MatchKind::Exact => "exact",
        MatchKind::Close => "close",
        MatchKind::Partial => "partial",
    };
    let cells = [
        field(&r.sermon),
        field(&r.sermon_id),
        field(r.speaker.as_deref().unwrap_or_default()),
        field(r.series.as_deref().unwrap_or_default()),
        field(&r.reference),
        r.book.to_string(),
        r.chapter.to_string(),
        r.verse.to_string(),
        field(&r.also_at.join("; ")),
        field(&r.translation),
        field(&r.also_in.join(" ")),
        format!("{:.2}", r.start),
        format!("{:.2}", r.end),
        kind.to_string(),
        format!("{:.3}", r.accuracy),
        format!("{:.3}", r.coverage),
        field(&r.said),
    ];
    let mut line = cells.join(",");
    line.push('\n');
    line
}

/// Readings as CSV, with a header row ([`csv_line`])
pub fn to_csv(rows: &[ReadingRow]) -> String {
    let mut out = String::from(CSV_HEADER);
    for r in rows {
        out += &csv_line(r);
    }
    out
}
