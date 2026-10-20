/*!
Finding every quotation in a transcript.

1. The transcript's word trigrams vote for (verse text, offset) pairs; votes on nearly the same
   offset are one candidate reading
2. Each candidate is aligned to the verse word by word, which gives the words read, the verse's
   words that were skipped or changed, and the words the speaker added. The longest part that is
   accurate enough is kept
3. Around every reading, the verses before and after are looked for directly, which finds short
   verses ("Jesus wept") that have too few trigrams to be voted for
4. Readings of the same verse in several translations are one quotation, in the translation it
   fits best; a passage that fits one verse far better than another (only sharing a phrase) is
   only the better one
5. Quotations that follow each other verse after verse are joined as runs
*/

use std::{borrow::Cow, ops::Range};

use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use topos_bible::{
    data::books::BookId,
    segments::{Passage, Segment},
};

use crate::{
    align::{Aligner, Prepared, Reading, UNKNOWN},
    engine::{Engine, TranslationId, VerseRef},
    index::{Index, pair, trigram},
    transcript::Transcript,
};

/// How faithful and how long a quotation must be
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    /**
    How accurately the verse's words must be said, 0–1 (default 0.85). Of the verse's words from
    the first quoted to the last, the share said right; a near miss ("thee" for "the") counts
    0.8, and each place the speaker added words (a comment mid-verse, however long) counts half a
    word against it. A quotation that is only accurate in part is that part
    */
    pub min_accuracy: f32,
    /// How accurately a quotation of less than three quarters of a verse must be said, when that
    /// is more than `min_accuracy` (default 0.9): a short stretch with a word wrong is more often
    /// an everyday phrase than a quotation
    pub min_partial_accuracy: f32,
    /// How much of the verse must be quoted, 0–1 (default 0: any part)
    pub min_coverage: f32,
    /// Fewest of the verse's words a quotation of part of it must have (default 6), so everyday
    /// phrases ("at the end of") aren't taken for verses. A whole verse always counts
    pub min_words: usize,
    /// Fewest words for a quotation of part of a verse when the transcript also quotes its
    /// chapter at least `min_words` long elsewhere (default 3): a sermon on 1 John 1 saying
    /// "we have no sin" again is quoting it. Their words must also be rare enough together (14
    /// bits of [`Quotation::information`]) not to be anyone's ("and we are")
    pub min_words_in_context: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min_accuracy: 0.85,
            min_partial_accuracy: 0.9,
            min_coverage: 0.0,
            min_words: 6,
            min_words_in_context: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Every word, in order, with next to nothing added
    Exact,
    /// Most of the verse, or all of it with some words changed or added
    Close,
    /// Only part of the verse
    Partial,
}

/// The same quotation, as another translation words it
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlsoIn {
    pub translation: TranslationId,
    pub words: Range<usize>,
    pub accuracy: f32,
    pub coverage: f32,
    pub score: f32,
}

/// A verse quoted in the transcript
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quotation {
    pub reference: VerseRef,
    /// The translation it fits best
    pub translation: TranslationId,
    /// The transcript's words quoted (end exclusive)
    pub words: Range<usize>,
    /// How much of the verse was quoted, discounting how far it is from the verse (0–1); used to
    /// rank quotations
    pub score: f32,
    /// See [`Options::min_accuracy`]
    pub accuracy: f32,
    /// How much of the verse was quoted
    pub coverage: f32,
    /// How much of what was said belongs to the verse
    pub precision: f32,
    /// How unlikely the quoted words are to be the verse's by chance, in bits: each word counts
    /// by how few verses have it
    pub information: f32,
    pub kind: Kind,
    /// Per word of the verse's text (split on whitespace), in its translation: whether it was said
    pub verse_words_heard: Vec<bool>,
    /// Words in the quotation that aren't the verse's (a comment, a repeated word)
    pub extra_words: Vec<usize>,
    /// Other translations it was quoted from as accurately as asked, best first
    pub also_in: Vec<AlsoIn>,
    /// Other verses worded the same, which it could as well be (`2 Samuel 22:3` and `Psalm
    /// 18:2`), when nothing in the transcript says which
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_at: Vec<VerseRef>,
    /// The run it's part of, by index into [`Found::runs`]
    pub run: Option<usize>,
}

