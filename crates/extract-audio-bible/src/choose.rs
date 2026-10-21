//! Choosing a reading for each verse automatically ("Choose best" in the app).

use std::collections::HashMap;

use crate::{
    bible::BibleBook,
    export::verse_key,
    matcher::Match,
    project::{MatchSet, Part, Project, Selection},
};

impl Selection {
    /// A reading chosen for its verse, keeping the volume, silence, and note chosen for the verse
    /// before (`previous`), if any
    pub fn of(m: &Match, previous: Option<&Selection>) -> Self {
        Selection {
            match_id: m.id.clone(),
            source_id: m.source_id.clone(),
            start: m.start,
            end: m.end,
            trim_start: 0.0,
            trim_end: 0.0,
            pad_before: previous.map_or(0.0, |p| p.pad_before),
            pad_after: previous.map_or(0.0, |p| p.pad_after),
            gain_db: previous.map_or(0.0, |p| p.gain_db),
            needs_trim: false,
            note: previous.map(|p| p.note.clone()).unwrap_or_default(),
            joined: vec![],
        }
    }
}

impl Part {
    /// A reading, uncut
    pub fn of(m: &Match) -> Self {
        Part {
            match_id: m.id.clone(),
            source_id: m.source_id.clone(),
            start: m.start,
            end: m.end,
            trim_start: 0.0,
            trim_end: 0.0,
        }
    }
}

/// Where in its verse a reading begins: the first of the verse's words heard
fn begins(m: &Match) -> usize {
    m.verse_words_heard.iter().position(|&h| h).unwrap_or(usize::MAX)
}

impl Project {
    /// Chooses `m` for its verse
    pub fn select(&mut self, m: &Match) {
        let key = verse_key(m.book, m.chapter, m.verse);
        let selection = Selection::of(m, self.selections.get(&key));
        self.selections.insert(key, selection);
    }

    /**
    Adds `m` to the reading chosen for its verse, to be played with it: for a verse read in
    pieces. It goes before the first part that begins later in the verse (by the words each
    has, looked up in `matches`), so the halves play in order; [`Selection::set_parts`]
    rearranges them. With nothing chosen for the verse, it's chosen. Returns false if it was
    already used
    */
    pub fn join(&mut self, m: &Match, matches: &MatchSet) -> bool {
        let key = verse_key(m.book, m.chapter, m.verse);
        let Some(selection) = self.selections.get_mut(&key) else {
            self.select(m);
            return true;
        };
        if selection.uses(&m.id) {
            return false;
        }
        let mut parts = selection.parts();
        let at = (parts.iter())
            .position(|p| {
                matches
                    .matches
                    .iter()
                    .find(|x| x.id == p.match_id)
                    .is_some_and(|x| begins(x) > begins(m))
            })
            .unwrap_or(parts.len());
        parts.insert(at, Part::of(m));
        selection.set_parts(parts);
        true
    }

    /**
    Takes a reading out of its verse's choice, leaving the other parts; taking the only one
    unchooses the verse. Returns false if it wasn't used
    */
    pub fn unjoin(&mut self, m: &Match) -> bool {
        let key = verse_key(m.book, m.chapter, m.verse);
        let Some(selection) = self.selections.get_mut(&key) else {
            return false;
        };
        if !selection.uses(&m.id) {
            return false;
        }
        let parts: Vec<Part> = selection.parts().into_iter().filter(|p| p.match_id != m.id).collect();
        if parts.is_empty() {
            self.selections.remove(&key);
        } else {
            selection.set_parts(parts);
        }
        true
    }

