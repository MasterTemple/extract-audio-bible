//! Readings added by hand: what was said somewhere in a sermon (found by searching what was said,
//! say), taken as a verse's reading, so it can be chosen for the verse or joined to its reading as a
//! piece of it. They're kept with the project ([`Project::added_readings`]), so searching the
//! sermons again keeps them.

use scripture_quotes::text::tokenize;

use crate::{
    matcher::{Match, MatchKind},
    project::{MatchSet, Project},
    transcript::Word,
};

/// Where an added reading's id begins, so it's told from those found
pub const PREFIX: &str = "added:";

/// The verse an added reading is of, and what was said
pub struct Said<'a> {
    pub source_id: &'a str,
    /// The sermon's transcript's words
    pub words: &'a [Word],
    /// The words read, by index into `words` (inclusive)
    pub word_start: u32,
    pub word_end: u32,
}

/**
A reading of a verse (`verse_text`, of the Bible `translation`) from words said in a sermon. The
verse's words heard, and the words said that aren't the verse's, are found by lining the two up
token by token (as searching compares them: case, accents, and punctuation aside, numbers spelled
out); it's exact, close, or partial as a search would call it, and cut halfway into the pauses
around it, as a search cuts. `None` if the words aren't in the transcript
*/
pub fn reading_of(
    said: &Said,
    book: u8,
    chapter: u16,
    verse: u16,
    verse_text: &str,
    translation: &str,
) -> Option<Match> {
    let Said {
        source_id,
        words,
        word_start,
        word_end,
    } = *said;
    let (ws, we) = (word_start as usize, word_end as usize);
    if ws > we || we >= words.len() {
        return None;
    }
    // Each side's tokens, with the word each is of
    let verse_words: Vec<&str> = verse_text.split_whitespace().collect();
    let verse_tokens: Vec<(usize, String)> = (verse_words.iter().enumerate())
        .flat_map(|(i, w)| tokenize(w).into_iter().map(move |t| (i, t)))
        .collect();
    let said_tokens: Vec<(usize, String)> = (ws..=we)
        .flat_map(|i| tokenize(&words[i].text).into_iter().map(move |t| (i, t)))
        .collect();

    // The longest run of tokens the two have in common, in order
    let (n, m) = (verse_tokens.len(), said_tokens.len());
    let mut longest = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            longest[i][j] = if verse_tokens[i].1 == said_tokens[j].1 {
                longest[i + 1][j + 1] + 1
            } else {
                longest[i + 1][j].max(longest[i][j + 1])
            };
        }
    }
    let (mut verse_matched, mut said_matched) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if verse_tokens[i].1 == said_tokens[j].1 {
            (verse_matched[i], said_matched[j]) = (true, true);
            (i, j) = (i + 1, j + 1);
        } else if longest[i + 1][j] >= longest[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    // A verse word is heard if all its tokens were; one of punctuation alone, with its neighbor
    let mut heard = vec![true; verse_words.len()];
    let mut has_tokens = vec![false; verse_words.len()];
    for (k, (w, _)) in verse_tokens.iter().enumerate() {
        has_tokens[*w] = true;
        heard[*w] &= verse_matched[k];
    }
    for w in 0..heard.len() {
        if !has_tokens[w] {
            heard[w] = w == 0 || heard[w - 1];
        }
    }
    let mut extra: Vec<u32> = (said_tokens.iter().zip(&said_matched))
        .filter(|(_, matched)| !**matched)
        .map(|((w, _), _)| *w as u32)
        .collect();
    extra.dedup();

    let matched = verse_matched.iter().filter(|m| **m).count() as f32;
    let coverage = if n == 0 { 0.0 } else { matched / n as f32 };
    let precision = if m == 0 {
        0.0
    } else {
        said_matched.iter().filter(|s| **s).count() as f32 / m as f32
    };
    // How faithfully, from the verse's first token said to its last
    let first = verse_matched.iter().position(|m| *m);
    let last = verse_matched.iter().rposition(|m| *m);
    let accuracy = match (first, last) {
        (Some(f), Some(l)) => matched / (l - f + 1) as f32,
        _ => 0.0,
    };
    let kind = if coverage >= 0.97 && precision >= 0.9 {
        MatchKind::Exact
    } else if coverage >= 0.75 {
        MatchKind::Close
    } else {
        MatchKind::Partial
    };

    let before = if ws > 0 {
        (words[ws].start - words[ws - 1].end).max(0.0)
    } else {
        0.6
    };
    let after = words
        .get(we + 1)
        .map_or(0.8, |next| (next.start - words[we].end).max(0.0));
    Some(Match {
        id: format!("{PREFIX}{source_id}:{book}.{chapter}.{verse}@{ws}-{we}"),
        source_id: source_id.to_string(),
        book,
        chapter,
        verse,
        word_start,
        word_end,
        start: (words[ws].start - (before / 2.0).min(0.35)).max(0.0),
        end: words[we].end + (after / 2.0).min(0.5),
        score: coverage * accuracy * (0.5 + 0.5 * precision),
        accuracy,
        coverage,
        precision,
        kind,
        verse_words_heard: heard,
        extra_words: extra,
        run_id: None,
        translation: translation.to_string(),
        also_in: vec![],
        also_at: vec![],
        said: (ws..=we).map(|i| words[i].text.trim()).collect::<Vec<_>>().join(" "),
    })
}