/// Verses quoted one after another
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub first: VerseRef,
    pub last: VerseRef,
    /// By index into [`Found::quotations`], in order
    pub quotations: Vec<usize>,
    pub words: Range<usize>,
}

impl Run {
    /// The verses as a topos passage, like `John 3:14-18`
    pub fn passage(&self) -> Passage {
        let (a, b) = (self.first, self.last);
        let segment = if a.chapter == b.chapter {
            Segment::chapter_verse_range(a.chapter, a.verse, b.verse)
        } else {
            Segment::chapter_range(a.chapter, a.verse, b.chapter, b.verse)
        };
        segment.as_segments().with_book(a.book)
    }

    pub fn book(&self) -> BookId {
        self.first.book
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Found {
    /// In the order they were said
    pub quotations: Vec<Quotation>,
    pub runs: Vec<Run>,
}

/// Searches transcripts for the verses of a scope (see [`Engine::searcher`] and
/// [`Engine::scoped`])
pub struct Searcher<'e> {
    engine: &'e Engine,
    index: Cow<'e, Index>,
}

impl<'e> Searcher<'e> {
    pub(crate) fn new(engine: &'e Engine, index: Cow<'e, Index>) -> Self {
        Self { engine, index }
    }

    pub fn engine(&self) -> &'e Engine {
        self.engine
    }

    /// Verses looked for
    pub fn verse_count(&self) -> usize {
        self.index.in_scope.iter().filter(|s| **s).count()
    }

    /// Every quotation in a transcript
    pub fn search(&self, transcript: &Transcript, options: &Options) -> Found {
        Search::new(self, transcript, options).run()
    }
}

struct Search<'a, 'e> {
    engine: &'e Engine,
    index: &'a Index,
    transcript: &'a Transcript,
    options: &'a Options,
    t: Prepared,
    aligner: Aligner,
}

/// Quotations of less than this much of a verse are partial
pub(crate) const WHOLE: f32 = 0.75;

/// Short quotations must be of words rare enough not to be anyone's ("and we are")
const SHORT_BITS: f32 = 14.0;

/// Part of a verse that other verses share, with nothing to say which it is, counts as a
/// quotation (of one, noting the others) only with this many of its words, this rare
const AMBIGUOUS_WORDS: usize = 8;
const AMBIGUOUS_BITS: f32 = 2.0 * SHORT_BITS;

/// How far insertions and skips may move a reading off the diagonal it was expected on
const DRIFT: isize = 10;

/// Whether two readings share words, as a share of the shorter
fn overlap(a: &Range<usize>, b: &Range<usize>) -> f32 {
    let (s, e) = (a.start.max(b.start), a.end.min(b.end));
    if e <= s {
        return 0.0;
    }
    let shorter = a.len().min(b.len()).max(1);
    (e - s) as f32 / shorter as f32
}

impl<'a, 'e> Search<'a, 'e> {
    fn new(searcher: &'a Searcher<'e>, transcript: &'a Transcript, options: &'a Options) -> Self {
        let engine = searcher.engine;
        let tokens = &transcript.tokens;
        let ids: Vec<u32> = tokens
            .iter()
            .map(|t| engine.vocab.get(t).unwrap_or(UNKNOWN))
            .collect();
        // Each distinct word's near misses, once
        let mut unknown: Vec<&str> = (tokens.iter().zip(&ids))
            .filter(|(_, id)| **id == UNKNOWN)
            .map(|(t, _)| t.as_str())
            .collect();
        unknown.sort_unstable();
        unknown.dedup();
        let unknown_near = engine.near_unknown(&unknown);
        let mut near_ids: Vec<u32> = vec![];
        let mut seen: FxHashMap<&str, (u32, u32)> = FxHashMap::default();
        let near = (tokens.iter().zip(&ids))
            .map(|(token, &id)| {
                *seen.entry(token).or_insert_with(|| {
                    let start = near_ids.len() as u32;
                    if id == UNKNOWN {
                        if let Ok(k) = unknown.binary_search(&token.as_str()) {
                            near_ids.extend_from_slice(&unknown_near[k]);
                        }
                    } else {
                        near_ids.extend_from_slice(engine.vocab.near(id));
                    }
                    (start, near_ids.len() as u32 - start)
                })
            })
            .collect();
        Search {
            engine,
            index: &searcher.index,
            transcript,
            options,
            t: Prepared {
                ids,
                near,
                near_ids,
            },
            aligner: Aligner::default(),
        }
    }

