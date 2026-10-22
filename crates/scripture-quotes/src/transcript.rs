//! What was said, as words. Times are the caller's: a quotation says which words (and segments)
//! it is, and the caller maps those to audio when it has any.

use std::ops::Range;

use crate::text::{tokenize, tokenize_into};

/// The words of a transcript, normalized once so it can be searched many times
#[derive(Clone, Debug, Default)]
pub struct Transcript {
    pub(crate) tokens: Vec<String>,
    /// The word each token came from
    pub(crate) token_word: Vec<u32>,
    words: usize,
    /// Each word's segment (a line, a caption, a WhisperX segment)
    word_segment: Vec<u32>,
    segments: usize,
    /// Each word's bytes in the text it came from, for [`Transcript::from_text`]
    word_bytes: Vec<Range<usize>>,
}

impl Transcript {
    /// Words as they were said (WhisperX words, say), one segment
    pub fn from_words<S: AsRef<str>>(words: impl IntoIterator<Item = S>) -> Self {
        let mut t = Self::default();
        for w in words {
            t.push_word(w.as_ref());
        }
        t.segments = 1;
        t
    }

    /// Plain text, split into words on whitespace; [`Transcript::byte_range`] gives a quotation's
    /// place in it
    pub fn from_text(text: &str) -> Self {
        let mut t = Self::default();
        let base = text.as_ptr() as usize;
        for w in text.split_whitespace() {
            let start = w.as_ptr() as usize - base;
            t.word_bytes.push(start..start + w.len());
            t.push_word(w);
        }
        t.segments = 1;
        t
    }

    /// Segments of text with no word times (a transcript aligned only by segment, subtitles):
    /// each is split into words on whitespace, and [`Transcript::segment_range`] gives the
    /// segments a quotation spans
    pub fn from_segments<S: AsRef<str>>(segments: impl IntoIterator<Item = S>) -> Self {
        let mut t = Self::default();
        for s in segments {
            for w in s.as_ref().split_whitespace() {
                t.push_word(w);
            }
            t.segments += 1;
        }
        t
    }

    /// Adds a word to the current segment
    pub fn push_word(&mut self, word: &str) {
        let index = self.words as u32;
        let before = self.tokens.len();
        tokenize_into(word, &mut self.tokens);
        self.token_word
            .extend(std::iter::repeat_n(index, self.tokens.len() - before));
        self.word_segment.push(self.segments as u32);
        self.words += 1;
    }

    /// Starts a new segment: words pushed from now on belong to it
    pub fn end_segment(&mut self) {
        self.segments += 1;
    }

    pub fn word_count(&self) -> usize {
        self.words
    }

    pub fn segment_count(&self) -> usize {
        let open = self.word_segment.last().map_or(0, |s| *s as usize + 1);
        self.segments.max(open)
    }

    /// The segment a word is in
    pub fn segment_of(&self, word: usize) -> Option<usize> {
        self.word_segment.get(word).map(|s| *s as usize)
    }

    /// The segments some words span (end exclusive)
    pub fn segment_range(&self, words: &Range<usize>) -> Range<usize> {
        let first = self.segment_of(words.start).unwrap_or(0);
        let last = self
            .segment_of(words.end.saturating_sub(1).max(words.start))
            .unwrap_or(first);
        first..last + 1
    }

    /**
    Where a phrase was said, as word ranges (end exclusive), in order. The phrase is compared as
    quotations are: case, accents, and punctuation aside, numerals spelled out ("70 times 7"
    finds "seventy times seven"). With `prefix`, its last word may be the start of one ("for
    giv" finds "forgive" and "forgiveness"), for searching as someone types. Nothing for a
    phrase with no words
    */
    pub fn find(&self, phrase: &str, prefix: bool) -> Vec<Range<usize>> {
        let query = tokenize(phrase);
        let Some((last, rest)) = query.split_last() else {
            return vec![];
        };
        let n = query.len();
        let mut found: Vec<Range<usize>> = vec![];
        for i in 0..self.tokens.len().saturating_sub(n - 1) {
            let window = &self.tokens[i..i + n];
            let end = &window[n - 1];
            if window[..n - 1] != *rest
                || !(end == last || prefix && end.starts_with(last.as_str()))
            {
                continue;
            }
            let words = self.token_word[i] as usize..self.token_word[i + n - 1] as usize + 1;
            // A word split into several tokens is found once
            if found.last().is_some_and(|f| f.start == words.start) {
                continue;
            }
            found.push(words);
        }
        found
    }