impl Project {
    /// Whether a reading was added by hand
    pub fn is_added(id: &str) -> bool {
        id.starts_with(PREFIX)
    }

    /**
    Adds its readings added by hand of a chapter (or of every chapter, with `chapter` none) to
    readings found, those of its sermons still in it, each verse's best first as found ones are
    */
    pub fn with_added_readings(&self, set: &mut MatchSet, chapter: Option<(u8, u16)>) {
        let sermons: std::collections::HashSet<&str> = self.sources.iter().map(|s| s.id.as_str()).collect();
        let found: std::collections::HashSet<&str> = set.matches.iter().map(|m| m.id.as_str()).collect();
        let added: Vec<crate::Match> = (self.added_readings.iter())
            .filter(|m| chapter.is_none_or(|(b, c)| m.book == b && m.chapter == c))
            .filter(|m| sermons.contains(m.source_id.as_str()) && !found.contains(m.id.as_str()))
            .cloned()
            .collect();
        set.matches.extend(added);
        set.matches.sort_by(|a, b| {
            (a.book, a.chapter, a.verse)
                .cmp(&(b.book, b.chapter, b.verse))
                .then(b.score.total_cmp(&a.score))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<Word> {
        (text.split_whitespace().enumerate())
            .map(|(i, w)| Word {
                text: w.into(),
                start: i as f64,
                end: i as f64 + 0.8,
                score: None,
                speaker: None,
                timed: true,
            })
            .collect()
    }

    #[test]
    fn reads_a_verse_from_what_was_said() {
        let said = words("and he said jesus wept and then the jews said");
        let of = |start, end| {
            reading_of(
                &Said {
                    source_id: "s",
                    words: &said,
                    word_start: start,
                    word_end: end,
                },
                43,
                11,
                35,
                "Jesus wept.",
                "esv",
            )
            .unwrap()
        };
        let exact = of(3, 4);
        assert_eq!(exact.kind, MatchKind::Exact);
        assert_eq!(exact.verse_words_heard, [true, true]);
        assert!(exact.extra_words.is_empty());
        assert_eq!(exact.said, "jesus wept");
        assert!(Project::is_added(&exact.id));
        // Cut halfway into the pauses around it (a fifth of a second each side here)
        assert!(
            (exact.start - 2.9).abs() < 1e-9 && (exact.end - 4.9).abs() < 1e-9,
            "{} {}",
            exact.start,
            exact.end
        );

        // More said than the verse: still all of it, the rest extra
        let wider = of(1, 6);
        assert_eq!(wider.coverage, 1.0);
        assert_eq!(wider.extra_words, [1, 2, 5, 6]);
        assert_ne!(wider.kind, MatchKind::Exact);

        // Only some of it
        let part = of(4, 6);
        assert_eq!(part.verse_words_heard, [false, true]);
        assert_eq!(part.kind, MatchKind::Partial);

        assert!(
            reading_of(
                &Said {
                    source_id: "s",
                    words: &said,
                    word_start: 9,
                    word_end: 12
                },
                43,
                11,
                35,
                "Jesus wept.",
                "esv"
            )
            .is_none()
        );
    }
}