    /// Whether a reading is a quotation on its own (`Some(true)`), only with others of its
    /// chapter (`Some(false)`), or not at all
    fn acceptable(&self, r: &Reading) -> Option<bool> {
        let m = self.engine.text_tokens(r.text).len();
        let o = self.options;
        let min_accuracy = if r.coverage < WHOLE {
            o.min_accuracy.max(o.min_partial_accuracy)
        } else {
            o.min_accuracy
        };
        if r.accuracy < min_accuracy || r.coverage < o.min_coverage {
            return None;
        }
        if r.heard_count >= o.min_words.min(m) {
            Some(true)
        } else {
            (r.heard_count >= o.min_words_in_context.min(m) && r.information >= SHORT_BITS)
                .then_some(false)
        }
    }

    fn align(
        &mut self,
        text: u32,
        from: usize,
        to: usize,
        band: Option<(isize, isize)>,
    ) -> Option<Reading> {
        let mut r = self.aligner.align(
            self.engine,
            &self.t,
            text,
            from,
            to,
            band,
            self.options.min_accuracy,
            self.options
                .min_accuracy
                .max(self.options.min_partial_accuracy),
        )?;
        r.alone = self.acceptable(&r)?;
        Some(r)
    }

    fn run(mut self) -> Found {
        let n = self.t.ids.len();

        // Votes: (text, transcript position, offset)
        let mut votes: Vec<(u32, u32, i32)> = vec![];
        for (i, w) in self.t.ids.windows(3).enumerate() {
            if w.contains(&UNKNOWN) {
                continue;
            }
            if let Some(&(start, len)) = self.index.trigrams.get(&trigram(w[0], w[1], w[2])) {
                for &(text, p) in &self.index.postings[start as usize..(start + len) as usize] {
                    votes.push((text, i as u32, i as i32 - p as i32));
                }
            }
        }
        votes.sort_unstable();

        let mut readings: Vec<Reading> = vec![];
        for hits in votes.chunk_by(|a, b| a.0 == b.0) {
            let text = hits[0].0;
            let m = self.engine.text_tokens(text).len();
            let needed = (m.saturating_sub(2) / 6).clamp(1, 3);
            if hits.len() < needed {
                continue;
            }
            // Hits on about the same offset, close together
            let mut clusters: Vec<(i32, i32, u32, usize)> = vec![]; // dmin, dmax, last, count
            for &(_, i, d) in hits {
                match clusters.last_mut() {
                    Some(c) if i <= c.2 + 12 + m as u32 / 4 && d >= c.0 - 10 && d <= c.1 + 10 => {
                        c.0 = c.0.min(d);
                        c.1 = c.1.max(d);
                        c.2 = i;
                        c.3 += 1;
                    }
                    _ => clusters.push((d, d, i, 1)),
                }
            }
            for (dmin, dmax, _, count) in clusters {
                if count < needed {
                    continue;
                }
                let from = (dmin - 8).max(0) as usize;
                let to = ((dmax + m as i32 + 8).max(0) as usize).min(n);
                if from < to {
                    let band = (
                        dmin as isize - from as isize - DRIFT,
                        dmax as isize - from as isize + DRIFT,
                    );
                    if let Some(r) = self.align(text, from, to, Some(band)) {
                        readings.push(r);
                    }
                }
            }
        }

        // Two-word verses, said exactly
        if !self.index.pairs.is_empty() {
            for (i, w) in self.t.ids.windows(2).enumerate() {
                if w.contains(&UNKNOWN) {
                    continue;
                }
                for &text in self
                    .index
                    .pairs
                    .get(&pair(w[0], w[1]))
                    .into_iter()
                    .flatten()
                {
                    readings.push(Reading {
                        text,
                        tok_start: i,
                        tok_end: i + 2,
                        score: 1.0,
                        accuracy: 1.0,
                        coverage: 1.0,
                        precision: 1.0,
                        heard: vec![true, true],
                        heard_count: 2,
                        information: 0.0,
                        alone: true,
                        extra: vec![],
                    });
                }
            }
        }

        let readings = self.neighbors(best_of_each(readings));
        self.quotations(best_of_each(readings))
    }

