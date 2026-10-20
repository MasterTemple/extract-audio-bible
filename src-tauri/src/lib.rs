//! The app's commands: the library (`extract-audio-bible`) does the work, and this keeps the
//! app's Bibles and projects, caches what's slow to make again, and reports progress.

mod error;
mod store;

use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use extract_audio_bible::{
    Bible, ChapterRef, ClipOptions, Disk, ExportedFile, MatchSet, Project, Transcript, Word, audio,
    bible::{BibleSummary, BookOutline},
    export, matcher,
    project::now,
    scripture_quotes,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State, ipc::Response};

use crate::{
    error::{AppError, Result},
    store::{ProjectSummary, Store},
};

/// The Bible last searched for, ready to search again, by its id
type CachedEngine = Arc<Mutex<Option<(String, Arc<scripture_quotes::Engine>)>>>;

struct AppState {
    store: Store,
    transcripts: Mutex<HashMap<String, Arc<Transcript>>>,
    engine: CachedEngine,
}

impl AppState {
    fn transcript(&self, path: &str) -> Result<Arc<Transcript>> {
        if let Some(t) = self.transcripts.lock().unwrap().get(path) {
            return Ok(t.clone());
        }
        let t = Arc::new(Transcript::open(&Disk, path)?);
        self.transcripts.lock().unwrap().insert(path.to_string(), t.clone());
        Ok(t)
    }
}

/// Runs blocking work off the main thread
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::msg(e.to_string()))?
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

// ---------------------------------------------------------------- Bibles

#[tauri::command]
fn list_bibles(state: State<AppState>) -> Result<Vec<BibleSummary>> {
    state.store.bibles()
}

#[tauri::command]
async fn import_bible(
    state: State<'_, AppState>,
    paths: Vec<String>,
    name: String,
    abbreviation: String,
    language: Option<String>,
) -> Result<BibleSummary> {
    let store = state.store.clone();
    blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let name = if name.trim().is_empty() { "Untitled translation" } else { name.trim() };
        let mut bible = Bible::new(new_id(), name, abbreviation.trim(), extract_audio_bible::import_paths(&paths)?);
        bible.language = language.filter(|l| !l.trim().is_empty());
        store.save_bible(&bible)?;
        Ok(bible.summary())
    })
    .await
}