    /**
    For each verse of a chapter with no reading chosen, chooses its best reading scoring at
    least `min_score` (0.9: exact readings, 0.75: exact and close, 0: anything), preferring one
    that carries on from the verse before (the same run, then the same sermon) and readings in
    longer runs. Sermons with no audio (a transcript alone, and nowhere to download it from) are
    passed over. Returns the verses chosen
    */
    pub fn choose_best(&mut self, book: &BibleBook, chapter: u16, matches: &MatchSet, min_score: f32) -> Vec<u16> {
        let audible: std::collections::HashSet<&str> = self
            .sources
            .iter()
            .filter(|s| !self.audio_path(s).is_empty())
            .map(|s| s.id.as_str())
            .collect();
        let mut by_verse: HashMap<u16, Vec<&Match>> = HashMap::new();
        for m in &matches.matches {
            if m.book == book.id && m.chapter == chapter && audible.contains(m.source_id.as_str()) {
                by_verse.entry(m.verse).or_default().push(m);
            }
        }
        // Best first, as the app lists them, so ties go the same way
        for list in by_verse.values_mut() {
            list.sort_by(|a, b| b.score.total_cmp(&a.score));
        }
        let run_length: HashMap<&str, usize> = matches
            .runs
            .iter()
            .map(|r| (r.id.as_str(), r.match_ids.len()))
            .collect();
        let by_id: HashMap<&str, &Match> = matches.matches.iter().map(|m| (m.id.as_str(), m)).collect();
        let Some(texts) = book.chapters.get(chapter as usize - 1) else {
            return vec![];
        };

        let mut chosen = vec![];
        let mut previous: Option<(Option<String>, String)> = None; // run, source
        for (i, text) in texts.iter().enumerate() {
            let verse = i as u16 + 1;
            if text.trim().is_empty() || !self.covers(book.id, chapter, verse) {
                continue;
            }
            if let Some(existing) = self.selections.get(&verse_key(book.id, chapter, verse)) {
                // What's heard last is what the next verse would carry on from
                let last = (existing.joined.last()).map_or((&existing.match_id, &existing.source_id), |p| {
                    (&p.match_id, &p.source_id)
                });
                let run = by_id.get(last.0.as_str()).and_then(|m| m.run_id.clone());
                previous = Some((run, last.1.clone()));
                continue;
            }
            let best = (by_verse.get(&verse).into_iter().flatten())
                .filter(|m| m.score >= min_score)
                .map(|m| {
                    let run = m
                        .run_id
                        .as_deref()
                        .and_then(|r| run_length.get(r))
                        .copied()
                        .unwrap_or(0);
                    let mut s = m.score + run.min(10) as f32 * 0.01;
                    match &previous {
                        Some((Some(run), _)) if m.run_id.as_ref() == Some(run) => s += 0.15,
                        Some((_, source)) if &m.source_id == source => s += 0.03,
                        _ => {}
                    }
                    (s, *m)
                })
                .fold(None::<(f32, &Match)>, |best, (s, m)| match best {
                    Some((b, _)) if b >= s => best,
                    _ => Some((s, m)),
                })
                .map(|(_, m)| m);
            previous = best.map(|m| (m.run_id.clone(), m.source_id.clone()));
            if let Some(m) = best {
                self.select(m);
                chosen.push(verse);
            }
        }
        chosen
    }
}

/// How far along a chapter is ([`Project::status`])
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterStatus {
    pub book: u8,
    pub chapter: u16,
    /// Verses with text of their own
    pub verses: usize,
    pub chosen: usize,
    pub with_readings: usize,
    /// Verses with text and no reading chosen
    pub missing: Vec<u16>,
}

