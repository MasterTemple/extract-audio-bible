//! Choosing a reading for each verse automatically ("Choose best" in the app).

use std::collections::HashMap;

use crate::{
    bible::BibleBook,
    export::verse_key,
    matcher::Match,
    project::{MatchSet, Project, Selection},
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
        }
    }
}

impl Project {
    /// Chooses `m` for its verse
    pub fn select(&mut self, m: &Match) {
        let key = verse_key(m.book, m.chapter, m.verse);
        let selection = Selection::of(m, self.selections.get(&key));
        self.selections.insert(key, selection);
    }

    /**
    For each verse of a chapter with no reading chosen, chooses its best reading scoring at
    least `min_score` (0.9: exact readings, 0.75: exact and close, 0: anything), preferring one
    that carries on from the verse before (the same run, then the same sermon) and readings in
    longer runs. Returns the verses chosen
    */
    pub fn choose_best(&mut self, book: &BibleBook, chapter: u16, matches: &MatchSet, min_score: f32) -> Vec<u16> {
        let mut by_verse: HashMap<u16, Vec<&Match>> = HashMap::new();
        for m in &matches.matches {
            if m.book == book.id && m.chapter == chapter {
                by_verse.entry(m.verse).or_default().push(m);
            }
        }
        // Best first, as the app lists them, so ties go the same way
        for list in by_verse.values_mut() {
            list.sort_by(|a, b| b.score.total_cmp(&a.score));
        }
        let run_length: HashMap<&str, usize> = matches.runs.iter().map(|r| (r.id.as_str(), r.match_ids.len())).collect();
        let by_id: HashMap<&str, &Match> = matches.matches.iter().map(|m| (m.id.as_str(), m)).collect();
        let Some(texts) = book.chapters.get(chapter as usize - 1) else { return vec![] };

        let mut chosen = vec![];
        let mut previous: Option<(Option<String>, String)> = None; // run, source
        for (i, text) in texts.iter().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            let verse = i as u16 + 1;
            if let Some(existing) = self.selections.get(&verse_key(book.id, chapter, verse)) {
                let run = by_id.get(existing.match_id.as_str()).and_then(|m| m.run_id.clone());
                previous = Some((run, existing.source_id.clone()));
                continue;
            }
            let best = (by_verse.get(&verse).into_iter().flatten())
                .filter(|m| m.score >= min_score)
                .map(|m| {
                    let run = m.run_id.as_deref().and_then(|r| run_length.get(r)).copied().unwrap_or(0);
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
                let verses: Vec<u16> =
                    (texts.iter().enumerate()).filter(|(_, t)| !t.trim().is_empty()).map(|(i, _)| i as u16 + 1).collect();
                let missing = crate::export::missing_verses(self, book, chapter);
                let with_readings = found.get(&(book.id, chapter)).map_or(0, |f| verses.iter().filter(|v| f.contains(v)).count());
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
