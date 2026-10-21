//! The app's commands: the library (`extract-audio-bible`) does the work, and this keeps the
//! app's Bibles and projects, caches what's slow to make again, and reports progress.

mod error;
mod store;

use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use extract_audio_bible::{
    Bible, ChapterRef, ClipOptions, Disk, ExportedFile, MatchSet, Project, Transcript, Word, audio,
    bible::{BibleSummary, BookOutline},
    db, export, matcher, scripture_quotes,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State, ipc::Response};

use crate::{
    error::{AppError, Result, file_error},
    store::{ProjectSummary, Store},
};

/// The Bibles last searched for, ready to search again, by their ids joined with `+`
type CachedEngine = Arc<Mutex<Option<(String, Arc<scripture_quotes::Engine>)>>>;

/// Transcripts read, by path, kept to read again
type Transcripts = Arc<Mutex<HashMap<String, Arc<Transcript>>>>;

struct AppState {
    store: Store,
    transcripts: Transcripts,
    engine: CachedEngine,
    /// The newest search of transcripts by its key (`search_transcripts`), so older ones stop
    searches: Arc<Mutex<HashMap<String, u64>>>,
}

/// A transcript, read once and kept (call it off the main thread: the first read is slow)
fn transcript_of(cache: &Transcripts, path: &str) -> Result<Arc<Transcript>> {
    if let Some(t) = lock(cache).get(path) {
        return Ok(t.clone());
    }
    let t = Arc::new(Transcript::open(&Disk, path)?);
    lock(cache).insert(path.to_string(), t.clone());
    Ok(t)
}

/// A cache's lock, even if a thread panicked holding it: what it holds is whole either way
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs blocking work off the main thread
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::msg(e.to_string()))?
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

// ---------------------------------------------------------------- Bibles

#[tauri::command]
async fn list_bibles(state: State<'_, AppState>) -> Result<Vec<BibleSummary>> {
    let store = state.store.clone();
    blocking(move || store.bibles()).await
}

/// An imported Bible, and what was left out of it
#[derive(Serialize)]
struct ImportedBible {
    #[serde(flatten)]
    summary: BibleSummary,
    warnings: Vec<String>,
}

#[tauri::command]
async fn import_bible(
    state: State<'_, AppState>,
    paths: Vec<String>,
    name: String,
    abbreviation: String,
    language: Option<String>,
) -> Result<ImportedBible> {
    let store = state.store.clone();
    blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let name = if name.trim().is_empty() {
            "Untitled translation"
        } else {
            name.trim()
        };
        let mut imported = extract_audio_bible::import_paths(&paths)?;
        let warnings = std::mem::take(&mut imported.warnings);
        let mut bible = Bible::new(new_id(), name, abbreviation.trim(), imported);
        bible.language = language.filter(|l| !l.trim().is_empty());
        store.save_bible(&bible)?;
        Ok(ImportedBible {
            summary: bible.summary(),
            warnings,
        })
    })
    .await
}

#[tauri::command]
async fn delete_bible(state: State<'_, AppState>, id: String) -> Result<()> {
    let store = state.store.clone();
    let cached = state.engine.clone();
    blocking(move || {
        let mut engine = lock(&cached);
        if engine.as_ref().is_some_and(|(key, _)| key.split('+').any(|k| k == id)) {
            *engine = None;
        }
        drop(engine);
        store.delete_bible(&id)
    })
    .await
}

/// Renames a translation (its name, abbreviation, and language), wherever it's kept and shown
#[tauri::command]
async fn rename_bible(
    state: State<'_, AppState>,
    id: String,
    name: String,
    abbreviation: String,
    language: Option<String>,
) -> Result<BibleSummary> {
    let store = state.store.clone();
    blocking(move || store.rename_bible(&id, &name, &abbreviation, language)).await
}

