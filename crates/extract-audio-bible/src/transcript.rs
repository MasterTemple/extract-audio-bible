//! WhisperX output (or a Let's Church transcript): its words and when each was said. Segments
//! without word times have their words spread evenly over the segment.

use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    files::Files,
};

#[derive(Deserialize)]
struct RawTranscript {
    /// WhisperX's segments, or the paragraphs of a Let's Church transcript (with or without
    /// word times)
    #[serde(default, alias = "paragraphs")]
    segments: Vec<RawSegment>,
    #[serde(default)]
    word_segments: Vec<RawWord>,
    /// A Let's Church transcript's media id (or another transcript's `id`)
    #[serde(rename = "mediaId")]
    media_id: Option<String>,
    id: Option<serde_json::Value>,
    title: Option<String>,
}

#[derive(Deserialize)]
struct RawSegment {
    start: Option<f64>,
    end: Option<f64>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    words: Vec<RawWord>,
}

#[derive(Deserialize, Clone)]
struct RawWord {
    #[serde(alias = "text")]
    word: String,
    start: Option<f64>,
    end: Option<f64>,
    score: Option<f64>,
    speaker: Option<String>,
}

/// One word as spoken, with its time
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
    /// WhisperX's alignment confidence, when it gave one
    pub score: Option<f64>,
    pub speaker: Option<String>,
    /// Whether WhisperX gave this word its own time (numerals often have none, and are timed
    /// from their neighbors)
    pub timed: bool,
}

#[derive(Clone, Debug)]
pub struct Transcript {
    /// The id it gives its sermon (a Let's Church transcript's media id), if any
    pub id: Option<String>,
    /// The title it gives its sermon, if any
    pub title: Option<String>,
    pub words: Vec<Word>,
    /// Where each paragraph begins, by word: the transcript's segments (or a Let's Church
    /// transcript's paragraphs), or, when it has none, its long pauses. Starts with 0 when it has
    /// words
    pub paragraphs: Vec<usize>,
    /// The words, ready to be searched for quotations
    pub quotes: scripture_quotes::Transcript,
}

impl Transcript {
    /// A transcript file (WhisperX JSON, or a Let's Church transcript)
    pub fn open(files: &dyn Files, path: &str) -> Result<Self> {
        Self::parse(&String::from_utf8_lossy(&files.read(path)?))
            .map_err(|e| Error::invalid(format!("{path}: {e}")))
    }

    /// A transcript's JSON
    pub fn parse(json: &str) -> Result<Self> {
        let raw: RawTranscript = serde_json::from_str(json)
            .map_err(|e| Error::invalid(format!("not a WhisperX or Let's Church transcript: {e}")))?;
        Ok(Self::from_raw(raw))
    }

    /// Words with times, already read (from another transcriber's output, say)
    pub fn from_words(words: Vec<Word>) -> Self {
        let mut words = words;
        fill_times(&mut words);
        let quotes = scripture_quotes::Transcript::from_words(words.iter().map(|w| &w.text));
        let paragraphs = at_pauses(&words);
        Transcript { id: None, title: None, words, paragraphs, quotes }
    }

    fn from_raw(raw: RawTranscript) -> Self {
        let mut words: Vec<Word> = vec![];
        fn push(words: &mut Vec<Word>, w: &RawWord) {
            let text = w.word.trim();
            if text.is_empty() {
                return;
            }
            words.push(Word {
                text: text.to_string(),
                start: w.start.unwrap_or(f64::NAN),
                end: w.end.unwrap_or(f64::NAN),
                score: w.score,
                speaker: w.speaker.clone(),
                timed: w.start.is_some() && w.end.is_some(),
            });
        }
        // WhisperX keeps every word in `word_segments` too; segments are read unless only that
        // has the words (a segment with no word times has its words spread over its time)
        let only_word_segments = !raw.word_segments.is_empty() && raw.segments.iter().all(|s| s.words.is_empty());
        let mut paragraphs = vec![];
        if !only_word_segments {
            for seg in &raw.segments {
                if paragraphs.last() != Some(&words.len()) {
                    paragraphs.push(words.len());
                }
                if seg.words.is_empty() {
                    // Not aligned: its words share the segment's time evenly
                    // (with no time of its own, from its neighbors below)
                    let parts: Vec<&str> = seg.text.split_whitespace().collect();
                    let step = match (seg.start, seg.end) {
                        (Some(s), Some(e)) => Some((s, (e - s) / parts.len().max(1) as f64)),
                        _ => None,
                    };
                    for (i, p) in parts.iter().enumerate() {
                        let w = RawWord {
                            word: p.to_string(),
                            start: step.map(|(s, step)| s + step * i as f64),
                            end: step.map(|(s, step)| s + step * (i + 1) as f64),
                            score: None,
                            speaker: None,
                        };
                        push(&mut words, &w);
                        if let Some(last) = words.last_mut() {
                            last.timed = false;
                        }
                    }
                    continue;
                }
                // Untimed words in a segment are first given no time, then timed from their
                // neighbors below
                for w in &seg.words {
                    push(&mut words, w);
                }
                if let Some(last) = words.last_mut()
                    && last.end.is_nan()
                {
                    last.end = seg.end.unwrap_or(f64::NAN);
                }
            }
        } else {
            for w in &raw.word_segments {
                push(&mut words, w);
            }
        }
        fill_times(&mut words);
        // A segment that had no words doesn't start a paragraph
        paragraphs.retain(|&i| i < words.len());
        if paragraphs.is_empty() {
            paragraphs = at_pauses(&words);
        }

        let quotes = scripture_quotes::Transcript::from_words(words.iter().map(|w| &w.text));
        let id = raw.media_id.or(match raw.id {
            Some(serde_json::Value::String(s)) => Some(s),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        });
        let nonempty = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        Transcript { id: nonempty(id), title: nonempty(raw.title), words, paragraphs, quotes }
    }