    /// The verses next to each reading, looked for right where they'd be
    fn neighbors(&mut self, mut readings: Vec<Reading>) -> Vec<Reading> {
        let engine = self.engine;
        let n = self.t.ids.len();
        let mut queue: Vec<usize> = (0..readings.len()).filter(|&k| readings[k].alone).collect();
        let mut tried: FxHashSet<(u32, usize, bool)> = FxHashSet::default();
        // Texts read, by where: to tell whether a neighbor is already found
        let mut at: FxHashMap<u32, Vec<Range<usize>>> = FxHashMap::default();
        for r in &readings {
            at.entry(r.text).or_default().push(r.tok_start..r.tok_end);
        }
        while let Some(k) = queue.pop() {
            let (text, start, end) = (readings[k].text, readings[k].tok_start, readings[k].tok_end);
            for &slot in engine.text_owners(text) {
                if !self.index.in_scope[slot as usize] {
                    continue;
                }
                for forward in [true, false] {
                    let neighbor = if forward {
                        engine.next_slot(slot)
                    } else {
                        engine.prev_slot(slot)
                    };
                    let Some(ns) = neighbor.filter(|s| self.index.in_scope[*s as usize]) else {
                        continue;
                    };
                    let nt = engine.slots[ns as usize].text;
                    let anchor = if forward { end } else { start };
                    if !tried.insert((nt, anchor, forward)) {
                        continue;
                    }
                    let exists = at.get(&nt).is_some_and(|spans| {
                        spans.iter().any(|s| {
                            if forward {
                                s.start + 3 >= anchor && s.start <= anchor + 15
                            } else {
                                s.end <= anchor + 3 && s.end + 15 >= anchor
                            }
                        })
                    });
                    if exists {
                        continue;
                    }
                    let len = engine.text_tokens(nt).len();
                    let reach = len * 3 / 2 + 10;
                    let (from, to) = if forward {
                        (anchor.saturating_sub(2), (anchor + reach).min(n))
                    } else {
                        (anchor.saturating_sub(reach), (anchor + 2).min(n))
                    };
                    if from >= to {
                        continue;
                    }
                    let Some(found) = self.align(nt, from, to, None) else {
                        continue;
                    };
                    let near = if forward {
                        found.tok_start <= anchor + 12
                    } else {
                        found.tok_end + 12 >= anchor
                    };
                    if near {
                        at.entry(nt)
                            .or_default()
                            .push(found.tok_start..found.tok_end);
                        if found.alone {
                            queue.push(readings.len());
                        }
                        readings.push(found);
                    }
                }
            }
        }
        readings
    }

