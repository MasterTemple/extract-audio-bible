//! WhisperX output: its words and when each was said.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, Result},
    text::tokenize,
};

#[derive(Deserialize)]
struct RawTranscript {
    #[serde(default)]
    segments: Vec<RawSegment>,
    #[serde(default)]
    word_segments: Vec<RawWord>,
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
    pub words: Vec<Word>,
    /// Normalized tokens, each with the word it came from
    pub tokens: Vec<String>,
    pub token_word: Vec<u32>,
}

impl Transcript {
    pub fn open(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let raw: RawTranscript = serde_json::from_str(&text)
            .map_err(|e| AppError::msg(format!("{} is not WhisperX JSON: {e}", path.display())))?;
        Ok(Self::from_raw(raw))
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
        if raw.segments.iter().any(|s| !s.words.is_empty()) {
            for seg in &raw.segments {
                if seg.words.is_empty() {
                    // Not aligned: its words share the segment's time evenly
                    let parts: Vec<&str> = seg.text.split_whitespace().collect();
                    let (Some(s), Some(e)) = (seg.start, seg.end) else { continue };
                    let step = (e - s) / parts.len().max(1) as f64;
                    for (i, p) in parts.iter().enumerate() {
                        let w = RawWord {
                            word: p.to_string(),
                            start: Some(s + step * i as f64),
                            end: Some(s + step * (i + 1) as f64),
                            score: None,
                            speaker: None,
                        };
                        push(&mut words, &w);
                    }
                    continue;
                }
                // Untimed words in a segment are first given no time, then timed from their
                // neighbors below
                for w in &seg.words {
                    push(&mut words, w);
                }
                if let Some(last) = words.last_mut() {
                    if last.end.is_nan() {
                        last.end = seg.end.unwrap_or(f64::NAN);
                    }
                }
            }
        } else {
            for w in &raw.word_segments {
                push(&mut words, w);
            }
        }
        fill_times(&mut words);

        let mut tokens = vec![];
        let mut token_word = vec![];
        for (i, w) in words.iter().enumerate() {
            for t in tokenize(&w.text) {
                tokens.push(t);
                token_word.push(i as u32);
            }
        }
        Transcript { words, tokens, token_word }
    }

    pub fn duration(&self) -> f64 {
        self.words.last().map(|w| w.end).unwrap_or(0.0)
    }
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
        assert_eq!(t.tokens, ["for", "god", "so", "loved", "seventy"]);
        assert!(t.words[4].start >= 1.6 && t.words[4].end <= 3.0);
        assert!(!t.words[4].timed);
    }
}
