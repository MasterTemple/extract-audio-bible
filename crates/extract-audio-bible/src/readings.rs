//! Every verse read in a project's sermons, as a table: to export as CSV or JSON.

use serde::{Deserialize, Serialize};

use crate::{
    bible::Bible,
    files::Files,
    matcher::MatchKind,
    project::{MatchSet, Project},
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
    /// "John 11:35", in the names of the project's first translation
    pub reference: String,
    pub book: u8,
    pub chapter: u16,
    pub verse: u16,
    /// The translation it fits best, by abbreviation
    pub translation: String,
    /// Other translations it was read from as faithfully, by abbreviation
    pub also_in: Vec<String>,
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

/**
Every reading found in a project's sermons (`matches`), in the sermons' order and then as said.
`bibles` name the translations and books (the project's first translation names the books);
transcripts are read through `files` for the words said, and a sermon whose transcript can't be
read has them empty
*/
pub fn reading_rows(project: &Project, matches: &MatchSet, bibles: &[&Bible], files: &dyn Files) -> Vec<ReadingRow> {
    let abbreviation = |id: &str| {
        (bibles.iter().find(|b| b.id == id))
            .map_or_else(|| id.to_string(), |b| if b.abbreviation.is_empty() { b.name.clone() } else { b.abbreviation.clone() })
    };
    let names = bibles.iter().find(|b| b.id == project.bible_id).or(bibles.first());
    let mut rows = vec![];
    for source in &project.sources {
        let mut found: Vec<_> = matches.matches.iter().filter(|m| m.source_id == source.id).collect();
        if found.is_empty() {
            continue;
        }
        found.sort_by_key(|m| m.word_start);
        let transcript = Transcript::open(files, &source.transcript_path).ok();
        let sermon = project.source_title(source);
        for m in found {
            let book = names.and_then(|b| b.book(m.book)).map_or_else(|| m.book.to_string(), |b| b.name.clone());
            let said = (transcript.as_ref())
                .and_then(|t| t.words.get(m.word_start as usize..=m.word_end as usize))
                .map(|words| words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            rows.push(ReadingRow {
                id: m.id.clone(),
                source_id: source.id.clone(),
                sermon: sermon.clone(),
                sermon_id: source.file_id(),
                reference: format!("{book} {}:{}", m.chapter, m.verse),
                book: m.book,
                chapter: m.chapter,
                verse: m.verse,
                translation: abbreviation(&m.translation),
                also_in: m.also_in.iter().map(|t| abbreviation(t)).collect(),
                start: m.start,
                end: m.end,
                kind: m.kind,
                accuracy: m.accuracy,
                coverage: m.coverage,
                said,
            });
        }
    }
    rows
}

/// Readings as CSV, with a header row (times in seconds, accuracy and coverage 0–1; the ids within
/// the project left out)
pub fn to_csv(rows: &[ReadingRow]) -> String {
    let field = |s: &str| {
        if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
    };
    let mut out = String::from(
        "sermon,sermon id,reference,book,chapter,verse,translation,also in,start,end,kind,accuracy,coverage,said\n",
    );
    for r in rows {
        let kind = match r.kind {
            MatchKind::Exact => "exact",
            MatchKind::Close => "close",
            MatchKind::Partial => "partial",
        };
        let cells = [
            field(&r.sermon),
            field(&r.sermon_id),
            field(&r.reference),
            r.book.to_string(),
            r.chapter.to_string(),
            r.verse.to_string(),
            field(&r.translation),
            field(&r.also_in.join(" ")),
            format!("{:.2}", r.start),
            format!("{:.2}", r.end),
            kind.to_string(),
            format!("{:.3}", r.accuracy),
            format!("{:.3}", r.coverage),
            field(&r.said),
        ];
        out += &cells.join(",");
        out.push('\n');
    }
    out
}