#[tauri::command]
async fn bible_outline(state: State<'_, AppState>, id: String) -> Result<Vec<BookOutline>> {
    let store = state.store.clone();
    blocking(move || Ok(store.bible(&id)?.outline())).await
}

#[tauri::command]
async fn book_text(state: State<'_, AppState>, bible_id: String, book: u8) -> Result<Vec<Vec<String>>> {
    let store = state.store.clone();
    blocking(move || {
        let bible = store.bible(&bible_id)?;
        Ok(bible.book(book).map(|b| b.chapters.clone()).unwrap_or_default())
    })
    .await
}

// ---------------------------------------------------------------- Projects

#[tauri::command]
async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>> {
    let store = state.store.clone();
    blocking(move || store.projects()).await
}

/// An audio Bible of a book, some of its verses (`passage`, like "2:14-17"), or the whole Bible
#[tauri::command]
async fn create_project(
    state: State<'_, AppState>,
    name: String,
    bible_id: String,
    book: Option<u8>,
    passage: Option<String>,
) -> Result<Project> {
    let store = state.store.clone();
    blocking(move || {
        let bible = store.bible(&bible_id)?;
        let project = audio_bible(new_id(), &name, &bible, book, passage.as_deref())?;
        store.save_project(&project)?;
        Ok(project)
    })
    .await
}

/// An audio Bible of a book, some of its verses (`passage`, like "2:14-17", if given), or the whole Bible
fn audio_bible(id: String, name: &str, bible: &Bible, book: Option<u8>, passage: Option<&str>) -> Result<Project> {
    let passage = passage.map(str::trim).filter(|p| !p.is_empty());
    match (book, passage) {
        (Some(book), Some(passage)) => {
            let of = bible.book(book).ok_or_else(|| AppError::msg("No such book"))?;
            let passage = extract_audio_bible::Passage::parse(passage, of)?;
            Ok(Project::passage(id, name, bible, book, passage))
        }
        (None, Some(_)) => Err(AppError::msg("Choose the book the verses are of")),
        (book, None) => Ok(Project::new(id, name, bible, book)),
    }
}

/// A project finding every verse read in its sermons, in any of some translations, in some books
#[tauri::command]
async fn create_readings_project(
    state: State<'_, AppState>,
    name: String,
    bible_ids: Vec<String>,
    scope: extract_audio_bible::BookScope,
) -> Result<Project> {
    let store = state.store.clone();
    blocking(move || {
        scope.books()?;
        let bibles = bible_ids.iter().map(|id| store.bible(id)).collect::<Result<Vec<_>>>()?;
        let project = Project::readings(new_id(), &name, &bibles.iter().collect::<Vec<_>>(), scope)?;
        store.save_project(&project)?;
        Ok(project)
    })
    .await
}

/**
An audio Bible of one translation and book (some of its verses, or the whole Bible) from some of another project's
sermons (those a filter of its readings kept, say): their transcripts and audio, what's known of
them, and where audio is downloaded from, ready to search
*/
#[tauri::command]
async fn create_project_from(
    state: State<'_, AppState>,
    project_id: String,
    source_ids: Vec<String>,
    name: String,
    bible_id: String,
    book: Option<u8>,
    passage: Option<String>,
) -> Result<Project> {
    let store = state.store.clone();
    blocking(move || {
        let bible = store.bible(&bible_id)?;
        let project = audio_bible(new_id(), &name, &bible, book, passage.as_deref())?;
        store.project_from(&project_id, &source_ids, project)
    })
    .await
}

/// topos's genres (groups of books), in order
#[tauri::command]
async fn genres() -> Result<Vec<extract_audio_bible::Genre>> {
    blocking(|| Ok(extract_audio_bible::BookScope::genres())).await
}

/// The books a scope searches, by number, or none for every book (an error if it names a genre
/// or book topos doesn't know, or leaves nothing)
#[tauri::command]
async fn scope_books(scope: extract_audio_bible::BookScope) -> Result<Option<Vec<u8>>> {
    blocking(move || Ok(scope.books()?.map(|books| books.into_iter().collect()))).await
}

