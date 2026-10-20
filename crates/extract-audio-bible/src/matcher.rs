/*!
Finding where a sermon reads a verse, with `scripture-quotes`: it finds the words of every
quotation, and this gives them times and cut points.

Cut points fall halfway into the pauses around a reading (capped), so no sound is clipped, and
inside a run they meet, so the reading plays back without a break.
*/

use scripture_quotes::{
    Engine, Kind, Options, Scope, ScopeError, Searcher, Translation, VerseRef, topos_bible::data::books::BookId,
};
use serde::{Deserialize, Serialize};

use crate::{
    bible::{Bible, bible_data},
    transcript::Transcript,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    /// Every word, in order, with next to nothing added
    Exact,
    /// Most of the verse, or all of it with some words changed or added
    Close,
    /// Only part of the verse
    Partial,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub id: String,
    pub source_id: String,
    pub book: u8,
    pub chapter: u16,
    pub verse: u16,
    /// The words read, by index into the transcript's words (inclusive)
    pub word_start: u32,
    pub word_end: u32,
    /// Where to cut: halfway into the pauses around the reading (capped), so no sound is clipped
    pub start: f64,
    pub end: f64,
    pub score: f32,
    /// How faithfully the verse's words were said, from the first read to the last
    #[serde(default)]
    pub accuracy: f32,
    /// How much of the verse was read
    pub coverage: f32,
    /// How much of what was said in the reading belongs to the verse
    pub precision: f32,
    pub kind: MatchKind,
    /// Per word of the verse's text (split on whitespace): whether it was heard
    pub verse_words_heard: Vec<bool>,
    /// Words in the reading that aren't the verse's (a comment, a repeated word)
    pub extra_words: Vec<u32>,
    pub run_id: Option<String>,
    /// The translation it fits best, by its Bible's id (`verse_words_heard` is of its text)
    #[serde(default)]
    pub translation: String,
    /// Other translations it was read from as faithfully as asked, best first, by their Bibles' ids
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_in: Vec<String>,
    /// Other verses worded the same, which it could as well be (`book.chapter.verse`, like
    /// `19.14.1`), when the sermon doesn't say which
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_at: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub source_id: String,
    pub book: u8,
    pub start_chapter: u16,
    pub start_verse: u16,
    pub end_chapter: u16,
    pub end_verse: u16,
    pub match_ids: Vec<String>,
    pub start: f64,
    pub end: f64,
}

/// A Bible's text, ready to be searched for
pub fn engine(bible: &Bible) -> Engine {
    engine_of([bible])
}

/// Several Bibles' texts, ready to be searched for together: each reading is of the translation
/// it fits best (named by its Bible's id). References are read with [`bible_data`], as the Bibles
/// were imported, so a deuterocanonical book is named and scoped like any other
pub fn engine_of<'b>(bibles: impl IntoIterator<Item = &'b Bible>) -> Engine {
    let mut builder = Engine::builder().data(bible_data().clone());
    for bible in bibles {
        let mut translation = Translation::new(&bible.id);
        for book in &bible.books {
            for (c, chapter) in book.chapters.iter().enumerate() {
                for (v, text) in chapter.iter().enumerate() {
                    translation.verse(VerseRef::new(book.id, c as u8 + 1, v as u8 + 1), text);
                }
            }
        }
        builder.add(translation);
    }
    builder.build()
}

/// Looks for one book's verses, or the whole Bible's
pub fn searcher(engine: &Engine, book: Option<u8>) -> Result<Searcher<'_>, ScopeError> {
    scoped(engine, book.map(|b| std::collections::BTreeSet::from([b])).as_ref())
}

/// Looks for some books' verses, or (with none) the whole Bible's
pub fn scoped<'e>(
    engine: &'e Engine,
    books: Option<&std::collections::BTreeSet<u8>>,
) -> Result<Searcher<'e>, ScopeError> {
    match books {
        Some(books) => engine.scoped(&books.iter().fold(Scope::default(), |scope, &b| scope.book(BookId(b)))),
        None => Ok(engine.searcher()),
    }
}