    fn quotations(&self, readings: Vec<Reading>) -> Found {
        let engine = self.engine;
        // Every in-scope verse each reading is a reading of, best first
        let mut entries: Vec<(VerseRef, u32, usize)> = vec![];
        for (k, r) in readings.iter().enumerate() {
            for &slot in engine.text_owners(r.text) {
                if self.index.in_scope[slot as usize] {
                    entries.push((engine.slots[slot as usize].reference, slot, k));
                }
            }
        }
        entries.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(readings[b.2].score.total_cmp(&readings[a.2].score))
                .then(readings[b.2].accuracy.total_cmp(&readings[a.2].accuracy))
                .then(a.1.cmp(&b.1))
        });

        // One quotation per verse per place, with the other translations that fit it
        let mut drafts: Vec<Draft> = vec![];
        for same_verse in entries.chunk_by(|a, b| a.0 == b.0) {
            let first = drafts.len();
            for &(_, slot, k) in same_verse {
                let span = readings[k].tok_start..readings[k].tok_end;
                let translation = engine.slots[slot as usize].translation;
                let joins = drafts[first..].iter_mut().find(|d| {
                    let r = &readings[d.reading];
                    overlap(&(r.tok_start..r.tok_end), &span) > 0.0
                });
                match joins {
                    Some(d) => {
                        let known = engine.slots[d.slot as usize].translation == translation
                            || d.also
                                .iter()
                                .any(|&(s, _)| engine.slots[s as usize].translation == translation);
                        if !known {
                            d.also.push((slot, k));
                        }
                    }
                    None => drafts.push(Draft {
                        slot,
                        reading: k,
                        also: vec![],
                        also_at: vec![],
                    }),
                }
            }
        }

        // Words that fit one verse clearly better than another are only the better one's. Better
        // is explaining as many of the words more accurately ("Then Satan answered the Lord" is
        // Job 1:9, not "Then Job answered the Lord"), or as many as accurately and much more of
        // the verse, or clearly more words and no worse otherwise. Parallel passages and verses
        // that quote each other (the commandments in Exodus 20 and Romans 13:9) fit alike, and
        // are both kept
        let matched = |r: &Reading| r.coverage * engine.text_tokens(r.text).len() as f32;
        let better = |a: &Reading, b: &Reading| {
            let (ma, mb) = (matched(a), matched(b));
            let as_many = ma >= mb - 0.5;
            let as_accurate = a.accuracy >= b.accuracy - 0.02;
            (as_many && a.accuracy >= b.accuracy + 0.05)
                || (as_many && as_accurate && a.score > b.score + 0.04)
                || (ma > mb + 0.5 && as_accurate && a.score >= b.score - 0.04)
        };
        drafts.sort_by(|a, b| {
            let (a, b) = (&readings[a.reading], &readings[b.reading]);
            matched(b)
                .total_cmp(&matched(a))
                .then(b.score.total_cmp(&a.score))
        });
        let mut kept: Vec<Draft> = vec![];
        for d in drafts {
            let r = &readings[d.reading];
            let span = r.tok_start..r.tok_end;
            let beaten = kept.iter().any(|k| {
                let kr = &readings[k.reading];
                overlap(&(kr.tok_start..kr.tok_end), &span) > 0.5 && better(kr, r)
            });
            if !beaten {
                kept.push(d);
            }
        }
        // Short quotations count where their chapter is quoted at length
        let chapters: FxHashSet<(BookId, u8)> = (kept.iter())
            .filter(|d| readings[d.reading].alone)
            .map(|d| engine.slots[d.slot as usize].reference)
            .map(|r| (r.book, r.chapter))
            .collect();
        kept.retain(|d| {
            let r = engine.slots[d.slot as usize].reference;
            readings[d.reading].alone || chapters.contains(&(r.book, r.chapter))
        });
        let mut kept = self.one_verse_per_place(kept, &readings);
        kept.sort_by_key(|d| {
            (
                readings[d.reading].tok_start,
                engine.slots[d.slot as usize].reference,
            )
        });

        let token_word = &self.transcript.token_word;
        let words_of =
            |r: &Reading| token_word[r.tok_start] as usize..token_word[r.tok_end - 1] as usize + 1;
        let mut quotations: Vec<Quotation> = kept
            .iter()
            .map(|d| {
                let r = &readings[d.reading];
                let slot = &engine.slots[d.slot as usize];
                let mut heard = vec![];
                let mut t = 0usize;
                for &count in engine.slot_words(d.slot) {
                    let count = count as usize;
                    // A word of punctuation alone has no tokens, and is heard with its neighbors
                    let h = if count == 0 {
                        t == 0 || r.heard.get(t - 1).copied().unwrap_or(false)
                    } else {
                        r.heard[t..(t + count).min(r.heard.len())]
                            .iter()
                            .all(|h| *h)
                    };
                    heard.push(h);
                    t += count;
                }
                let mut extra: Vec<usize> =
                    r.extra.iter().map(|&k| token_word[k] as usize).collect();
                extra.dedup();
                let kind = if r.coverage >= 0.97 && r.precision >= 0.9 {
                    Kind::Exact
                } else if r.coverage >= WHOLE {
                    Kind::Close
                } else {
                    Kind::Partial
                };
                Quotation {
                    reference: slot.reference,
                    translation: TranslationId(slot.translation),
                    words: words_of(r),
                    score: r.score,
                    accuracy: r.accuracy,
                    coverage: r.coverage,
                    precision: r.precision,
                    information: r.information,
                    kind,
                    verse_words_heard: heard,
                    extra_words: extra,
                    also_in: (d.also.iter())
                        .map(|&(s, k)| {
                            let r = &readings[k];
                            AlsoIn {
                                translation: TranslationId(engine.slots[s as usize].translation),
                                words: words_of(r),
                                accuracy: r.accuracy,
                                coverage: r.coverage,
                                score: r.score,
                            }
                        })
                        .collect(),
                    also_at: d.also_at.clone(),
                    run: None,
                }
            })
            .collect();
        let spans: Vec<Range<usize>> = kept
            .iter()
            .map(|d| readings[d.reading].tok_start..readings[d.reading].tok_end)
            .collect();
        let runs = runs(&mut quotations, &spans);
        Found { quotations, runs }
    }
}

