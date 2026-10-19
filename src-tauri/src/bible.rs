//! Bible text: imported once (USX, USFM, OSIS, ThML, and GBF through logoxide-text; JSON, CSV,
//! TSV, and plain "Book 1:1 text" lines here) and kept as plain verse text.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use logoxide_text::{
    MemoryFiles, TextFormat,
    hash::{HashRule, verse_text},
    markup::verses::slice,
    source,
    topos_bible::data::books::{BookId, Books},
};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bible {
    pub id: String,
    pub name: String,
    pub abbreviation: String,
    pub language: Option<String>,
    /// What it was imported from ("usfm", "json", ...)
    pub format: String,
    pub books: Vec<BibleBook>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BibleBook {
    /// topos's book number (1 = Genesis)
    pub id: u8,
    pub name: String,
    pub osis: String,
    /// `chapters[c - 1][v - 1]`: a verse's text, empty when the text has none (a bridged verse
    /// whose words are in the verse before it)
    pub chapters: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BibleSummary {
    pub id: String,
    pub name: String,
    pub abbreviation: String,
    pub language: Option<String>,
    pub format: String,
    pub book_count: usize,
    pub verse_count: usize,
}

/// A book's shape, without its text, for the navigator
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookOutline {
    pub id: u8,
    pub name: String,
    pub osis: String,
    /// Verses per chapter
    pub chapters: Vec<u16>,
    /// Per chapter, the verses that have no text of their own (so they need no audio)
    pub empty: Vec<Vec<u16>>,
}

impl Bible {
    pub fn summary(&self) -> BibleSummary {
        BibleSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            abbreviation: self.abbreviation.clone(),
            language: self.language.clone(),
            format: self.format.clone(),
            book_count: self.books.len(),
            verse_count: self
                .books
                .iter()
                .flat_map(|b| &b.chapters)
                .map(|c| c.iter().filter(|v| !v.is_empty()).count())
                .sum(),
        }
    }

    pub fn outline(&self) -> Vec<BookOutline> {
        self.books
            .iter()
            .map(|b| BookOutline {
                id: b.id,
                name: b.name.clone(),
                osis: b.osis.clone(),
                chapters: b.chapters.iter().map(|c| c.len() as u16).collect(),
                empty: b
                    .chapters
                    .iter()
                    .map(|c| {
                        c.iter()
                            .enumerate()
                            .filter(|(_, t)| t.trim().is_empty())
                            .map(|(i, _)| i as u16 + 1)
                            .collect()
                    })
                    .collect(),
            })
            .collect()
    }

    pub fn book(&self, id: u8) -> Option<&BibleBook> {
        self.books.iter().find(|b| b.id == id)
    }
}

fn books() -> &'static Books {
    logoxide_text::bible_data().books()
}

/// A book named any way people name them: a USFM code (`JHN`), an OSIS id (`John`), a name or
/// abbreviation (`Jn`), or its number (`43`)
pub fn find_book(name: &str) -> Option<BookId> {
    let name = name.trim();
    if let Ok(n) = name.parse::<u8>() {
        return books().ids().find(|id| id.0 == n);
    }
    books()
        .search_usfm(name)
        .or_else(|| books().search_osis(name))
        .or_else(|| books().search(name))
        .or_else(|| books().search(&name.replace('_', " ")))
}

/// Verses gathered while importing, before they're put in order
#[derive(Default)]
struct Collected(BTreeMap<u8, BTreeMap<u16, BTreeMap<u16, String>>>);

impl Collected {
    fn add(&mut self, book: BookId, chapter: u16, verse: u16, text: &str) {
        if chapter == 0 || verse == 0 {
            return;
        }
        let slot = (self.0.entry(book.0).or_default().entry(chapter).or_default())
            .entry(verse)
            .or_default();
        if !slot.is_empty() {
            slot.push(' ');
        }
        slot.push_str(text.trim());
    }