    /**
    Where a query's words were said near each other, in order: each word, or "phrase in quotes",
    with at most [`Near::within`] words said between it and the next (in the order given, if
    [`Near::in_order`]). A word ending in `*` may be the start of one (`forgiv*`), as may the
    last with `prefix` (for searching as someone types, unless the query ends in a space). Words
    are compared as [`Transcript::find`] compares them, and with [`Near::skip_common`], common
    words outside quotes ("is", "the") are left out of the query (unless it's nothing else), so
    `Jesus is God` finds "Jesus, who is our God". Each place found gives the words that were the
    query's
    */
    pub fn find_near(&self, query: &str, near: &Near, prefix: bool) -> Vec<Place> {
        let terms = terms(query, near.skip_common, prefix);
        if terms.is_empty() {
            return vec![];
        }
        // Each term's places, as token ranges
        let occurs: Vec<Vec<Range<usize>>> = terms.iter().map(|t| self.occurrences(t)).collect();
        let spans = if near.in_order || terms.len() == 1 {
            in_order(&occurs, near.within)
        } else {
            any_order(&occurs, near.within)
        };
        let mut places: Vec<Place> = vec![];
        for hits in spans {
            let words = |r: &Range<usize>| {
                self.token_word[r.start] as usize..self.token_word[r.end - 1] as usize + 1
            };
            let hits: Vec<Range<usize>> = hits.iter().map(words).collect();
            let place = Place {
                words: hits.iter().map(|h| h.start).min().unwrap_or(0)
                    ..hits.iter().map(|h| h.end).max().unwrap_or(0),
                hits,
            };
            // A word split into several tokens is found once
            if places
                .last()
                .is_some_and(|p| p.words.start == place.words.start)
            {
                continue;
            }
            places.push(place);
        }
        places
    }

    /// Where a term was said, as token ranges, in order
    fn occurrences(&self, term: &Term) -> Vec<Range<usize>> {
        let n = term.tokens.len();
        let Some((last, rest)) = term.tokens.split_last() else {
            return vec![];
        };
        (0..self.tokens.len().saturating_sub(n - 1))
            .filter(|&i| {
                let end = &self.tokens[i + n - 1];
                self.tokens[i..i + n - 1] == *rest
                    && (end == last || term.prefix && end.starts_with(last.as_str()))
            })
            .map(|i| i..i + n)
            .collect()
    }

    /// Where some words are in the text given to [`Transcript::from_text`]
    pub fn byte_range(&self, words: &Range<usize>) -> Option<Range<usize>> {
        let first = self.word_bytes.get(words.start)?;
        let last = self.word_bytes.get(words.end.checked_sub(1)?)?;
        Some(first.start..last.end)
    }
}

/// How a query's words must be said to be found near each other ([`Transcript::find_near`])
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Near {
    /// The most words said between one of the query's words (or phrases) and the next
    pub within: usize,
    /// Its words said in the order given
    pub in_order: bool,
    /// Common words ("is", "the", "of") outside quotes left out of the query
    pub skip_common: bool,
}

impl Default for Near {
    fn default() -> Self {
        Near {
            within: 5,
            in_order: false,
            skip_common: true,
        }
    }
}

/// Where a query was found: the words from its first found to its last (end exclusive), and
/// the words that were the query's, in order
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub words: Range<usize>,
    pub hits: Vec<Range<usize>>,
}

