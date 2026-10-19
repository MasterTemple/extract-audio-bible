/*!
Finding where a sermon reads a verse.

1. Every verse's word trigrams are indexed (trigrams too common to tell verses apart are left
   out)
2. A transcript's trigrams vote for (verse, offset) pairs; votes on nearly the same offset are
   one candidate reading
3. Each candidate is aligned to the verse word by word (Smith-Waterman, with near-miss words
   counted), which gives the exact words read, the verse's words that were skipped, and the
   words the speaker added
4. Around every reading, the verses before and after are looked for directly, which finds short
   verses ("Jesus wept") that have too few trigrams to be voted for
5. Readings that follow each other verse after verse are joined as runs
*/

use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
};

use serde::{Deserialize, Serialize};

use crate::{
    bible::Bible,
    text::{token_similarity, tokenize},
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchOptions {
    /// Readings scoring below this are dropped (0–1)
    pub min_score: f32,
    /// Only these books, when given
    #[serde(default)]
    pub books: Option<Vec<u8>>,
}

impl Default for MatchOptions {
    fn default() -> Self {
        Self { min_score: 0.45, books: None }
    }
}

struct IndexedVerse {
    book: u8,
    chapter: u16,
    verse: u16,
    tokens: Vec<String>,
    /// Tokens per whitespace-separated word of the text, to map heard tokens back to words
    word_tokens: Vec<u16>,
}

pub struct VerseIndex {
    verses: Vec<IndexedVerse>,
    trigrams: HashMap<u64, Vec<(u32, u16)>>,
    /// Verses of two words, which have no trigram, by their words
    pairs: HashMap<(String, String), Vec<u32>>,
}

/// Trigrams in more verses than this say little about which verse is read
const MAX_POSTINGS: usize = 48;

fn trigram(a: &str, b: &str, c: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (a, b, c).hash(&mut h);
    h.finish()
}

impl VerseIndex {
    pub fn new(bible: &Bible, books: Option<&[u8]>) -> Self {
        let mut verses = vec![];
        for book in &bible.books {
            if books.is_some_and(|only| !only.contains(&book.id)) {
                continue;
            }
            for (c, chapter) in book.chapters.iter().enumerate() {
                for (v, text) in chapter.iter().enumerate() {
                    if text.trim().is_empty() {
                        continue;
                    }
                    let word_tokens: Vec<u16> =
                        text.split_whitespace().map(|w| tokenize(w).len() as u16).collect();
                    verses.push(IndexedVerse {
                        book: book.id,
                        chapter: c as u16 + 1,
                        verse: v as u16 + 1,
                        tokens: tokenize(text),
                        word_tokens,
                    });
                }
            }
        }
        let mut trigrams: HashMap<u64, Vec<(u32, u16)>> = HashMap::new();
        for (i, v) in verses.iter().enumerate() {
            for (p, w) in v.tokens.windows(3).enumerate() {
                trigrams.entry(trigram(&w[0], &w[1], &w[2])).or_default().push((i as u32, p as u16));
            }
        }
        trigrams.retain(|_, list| list.len() <= MAX_POSTINGS);
        let mut pairs: HashMap<(String, String), Vec<u32>> = HashMap::new();
        for (i, v) in verses.iter().enumerate() {
            if let [a, b] = v.tokens.as_slice() {
                pairs.entry((a.clone(), b.clone())).or_default().push(i as u32);
            }
        }
        VerseIndex { verses, trigrams, pairs }
    }

    pub fn len(&self) -> usize {
        self.verses.len()
    }

    /// The verse after `i`, if it's the next verse of the same book
    fn next(&self, i: usize) -> Option<usize> {
        let (a, b) = (&self.verses[i], self.verses.get(i + 1)?);
        (a.book == b.book && is_next(a.chapter, a.verse, b.chapter, b.verse)).then_some(i + 1)
    }

    fn prev(&self, i: usize) -> Option<usize> {
        let (a, b) = (self.verses.get(i.checked_sub(1)?)?, &self.verses[i]);
        (a.book == b.book && is_next(a.chapter, a.verse, b.chapter, b.verse)).then_some(i - 1)
    }
}

/// Whether b follows a: the next verse, the first of the next chapter, or after a gap of
/// textless (bridged) verses, which the index never holds
fn is_next(ac: u16, av: u16, bc: u16, bv: u16) -> bool {
    (ac == bc && bv > av && bv - av <= 3) || (bc == ac + 1 && bv == 1)
}

/// A reading of one verse, by token positions in the transcript
#[derive(Clone, Debug)]
struct Candidate {
    verse: usize,
    tok_start: usize,
    tok_end: usize,
    score: f32,
    coverage: f32,
    precision: f32,
    heard: Vec<bool>,
    /// Token positions in the span not aligned to the verse
    extra: Vec<usize>,
}

const MATCH: i32 = 3;
const NEAR: i32 = 2;
const MISMATCH: i32 = -2;
const SKIPPED: i32 = -2;
const ADDED: i32 = -1;

/// The best local alignment of the verse's tokens with a window of the transcript's
fn align(verse: &[String], window: &[String], offset: usize, index: usize) -> Option<Candidate> {
    let (m, n) = (verse.len(), window.len());
    if m == 0 || n == 0 {
        return None;
    }
    let w = n + 1;
    let mut h = vec![0i32; (m + 1) * w];
    // 0 stop, 1 diagonal, 2 up (skipped a verse token), 3 left (added a transcript token)
    let mut dir = vec![0u8; (m + 1) * w];
    let mut best = (0, 0, 0);
    for i in 1..=m {
        for j in 1..=n {
            let sim = token_similarity(&verse[i - 1], &window[j - 1]);
            let diag = h[(i - 1) * w + j - 1] + match sim {
                2 => MATCH,
                1 => NEAR,
                _ => MISMATCH,
            };
            let up = h[(i - 1) * w + j] + SKIPPED;
            let left = h[i * w + j - 1] + ADDED;
            let (val, d) = [(0, 0), (diag, 1), (up, 2), (left, 3)]
                .into_iter()
                .max_by_key(|&(v, d)| (v, d == 1))
                .unwrap();
            h[i * w + j] = val;
            dir[i * w + j] = d;
            if val > best.0 {
                best = (val, i, j);
            }
        }
    }
    let (_, mut i, mut j) = best;
    if i == 0 {
        return None;
    }
    let end_j = j;
    let mut heard = vec![false; m];
    let mut aligned = HashSet::new();
    let mut matched = 0.0f32;
    while i > 0 && j > 0 && h[i * w + j] > 0 {
        match dir[i * w + j] {
            1 => {
                match token_similarity(&verse[i - 1], &window[j - 1]) {
                    2 => {
                        heard[i - 1] = true;
                        matched += 1.0;
                        aligned.insert(j - 1);
                    }
                    1 => {
                        heard[i - 1] = true;
                        matched += 0.8;
                        aligned.insert(j - 1);
                    }
                    _ => {}
                }
                i -= 1;
                j -= 1;
            }
            2 => i -= 1,
            3 => j -= 1,
            _ => break,
        }
    }
    let start_j = j;
    let span = end_j - start_j;
    let coverage = matched / m as f32;
    let precision = aligned.len() as f32 / span.max(1) as f32;
    let score = coverage * (0.75 + 0.25 * precision);
    let extra = (start_j..end_j).filter(|k| !aligned.contains(k)).map(|k| k + offset).collect();
    Some(Candidate {
        verse: index,
        tok_start: start_j + offset,
        tok_end: end_j + offset,
        score,
        coverage,
        precision,
        heard,
        extra,
    })
}

fn acceptable(c: &Candidate, verse_len: usize, min_score: f32) -> bool {
    let heard = c.heard.iter().filter(|h| **h).count();
    c.score >= min_score && (heard >= 3 || heard == verse_len)
}

fn overlap(a: &Candidate, b: &Candidate) -> f32 {
    let s = a.tok_start.max(b.tok_start);
    let e = a.tok_end.min(b.tok_end);
    if e <= s {
        return 0.0;
    }
    let shorter = (a.tok_end - a.tok_start).min(b.tok_end - b.tok_start).max(1);
    (e - s) as f32 / shorter as f32
}

/// Every reading of every verse in one transcript
pub fn find_matches(
    index: &VerseIndex,
    transcript: &Transcript,
    source_id: &str,
    options: &MatchOptions,
) -> (Vec<Match>, Vec<Run>) {
    let tokens = &transcript.tokens;
    let n = tokens.len();

    // Votes: verse → (transcript position, offset)
    let mut votes: HashMap<u32, Vec<(usize, isize)>> = HashMap::new();
    for (i, w) in tokens.windows(3).enumerate() {
        if let Some(list) = index.trigrams.get(&trigram(&w[0], &w[1], &w[2])) {
            for &(v, p) in list {
                votes.entry(v).or_default().push((i, i as isize - p as isize));
            }
        }
    }

    let mut candidates: Vec<Candidate> = vec![];
    for (v, mut hits) in votes {
        let verse = &index.verses[v as usize];
        let m = verse.tokens.len();
        hits.sort_unstable();
        let needed = ((m.saturating_sub(2)) / 6).clamp(1, 3);
        // Clusters of hits on about the same offset, close together
        let mut clusters: Vec<(isize, isize, usize, usize, usize)> = vec![]; // dmin, dmax, first, last, count
        for (i, d) in hits {
            match clusters.last_mut() {
                Some(c) if i <= c.3 + 12 + m / 4 && d >= c.0 - 10 && d <= c.1 + 10 => {
                    c.0 = c.0.min(d);
                    c.1 = c.1.max(d);
                    c.3 = i;
                    c.4 += 1;
                }
                _ => clusters.push((d, d, i, i, 1)),
            }
        }
        for (dmin, dmax, _, _, count) in clusters {
            if count < needed {
                continue;
            }
            let from = (dmin - 8).max(0) as usize;
            let to = ((dmax + m as isize + 8).max(0) as usize).min(n);
            if from >= to {
                continue;
            }
            if let Some(c) = align(&verse.tokens, &tokens[from..to], from, v as usize) {
                if acceptable(&c, m, options.min_score) {
                    candidates.push(c);
                }
            }
        }
    }

    // Two-word verses, said exactly
    for (i, w) in tokens.windows(2).enumerate() {
        if let Some(list) = index.pairs.get(&(w[0].clone(), w[1].clone())) {
            for &v in list {
                candidates.push(Candidate {
                    verse: v as usize,
                    tok_start: i,
                    tok_end: i + 2,
                    score: 1.0,
                    coverage: 1.0,
                    precision: 1.0,
                    heard: vec![true, true],
                    extra: vec![],
                });
            }
        }
    }

    // The verses next to each reading, looked for right where they'd be
    let mut accepted = suppress(candidates);
    let mut queue: Vec<usize> = (0..accepted.len()).collect();
    let mut tried: HashSet<(usize, usize, bool)> = HashSet::new();
    while let Some(k) = queue.pop() {
        let c = accepted[k].clone();
        for forward in [true, false] {
            let neighbor = if forward { index.next(c.verse) } else { index.prev(c.verse) };
            let Some(nv) = neighbor else { continue };
            let anchor = if forward { c.tok_end } else { c.tok_start };
            if !tried.insert((nv, anchor, forward)) {
                continue;
            }
            let exists = accepted.iter().any(|o| {
                o.verse == nv
                    && if forward {
                        o.tok_start + 3 >= anchor && o.tok_start <= anchor + 15
                    } else {
                        o.tok_end <= anchor + 3 && o.tok_end + 15 >= anchor
                    }
            });
            if exists {
                continue;
            }
            let len = index.verses[nv].tokens.len();
            let reach = len * 3 / 2 + 10;
            let (from, to) = if forward {
                (anchor.saturating_sub(2), (anchor + reach).min(n))
            } else {
                (anchor.saturating_sub(reach), (anchor + 2).min(n))
            };
            if from >= to {
                continue;
            }
            let Some(found) = align(&index.verses[nv].tokens, &tokens[from..to], from, nv) else {
                continue;
            };
            let near = if forward {
                found.tok_start <= anchor + 12
            } else {
                found.tok_end + 12 >= anchor
            };
            if near && acceptable(&found, len, options.min_score) {
                accepted.push(found);
                queue.push(accepted.len() - 1);
            }
        }
    }
    let accepted = suppress(accepted);
    to_matches(index, transcript, source_id, accepted)
}

/// The best readings: a verse read once is one reading (its best alignment), and a passage that
/// fits one verse far better than another (only sharing a phrase) is only the better one
fn suppress(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.tok_start.cmp(&b.tok_start)));
    let mut kept: Vec<Candidate> = vec![];
    'next: for c in candidates {
        for k in &kept {
            let o = overlap(&c, k);
            if k.verse == c.verse && o > 0.0 {
                continue 'next;
            }
            // Parallel passages with the same words score the same, and are both kept
            if o > 0.5 && k.score - c.score > 0.04 {
                continue 'next;
            }
        }
        kept.push(c);
    }
    kept
}