#[tauri::command]
fn delete_bible(state: State<AppState>, id: String) -> Result<()> {
    state.store.delete_bible(&id)
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
fn list_projects(state: State<AppState>) -> Result<Vec<ProjectSummary>> {
    state.store.projects()
}

#[tauri::command]
fn create_project(state: State<AppState>, name: String, bible_id: String, book: Option<u8>) -> Result<Project> {
    let bible = state.store.bible(&bible_id)?;
    let project = Project::new(new_id(), &name, &bible, book);
    state.store.save_project(&project)?;
    Ok(project)
}

/// A project finding every verse read in its sermons, in any of some translations, in some books
#[tauri::command]
fn create_readings_project(
    state: State<AppState>,
    name: String,
    bible_ids: Vec<String>,
    scope: extract_audio_bible::BookScope,
) -> Result<Project> {
    scope.books()?;
    let bibles = bible_ids.iter().map(|id| state.store.bible(id)).collect::<Result<Vec<_>>>()?;
    let project = Project::readings(new_id(), &name, &bibles.iter().collect::<Vec<_>>(), scope)?;
    state.store.save_project(&project)?;
    Ok(project)
}

/// topos's genres (groups of books), in order
#[tauri::command]
fn genres() -> Vec<extract_audio_bible::Genre> {
    extract_audio_bible::BookScope::genres()
}

/// The books a scope searches, by number, or none for every book (an error if it names a genre
/// or book topos doesn't know, or leaves nothing)
#[tauri::command]
fn scope_books(scope: extract_audio_bible::BookScope) -> Result<Option<Vec<u8>>> {
    Ok(scope.books()?.map(|books| books.into_iter().collect()))
}

/// Every reading found, as rows, and written to `path` as CSV or JSON if given
#[tauri::command]
async fn reading_rows(
    state: State<'_, AppState>,
    project: Project,
    path: Option<String>,
) -> Result<Vec<extract_audio_bible::ReadingRow>> {
    let store = state.store.clone();
    blocking(move || {
        let matches = store.matches(&project.id)?;
        let bibles = project.bibles_searched().iter().map(|id| store.bible(id)).collect::<Result<Vec<_>>>()?;
        let rows = extract_audio_bible::reading_rows(&project, &matches, &bibles.iter().collect::<Vec<_>>(), &Disk);
        if let Some(path) = path {
            let text = if path.to_ascii_lowercase().ends_with(".json") {
                serde_json::to_string_pretty(&rows)?
            } else {
                extract_audio_bible::readings::to_csv(&rows)
            };
            std::fs::write(&path, text).map_err(|e| AppError::msg(format!("{path}: {e}")))?;
        }
        Ok(rows)
    })
    .await
}

#[tauri::command]
fn get_project(state: State<AppState>, id: String) -> Result<Project> {
    state.store.project(&id)
}

#[tauri::command]
fn save_project(state: State<AppState>, mut project: Project) -> Result<u64> {
    project.updated_at = now();
    state.store.save_project(&project)?;
    Ok(project.updated_at)
}

#[tauri::command]
fn delete_project(state: State<AppState>, id: String) -> Result<()> {
    state.store.delete_project(&id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AddedSources {
    project: Project,
    added: usize,
    /// Files that had no partner (audio without a transcript, or the reverse)
    unpaired: Vec<String>,
}

/// Add sermons from files or folders, pairing each audio file with the WhisperX JSON of the
/// same name (in the same folder first, then anywhere among what was given)
#[tauri::command]
fn add_sources(state: State<AppState>, project_id: String, paths: Vec<String>) -> Result<AddedSources> {
    let mut project = state.store.project(&project_id)?;
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let files: Vec<String> =
        extract_audio_bible::files::gather(&paths)?.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let added = project.add_sources(&files);
    state.store.save_project(&project)?;
    Ok(AddedSources { project, added: added.added, unpaired: added.unpaired })
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
    matches: MatchSet,
    errors: Vec<String>,
}

/// Find every reading of every verse in every sermon
#[tauri::command]
async fn analyze(app: tauri::AppHandle, state: State<'_, AppState>, project: Project) -> Result<Analyzed> {
    let store = state.store.clone();
    let cached = state.engine.clone();
    blocking(move || {
        let mut project = project;
        let total = project.sources.len();
        let emit = |done: usize, message: String| {
            let _ = app.emit("analyze-progress", AnalyzeProgress { done, total, message });
        };
        emit(0, "Reading the Bible text".into());
        // Kept for the next search of the same translations
        let ids = project.bibles_searched();
        let key = ids.join("+");
        let engine = match &*cached.lock().unwrap() {
            Some((k, engine)) if *k == key => Some(engine.clone()),
            _ => None,
        };
        let engine = match engine {
            Some(engine) => engine,
            None => {
                let bibles = ids.iter().map(|id| store.bible(id)).collect::<Result<Vec<_>>>()?;
                let engine = Arc::new(matcher::engine_of(&bibles));
                *cached.lock().unwrap() = Some((key, engine.clone()));
                engine
            }
        };
        let analysis = extract_audio_bible::analyze_with(&mut project, &engine, &Disk, &|done, _, label| {
            emit(done, format!("Searched {label}"))
        })?;
        let errors = analysis.errors.iter().map(|(label, e)| format!("{label}: {e}")).collect();
        store.save_matches(&project.id, &analysis.matches)?;
        store.save_project(&project)?;
        emit(total, "Done".into());
        Ok(Analyzed { project, matches: analysis.matches, errors })
    })
    .await
}

#[tauri::command]
async fn get_matches(state: State<'_, AppState>, project_id: String) -> Result<MatchSet> {
    let store = state.store.clone();
    blocking(move || store.matches(&project_id)).await
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
    let t = state.transcript(&transcript_path)?;
    Ok(FullTranscript { id: t.id.clone(), title: t.title.clone(), words: t.words.clone(), paragraphs: t.paragraphs.clone() })
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

/**
Where a phrase was said in the project's sermons (or some of them, by id), up to `limit` places. The
transcripts not already held are read for each search (all at once) and not kept: every sermon's,
held, could take hundreds of megabytes, but reading them is quick
*/
#[tauri::command]
async fn search_transcripts(
    state: State<'_, AppState>,
    project: Project,
    phrase: String,
    prefix: bool,
    source_ids: Option<Vec<String>>,
    limit: usize,
) -> Result<SaidIn> {
    use rayon::prelude::*;
    let held = state.transcripts.lock().unwrap().clone();
    blocking(move || {
        let options = extract_audio_bible::FindOptions { prefix, context: 14, limit: limit + 1 };
        let sources: Vec<_> =
            project.sources.iter().filter(|s| source_ids.as_ref().is_none_or(|ids| ids.contains(&s.id))).collect();
        let results: Vec<_> = (sources.par_iter())
            .map(|source| {
                let t = match held.get(&source.transcript_path) {
                    Some(t) => t.clone(),
                    None => Arc::new(Transcript::open(&Disk, &source.transcript_path)?),
                };
                Ok::<_, extract_audio_bible::Error>(extract_audio_bible::find_in(&t, &source.id, &phrase, &options))
            })
            .collect();
        let mut out = SaidIn { found: vec![], more: false, errors: vec![] };
        for (source, result) in sources.iter().zip(results) {
            match result {
                Ok(found) => out.found.extend(found),
                Err(e) => out.errors.push(format!("{}: {e}", project.source_title(source))),
            }
        }
        out.more = out.found.len() > limit;
        out.found.truncate(limit);
        Ok(out)
    })
    .await
}

/// Words `from` to `to` (inclusive) of a transcript
#[tauri::command]
async fn transcript_words(state: State<'_, AppState>, transcript_path: String, from: i64, to: i64) -> Result<WordWindow> {
    let t = state.transcript(&transcript_path)?;
    let from = from.max(0) as usize;
    let to = (to.max(0) as usize).min(t.words.len().saturating_sub(1));
    let words = if from <= to { t.words[from..=to].to_vec() } else { vec![] };
    Ok(WordWindow { from: from as u32, words })
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
        let peaks = bytes[8..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
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
    project: Project,
    book: u8,
    chapter: u16,
) -> Result<Vec<export::VerseTime>> {
    let store = state.store.clone();
    blocking(move || {
        let bible = store.bible(&project.bible_id)?;
        let book = bible.book(book).ok_or_else(|| AppError::msg("No such book"))?;
        Ok(export::chapter_timeline(&project, book, chapter))
    })
    .await
}

/// A chapter as it will be exported, as WAV
#[tauri::command]
async fn render_chapter(state: State<'_, AppState>, project: Project, book: u8, chapter: u16) -> Result<Response> {
    let store = state.store.clone();
    blocking(move || {
        let bible = store.bible(&project.bible_id)?;
        let book = bible.book(book).ok_or_else(|| AppError::msg("No such book"))?;
        let pcm = export::render_chapter(&project, book, chapter, 24_000, &store.audio_files())?;
        Ok(Response::new(audio::wav(&pcm)))
    })
    .await
}

/// Titles and passages by file id, from a JSON file (`{"<file id>": {"title": …, "passage": …}}`)
#[tauri::command]
fn read_file_metadata(path: String) -> Result<std::collections::BTreeMap<String, extract_audio_bible::FileMetadata>> {
    let bytes = std::fs::read(&path).map_err(|e| AppError::msg(format!("{path}: {e}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| AppError::msg(format!("{path}: expected {{\"file id\": {{\"title\": …, \"passage\": …}}}}: {e}")))
}

/// A verse as it will be exported (all its readings, one after another), as WAV
#[tauri::command]
async fn render_verse(
    state: State<'_, AppState>,
    project: Project,
    book: u8,
    chapter: u16,
    verse: u16,
) -> Result<Response> {
    let files = state.store.audio_files();
    blocking(move || {
        let key = export::verse_key(book, chapter, verse);
        let pcm = extract_audio_bible::render_verse(&project, &key, 44_100, &files)?
            .ok_or_else(|| AppError::msg("No reading is chosen for this verse"))?;
        Ok(Response::new(audio::wav(&pcm)))
    })
    .await
}

#[tauri::command]
async fn chapters_to_export(state: State<'_, AppState>, project: Project) -> Result<Vec<ChapterRef>> {
    let store = state.store.clone();
    blocking(move || Ok(export::chapters_to_export(&project, &store.bible(&project.bible_id)?))).await
}

#[tauri::command]
async fn export_chapters(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project: Project,
    chapters: Option<Vec<ChapterRef>>,
) -> Result<Vec<ExportedFile>> {
    let store = state.store.clone();
    blocking(move || {
        let bible = store.bible(&project.bible_id)?;
        let chapters = chapters.unwrap_or_else(|| export::chapters_to_export(&project, &bible));
        if chapters.is_empty() {
            return Err(AppError::msg("No chapters have chosen verses yet"));
        }
        let dir = (project.metadata.output_dir.as_deref())
            .filter(|d| !d.is_empty())
            .ok_or_else(|| AppError::msg("Choose a folder to export to"))?;
        Ok(export::export_to_dir(&project, &bible, &chapters, &store.audio_files(), Path::new(dir), |p| {
            let _ = app.emit("export-progress", p);
        })?)
    })
    .await
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
                transcripts: Mutex::new(HashMap::new()),
                engine: Arc::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_bibles,
            import_bible,
            delete_bible,
            bible_outline,
            book_text,
            list_projects,
            create_project,
            get_project,
            save_project,
            delete_project,
            add_sources,
            analyze,
            get_matches,
            transcript_words,
            transcript,
            create_readings_project,
            genres,
            scope_books,
            reading_rows,
            search_transcripts,
            audio_peaks,
            render_clip,
            render_chapter,
            render_verse,
            read_file_metadata,
            chapter_timeline,
            chapters_to_export,
            export_chapters,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