/// Words left out of a query, when asked: English's commonest, which say little on their own
pub const COMMON_WORDS: &[&str] = &[
    "a", "about", "after", "all", "also", "am", "an", "and", "any", "are", "as", "at", "be",
    "been", "but", "by", "can", "did", "do", "does", "for", "from", "had", "has", "have", "he",
    "her", "him", "his", "how", "i", "if", "in", "into", "is", "it", "its", "just", "me", "my",
    "no", "not", "of", "on", "or", "our", "out", "she", "so", "than", "that", "the", "their",
    "them", "then", "there", "these", "they", "this", "those", "to", "up", "us", "very", "was",
    "we", "were", "what", "when", "which", "who", "will", "with", "would", "you", "your",
];

/// One of a query's words, or a phrase of them (quoted, or a word that's several tokens)
#[derive(Debug, PartialEq)]
struct Term {
    tokens: Vec<String>,
    /// Its last token may be the start of a word
    prefix: bool,
}

/// A query's terms: its quoted phrases and its words, without common ones if asked
fn terms(query: &str, skip_common: bool, prefix: bool) -> Vec<Term> {
    let mut terms = vec![];
    let mut common = vec![];
    // Typed last, its last word may still be being typed
    let open_end = prefix && !query.ends_with(char::is_whitespace) && !query.ends_with('"');
    for (i, piece) in query.split('"').enumerate() {
        if i % 2 == 1 {
            let tokens = tokenize(piece);
            if !tokens.is_empty() {
                terms.push(Term {
                    tokens,
                    prefix: false,
                });
            }
            continue;
        }
        for word in piece.split_whitespace() {
            let star = word.ends_with('*');
            let tokens = tokenize(word);
            if tokens.is_empty() {
                continue;
            }
            let term = Term {
                tokens,
                prefix: star,
            };
            if skip_common
                && term
                    .tokens
                    .iter()
                    .all(|t| COMMON_WORDS.contains(&t.as_str()))
                && !star
            {
                common.push((terms.len(), term));
            } else {
                terms.push(term);
            }
        }
    }
    // Nothing but common words: those are the query
    if terms.is_empty() {
        terms = common.into_iter().map(|(_, t)| t).collect();
    }
    if open_end
        && !query.trim_end().ends_with('"')
        && let Some(last) = terms.last_mut()
    {
        last.prefix = true;
    }
    terms
}

/// Places where every term was said, in order, with at most `within` tokens between each and the
/// next: for each place of the first term, the soonest place of each term after
fn in_order(occurs: &[Vec<Range<usize>>], within: usize) -> Vec<Vec<Range<usize>>> {
    let mut found = vec![];
    let mut after = 0;
    'first: for first in &occurs[0] {
        if first.start < after {
            continue;
        }
        let mut hits = vec![first.clone()];
        for places in &occurs[1..] {
            let end = hits.last().expect("one at least").end;
            let i = places.partition_point(|p| p.start < end);
            match places.get(i) {
                Some(next) if next.start - end <= within => hits.push(next.clone()),
                _ => continue 'first,
            }
        }
        after = hits.last().expect("one at least").end;
        found.push(hits);
    }
    found
}