/// Every reading of every verse in one transcript
pub fn find_matches(
    searcher: &Searcher,
    transcript: &Transcript,
    source_id: &str,
    options: &Options,
) -> (Vec<Match>, Vec<Run>) {
    let found = searcher.search(&transcript.quotes, options);
    let words = &transcript.words;
    let engine = searcher.engine();
    let name = |t| engine.translation_name(t).unwrap_or_default().to_string();
    let mut matches: Vec<Match> = found
        .quotations
        .iter()
        .map(|q| {
            let (ws, we) = (q.words.start, q.words.end - 1);
            let before = if ws > 0 {
                (words[ws].start - words[ws - 1].end).max(0.0)
            } else {
                0.6
            };
            let after = match words.get(we + 1) {
                Some(next) => (next.start - words[we].end).max(0.0),
                None => 0.8,
            };
            let VerseRef { book, chapter, verse } = q.reference;
            Match {
                id: format!("{source_id}:{}.{chapter}.{verse}@{ws}", book.0),
                source_id: source_id.to_string(),
                book: book.0,
                chapter: chapter as u16,
                verse: verse as u16,
                word_start: ws as u32,
                word_end: we as u32,
                start: (words[ws].start - (before / 2.0).min(0.35)).max(0.0),
                end: words[we].end + (after / 2.0).min(0.5),
                score: q.score,
                accuracy: q.accuracy,
                coverage: q.coverage,
                precision: q.precision,
                kind: match q.kind {
                    Kind::Exact => MatchKind::Exact,
                    Kind::Close => MatchKind::Close,
                    Kind::Partial => MatchKind::Partial,
                },
                verse_words_heard: q.verse_words_heard.clone(),
                extra_words: q.extra_words.iter().map(|&w| w as u32).collect(),
                run_id: None,
                translation: name(q.translation),
                also_in: q.also_in.iter().map(|a| name(a.translation)).collect(),
                also_at: (q.also_at.iter())
                    .map(|r| crate::export::verse_key(r.book.0, r.chapter as u16, r.verse as u16))
                    .collect(),
            }
        })
        .collect();

    let mut runs = vec![];
    for r in &found.runs {
        let first = &matches[r.quotations[0]];
        let id = format!("run:{}", first.id);
        let last = &matches[r.quotations[r.quotations.len() - 1]];
        runs.push(Run {
            id: id.clone(),
            source_id: source_id.to_string(),
            book: first.book,
            start_chapter: first.chapter,
            start_verse: first.verse,
            end_chapter: last.chapter,
            end_verse: last.verse,
            match_ids: r.quotations.iter().map(|&k| matches[k].id.clone()).collect(),
            start: first.start,
            end: last.end,
        });
        for &k in &r.quotations {
            matches[k].run_id = Some(id.clone());
        }
        // Within a run, cuts between verses meet, so the reading plays back without a jump
        for pair in r.quotations.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (ea, sb) = (
                words[matches[a].word_end as usize].end,
                words[matches[b].word_start as usize].start,
            );
            if sb >= ea {
                let mid = (ea + sb) / 2.0;
                matches[a].end = mid;
                matches[b].start = mid;
            }
        }
    }
    (matches, runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bible::BibleBook, scope::BookScope};

    fn bible() -> Bible {
        Bible {
            id: "t".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![BibleBook {
                id: 43,
                name: "John".into(),
                osis: "John".into(),
                chapters: vec![
                    vec![],
                    vec![],
                    vec![
                        "Jesus answered and said unto him, Verily, verily, I say unto thee, Except a man be born again, he cannot see the kingdom of God.".into(),
                        "Nicodemus saith unto him, How can a man be born when he is old? can he enter the second time into his mother's womb, and be born?".into(),
                    ],
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    vec!["Jesus wept.".into(), "Then said the Jews, Behold how he loved him!".into()],
                ],
            }],
        }
    }

    fn transcript(text: &str) -> Transcript {
        let words: Vec<String> = text
            .split_whitespace()
            .enumerate()
            .map(|(i, w)| {
                format!(
                    r#"{{"word": "{w}", "start": {}, "end": {}}}"#,
                    i as f64 * 0.4,
                    i as f64 * 0.4 + 0.3
                )
            })
            .collect();
        let json = format!(
            r#"{{"segments": [{{"start": 0, "end": 1000, "text": "", "words": [{}]}}]}}"#,
            words.join(",")
        );
        Transcript::parse(&json).unwrap()
    }

    #[test]
    fn finds_exact_close_and_neighbors() {
        let b = bible();
        let engine = engine(&b);
        let searcher = searcher(&engine, Some(43)).unwrap();
        let t = transcript(
            "so turn with me. Jesus answered and said unto him verily verily I say unto thee except a man be born again he cannot see the kingdom of God. \
             Nicodemus saith unto him how can a man be born when he is old can he enter the second time into his mother's womb and be born? amen. \
             later on we read jesus wept then said the jews behold how he loved him and that's the point. \
             and again except a man be born again he can't see the kingdom of God",
        );
        let (matches, runs) = find_matches(&searcher, &t, "s", &Options::default());
        let refs: Vec<_> = matches.iter().map(|m| (m.chapter, m.verse, m.kind)).collect();
        assert!(refs.contains(&(3, 1, MatchKind::Exact)), "{refs:?}");
        assert!(refs.contains(&(3, 2, MatchKind::Exact)), "{refs:?}");
        assert!(
            refs.contains(&(11, 1, MatchKind::Exact)),
            "short verse by its neighbor: {refs:?}"
        );
        assert!(refs.contains(&(11, 2, MatchKind::Exact)), "{refs:?}");
        assert!(
            refs.iter().any(|r| r.0 == 3 && r.1 == 1 && r.2 != MatchKind::Exact),
            "the partial re-reading: {refs:?}"
        );
        assert_eq!(runs.len(), 2, "{runs:?}");
        let jn3 = matches
            .iter()
            .find(|m| m.chapter == 3 && m.verse == 1 && m.kind == MatchKind::Exact)
            .unwrap();
        assert_eq!(t.words[jn3.word_start as usize].text, "Jesus");
        assert_eq!(t.words[jn3.word_end as usize].text, "God.");
        // Cuts within a run meet
        let (a, b) = (&matches[0], &matches[1]);
        assert_eq!(a.run_id, b.run_id);
        assert_eq!(a.end, b.start);
    }

    #[test]
    fn deuterocanonical_books_are_named_and_scoped() {
        let mut b = bible();
        b.books = vec![BibleBook {
            id: 67,
            name: "Tobit".into(),
            osis: "Tob".into(),
            chapters: vec![vec![
                "I Tobit have walked all the days of my life in the ways of truth and justice".into(),
            ]],
        }];
        let engine = engine(&b);
        let tobit = scoped(
            &engine,
            BookScope {
                books: vec!["Tobit".into()],
                ..Default::default()
            }
            .books()
            .unwrap()
            .as_ref(),
        )
        .unwrap();
        let t = transcript("he says i tobit have walked all the days of my life in the ways of truth and justice");
        let (matches, _) = find_matches(&tobit, &t, "s", &Options::default());
        assert_eq!(matches.len(), 1, "{matches:?}");
        assert_eq!(engine.format(&VerseRef::new(67, 1, 1).passage()), "Tobit 1:1");
    }
}