    fn into_books(self) -> Vec<BibleBook> {
        let names = books();
        (self.0.into_iter())
            .filter_map(|(id, chapters)| {
                let book = BookId(id);
                let last = *chapters.keys().max()?;
                let chapters = (1..=last)
                    .map(|c| match chapters.get(&c) {
                        Some(verses) => {
                            let last = verses.keys().max().copied().unwrap_or(0);
                            (1..=last).map(|v| verses.get(&v).cloned().unwrap_or_default()).collect()
                        }
                        None => vec![],
                    })
                    .collect();
                Some(BibleBook {
                    id,
                    name: names.get_name(book).cloned().unwrap_or_else(|| format!("Book {id}")),
                    osis: names.get_osis(book).cloned().unwrap_or_default(),
                    chapters,
                })
            })
            .collect()
    }
}

/// Every file under the given files and folders
fn gather(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = vec![];
    for path in paths {
        if path.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(path)?.flatten().map(|e| e.path()).collect();
            entries.sort();
            out.extend(gather(&entries)?);
        } else if path.is_file() {
            out.push(path.clone());
        }
    }
    Ok(out)
}

fn ext(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Read a Bible from files or folders. The format comes from the files' extensions (and, for
/// `.xml`, what's in them)
pub fn import(paths: &[PathBuf]) -> Result<(Vec<BibleBook>, String)> {
    let files = gather(paths)?;
    if files.is_empty() {
        return Err(AppError::msg("No files found to import"));
    }
    let markup: Vec<&PathBuf> = files
        .iter()
        .filter(|p| matches!(ext(p).as_str(), "usx" | "usfm" | "sfm" | "xml" | "osis" | "thml" | "gbf"))
        .collect();
    let (books, format) = if !markup.is_empty() {
        import_markup(&markup)?
    } else {
        let mut collected = Collected::default();
        let mut format = String::new();
        for path in &files {
            let text = std::fs::read_to_string(path)?;
            let text = text.trim_start_matches('\u{feff}');
            format = match ext(path).as_str() {
                "json" => {
                    import_json(text, &mut collected)?;
                    "json"
                }
                "csv" => {
                    import_delimited(text, ',', &mut collected);
                    "csv"
                }
                "tsv" => {
                    import_delimited(text, '\t', &mut collected);
                    "tsv"
                }
                _ => {
                    import_lines(text, &mut collected);
                    "text"
                }
            }
            .into();
        }
        (collected.into_books(), format)
    };
    if books.is_empty() {
        return Err(AppError::msg(
            "No verses were found. Supported: USFM, USX, OSIS, ThML, GBF, JSON, CSV/TSV (book, chapter, verse, text), or lines like \"John 3:16 For God so loved…\"",
        ));
    }
    Ok((books, format))
}

/// USX, USFM, OSIS, ThML, or GBF, read by logoxide-text
fn import_markup(paths: &[&PathBuf]) -> Result<(Vec<BibleBook>, String)> {
    let mut by_format: BTreeMap<TextFormat, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    for (i, path) in paths.iter().enumerate() {
        let bytes = std::fs::read(path)?;
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(8192)]).to_ascii_lowercase();
        let format = match ext(path).as_str() {
            "usx" => TextFormat::Usx,
            "usfm" | "sfm" => TextFormat::Usfm,
            "gbf" => TextFormat::Gbf,
            "thml" => TextFormat::Thml,
            _ if head.contains("<thml") => TextFormat::Thml,
            _ if head.contains("<usx") => TextFormat::Usx,
            _ => TextFormat::Osis,
        };
        // Flat, so every file is in the one folder a source reads; the index keeps names apart
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let name = if by_format.values().flatten().any(|(n, _)| n == name) {
            format!("{i}-{name}")
        } else {
            name.to_string()
        };
        by_format.entry(format).or_default().push((name, bytes));
    }
    // The richest format given, as logoxide orders them
    let (format, files) = by_format.into_iter().next().expect("some markup");
    let memory = MemoryFiles::with(files.into_iter().map(|(n, b)| (format!("text/{n}"), b)));
    let source = source::open(Arc::new(memory), format, "text")?;
    let mut collected = Collected::default();
    for id in source.books() {
        let book = source.book(id)?;
        for span in &book.spans {
            let nodes = slice(&book.nodes, &span.start, Some(&span.end));
            let text = verse_text(&nodes, HashRule::One);
            collected.add(id, span.chapter as u16, span.verse as u16, &text);
            // A bridge (4-5) keeps its words in its first verse; the rest are present but empty
            if span.last_chapter == span.chapter {
                for v in span.verse + 1..=span.last {
                    collected.add(id, span.chapter as u16, v as u16, "");
                }
            }
        }
    }
    let mut books = collected.into_books();
    // `add` drops empty text into an empty slot, which is what a bridge's later verses need
    for book in &mut books {
        for ch in &mut book.chapters {
            for v in ch.iter_mut() {
                *v = v.trim().to_string();
            }
        }
    }
    Ok((books, format.name().to_string()))
}