// ---------------------------------------------------------------- Readings, read as they're shown

/**
How many readings a project has, and of which verses. Readies its readings database first: made
from the readings it kept as JSON before (which can take a while, once), and its list of sermons
kept as the project has them
*/
#[tauri::command]
async fn match_summary(state: State<'_, AppState>, project_id: String) -> Result<db::Summary> {
    let store = state.store.clone();
    blocking(move || {
        let project = store.project(&project_id)?;
        if !store.prepare_readings(&project)? {
            return Ok(db::Summary::default());
        }
        let readings = store
            .readings(&project_id)?
            .ok_or_else(|| AppError::msg("No readings"))?;
        Ok(readings.summary()?)
    })
    .await
}

/// A project's readings database, if it's been searched
fn readings_of(store: &Store, project_id: &str) -> Result<Option<db::Readings>> {
    store.readings(project_id)
}

/// A chapter's readings, best first within each verse, with the runs they're in and those runs'
/// other readings
#[tauri::command]
async fn chapter_matches(state: State<'_, AppState>, project_id: String, book: u8, chapter: u16) -> Result<MatchSet> {
    let store = state.store.clone();
    blocking(move || match readings_of(&store, &project_id)? {
        Some(readings) => Ok(readings.chapter(book, chapter)?),
        None => Ok(MatchSet::default()),
    })
    .await
}

/// A sermon's readings as said, with its runs
#[tauri::command]
async fn sermon_matches(state: State<'_, AppState>, project_id: String, source_id: String) -> Result<MatchSet> {
    let store = state.store.clone();
    blocking(move || match readings_of(&store, &project_id)? {
        Some(readings) => Ok(readings.sermon(&source_id)?),
        None => Ok(MatchSet::default()),
    })
    .await
}

/// The readings a filter keeps, counted and grouped by sermon and by book and chapter
#[tauri::command]
async fn readings_overview(
    state: State<'_, AppState>,
    project_id: String,
    filter: db::RowFilter,
) -> Result<db::Overview> {
    let store = state.store.clone();
    blocking(move || match readings_of(&store, &project_id)? {
        Some(readings) => Ok(readings.overview(&filter)?),
        None => Ok(db::Overview::default()),
    })
    .await
}

/// The readings a filter keeps of one group (a sermon, a chapter), as rows
#[tauri::command]
async fn readings_rows(
    state: State<'_, AppState>,
    project_id: String,
    filter: db::RowFilter,
    group: db::Group,
) -> Result<Vec<extract_audio_bible::ReadingRow>> {
    let store = state.store.clone();
    blocking(move || match readings_of(&store, &project_id)? {
        Some(readings) => Ok(readings.rows(&filter, &group)?),
        None => Ok(vec![]),
    })
    .await
}

/// Every reading a filter keeps written to `path`: JSON for a `.json` path, else CSV (written as
/// it's read). Returns how many
#[tauri::command]
async fn export_readings(
    state: State<'_, AppState>,
    project_id: String,
    filter: db::RowFilter,
    path: String,
) -> Result<usize> {
    use std::io::Write;
    let store = state.store.clone();
    blocking(move || {
        let readings = readings_of(&store, &project_id)?.ok_or_else(|| AppError::msg("Search the sermons first"))?;
        let file = std::fs::File::create(&path).map_err(|e| file_error(&path, e))?;
        let mut out = std::io::BufWriter::new(file);
        let mut count = 0;
        if path.to_ascii_lowercase().ends_with(".json") {
            let rows = readings.rows(&filter, &db::Group::All)?;
            count = rows.len();
            serde_json::to_writer_pretty(&mut out, &rows)?;
        } else {
            let mut failed = None;
            out.write_all(extract_audio_bible::readings::CSV_HEADER.as_bytes())?;
            readings.rows_each(&filter, &db::Group::All, |row| {
                if failed.is_none()
                    && let Err(e) = out.write_all(extract_audio_bible::readings::csv_line(&row).as_bytes())
                {
                    failed = Some(e);
                }
                count += 1;
            })?;
            if let Some(e) = failed {
                return Err(file_error(&path, e));
            }
        }
        out.flush().map_err(|e| file_error(&path, e))?;
        Ok(count)
    })
    .await
}

