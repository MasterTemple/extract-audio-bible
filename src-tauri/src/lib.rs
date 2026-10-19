mod audio;
mod bible;
mod error;
mod export;
mod matcher;
mod project;
mod text;
mod transcript;
#[cfg(test)]
mod e2e_test;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State, ipc::Response};

use crate::{
    bible::{Bible, BibleSummary, BookOutline},
    error::{AppError, Result},
    export::{ChapterRef, ExportedFile},
    matcher::{MatchOptions, VerseIndex},
    project::{MatchSet, Project, ProjectSummary, Source, Store, now},
    transcript::{Transcript, Word},
};

struct AppState {
    store: Store,
    transcripts: Mutex<HashMap<String, Arc<Transcript>>>,
}

impl AppState {
    fn transcript(&self, path: &str) -> Result<Arc<Transcript>> {
        if let Some(t) = self.transcripts.lock().unwrap().get(path) {
            return Ok(t.clone());
        }
        let t = Arc::new(Transcript::open(Path::new(path))?);
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
        let (books, format) = bible::import(&paths)?;
        let bible = Bible {
            id: new_id(),
            name: if name.trim().is_empty() { "Untitled translation".into() } else { name.trim().into() },
            abbreviation: abbreviation.trim().to_string(),
            language: language.filter(|l| !l.trim().is_empty()),
            format,
            books,
        };
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
fn create_project(state: State<AppState>, name: String, bible_id: String) -> Result<Project> {
    let bible = state.store.bible(&bible_id)?;
    let mut metadata = project::Metadata::default();
    metadata.language = bible.language.clone().unwrap_or_default();
    let project = Project {
        id: new_id(),
        name: if name.trim().is_empty() { format!("{} audio Bible", bible.name) } else { name.trim().into() },
        bible_id,
        created_at: now(),
        updated_at: now(),
        sources: vec![],
        selections: BTreeMap::new(),
        settings: Default::default(),
        metadata,
    };
    state.store.save_project(&project)?;
    Ok(project)
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

const AUDIO: [&str; 7] = ["mp3", "m4a", "wav", "flac", "ogg", "aac", "mp4"];

fn walk(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path).into_iter().flatten().flatten().map(|e| e.path()).collect();
        entries.sort();
        for e in entries {
            walk(&e, out);
        }
    } else {
        out.push(path.to_path_buf());
    }
}

fn lower_ext(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// A file's name without its extensions (`sermon.mp3.json` and `sermon.json` are `sermon`)
fn base_name(p: &Path) -> String {
    let mut stem = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    while let Some((s, ext)) = stem.rsplit_once('.') {
        if ext.eq_ignore_ascii_case("json") || AUDIO.contains(&ext.to_ascii_lowercase().as_str()) {
            stem = s.to_string();
        } else {
            break;
        }
    }
    stem
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
    let mut files = vec![];
    for p in &paths {
        walk(Path::new(p), &mut files);
    }
    let (jsons, audios): (Vec<PathBuf>, Vec<PathBuf>) = files
        .into_iter()
        .filter(|f| lower_ext(f) == "json" || AUDIO.contains(&lower_ext(f).as_str()))
        .partition(|f| lower_ext(f) == "json");
    let mut used = vec![false; jsons.len()];
    let mut unpaired = vec![];
    let mut added = 0;
    for audio in audios {
        let base = base_name(&audio);
        let pick = (0..jsons.len())
            .filter(|&i| !used[i] && base_name(&jsons[i]) == base)
            .min_by_key(|&i| jsons[i].parent() != audio.parent());
        let Some(i) = pick else {
            unpaired.push(audio.to_string_lossy().into_owned());
            continue;
        };
        used[i] = true;
        let audio_path = audio.to_string_lossy().into_owned();
        if project.sources.iter().any(|s| s.audio_path == audio_path) {
            continue;
        }
        project.sources.push(Source {
            id: new_id(),
            label: base,
            audio_path,
            transcript_path: jsons[i].to_string_lossy().into_owned(),
            duration: None,
            word_count: 0,
            match_count: 0,
        });
        added += 1;
    }
    unpaired.extend(jsons.iter().zip(&used).filter(|(_, u)| !**u).map(|(j, _)| j.to_string_lossy().into_owned()));
    project.updated_at = now();
    state.store.save_project(&project)?;
    Ok(AddedSources { project, added, unpaired })
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
    let transcripts: Vec<Result<Arc<Transcript>>> =
        project.sources.iter().map(|s| state.transcript(&s.transcript_path)).collect();
    blocking(move || {
        let mut project = project;
        let total = project.sources.len();
        let emit = |done: usize, message: String| {
            let _ = app.emit("analyze-progress", AnalyzeProgress { done, total, message });
        };
        emit(0, "Reading the Bible text".into());
        let bible = store.bible(&project.bible_id)?;
        let index = VerseIndex::new(&bible, None);
        emit(0, format!("Indexed {} verses", index.len()));
        let options = MatchOptions { min_score: project.settings.min_score, books: None };
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let results: Vec<_> = (project.sources.par_iter().zip(transcripts.into_par_iter()))
            .map(|(source, transcript)| {
                let out = transcript.map(|t| {
                    let (matches, runs) = matcher::find_matches(&index, &t, &source.id, &options);
                    (t.words.len(), t.duration(), matches, runs)
                });
                let done = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                emit(done, format!("Searched {}", source.label));
                out
            })
            .collect();
        let mut set = MatchSet { analyzed_at: now(), ..Default::default() };
        let mut errors = vec![];
        for (source, result) in project.sources.iter_mut().zip(results) {
            match result {
                Ok((words, duration, matches, runs)) => {
                    source.word_count = words;
                    source.match_count = matches.len();
                    source.duration = source.duration.or(Some(duration));
                    set.matches.extend(matches);
                    set.runs.extend(runs);
                }
                Err(e) => errors.push(format!("{}: {e}", source.label)),
            }
        }
        // Selections follow their reading to its new id, if the analysis changed it
        for sel in project.selections.values_mut() {
            if set.matches.iter().any(|m| m.id == sel.match_id) {
                continue;
            }
            let found = set.matches.iter().find(|m| {
                m.source_id == sel.source_id && m.start < sel.end && m.end > sel.start && sel.match_id.contains(&format!(":{}.{}.{}@", m.book, m.chapter, m.verse))
            });
            if let Some(m) = found {
                sel.match_id = m.id.clone();
            }
        }
        project.updated_at = now();
        store.save_matches(&project.id, &set)?;
        store.save_project(&project)?;
        emit(total, "Done".into());
        Ok(Analyzed { project, matches: set, errors })
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
    blocking(move || {
        let (all, duration) = audio::peaks(Path::new(&audio_path), &cache)?;
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipRequest {
    audio_path: String,
    start: f64,
    end: f64,
    #[serde(default)]
    pad_before: f64,
    #[serde(default)]
    pad_after: f64,
    /// Level and fade it as the export will
    #[serde(default)]
    processed: bool,
    #[serde(default)]
    target_db: Option<f64>,
}

/// A stretch of a sermon as WAV, for listening
#[tauri::command]
async fn render_clip(request: ClipRequest) -> Result<Response> {
    blocking(move || {
        let r = request;
        let pcm = if r.processed {
            export::render_clip(&r.audio_path, r.start, r.end, 44_100, r.target_db, 8.0)?
        } else {
            audio::decode_range(Path::new(&r.audio_path), r.start, r.end)?
        };
        let rate = pcm.rate as f64;
        let mut samples = vec![0.0; (r.pad_before.max(0.0) * rate) as usize];
        samples.extend(pcm.samples);
        samples.resize(samples.len() + (r.pad_after.max(0.0) * rate) as usize, 0.0);
        Ok(Response::new(audio::wav(&audio::Pcm { rate: pcm.rate, samples })))
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
        let pcm = export::render_chapter(&project, book, chapter, 24_000)?;
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
        export::export(&project, &bible, &chapters, |p| {
            let _ = app.emit("export-progress", p);
        })
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
            app.manage(AppState { store: Store::new(root), transcripts: Mutex::new(HashMap::new()) });
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
            audio_peaks,
            render_clip,
            render_chapter,
            chapters_to_export,
            export_chapters,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