/// JSON in any of the common shapes:
/// - `[{"book": "John", "chapter": 3, "verse": 16, "text": "..."}]` (also `{"verses": [...]}`)
/// - `{"John": {"3": {"16": "..."}}}`
/// - `{"John": [["1:1 text", "1:2 text"], [...]]}` (chapters of verses)
/// - `{"books": [{"name": "John", "chapters": [{"chapter": 1, "verses": [{"verse": 1, "text": ""}]}]}]}`
fn import_json(text: &str, out: &mut Collected) -> Result<()> {
    use serde_json::Value;
    let value: Value = serde_json::from_str(text)?;

    fn num(v: &Value) -> Option<u16> {
        v.as_u64().map(|n| n as u16).or_else(|| v.as_str()?.trim().parse().ok())
    }
    fn get<'a>(obj: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
        keys.iter().find_map(|k| obj.get(*k))
    }
    fn verse_text(v: &Value) -> Option<String> {
        match v {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => get(o, &["text", "content", "t"])?.as_str().map(str::to_string),
            _ => None,
        }
    }
    fn flat(items: &[Value], out: &mut Collected) -> bool {
        let mut any = false;
        for item in items {
            let Value::Object(o) = item else { continue };
            let book = get(o, &["book", "book_name", "bookName", "b", "book_id", "bookId"]);
            let book = book.and_then(|b| match b {
                Value::String(s) => find_book(s),
                Value::Number(n) => find_book(&n.to_string()),
                _ => None,
            });
            let chapter = get(o, &["chapter", "c", "chapter_number"]).and_then(num);
            let verse = get(o, &["verse", "v", "verse_number"]).and_then(num);
            let text = get(o, &["text", "content", "t"]).and_then(Value::as_str);
            if let (Some(b), Some(c), Some(v), Some(t)) = (book, chapter, verse, text) {
                out.add(b, c, v, t);
                any = true;
            }
        }
        any
    }
    fn chapters(book: BookId, value: &Value, out: &mut Collected) {
        let list: Vec<(u16, &Value)> = match value {
            Value::Object(o) => o.iter().filter_map(|(k, v)| Some((k.trim().parse().ok()?, v))).collect(),
            Value::Array(a) => a
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let n = v.as_object().and_then(|o| get(o, &["chapter", "number", "c"])).and_then(num);
                    (n.unwrap_or(i as u16 + 1), v)
                })
                .collect(),
            _ => vec![],
        };
        for (c, ch) in list {
            let ch = match ch {
                Value::Object(o) if get(o, &["verses"]).is_some() => get(o, &["verses"]).unwrap(),
                other => other,
            };
            let verses: Vec<(u16, &Value)> = match ch {
                Value::Object(o) => o.iter().filter_map(|(k, v)| Some((k.trim().parse().ok()?, v))).collect(),
                Value::Array(a) => a
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let n = v.as_object().and_then(|o| get(o, &["verse", "number", "v"])).and_then(num);
                        (n.unwrap_or(i as u16 + 1), v)
                    })
                    .collect(),
                _ => vec![],
            };
            for (v, t) in verses {
                if let Some(t) = verse_text(t) {
                    out.add(book, c, v, &t);
                }
            }
        }
    }

    match &value {
        Value::Array(items) => {
            flat(items, out);
        }
        Value::Object(o) => {
            if let Some(Value::Array(items)) = get(o, &["verses"]) {
                flat(items, out);
            } else if let Some(Value::Array(list)) = get(o, &["books"]) {
                for b in list {
                    let Value::Object(bo) = b else { continue };
                    let book = get(bo, &["name", "book", "id", "abbreviation", "osis", "usfm"])
                        .and_then(|n| n.as_str().map(str::to_string).or_else(|| n.as_u64().map(|n| n.to_string())))
                        .and_then(|n| find_book(&n));
                    if let (Some(book), Some(chs)) = (book, get(bo, &["chapters"])) {
                        chapters(book, chs, out);
                    }
                }
            } else {
                for (name, chs) in o {
                    if let Some(book) = find_book(name) {
                        chapters(book, chs, out);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Rows of book, chapter, verse, text (a header row is skipped; quoted fields are allowed)
fn import_delimited(text: &str, delimiter: char, out: &mut Collected) {
    for line in text.lines() {
        let fields = split_row(line, delimiter);
        if fields.len() < 4 {
            // "John 3:16<tab>text"
            if fields.len() == 2 {
                import_lines(&format!("{} {}", fields[0], fields[1]), out);
            }
            continue;
        }
        let (Some(book), Ok(c), Ok(v)) =
            (find_book(&fields[0]), fields[1].trim().parse::<u16>(), fields[2].trim().parse::<u16>())
        else {
            continue;
        };
        out.add(book, c, v, &fields[3..].join(&delimiter.to_string()));
    }
}

fn split_row(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delimiter && !quoted => fields.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// Lines like `John 3:16 For God so loved…` or `Jn 3.16<tab>…`
fn import_lines(text: &str, out: &mut Collected) {
    for line in text.lines() {
        let line = line.trim();
        // The reference ends at the first `c:v` (or `c.v`) after the book name
        let Some((book, c, v, rest)) = split_reference(line) else { continue };
        if let Some(book) = find_book(book) {
            out.add(book, c, v, rest);
        }
    }
}

fn split_reference(line: &str) -> Option<(&str, u16, u16, &str)> {
    let bytes = line.as_bytes();
    // Skip a leading book number ("1 John") before looking for the chapter
    let start = line.find(|c: char| c.is_alphabetic())?;
    let mut i = start;
    while i < bytes.len() && !bytes[i].is_ascii_digit() {
        i += 1;
    }
    let book = line[..i].trim();
    let rest = &line[i..];
    let sep = rest.find([':', '.'])?;
    let c: u16 = rest[..sep].trim().parse().ok()?;
    let after = &rest[sep + 1..];
    let end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
    let v: u16 = after[..end].parse().ok()?;
    Some((book, c, v, after[end..].trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        let mut c = Collected::default();
        import_lines("John 3:16 For God so loved\n1 John 1:1 That which was\nGen 1.1\tIn the beginning", &mut c);
        let books = c.into_books();
        assert_eq!(books.len(), 3);
        assert_eq!(books[0].name, "Genesis");
        assert_eq!(books[0].chapters[0][0], "In the beginning");
        assert_eq!(books[2].name, "1 John");
    }

    #[test]
    fn json_nested() {
        let mut c = Collected::default();
        import_json(r#"{"John": {"3": {"16": "For God", "17": "For God sent"}}}"#, &mut c).unwrap();
        let books = c.into_books();
        assert_eq!(books[0].chapters[2][15], "For God");
        assert_eq!(books[0].chapters[0].len(), 0);
    }

    #[test]
    fn usfm() {
        let dir = std::env::temp_dir().join(format!("cab-usfm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("JHN.usfm");
        std::fs::write(&path, "\\id JHN\n\\c 3\n\\p\n\\v 16 For God so loved the world.\n\\v 17-18 For God sent not.\n\\v 19 And this is.\n").unwrap();
        let (books, format) = import(&[path]).unwrap();
        assert_eq!(format, "usfm");
        let ch = &books[0].chapters[2];
        assert_eq!(ch[15], "For God so loved the world.");
        assert_eq!(ch[16], "For God sent not.");
        assert_eq!(ch[17], "");
        assert_eq!(ch[18], "And this is.");
    }
}
