//! What was said, as words. Times are the caller's: a quotation says which words (and segments)
//! it is, and the caller maps those to audio when it has any.

use std::ops::Range;

use crate::text::tokenize_into;

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

    /// Where some words are in the text given to [`Transcript::from_text`]
    pub fn byte_range(&self, words: &Range<usize>) -> Option<Range<usize>> {
        let first = self.word_bytes.get(words.start)?;
        let last = self.word_bytes.get(words.end.checked_sub(1)?)?;
        Some(first.start..last.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_segments_and_bytes() {
        let t = Transcript::from_segments(["Jesus wept.", "Then said the Jews 70"]);
        assert_eq!(
            t.tokens,
            ["jesus", "wept", "then", "said", "the", "jews", "seventy"]
        );
        assert_eq!(t.word_count(), 7);
        assert_eq!(t.segment_range(&(1..3)), 0..2);

        let text = "said  Jesus wept, and";
        let t = Transcript::from_text(text);
        assert_eq!(&text[t.byte_range(&(1..3)).unwrap()], "Jesus wept,");
    }
}