    pub fn duration(&self) -> f64 {
        self.words.last().map(|w| w.end).unwrap_or(0.0)
    }
}

/// Paragraphs for words with none of their own: a new one after a pause of two seconds or more
fn at_pauses(words: &[Word]) -> Vec<usize> {
    if words.is_empty() {
        return vec![];
    }
    std::iter::once(0).chain((1..words.len()).filter(|&i| words[i].start - words[i - 1].end >= 2.0)).collect()
}

/// Give untimed words times between their timed neighbors, spread evenly
fn fill_times(words: &mut [Word]) {
    let n = words.len();
    let mut i = 0;
    while i < n {
        if !words[i].start.is_nan() && !words[i].end.is_nan() {
            i += 1;
            continue;
        }
        let gap_start = i;
        while i < n && (words[i].start.is_nan() || words[i].end.is_nan()) {
            i += 1;
        }
        let before = if gap_start > 0 { words[gap_start - 1].end } else { 0.0 };
        let after = if i < n { words[i].start } else { before + 0.4 * (i - gap_start) as f64 };
        let after = after.max(before);
        let step = (after - before) / (i - gap_start) as f64;
        for (k, w) in words[gap_start..i].iter_mut().enumerate() {
            let s = before + step * k as f64;
            if w.start.is_nan() {
                w.start = s;
            }
            if w.end.is_nan() || w.end < w.start {
                w.end = s + step;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_whisperx() {
        let json = r#"{"segments": [{"start": 0.5, "end": 3.0, "text": " For God so loved 70", "words": [
            {"word": "For", "start": 0.5, "end": 0.7, "score": 0.9},
            {"word": "God", "start": 0.8, "end": 1.0},
            {"word": "so", "start": 1.1, "end": 1.2},
            {"word": "loved", "start": 1.3, "end": 1.6},
            {"word": "70"}
        ]}], "language": "en"}"#;
        let t = Transcript::from_raw(serde_json::from_str(json).unwrap());
        assert_eq!(t.words.len(), 5);
        assert_eq!(t.quotes.word_count(), 5);
        assert!(t.words[4].start >= 1.6 && t.words[4].end <= 3.0);
        assert!(!t.words[4].timed);
    }

    #[test]
    fn no_word_times_at_all() {
        let json = r#"{"segments": [{"start": 0.0, "end": 2.0, "text": "Jesus wept."}, {"start": 2.0, "end": 5.0, "text": "Then said the Jews"}]}"#;
        let t = Transcript::parse(json).unwrap();
        assert_eq!(t.words.len(), 6);
        assert_eq!((t.words[1].start, t.words[1].end), (1.0, 2.0));
        assert!(t.words.iter().all(|w| !w.timed));
        assert_eq!(t.paragraphs, [0, 2]);
    }

    #[test]
    fn paragraphs_without_word_times() {
        let json = r#"{"mediaId": "x", "title": "Jesus Wept", "duration": 20, "paragraphs": [
            {"start": 0.0, "end": 2.0, "speaker": null, "text": "Jesus wept."},
            {"start": 3.0, "end": 6.0, "speaker": null, "text": "Then said the Jews,", "words": [
                {"word": "Then", "start": 3.0, "end": 3.5}, {"word": "said", "start": 3.6, "end": 4.0},
                {"word": "the", "start": 4.1, "end": 4.3}, {"word": "Jews,", "start": 4.4, "end": 5.0}]}
        ]}"#;
        let t = Transcript::from_raw(serde_json::from_str(json).unwrap());
        assert_eq!(t.words.len(), 6);
        assert_eq!((t.words[1].start, t.words[1].end), (1.0, 2.0));
        assert_eq!(t.words[2].start, 3.0);
        assert_eq!((t.id.as_deref(), t.title.as_deref()), (Some("x"), Some("Jesus Wept")));
        assert_eq!(t.paragraphs, [0, 2]);
        let from_words = Transcript::from_words(t.words.clone());
        assert_eq!(from_words.paragraphs, [0]);
    }
}
