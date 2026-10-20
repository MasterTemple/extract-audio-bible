//! Finding where something was said in sermons' transcripts: a phrase, compared the way verses
//! are (case, accents, and punctuation aside, numerals spelled out).

use serde::{Deserialize, Serialize};

use crate::{files::Files, project::Project, transcript::Transcript};

/// Where a phrase was said in a sermon
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Said {
    pub source_id: String,
    /// Its words, by index into the transcript's words (inclusive)
    pub word_start: u32,
    pub word_end: u32,
    /// When it was said, in seconds
    pub start: f64,
    pub end: f64,
    /// The words before it, it, and the words after it, as the transcript has them
    pub before: String,
    pub text: String,
    pub after: String,
}

/// How to look for a phrase
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FindOptions {
    /// Its last word may be the start of one ("forgiv" finds "forgive" and "forgiveness"), for
    /// searching as someone types
    pub prefix: bool,
    /// Words of context given on each side
    pub context: usize,
    /// The most places to give (0 for all)
    pub limit: usize,
}

impl Default for FindOptions {
    fn default() -> Self {
        FindOptions { prefix: false, context: 12, limit: 0 }
    }
}

/// Where `phrase` was said in a transcript (of the sermon `source_id`), in order
pub fn find_in(transcript: &Transcript, source_id: &str, phrase: &str, options: &FindOptions) -> Vec<Said> {
    let words = &transcript.words;
    let join = |range: std::ops::Range<usize>| words[range].iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    let mut found = transcript.quotes.find(phrase, options.prefix);
    if options.limit > 0 {
        found.truncate(options.limit);
    }
    (found.into_iter())
        .filter(|r| r.end <= words.len() && !r.is_empty())
        .map(|r| Said {
            source_id: source_id.to_string(),
            word_start: r.start as u32,
            word_end: r.end as u32 - 1,
            start: words[r.start].start,
            end: words[r.end - 1].end,
            before: join(r.start.saturating_sub(options.context)..r.start),
            text: join(r.clone()),
            after: join(r.end..(r.end + options.context).min(words.len())),
        })
        .collect()
}

/**
Where `phrase` was said in each of a project's sermons, in the sermons' order, their
transcripts read through `files` (up to `options.limit` places in all). Sermons whose transcripts
can't be read are given with why
*/
pub fn search_transcripts(
    project: &Project,
    files: &dyn Files,
    phrase: &str,
    options: &FindOptions,
) -> (Vec<Said>, Vec<(String, crate::Error)>) {
    let mut found = vec![];
    let mut errors = vec![];
    for source in &project.sources {
        let left = if options.limit == 0 { 0 } else { options.limit - found.len() };
        if options.limit > 0 && left == 0 {
            break;
        }
        match Transcript::open(files, &source.transcript_path) {
            Ok(t) => found.extend(find_in(&t, &source.id, phrase, &FindOptions { limit: left, ..options.clone() })),
            Err(e) => errors.push((project.source_title(source), e)),
        }
    }
    (found, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_phrase_with_its_context() {
        let t = Transcript::parse(
            r#"{"segments": [{"start": 0, "end": 7, "text": "So Jesus wept. Then said the Jews, behold how he loved him."}]}"#,
        )
        .unwrap();
        let found = find_in(&t, "s", "jesus WEPT", &FindOptions { context: 2, ..Default::default() });
        assert_eq!(found.len(), 1);
        let said = &found[0];
        assert_eq!((said.word_start, said.word_end), (1, 2));
        assert_eq!((said.before.as_str(), said.text.as_str(), said.after.as_str()), ("So", "Jesus wept.", "Then said"));
        assert!(said.start > 0.0 && said.end > said.start);

        assert_eq!(find_in(&t, "s", "lov", &FindOptions::default()).len(), 0);
        assert_eq!(find_in(&t, "s", "he lov", &FindOptions { prefix: true, ..Default::default() })[0].text, "he loved");
        let limited = find_in(&t, "s", "h", &FindOptions { prefix: true, limit: 2, ..Default::default() });
        assert_eq!(limited.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["how", "he"]);
    }
}