fn to_matches(
    index: &VerseIndex,
    transcript: &Transcript,
    source_id: &str,
    mut accepted: Vec<Candidate>,
) -> (Vec<Match>, Vec<Run>) {
    let words = &transcript.words;
    accepted.sort_by_key(|c| (c.tok_start, c.verse));
    let mut matches: Vec<Match> = accepted
        .iter()
        .map(|c| {
            let verse = &index.verses[c.verse];
            let ws = transcript.token_word[c.tok_start] as usize;
            let we = transcript.token_word[c.tok_end - 1] as usize;
            let before = if ws > 0 { (words[ws].start - words[ws - 1].end).max(0.0) } else { 0.6 };
            let after = match words.get(we + 1) {
                Some(next) => (next.start - words[we].end).max(0.0),
                None => 0.8,
            };
            let mut heard = vec![];
            let mut t = 0usize;
            for &count in &verse.word_tokens {
                let count = count as usize;
                // A word of punctuation alone has no tokens, and is heard with its neighbors
                let h = if count == 0 {
                    t == 0 || c.heard.get(t - 1).copied().unwrap_or(false)
                } else {
                    c.heard[t..(t + count).min(c.heard.len())].iter().all(|h| *h)
                };
                heard.push(h);
                t += count;
            }
            let mut extra: Vec<u32> = c.extra.iter().map(|&k| transcript.token_word[k]).collect();
            extra.dedup();
            let kind = if c.coverage >= 0.97 && c.precision >= 0.9 {
                MatchKind::Exact
            } else if c.coverage >= 0.75 {
                MatchKind::Close
            } else {
                MatchKind::Partial
            };
            Match {
                id: format!("{source_id}:{}.{}.{}@{ws}", verse.book, verse.chapter, verse.verse),
                source_id: source_id.to_string(),
                book: verse.book,
                chapter: verse.chapter,
                verse: verse.verse,
                word_start: ws as u32,
                word_end: we as u32,
                start: (words[ws].start - (before / 2.0).min(0.35)).max(0.0),
                end: words[we].end + (after / 2.0).min(0.5),
                score: c.score,
                coverage: c.coverage,
                precision: c.precision,
                kind,
                verse_words_heard: heard,
                extra_words: extra,
                run_id: None,
            }
        })
        .collect();

    // Runs: each reading followed closely by the next verse's
    let mut next_of: HashMap<usize, usize> = HashMap::new();
    let mut has_prev: HashSet<usize> = HashSet::new();
    for (a, ca) in accepted.iter().enumerate() {
        let Some(nv) = index.next(ca.verse) else { continue };
        let follower = (accepted.iter().enumerate())
            .filter(|(b, cb)| {
                cb.verse == nv
                    && !has_prev.contains(b)
                    && cb.tok_start + 3 >= ca.tok_end
                    && cb.tok_start <= ca.tok_end + 12
            })
            .min_by_key(|(_, cb)| cb.tok_start.abs_diff(ca.tok_end))
            .map(|(b, _)| b);
        if let Some(b) = follower {
            next_of.insert(a, b);
            has_prev.insert(b);
        }
    }
    let mut runs = vec![];
    for start in 0..accepted.len() {
        if has_prev.contains(&start) || !next_of.contains_key(&start) {
            continue;
        }
        let mut chain = vec![start];
        while let Some(&n) = next_of.get(chain.last().unwrap()) {
            chain.push(n);
        }
        let (first, last) = (&matches[chain[0]], &matches[*chain.last().unwrap()]);
        let id = format!("run:{}", first.id);
        let run = Run {
            id: id.clone(),
            source_id: source_id.to_string(),
            book: first.book,
            start_chapter: first.chapter,
            start_verse: first.verse,
            end_chapter: last.chapter,
            end_verse: last.verse,
            match_ids: chain.iter().map(|&k| matches[k].id.clone()).collect(),
            start: first.start,
            end: last.end,
        };
        for &k in &chain {
            matches[k].run_id = Some(id.clone());
        }
        runs.push(run);
    }
    // Within a run, cuts between verses meet, so the reading plays back without a jump
    for run in &runs {
        let ids: Vec<usize> =
            run.match_ids.iter().filter_map(|id| matches.iter().position(|m| &m.id == id)).collect();
        for pair in ids.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (ea, sb) = (words[matches[a].word_end as usize].end, words[matches[b].word_start as usize].start);
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
    use crate::bible::BibleBook;

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
            .map(|(i, w)| format!(r#"{{"word": "{w}", "start": {}, "end": {}}}"#, i as f64 * 0.4, i as f64 * 0.4 + 0.3))
            .collect();
        let json = format!(r#"{{"segments": [{{"start": 0, "end": 1000, "text": "", "words": [{}]}}]}}"#, words.join(","));
        let path = std::env::temp_dir().join(format!("cab-t-{}-{}.json", std::process::id(), text.len()));
        std::fs::write(&path, json).unwrap();
        Transcript::open(&path).unwrap()
    }

    #[test]
    fn finds_exact_close_and_neighbors() {
        let b = bible();
        let index = VerseIndex::new(&b, None);
        let t = transcript(
            "so turn with me. Jesus answered and said unto him verily verily I say unto thee except a man be born again he cannot see the kingdom of God. \
             Nicodemus saith unto him how can a man be born when he is old can he enter the second time into his mother's womb and be born? amen. \
             later on we read jesus wept then said the jews behold how he loved him and that's the point. \
             and again except a man be born again he can't see the kingdom of God",
        );
        let (matches, runs) = find_matches(&index, &t, "s", &MatchOptions::default());
        let refs: Vec<_> = matches.iter().map(|m| (m.chapter, m.verse, m.kind)).collect();
        assert!(refs.contains(&(3, 1, MatchKind::Exact)), "{refs:?}");
        assert!(refs.contains(&(3, 2, MatchKind::Exact)), "{refs:?}");
        assert!(refs.contains(&(11, 1, MatchKind::Exact)), "short verse by its neighbor: {refs:?}");
        assert!(refs.contains(&(11, 2, MatchKind::Exact)), "{refs:?}");
        assert!(refs.iter().any(|r| r.0 == 3 && r.1 == 1 && r.2 != MatchKind::Exact), "the partial re-reading: {refs:?}");
        assert_eq!(runs.len(), 2, "{runs:?}");
        let jn3 = matches.iter().find(|m| m.chapter == 3 && m.verse == 1 && m.kind == MatchKind::Exact).unwrap();
        assert_eq!(t.words[jn3.word_start as usize].text, "Jesus");
        assert_eq!(t.words[jn3.word_end as usize].text, "God.");
    }
}
