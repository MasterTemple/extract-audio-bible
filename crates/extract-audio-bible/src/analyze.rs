//! Searching all of a project's sermons for its verses.

use scripture_quotes::Engine;

use crate::{
    bible::Bible,
    error::{Error, Result},
    files::Files,
    matcher,
    project::{MatchSet, Project, now},
    transcript::Transcript,
};

/// What [`analyze`] found
#[derive(Debug, Default)]
pub struct Analysis {
    pub matches: MatchSet,
    /// Sermons that couldn't be read, with why
    pub errors: Vec<(String, Error)>,
}

/**
Finds every reading of the project's book (or the whole Bible) in every sermon's transcript.
The project's sources get their word counts, durations, and match counts (and the ids and titles
their transcripts give: [`Project::identify`]), and its selections follow their readings to their
new ids. `progress` is told each sermon as it's searched (from
several threads at once, natively)
*/
pub fn analyze(
    project: &mut Project,
    bible: &Bible,
    files: &dyn Files,
    progress: &(dyn Fn(usize, usize, &str) + Sync),
) -> Result<Analysis> {
    analyze_with(project, &matcher::engine(bible), files, progress)
}

/// [`analyze`] with the Bible already indexed ([`matcher::engine`]: worth keeping when searching
/// the same Bible again)
pub fn analyze_with(
    project: &mut Project,
    engine: &Engine,
    files: &dyn Files,
    progress: &(dyn Fn(usize, usize, &str) + Sync),
) -> Result<Analysis> {
    let books = project.books_searched()?;
    let searcher = matcher::scoped(engine, books.as_ref()).map_err(|e| Error::invalid(e.to_string()))?;
    let options = scripture_quotes::Options { min_accuracy: project.settings.min_accuracy, ..Default::default() };
    let total = project.sources.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let search = |source: &crate::project::Source| {
        let found = Transcript::open(files, &source.transcript_path).map(|t| {
            let (matches, runs) = matcher::find_matches(&searcher, &t, &source.id, &options);
            (t.words.len(), t.duration(), matches, runs, t.id, t.title)
        });
        let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        progress(n, total, &source.label);
        found
    };
    #[cfg(not(target_arch = "wasm32"))]
    let results: Vec<_> = {
        use rayon::prelude::*;
        project.sources.par_iter().map(search).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let results: Vec<_> = project.sources.iter().map(search).collect();

    let mut analysis = Analysis { matches: MatchSet { analyzed_at: now(), ..Default::default() }, errors: vec![] };
    let mut identities = vec![];
    for (source, result) in project.sources.iter_mut().zip(results) {
        match result {
            Ok((words, duration, matches, runs, id, title)) => {
                identities.push((source.id.clone(), id, title));
                source.word_count = words;
                source.match_count = matches.len();
                source.duration = source.duration.or(Some(duration));
                analysis.matches.matches.extend(matches);
                analysis.matches.runs.extend(runs);
            }
            Err(e) => analysis.errors.push((source.label.clone(), e)),
        }
    }
    for (source, id, title) in identities {
        project.identify(&source, id.as_deref(), title.as_deref());
    }
    // Selections follow their readings to their new ids, if the analysis changed them
    let set = &analysis.matches;
    let follow = |match_id: &mut String, source_id: &str, start: f64, end: f64| {
        if set.matches.iter().any(|m| &m.id == match_id) {
            return;
        }
        let found = set.matches.iter().find(|m| {
            m.source_id == source_id
                && m.start < end
                && m.end > start
                && match_id.contains(&format!(":{}.{}.{}@", m.book, m.chapter, m.verse))
        });
        if let Some(m) = found {
            *match_id = m.id.clone();
        }
    };
    for sel in project.selections.values_mut() {
        follow(&mut sel.match_id, &sel.source_id, sel.start, sel.end);
        for part in &mut sel.joined {
            follow(&mut part.match_id, &part.source_id, part.start, part.end);
        }
    }
    project.updated_at = now();
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BookScope, Memory, Testament, import};

    fn bible(id: &str, lines: &str) -> Bible {
        Bible::new(id, id, id.to_uppercase(), import(vec![(format!("{id}.txt"), lines.as_bytes().to_vec())]).unwrap())
    }

    #[test]
    fn every_verse_read_in_any_translation() {
        let kjv = bible(
            "kjv",
            "John 11:35 Jesus wept.\nJohn 11:36 Then said the Jews, Behold how he loved him!\nGenesis 1:1 In the beginning God created the heaven and the earth.",
        );
        let web = bible(
            "web",
            "John 11:35 Jesus wept.\nJohn 11:36 The Jews therefore said, See how much affection he had for him!\nGenesis 1:1 In the beginning, God created the heavens and the earth.",
        );
        let mut files = Memory::new();
        files.insert(
            "s.json",
            br#"{"segments": [{"start": 0, "end": 30, "text": "In the beginning, God created the heavens and the earth. Then Jesus wept. The Jews therefore said, See how much affection he had for him!"}]}"#.to_vec(),
        );
        let new_testament = BookScope { testaments: vec![Testament::New], ..Default::default() };
        let mut project = Project::readings("p", "", &[&kjv, &web], new_testament).unwrap();
        assert_eq!(project.name, "Verses read (KJV, WEB)");
        project.sources.push(crate::Source {
            id: "s".into(),
            label: "s".into(),
            audio_path: String::new(),
            transcript_path: "s.json".into(),
            duration: None,
            word_count: 0,
            match_count: 0,
            file_id: "s".into(),
        });
        let engine = matcher::engine_of([&kjv, &web]);
        let found = analyze_with(&mut project, &engine, &files, &|_, _, _| {}).unwrap();
        let read: Vec<(u16, &str, &[String])> =
            found.matches.matches.iter().map(|m| (m.verse, m.translation.as_str(), m.also_in.as_slice())).collect();
        // Genesis is outside the New Testament; "Jesus wept" is both translations'
        assert_eq!(read.len(), 2, "{read:?}");
        assert_eq!(read[0].0, 35);
        assert!(read[0].1 == "kjv" && read[0].2 == ["web"] || read[0].1 == "web" && read[0].2 == ["kjv"], "{read:?}");
        assert_eq!((read[1].0, read[1].1), (36, "web"));

        let rows = crate::reading_rows(&project, &found.matches, &[&kjv, &web], &files);
        assert_eq!(rows[1].reference, "John 11:36");
        assert_eq!(rows[1].translation, "WEB");
        assert_eq!(rows[1].said, "The Jews therefore said, See how much affection he had for him!");
        let csv = crate::readings::to_csv(&rows);
        assert!(csv.lines().nth(2).unwrap().starts_with("s,s,John 11:36,43,11,36,,WEB,,"), "{csv}");
        assert!(csv.contains(r#""The Jews therefore said, See how much affection he had for him!""#));
    }
}