#[tauri::command]
async fn get_project(state: State<'_, AppState>, id: String) -> Result<Project> {
    let store = state.store.clone();
    blocking(move || store.project(&id)).await
}

/// Saves what's changed in a project since it was last saved (its parts changed, and its chosen
/// verses and sermon information entry by entry); returns when it was changed
#[tauri::command]
async fn save_changes(
    state: State<'_, AppState>,
    project_id: String,
    changes: extract_audio_bible::ProjectChanges,
) -> Result<u64> {
    let store = state.store.clone();
    blocking(move || store.save_changes(&project_id, changes)).await
}

#[tauri::command]
async fn delete_project(state: State<'_, AppState>, id: String) -> Result<()> {
    let store = state.store.clone();
    blocking(move || store.delete_project(&id)).await
}

/// Opens a project kept in a folder of its own (as the command line makes them), where it is
#[tauri::command]
async fn open_project_folder(state: State<'_, AppState>, path: String) -> Result<Project> {
    let store = state.store.clone();
    blocking(move || store.link_project(Path::new(&path))).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AddedSources {
    project: Project,
    added: usize,
    /// Audio files that had no transcript
    unpaired: Vec<String>,
    /// JSON files that aren't transcripts
    skipped: Vec<String>,
}

/// Add sermons from files or folders, pairing each audio file with the transcript of the same
/// name (in the same folder first, then anywhere among what was given); a transcript alone is a
/// sermon too, and JSON that isn't a transcript is skipped
#[tauri::command]
async fn add_sources(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    paths: Vec<String>,
) -> Result<AddedSources> {
    let store = state.store.clone();
    blocking(move || {
        let mut project = store.project(&project_id)?;
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let files: Vec<String> = extract_audio_bible::files::gather(&paths)?
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        // How many transcripts have been read, a few times a second at most
        let last = Mutex::new(None::<std::time::Instant>);
        let progress = |done: usize, total: usize| {
            let mut last = lock(&last);
            if done < total && last.is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(100)) {
                return;
            }
            *last = Some(std::time::Instant::now());
            let message = "Reading transcripts".to_string();
            let _ = app.emit("add-progress", AnalyzeProgress { done, total, message });
        };
        let added = project.add_sources_with(&Disk, &files, &progress);
        store.save_project(&project)?;
        Ok(AddedSources {
            project,
            added: added.added,
            unpaired: added.unpaired,
            skipped: added.skipped,
        })
    })
    .await
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeProgress {
    done: usize,
    total: usize,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Analyzed {
    project: Project,
    /// What was found (each chapter's and sermon's readings are read as they're shown)
    summary: db::Summary,
    errors: Vec<String>,
}

/// Find every reading of every verse in every sermon
#[tauri::command]
async fn analyze(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String) -> Result<Analyzed> {
    let store = state.store.clone();
    let cached = state.engine.clone();
    blocking(move || {
        let mut project = store.project(&project_id)?;
        let total = project.sources.len();
        // A few times a second at most: thousands of sermons are searched in seconds
        let last = Mutex::new(None::<std::time::Instant>);
        let emit = |done: usize, message: String| {
            let mut last = lock(&last);
            if done < total && last.is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(100)) {
                return;
            }
            *last = Some(std::time::Instant::now());
            let _ = app.emit("analyze-progress", AnalyzeProgress { done, total, message });
        };
        emit(0, "Reading the Bible text".into());
        // Kept for the next search of the same translations
        let ids = project.bibles_searched();
        let key = ids.join("+");
        let engine = match &*lock(&cached) {
            Some((k, engine)) if *k == key => Some(engine.clone()),
            _ => None,
        };
        let engine = match engine {
            Some(engine) => engine,
            None => {
                let bibles = ids.iter().map(|id| store.bible(id)).collect::<Result<Vec<_>>>()?;
                let engine = Arc::new(matcher::engine_of(&bibles)?);
                *lock(&cached) = Some((key, engine.clone()));
                engine
            }
        };
        let analysis = extract_audio_bible::analyze_with(&mut project, &engine, &Disk, &|done, _, label| {
            emit(done, format!("Searched {label}"))
        })?;
        let errors = analysis
            .errors
            .iter()
            .map(|(label, e)| format!("{label}: {e}"))
            .collect();
        emit(total, "Saving what was found".into());
        store.save_project(&project)?;
        store.save_matches(&project, &analysis.matches)?;
        let summary = (store.readings(&project.id)?)
            .map(|r| r.summary())
            .transpose()?
            .unwrap_or_default();
        emit(total, "Done".into());
        Ok(Analyzed {
            project,
            summary,
            errors,
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WordWindow {
    /// The index of the first word given
    from: u32,
    words: Vec<Word>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FullTranscript {
    id: Option<String>,
    title: Option<String>,
    words: Vec<Word>,
    /// Where each paragraph begins, by word
    paragraphs: Vec<usize>,
}

/// A whole transcript, in paragraphs
#[tauri::command]
async fn transcript(state: State<'_, AppState>, transcript_path: String) -> Result<FullTranscript> {
    let cache = state.transcripts.clone();
    blocking(move || {
        let t = transcript_of(&cache, &transcript_path)?;
        Ok(FullTranscript {
            id: t.id.clone(),
            title: t.title.clone(),
            words: t.words.clone(),
            paragraphs: t.paragraphs.clone(),
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaidIn {
    found: Vec<extract_audio_bible::Said>,
    /// Whether there were more than the limit
    more: bool,
    /// Sermons whose transcripts couldn't be read, with why
    errors: Vec<String>,
}

/// What a search has found since it last said, as it goes
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchProgress {
    /// Places found since, in the sermons' order (after those given before)
    found: Vec<extract_audio_bible::Said>,
    /// Sermons whose transcripts couldn't be read since, with why
    errors: Vec<String>,
    /// How many sermons have been searched, of how many
    searched: usize,
    total: usize,
    /// Whether it's over: every sermon searched, the limit reached, or a newer search begun
    done: bool,
    /// Whether there were more than the limit
    more: bool,
}

/// Sermons searched at once, in their order, before what they have is given
const SEARCH_BATCH: usize = 256;

/**
Where a phrase was said in the project's sermons (or some of them, by id), the first `limit` places
in the sermons' order. Sermons are searched a batch at a time, in their order (each batch several at
once), and what each batch finds is given to `on_progress` as it's found; the search stops once it
has `limit` places, or when another search with the same `search` key begins (the Search tab's and a
transcript's don't stop each other). The transcripts not already held are read for each search and
not kept: every sermon's, held, could take hundreds of megabytes, but reading them is quick
*/
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn search_transcripts(
    state: State<'_, AppState>,
    project_id: String,
    phrase: String,
    prefix: bool,
    source_ids: Option<Vec<String>>,
    limit: usize,
    search: String,
    on_progress: tauri::ipc::Channel<SearchProgress>,
) -> Result<SaidIn> {
    use rayon::prelude::*;
    let held = lock(&state.transcripts).clone();
    let store = state.store.clone();
    let searches = state.searches.clone();
    // This search, the newest with its key: an older one stops when it sees it's been passed
    let this = {
        let mut searches = lock(&searches);
        let n = searches.entry(search.clone()).or_insert(0);
        *n += 1;
        *n
    };
    blocking(move || {
        let project = store.project_ref(&project_id)?;
        let options = extract_audio_bible::FindOptions {
            prefix,
            context: 14,
            limit: limit + 1,
        };
        let sources: Vec<_> = project
            .sources
            .iter()
            .filter(|s| source_ids.as_ref().is_none_or(|ids| ids.contains(&s.id)))
            .collect();
        let total = sources.len();
        let mut out = SaidIn {
            found: vec![],
            more: false,
            errors: vec![],
        };
        let mut searched = 0;
        for batch in sources.chunks(SEARCH_BATCH) {
            if lock(&searches).get(&search) != Some(&this) {
                break;
            }
            let results: Vec<_> = (batch.par_iter())
                .map(|source| {
                    let t = match held.get(&source.transcript_path) {
                        Some(t) => t.clone(),
                        None => Arc::new(Transcript::open(&Disk, &source.transcript_path)?),
                    };
                    Ok::<_, extract_audio_bible::Error>(extract_audio_bible::find_in(&t, &source.id, &phrase, &options))
                })
                .collect();
            searched += batch.len();
            let mut found = vec![];
            let mut errors = vec![];
            for (source, result) in batch.iter().zip(results) {
                match result {
                    Ok(places) => found.extend(places),
                    Err(e) => errors.push(format!("{}: {e}", project.source_title(source))),
                }
            }
            // The first `limit` places, in the sermons' order
            let room = limit.saturating_sub(out.found.len());
            if found.len() > room {
                found.truncate(room);
                out.more = true;
            }
            out.found.extend(found.iter().cloned());
            out.errors.extend(errors.iter().cloned());
            let done = out.more || searched == total;
            let _ = on_progress.send(SearchProgress {
                found,
                errors,
                searched,
                total,
                done,
                more: out.more,
            });
            if done {
                return Ok(out);
            }
        }
        // Passed by a newer search
        let _ = on_progress.send(SearchProgress {
            found: vec![],
            errors: vec![],
            searched,
            total,
            done: true,
            more: out.more,
        });
        Ok(out)
    })
    .await
}

/// Words `from` to `to` (inclusive) of a transcript
#[tauri::command]
async fn transcript_words(
    state: State<'_, AppState>,
    transcript_path: String,
    from: i64,
    to: i64,
) -> Result<WordWindow> {
    let cache = state.transcripts.clone();
    blocking(move || {
        let t = transcript_of(&cache, &transcript_path)?;
        let from = from.max(0) as usize;
        let to = (to.max(0) as usize).min(t.words.len().saturating_sub(1));
        let words = if from <= to {
            t.words[from..=to].to_vec()
        } else {
            vec![]
        };
        Ok(WordWindow {
            from: from as u32,
            words,
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Peaks {
    start: f64,
    per_second: u32,
    peaks: Vec<f32>,
    duration: f64,
}

/// The waveform from `start` to `end` seconds
#[tauri::command]
async fn audio_peaks(state: State<'_, AppState>, audio_path: String, start: f64, end: f64) -> Result<Peaks> {
    let cache = state.store.cache_dir().join("peaks");
    let files = state.store.audio_files();
    blocking(move || {
        // A stream's waveform is of the stretch shown alone (all of it would be every segment)
        if extract_audio_bible::hls::is_playlist(&audio_path) {
            let start = start.max(0.0);
            let duration = extract_audio_bible::hls::read(&files, &audio_path)?.duration();
            let pcm = audio::decode_range(&files, &audio_path, start, end.min(duration))?;
            return Ok(Peaks {
                start,
                per_second: audio::PEAKS_PER_SECOND,
                peaks: audio::peaks_of(&pcm),
                duration,
            });
        }
        let (all, duration) = cached_peaks(&files.local(&audio_path)?, &cache)?;
        let per = audio::PEAKS_PER_SECOND as f64;
        let s = ((start.max(0.0)) * per) as usize;
        let e = ((end * per).ceil() as usize).min(all.len());
        Ok(Peaks {
            start: s as f64 / per,
            per_second: audio::PEAKS_PER_SECOND,
            peaks: if s < e { all[s..e].to_vec() } else { vec![] },
            duration,
        })
    })
    .await
}

/// A whole file's waveform, kept in `cache_dir` by the file's path, size, and time
fn cached_peaks(path: &str, cache_dir: &Path) -> Result<(Vec<f32>, f64)> {
    let meta = std::fs::metadata(path)?;
    let mut h = DefaultHasher::new();
    (path, meta.len(), meta.modified().ok()).hash(&mut h);
    let cache = cache_dir.join(format!("{:016x}.peaks", h.finish()));
    if let Ok(bytes) = std::fs::read(&cache)
        && bytes.len() >= 8
    {
        let duration = f64::from_le_bytes(bytes[..8].try_into().unwrap_or_default());
        let peaks = bytes[8..]
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        return Ok((peaks, duration));
    }
    let (peaks, duration) = audio::peaks(&Disk, path)?;
    std::fs::create_dir_all(cache_dir)?;
    let mut bytes = duration.to_le_bytes().to_vec();
    bytes.extend(peaks.iter().flat_map(|p| p.to_le_bytes()));
    std::fs::write(&cache, bytes)?;
    Ok((peaks, duration))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipRequest {
    audio_path: String,
    start: f64,
    end: f64,
    #[serde(flatten)]
    options: ClipOptions,
}

/// A stretch of a sermon as WAV, for listening
#[tauri::command]
async fn render_clip(state: State<'_, AppState>, request: ClipRequest) -> Result<Response> {
    let files = state.store.audio_files();
    blocking(move || {
        let r = request;
        let pcm = extract_audio_bible::render_clip(&files, &r.audio_path, r.start, r.end, &r.options)?;
        Ok(Response::new(audio::wav(&pcm)))
    })
    .await
}

/// When each chosen verse is heard in a chapter's audio (as `render_chapter` makes it)
#[tauri::command]
async fn chapter_timeline(
    state: State<'_, AppState>,
    project_id: String,
    book: u8,
    chapter: u16,
) -> Result<Vec<export::VerseTime>> {
    let store = state.store.clone();
    blocking(move || {
        let project = store.project_ref(&project_id)?;
        let bible = store.bible(&project.bible_id)?;
        let book = bible.book(book).ok_or_else(|| AppError::msg("No such book"))?;
        Ok(export::chapter_timeline(&project, book, chapter))
    })
    .await
}

/// A chapter as it will be exported, as WAV
#[tauri::command]
async fn render_chapter(state: State<'_, AppState>, project_id: String, book: u8, chapter: u16) -> Result<Response> {
    let store = state.store.clone();
    blocking(move || {
        let project = store.project_ref(&project_id)?;
        let bible = store.bible(&project.bible_id)?;
        let book = bible.book(book).ok_or_else(|| AppError::msg("No such book"))?;
        let pcm = export::render_chapter(&project, book, chapter, 24_000, &store.audio_files())?;
        Ok(Response::new(audio::wav(&pcm)))
    })
    .await
}

/// Titles and passages by file id, from a JSON file (`{"<file id>": {"title": …, "passage": …}}`)
#[tauri::command]
async fn read_file_metadata(
    path: String,
) -> Result<std::collections::BTreeMap<String, extract_audio_bible::FileMetadata>> {
    blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| file_error(&path, e))?;
        serde_json::from_slice(&bytes).map_err(|e| {
            AppError::msg(format!(
                "{path}: expected {{\"file id\": {{\"title\": …, \"passage\": …}}}}: {e}"
            ))
        })
    })
    .await
}

/// A verse as it will be exported (all its readings, one after another), as WAV
#[tauri::command]
async fn render_verse(
    state: State<'_, AppState>,
    project_id: String,
    book: u8,
    chapter: u16,
    verse: u16,
) -> Result<Response> {
    let store = state.store.clone();
    let files = store.audio_files();
    blocking(move || {
        let project = store.project_ref(&project_id)?;
        let key = export::verse_key(book, chapter, verse);
        let pcm = extract_audio_bible::render_verse(&project, &key, 44_100, &files)?
            .ok_or_else(|| AppError::msg("No reading is chosen for this verse"))?;
        Ok(Response::new(audio::wav(&pcm)))
    })
    .await
}

#[tauri::command]
async fn chapters_to_export(state: State<'_, AppState>, project_id: String) -> Result<Vec<ChapterRef>> {
    let store = state.store.clone();
    blocking(move || {
        let project = store.project_ref(&project_id)?;
        Ok(export::chapters_to_export(&project, &store.bible(&project.bible_id)?))
    })
    .await
}

#[tauri::command]
async fn export_chapters(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    chapters: Option<Vec<ChapterRef>>,
) -> Result<Vec<ExportedFile>> {
    let store = state.store.clone();
    blocking(move || {
        let project = store.project_ref(&project_id)?;
        let bible = store.bible(&project.bible_id)?;
        let chapters = chapters.unwrap_or_else(|| export::chapters_to_export(&project, &bible));
        if chapters.is_empty() {
            return Err(AppError::msg("No chapters have chosen verses yet"));
        }
        let dir = (project.metadata.output_dir.as_deref())
            .filter(|d| !d.is_empty())
            .ok_or_else(|| AppError::msg("Choose a folder to export to"))?;
        Ok(export::export_to_dir(
            &project,
            &bible,
            &chapters,
            &store.audio_files(),
            Path::new(dir),
            |p| {
                let _ = app.emit("export-progress", p);
            },
        )?)
    })
    .await
}

// ---------------------------------------------------------------- Benchmarks

/// Project folders to time the interface with, from `EAB_BENCH` (separated by `:`): with any, the
/// app runs its benchmark (`src/lib/bench.ts`) on them, prints what it timed, and quits
#[tauri::command]
async fn bench_folders() -> Result<Vec<String>> {
    Ok(std::env::var("EAB_BENCH")
        .map(|v| v.split(':').filter(|s| !s.is_empty()).map(String::from).collect())
        .unwrap_or_default())
}

/// Only the benchmark's steps whose names have these words (`EAB_BENCH_STEPS`, separated by `,`),
/// besides opening each project; all of them if it isn't set
#[tauri::command]
async fn bench_steps() -> Result<Vec<String>> {
    Ok(std::env::var("EAB_BENCH_STEPS")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

/// A line of the benchmark's progress, printed as it goes
#[tauri::command]
async fn bench_log(line: String) -> Result<()> {
    use std::io::Write;
    eprintln!("{line}");
    let _ = std::io::stderr().flush();
    Ok(())
}

/// The benchmark's report, printed, and the app closed
#[tauri::command]
async fn bench_done(app: tauri::AppHandle, report: String) -> Result<()> {
    println!("{report}");
    app.exit(0);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            std::fs::create_dir_all(&root)?;
            app.manage(AppState {
                store: Store::new(root),
                transcripts: Arc::default(),
                searches: Arc::default(),
                engine: Arc::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_bibles,
            import_bible,
            delete_bible,
            rename_bible,
            bible_outline,
            book_text,
            list_projects,
            create_project,
            get_project,
            save_changes,
            delete_project,
            open_project_folder,
            add_sources,
            analyze,
            match_summary,
            chapter_matches,
            sermon_matches,
            readings_overview,
            readings_rows,
            export_readings,
            transcript_words,
            transcript,
            create_readings_project,
            create_project_from,
            genres,
            scope_books,
            search_transcripts,
            audio_peaks,
            render_clip,
            render_chapter,
            render_verse,
            read_file_metadata,
            chapter_timeline,
            chapters_to_export,
            export_chapters,
            bench_folders,
            bench_steps,
            bench_log,
            bench_done,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