impl Search<'_, '_> {
    /**
    Words that fit several verses alike (a phrase they share, or verses worded the same) are a
    quotation of one verse, not one of each. The transcript decides: a verse whose chapter it
    quotes elsewhere is the one (each, if several are). Otherwise a whole verse, or enough of one
    in rare enough words to be a quotation ("I will pour out my spirit on all flesh"), is kept
    once, noting the others it could be; a shorter or commoner part ("according to the will of
    God") is left out, being more often only a phrase verses share. `kept` is best first, as left
    by [`Search::quotations`]
    */
    fn one_verse_per_place(&self, kept: Vec<Draft>, readings: &[Reading]) -> Vec<Draft> {
        let engine = self.engine;
        let span = |d: &Draft| {
            let r = &readings[d.reading];
            r.tok_start..r.tok_end
        };
        let reference = |d: &Draft| engine.slots[d.slot as usize].reference;
        // Places: drafts of different verses over mostly the same words (most of the longer's,
        // so a phrase within a longer quotation isn't one)
        let same_words = |a: &Range<usize>, b: &Range<usize>| {
            let shared = a.end.min(b.end).saturating_sub(a.start.max(b.start));
            shared * 2 > a.len().max(b.len())
        };
        let n = kept.len();
        let mut place: Vec<usize> = (0..n).collect();
        fn root(place: &mut [usize], mut i: usize) -> usize {
            while place[i] != i {
                place[i] = place[place[i]];
                i = place[i];
            }
            i
        }
        for a in 0..n {
            for b in a + 1..n {
                if reference(&kept[a]) != reference(&kept[b])
                    && same_words(&span(&kept[a]), &span(&kept[b]))
                {
                    let (ra, rb) = (root(&mut place, a), root(&mut place, b));
                    place[ra.max(rb)] = ra.min(rb);
                }
            }
        }
        let places: Vec<usize> = (0..n).map(|i| root(&mut place, i)).collect();
        let mut keep = vec![true; n];
        let mut noted: Vec<Vec<VerseRef>> = vec![vec![]; n];
        for p in 0..n {
            let members: Vec<usize> = (0..n).filter(|&i| places[i] == p).collect();
            if members.len() < 2 {
                continue;
            }
            // The chapters quoted elsewhere in the transcript
            let quoted_elsewhere = |i: usize| {
                let r = reference(&kept[i]);
                (0..n).any(|e| {
                    places[e] != p
                        && (reference(&kept[e]).book, reference(&kept[e]).chapter)
                            == (r.book, r.chapter)
                })
            };
            let supported: Vec<usize> = members
                .iter()
                .copied()
                .filter(|&i| quoted_elsewhere(i))
                .collect();
            for &i in &members {
                keep[i] = supported.contains(&i);
            }
            if !supported.is_empty() {
                continue;
            }
            let quotation = |i: &usize| {
                let r = &readings[kept[*i].reading];
                r.coverage >= WHOLE
                    || r.heard_count >= AMBIGUOUS_WORDS && r.information >= AMBIGUOUS_BITS
            };
            let whole = |i: &usize| readings[kept[*i].reading].coverage >= WHOLE;
            // The verses it could as well be fit as well: whole too, or (when it's part of one)
            // as much a quotation
            let alike: Vec<usize> = if members.iter().any(whole) {
                members.iter().copied().filter(whole).collect()
            } else {
                members.iter().copied().filter(quotation).collect()
            };
            if let Some((&first, rest)) = alike.split_first() {
                keep[first] = true;
                noted[first] = rest.iter().map(|&i| reference(&kept[i])).collect();
                noted[first].sort();
                noted[first].dedup();
            }
        }
        let mut out = Vec::with_capacity(n);
        for ((mut d, k), refs) in kept.into_iter().zip(keep).zip(noted) {
            if k {
                d.also_at = refs;
                out.push(d);
            }
        }
        out
    }
}

