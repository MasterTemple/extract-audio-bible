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
The project's sources get their word counts, durations, and match counts, and its selections
follow their readings to their new ids. `progress` is told each sermon as it's searched (from
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
    let searcher = matcher::searcher(engine, project.book).map_err(|e| Error::invalid(e.to_string()))?;
    let options = scripture_quotes::Options { min_accuracy: project.settings.min_accuracy, ..Default::default() };
    let total = project.sources.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let search = |source: &crate::project::Source| {
        let found = Transcript::open(files, &source.transcript_path).map(|t| {
            let (matches, runs) = matcher::find_matches(&searcher, &t, &source.id, &options);
            (t.words.len(), t.duration(), matches, runs)
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
    for (source, result) in project.sources.iter_mut().zip(results) {
        match result {
            Ok((words, duration, matches, runs)) => {
                source.word_count = words;
                source.match_count = matches.len();
                source.duration = source.duration.or(Some(duration));
                analysis.matches.matches.extend(matches);
                analysis.matches.runs.extend(runs);
            }
            Err(e) => analysis.errors.push((source.label.clone(), e)),
        }
    }
    // Selections follow their reading to its new id, if the analysis changed it
    let set = &analysis.matches;
    for sel in project.selections.values_mut() {
        if set.matches.iter().any(|m| m.id == sel.match_id) {
            continue;
        }
        let found = set.matches.iter().find(|m| {
            m.source_id == sel.source_id
                && m.start < sel.end
                && m.end > sel.start
                && sel.match_id.contains(&format!(":{}.{}.{}@", m.book, m.chapter, m.verse))
        });
        if let Some(m) = found {
            sel.match_id = m.id.clone();
        }
    }
    project.updated_at = now();
    Ok(analysis)
}