/**
Places where every term was said, in any order, each place said within `within` tokens of the
one before: the places of all the terms in order, in clusters that close, and in each cluster
the shortest stretches with every term, one after another
*/
fn any_order(occurs: &[Vec<Range<usize>>], within: usize) -> Vec<Vec<Range<usize>>> {
    let mut all: Vec<(Range<usize>, usize)> = (occurs.iter().enumerate())
        .flat_map(|(term, places)| places.iter().map(move |p| (p.clone(), term)))
        .collect();
    all.sort_by_key(|(p, _)| (p.start, p.end));
    let mut found = vec![];
    let mut cluster: Vec<(Range<usize>, usize)> = vec![];
    let mut reach = 0;
    let mut close = |cluster: &mut Vec<(Range<usize>, usize)>| {
        let mut counts = vec![0usize; occurs.len()];
        let (mut covered, mut from) = (0, 0);
        for to in 0..cluster.len() {
            let term = cluster[to].1;
            counts[term] += 1;
            if counts[term] == 1 {
                covered += 1;
            }
            if covered < occurs.len() {
                continue;
            }
            // The shortest stretch ending here
            while counts[cluster[from].1] > 1 {
                counts[cluster[from].1] -= 1;
                from += 1;
            }
            found.push(cluster[from..=to].iter().map(|(p, _)| p.clone()).collect());
            counts.iter_mut().for_each(|c| *c = 0);
            (covered, from) = (0, to + 1);
        }
        cluster.clear();
    };
    for (place, term) in all {
        if !cluster.is_empty() && place.start > reach + within {
            close(&mut cluster);
        }
        reach = if cluster.is_empty() {
            place.end
        } else {
            reach.max(place.end)
        };
        cluster.push((place, term));
    }
    close(&mut cluster);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn words_segments_and_bytes() {
        let t = Transcript::from_segments(["Jesus wept.", "Then said the Jews 70"]);
        assert_eq!(
            t.tokens,
            ["jesus", "wept", "then", "said", "the", "jews", "seventy"]
        );
        assert_eq!(t.word_count(), 7);
        assert_eq!(t.segment_range(&(1..3)), 0..2);

        assert_eq!(t.find("JESUS, wept", false), [0..2]);
        assert_eq!(t.find("the jews seventy", false), [4..7]);
        assert_eq!(t.find("jews 70", false), [5..7]);
        assert!(t.find("je", false).is_empty());
        assert_eq!(t.find("je", true), [0..1, 5..6]);
        assert!(t.find(" .. ", true).is_empty());

        let text = "said  Jesus wept, and";
        let t = Transcript::from_text(text);
        assert_eq!(&text[t.byte_range(&(1..3)).unwrap()], "Jesus wept,");
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn words_near_each_other() {
        let t = Transcript::from_text(
            "Jesus is God. And God the Father sent Jesus, his only Son, to forgive. Jesus wept.",
        );
        let near = |within, in_order, skip_common| Near {
            within,
            in_order,
            skip_common,
        };
        let found = |query: &str, n: &Near| {
            (t.find_near(query, n, false).into_iter())
                .map(|p| (p.words, p.hits))
                .collect::<Vec<_>>()
        };
        // "is" is left out, and only the query's words are hits, in either order
        assert_eq!(
            found("Jesus is God", &near(3, false, true)),
            [(0..3, vec![0..1, 2..3]), (4..9, vec![4..5, 8..9])]
        );
        assert_eq!(
            found("Jesus is God", &near(1, false, true)),
            [(0..3, vec![0..1, 2..3])]
        );
        // In order, "God ... Jesus" isn't "Jesus ... God"; with nothing between, "Jesus is God" is too far
        assert_eq!(
            found("Jesus God", &near(3, true, true)),
            [(0..3, vec![0..1, 2..3])]
        );
        assert!(found("Jesus God", &near(0, false, true)).is_empty());
        // Common words kept, and a quoted phrase as one
        assert_eq!(
            found("Jesus is God", &near(0, true, false)),
            [(0..3, vec![0..1, 1..2, 2..3])]
        );
        assert_eq!(
            found("\"only son\" forgiv*", &near(2, true, true)),
            [(10..14, vec![10..12, 13..14])]
        );
        // As typed, the last word may be the start of one; nothing but common words is the query
        assert_eq!(t.find_near("son forg", &near(3, true, true), true).len(), 1);
        assert!(
            t.find_near("son forg ", &near(3, true, true), true)
                .is_empty()
        );
        assert_eq!(found("the", &near(3, true, true)), [(5..6, vec![5..6])]);
        assert!(found(" \"\" ", &near(3, true, true)).is_empty());
    }
}