/// A verse quoted at a place, before it's a [`Quotation`]: its reading, and the other
/// translations (and verses) it fits
struct Draft {
    slot: u32,
    reading: usize,
    also: Vec<(u32, usize)>,
    also_at: Vec<VerseRef>,
}

/// A verse text read once is one reading: its best alignment
fn best_of_each(mut readings: Vec<Reading>) -> Vec<Reading> {
    readings.sort_by(|a, b| {
        a.text
            .cmp(&b.text)
            .then(b.score.total_cmp(&a.score))
            .then(a.tok_start.cmp(&b.tok_start))
    });
    let mut kept: Vec<Reading> = Vec::with_capacity(readings.len());
    let mut group_start = 0;
    for r in readings {
        if kept.last().is_none_or(|k| k.text != r.text) {
            group_start = kept.len();
        }
        let span = r.tok_start..r.tok_end;
        if kept[group_start..]
            .iter()
            .any(|k| overlap(&(k.tok_start..k.tok_end), &span) > 0.0)
        {
            continue;
        }
        kept.push(r);
    }
    kept
}

/// Each quotation followed closely by the next verse's is a run
fn runs(quotations: &mut [Quotation], spans: &[Range<usize>]) -> Vec<Run> {
    let mut next_of: FxHashMap<usize, usize> = FxHashMap::default();
    let mut has_prev: FxHashSet<usize> = FxHashSet::default();
    for a in 0..quotations.len() {
        let follower = (0..quotations.len())
            .filter(|&b| {
                quotations[a]
                    .reference
                    .is_followed_by(&quotations[b].reference)
                    && !has_prev.contains(&b)
                    && spans[b].start + 3 >= spans[a].end
                    && spans[b].start <= spans[a].end + 12
            })
            .min_by_key(|&b| spans[b].start.abs_diff(spans[a].end));
        if let Some(b) = follower {
            next_of.insert(a, b);
            has_prev.insert(b);
        }
    }
    let mut runs = vec![];
    for start in 0..quotations.len() {
        if has_prev.contains(&start) || !next_of.contains_key(&start) {
            continue;
        }
        let mut chain = vec![start];
        while let Some(&n) = chain.last().and_then(|l| next_of.get(l)) {
            chain.push(n);
        }
        let (first, last) = (chain[0], chain[chain.len() - 1]);
        for &k in &chain {
            quotations[k].run = Some(runs.len());
        }
        runs.push(Run {
            first: quotations[first].reference,
            last: quotations[last].reference,
            words: quotations[first].words.start..quotations[last].words.end,
            quotations: chain,
        });
    }
    runs
}