impl Project {
    /// The chapters of the project's book (or, for the whole Bible, those with a reading found
    /// or chosen), each with how many verses have readings and are chosen
    pub fn status(&self, bible: &crate::Bible, matches: &MatchSet) -> Vec<ChapterStatus> {
        let mut found: HashMap<(u8, u16), std::collections::HashSet<u16>> = HashMap::new();
        for m in &matches.matches {
            found.entry((m.book, m.chapter)).or_default().insert(m.verse);
        }
        let mut out = vec![];
        for book in bible.books.iter().filter(|b| self.book.is_none_or(|only| only == b.id)) {
            for (c, texts) in book.chapters.iter().enumerate() {
                let chapter = c as u16 + 1;
                if self.passage.is_some_and(|p| !p.has_chapter(chapter)) {
                    continue;
                }
                let verses: Vec<u16> = (texts.iter().enumerate())
                    .filter(|(_, t)| !t.trim().is_empty())
                    .map(|(i, _)| i as u16 + 1)
                    .filter(|&v| self.covers(book.id, chapter, v))
                    .collect();
                let missing = crate::export::missing_verses(self, book, chapter);
                let with_readings = found
                    .get(&(book.id, chapter))
                    .map_or(0, |f| verses.iter().filter(|v| f.contains(v)).count());
                if self.book.is_none() && with_readings == 0 && missing.len() == verses.len() {
                    continue;
                }
                out.push(ChapterStatus {
                    book: book.id,
                    chapter,
                    verses: verses.len(),
                    chosen: verses.len() - missing.len(),
                    with_readings,
                    missing,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Bible, BibleBook,
        matcher::MatchKind,
        project::{Project, Source},
    };

    /// A reading of John 11:35–36 that heard the verse's words `heard` (of four)
    fn reading(id: &str, verse: u16, start: f64, end: f64, heard: [bool; 4]) -> Match {
        Match {
            id: id.into(),
            source_id: "s".into(),
            book: 43,
            chapter: 11,
            verse,
            word_start: 0,
            word_end: 0,
            start,
            end,
            score: 0.5,
            accuracy: 1.0,
            coverage: 0.5,
            precision: 1.0,
            kind: MatchKind::Partial,
            verse_words_heard: heard.into(),
            extra_words: vec![],
            run_id: None,
            translation: "b".into(),
            also_in: vec![],
            also_at: vec![],
            said: String::new(),
        }
    }

    #[test]
    fn joins_a_verse_read_in_pieces() {
        let book = BibleBook {
            id: 43,
            name: "John".into(),
            osis: "John".into(),
            chapters: (1..=11)
                .map(|c| if c == 11 { vec!["a b c d".into(); 36] } else { vec![] })
                .collect(),
        };
        let bible = Bible {
            id: "b".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![book],
        };
        let mut project = Project::new("p", "", &bible, Some(43));
        project.sources.push(Source {
            id: "s".into(),
            label: "s".into(),
            audio_path: "s.mp3".into(),
            transcript_path: "s.json".into(),
            duration: None,
            word_count: 0,
            match_count: 0,
            file_id: String::new(),
        });
        project.settings.chapter_lead_in = 0.0;
        let second_half = reading("late", 35, 100.0, 102.0, [false, false, true, true]);
        let first_half = reading("early", 35, 10.0, 11.0, [true, true, false, false]);
        let next = reading("next", 36, 102.0, 105.0, [true; 4]);
        let matches = MatchSet {
            matches: vec![second_half.clone(), first_half.clone(), next.clone()],
            ..Default::default()
        };

        project.select(&second_half);
        // The first half goes first, though it was added second
        assert!(project.join(&first_half, &matches));
        assert!(!project.join(&first_half, &matches));
        let sel = &project.selections["43.11.35"];
        assert_eq!(
            sel.parts().iter().map(|p| p.match_id.as_str()).collect::<Vec<_>>(),
            ["early", "late"]
        );

        // Heard one after the other, and verse 36 carries straight on from the second half
        project.select(&next);
        let book = bible.book(43).unwrap();
        let times = crate::chapter_timeline(&project, book, 11);
        assert_eq!((times[0].start, times[0].end), (0.0, 3.0));
        assert_eq!((times[1].start, times[1].end), (3.0, 6.0));
        let (pieces, _) = crate::export::chapter_pieces(&project, book, 11);
        let audio = pieces
            .iter()
            .filter(|p| matches!(p, crate::export::Piece::Audio(_)))
            .count();
        assert_eq!(audio, 2);

        // Rearranged, then taken apart
        let sel = project.selections.get_mut("43.11.35").unwrap();
        let mut parts = sel.parts();
        parts.reverse();
        sel.set_parts(parts);
        assert_eq!(project.selections["43.11.35"].match_id, "late");
        assert!(project.unjoin(&second_half));
        assert_eq!(project.selections["43.11.35"].parts().len(), 1);
        assert!(project.unjoin(&first_half));
        assert!(!project.selections.contains_key("43.11.35"));
    }

    #[test]
    fn chooses_only_from_sermons_with_audio() {
        let book = BibleBook {
            id: 43,
            name: "John".into(),
            osis: "John".into(),
            chapters: (1..=11)
                .map(|c| if c == 11 { vec!["a b c d".into(); 36] } else { vec![] })
                .collect(),
        };
        let bible = Bible {
            id: "b".into(),
            name: "Test".into(),
            abbreviation: "T".into(),
            language: None,
            format: "text".into(),
            books: vec![book],
        };
        let mut project = Project::new("p", "", &bible, Some(43));
        // A transcript alone
        project.add_sources(&["s.json".into()]);
        project.sources[0].id = "s".into();
        let matches = MatchSet {
            matches: vec![reading("wept", 35, 1.0, 2.0, [true; 4])],
            ..Default::default()
        };
        let john = bible.book(43).unwrap();
        assert!(
            project.choose_best(john, 11, &matches, 0.0).is_empty(),
            "it has no audio"
        );
        project.settings.audio_url = "https://example.com/{id}.mp3".into();
        assert_eq!(project.choose_best(john, 11, &matches, 0.0), [35]);
    }
}
