//! A project: the sermons, the readings found in them, the one chosen for each verse, and how
//! to export.

use std::{path::PathBuf, sync::Mutex};

use boltffi::*;
use eab::MatchSet;

use crate::{
    Bible, ExtractAudioBibleError, Files, invalid, not_found,
    records::{
        Added, ChapterChoice, ChapterRef, ChapterStatus, ExportMetadata, ExportedFile, Reading, ReadingRun,
        SearchSummary, Selection, Settings, SourceInfo, VerseTime, WrittenFile,
    },
};

struct State {
    project: eab::Project,
    matches: MatchSet,
}

/**
An audio Bible being built: its sermons, every reading found in them, the reading chosen for
each verse, and how it's exported. Save it with `toJson` and `readingsJson`, and open it again
with `ExtractAudioBible.readProject`.

```ts
const project = eab.newProject(bible, "", "1 Thessalonians");
project.addSources(["sermons/5_1-8.m4a", "sermons/5_1-8.json"]);
project.search(bible, files);
project.chooseBest(bible, 0.75);
const mp3s = project.export(bible, files, null);
```
*/
pub struct Project {
    state: Mutex<State>,
}

impl Project {
    pub(crate) fn new(project: eab::Project, matches: MatchSet) -> Self {
        Project {
            state: Mutex::new(State { project, matches }),
        }
    }

    /// The state, even if a call panicked while holding it (every change is whole before it's
    /// made)
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn book_of(bible: &Bible, book: u8) -> Result<&eab::BibleBook, ExtractAudioBibleError> {
    bible
        .inner
        .book(book)
        .ok_or_else(|| not_found(format!("{} doesn't have book {book}", bible.inner.name)))
}

fn check_bible(state: &State, bible: &Bible) -> Result<(), ExtractAudioBibleError> {
    if state.project.bible_id != bible.inner.id {
        return Err(invalid(format!(
            "this project uses Bible {}, not {} ({})",
            state.project.bible_id, bible.inner.id, bible.inner.name
        )));
    }
    Ok(())
}

#[export]
impl Project {
    pub fn id(&self) -> String {
        self.state().project.id.clone()
    }

    pub fn name(&self) -> String {
        self.state().project.name.clone()
    }

    pub fn rename(&self, name: String) {
        self.state().project.name = name;
    }

    /// The id of the Bible it uses (`Bible.id`)
    pub fn bible_id(&self) -> String {
        self.state().project.bible_id.clone()
    }

    /// The book being built, or none for the whole Bible
    pub fn book(&self) -> Option<u8> {
        self.state().project.book
    }

    /// The project as JSON (sermons, choices, settings), the same as the app keeps
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.state().project).unwrap_or_default()
    }

    /// The readings found by `search`, as JSON, to give `readProject` so it needn't search again
    pub fn readings_json(&self) -> String {
        serde_json::to_string(&self.state().matches).unwrap_or_default()
    }

    pub fn sources(&self) -> Vec<SourceInfo> {
        self.state().project.sources.iter().map(SourceInfo::from).collect()
    }

    /**
    Adds sermons by their files' paths: each audio file (mp3, m4a, wav, flac, ogg, aac, mp4)
    with the transcript (`.json`) of the same name, in the same folder first. Audio already in
    the project is skipped
    */
    pub fn add_sources(&self, paths: Vec<String>) -> Added {
        let added = self.state().project.add_sources(&paths);
        Added {
            added: added.added as u32,
            unpaired: added.unpaired,
        }
    }

    /// Removes a sermon, and the readings chosen from it
    pub fn remove_source(&self, id: String) -> bool {
        let mut s = self.state();
        let before = s.project.sources.len();
        s.project.sources.retain(|src| src.id != id);
        s.project.selections.retain(|_, sel| sel.source_id != id);
        s.matches.matches.retain(|m| m.source_id != id);
        s.matches.runs.retain(|r| r.source_id != id);
        s.project.sources.len() < before
    }

    /**
    Finds every reading of the project's verses in its sermons' transcripts (read through
    `files`), replacing what was found before; chosen readings follow to their new ids. About a
    fifth of a second to index a whole Bible the first time, then milliseconds per sermon
    */
    pub fn search(&self, bible: &Bible, files: &Files) -> Result<SearchSummary, ExtractAudioBibleError> {
        let mut s = self.state();
        check_bible(&s, bible)?;
        let analysis = eab::analyze_with(&mut s.project, &bible.engine(), files, &|_, _, _| {})?;
        let errors = analysis
            .errors
            .iter()
            .map(|(label, e)| format!("{label}: {e}"))
            .collect();
        s.matches = analysis.matches;
        Ok(SearchSummary {
            sermons: s.project.sources.len() as u32,
            readings: s.matches.matches.len() as u32,
            errors,
        })
    }

    /// Every reading found, in sermon order
    pub fn readings(&self) -> Vec<Reading> {
        self.state().matches.matches.iter().map(Reading::from).collect()
    }

    /// A verse's readings, best first
    pub fn verse_readings(&self, book: u8, chapter: u16, verse: u16) -> Vec<Reading> {
        let s = self.state();
        let mut found: Vec<Reading> = (s.matches.matches.iter())
            .filter(|m| m.book == book && m.chapter == chapter && m.verse == verse)
            .map(Reading::from)
            .collect();
        found.sort_by(|a, b| b.score.total_cmp(&a.score));
        found
    }

    /// Verses read one after another
    pub fn runs(&self) -> Vec<ReadingRun> {
        self.state().matches.runs.iter().map(ReadingRun::from).collect()
    }

    /**
    For every verse without a reading chosen, chooses its best reading scoring at least
    `minScore` (0.9 exact, 0.75 exact and close, 0 anything), preferring one that carries on
    from the verse before. Returns the verses chosen, by chapter
    */
    pub fn choose_best(&self, bible: &Bible, min_score: f64) -> Result<Vec<ChapterChoice>, ExtractAudioBibleError> {
        let mut s = self.state();
        check_bible(&s, bible)?;
        let State { project, matches } = &mut *s;
        let mut out = vec![];
        let only = project.book;
        for book in bible
            .inner
            .books
            .iter()
            .filter(|b| only.is_none_or(|only| only == b.id))
        {
            for chapter in 1..=book.chapters.len() as u16 {
                let verses = project.choose_best(book, chapter, matches, min_score as f32);
                if !verses.is_empty() {
                    out.push(ChapterChoice {
                        book: book.id,
                        chapter,
                        verses,
                    });
                }
            }
        }
        Ok(out)
    }

    /// [`Project::choose_best`] in one chapter
    pub fn choose_best_in(
        &self,
        bible: &Bible,
        book: u8,
        chapter: u16,
        min_score: f64,
    ) -> Result<Vec<u16>, ExtractAudioBibleError> {
        let mut s = self.state();
        check_bible(&s, bible)?;
        let book = book_of(bible, book)?;
        let State { project, matches } = &mut *s;
        Ok(project.choose_best(book, chapter, matches, min_score as f32))
    }

    /// Chooses a reading for its verse (keeping the verse's volume, silence, and note)
    pub fn select(&self, reading_id: String) -> Result<Selection, ExtractAudioBibleError> {
        let mut s = self.state();
        let m = reading(&s, &reading_id)?;
        s.project.select(&m);
        selection_of(&s.project, eab::export::verse_key(m.book, m.chapter, m.verse))
    }

    /**
    Adds a reading to its verse's choice, to be played with it: for a verse read in pieces (its
    first half, then the rest minutes later). It goes before any part that begins later in the
    verse, so the pieces play in order; `arrange` reorders them. With nothing chosen for the
    verse, it's chosen. Returns the verse's choice
    */
    pub fn join(&self, reading_id: String) -> Result<Selection, ExtractAudioBibleError> {
        let mut s = self.state();
        let m = reading(&s, &reading_id)?;
        let State { project, matches } = &mut *s;
        project.join(&m, matches);
        selection_of(project, eab::export::verse_key(m.book, m.chapter, m.verse))
    }

    /// Takes a reading out of its verse's choice, leaving its other parts (taking the only one
    /// unchooses the verse). Returns what's left, if anything
    pub fn unjoin(&self, reading_id: String) -> Result<Option<Selection>, ExtractAudioBibleError> {
        let mut s = self.state();
        let m = reading(&s, &reading_id)?;
        s.project.unjoin(&m);
        Ok(selection_of(&s.project, eab::export::verse_key(m.book, m.chapter, m.verse)).ok())
    }

    /**
    Puts a verse's readings in the order given (each one of its parts, each once), keeping how
    each is cut. Returns the verse's choice
    */
    pub fn arrange(
        &self,
        book: u8,
        chapter: u16,
        verse: u16,
        reading_ids: Vec<String>,
    ) -> Result<Selection, ExtractAudioBibleError> {
        let key = eab::export::verse_key(book, chapter, verse);
        let mut s = self.state();
        let sel = (s.project.selections.get_mut(&key)).ok_or_else(|| not_found(format!("nothing chosen for {key}")))?;
        let parts = sel.parts();
        if reading_ids.len() != parts.len() {
            return Err(invalid(format!(
                "{key} has {} parts, not {}",
                parts.len(),
                reading_ids.len()
            )));
        }
        let mut arranged = vec![];
        for id in &reading_ids {
            let part = (parts.iter())
                .find(|p| &p.match_id == id)
                .ok_or_else(|| invalid(format!("{id} isn't one of {key}'s readings")))?;
            if arranged.contains(part) {
                return Err(invalid(format!("{id} is given twice")));
            }
            arranged.push(part.clone());
        }
        sel.set_parts(arranged);
        selection_of(&s.project, key)
    }

    /// Unchooses a verse's reading
    pub fn unselect(&self, book: u8, chapter: u16, verse: u16) -> bool {
        self.state()
            .project
            .selections
            .remove(&eab::export::verse_key(book, chapter, verse))
            .is_some()
    }

    /// The chosen readings, in Bible order
    pub fn selections(&self) -> Vec<Selection> {
        let s = self.state();
        let mut out: Vec<Selection> = s
            .project
            .selections
            .iter()
            .filter_map(|(k, sel)| Selection::from_core(k, sel))
            .collect();
        out.sort_by_key(|sel| (sel.book, sel.chapter, sel.verse));
        out
    }

    pub fn selection(&self, book: u8, chapter: u16, verse: u16) -> Option<Selection> {
        let key = eab::export::verse_key(book, chapter, verse);
        self.state()
            .project
            .selections
            .get(&key)
            .and_then(|sel| Selection::from_core(&key, sel))
    }

    /// Changes a verse's choice: its trims, silence, volume, or note (or sets it outright)
    pub fn update_selection(&self, selection: Selection) {
        let (key, sel) = selection.to_core();
        self.state().project.selections.insert(key, sel);
    }

    pub fn settings(&self) -> Settings {
        Settings::from(&self.state().project.settings)
    }

    pub fn set_settings(&self, settings: Settings) {
        self.state().project.settings = (&settings).into();
    }

    pub fn metadata(&self) -> ExportMetadata {
        ExportMetadata::from(&self.state().project.metadata)
    }

    pub fn set_metadata(&self, metadata: ExportMetadata) {
        metadata.apply(&mut self.state().project.metadata);
    }

    /**
    Changes settings and metadata from JSON, the way the project's JSON names them:
    `{"settings": {"gapBetweenVerses": 0.3}, "metadata": {"artist": "Grace Church"}}`.
    An unknown name is an error
    */
    pub fn apply_json(&self, json: String) -> Result<(), ExtractAudioBibleError> {
        let changes: serde_json::Value = serde_json::from_str(&json).map_err(|e| invalid(format!("not JSON: {e}")))?;
        let changes = changes.as_object().ok_or_else(|| invalid("expected an object"))?;
        let mut s = self.state();
        let mut value = serde_json::to_value(&s.project).map_err(|e| invalid(e.to_string()))?;
        for (group, fields) in changes {
            if !matches!(group.as_str(), "settings" | "metadata") {
                return Err(invalid(format!("{group}: only settings and metadata can be changed")));
            }
            let target = value[group]
                .as_object_mut()
                .ok_or_else(|| invalid(format!("no {group}")))?;
            for (field, v) in fields
                .as_object()
                .ok_or_else(|| invalid(format!("{group}: expected an object")))?
            {
                if !target.contains_key(field) {
                    return Err(invalid(format!("{group}.{field}: no such setting")));
                }
                target.insert(field.clone(), v.clone());
            }
        }
        s.project = serde_json::from_value(value)
            .map_err(|e| invalid(format!("a setting has the wrong kind of value: {e}")))?;
        Ok(())
    }

    /// The chapters of the project's book (or, for the whole Bible, those with readings), each
    /// with how many verses are found and chosen
    pub fn status(&self, bible: &Bible) -> Vec<ChapterStatus> {
        let s = self.state();
        (s.project.status(&bible.inner, &s.matches).into_iter())
            .map(|c| ChapterStatus {
                book: c.book,
                chapter: c.chapter,
                verses: c.verses as u32,
                chosen: c.chosen as u32,
                with_readings: c.with_readings as u32,
                missing: c.missing,
            })
            .collect()
    }

    /// The chapters an export would make: those with a chosen verse (only complete ones, if
    /// the metadata says so)
    pub fn chapters_to_export(&self, bible: &Bible) -> Vec<ChapterRef> {
        eab::chapters_to_export(&self.state().project, &bible.inner)
            .iter()
            .map(ChapterRef::from)
            .collect()
    }

    /// When each chosen verse is heard in a chapter's audio (`renderChapter`), to follow along
    pub fn chapter_timeline(
        &self,
        bible: &Bible,
        book: u8,
        chapter: u16,
    ) -> Result<Vec<VerseTime>, ExtractAudioBibleError> {
        let book = book_of(bible, book)?;
        Ok(
            (eab::chapter_timeline(&self.state().project, book, chapter).into_iter())
                .map(|t| VerseTime {
                    verse: t.verse,
                    start: t.start,
                    end: t.end,
                })
                .collect(),
        )
    }

    /// A chapter as it will be exported, as WAV (decodes its readings: about a second)
    pub fn render_chapter(
        &self,
        bible: &Bible,
        files: &Files,
        book: u8,
        chapter: u16,
    ) -> Result<Vec<u8>, ExtractAudioBibleError> {
        let book = book_of(bible, book)?;
        let project = self.state().project.clone();
        let pcm = eab::render_chapter(&project, book, chapter, project.metadata.sample_rate, files)?;
        Ok(eab::audio::wav(&pcm))
    }

    /// A verse as it will be exported in a file of its own, as WAV: its readings one after
    /// another, with the silence chosen around it. Null if nothing is chosen for it
    pub fn render_verse(
        &self,
        files: &Files,
        book: u8,
        chapter: u16,
        verse: u16,
    ) -> Result<Option<Vec<u8>>, ExtractAudioBibleError> {
        let project = self.state().project.clone();
        let key = eab::export::verse_key(book, chapter, verse);
        let pcm = eab::render_verse(&project, &key, project.metadata.sample_rate, files)?;
        Ok(pcm.map(|pcm| eab::audio::wav(&pcm)))
    }

    /**
    Exports tagged MP3s (by chapter, verse, or both, as the metadata says) of the given
    chapters, or every chapter with a chosen verse, and returns them with their bytes. About a
    second per chapter. To write them straight to a folder natively, use `exportToFolder`
    */
    pub fn export(
        &self,
        bible: &Bible,
        files: &Files,
        chapters: Option<Vec<ChapterRef>>,
    ) -> Result<Vec<ExportedFile>, ExtractAudioBibleError> {
        let project = self.state().project.clone();
        let chapters = chapters_or_all(&project, bible, chapters)?;
        let mut out = vec![];
        eab::export(
            &project,
            &bible.inner,
            &chapters,
            files,
            |_| {},
            |file, bytes| {
                out.push(ExportedFile {
                    path: file.path.clone(),
                    book: file.book,
                    chapter: file.chapter,
                    verse: file.verse,
                    duration: file.duration,
                    missing: file.missing.clone(),
                    bytes,
                });
                Ok(())
            },
        )?;
        Ok(out)
    }

    /// [`Project::export`] written into a folder (native only; in WebAssembly, use `export` and
    /// save the bytes)
    pub fn export_to_folder(
        &self,
        bible: &Bible,
        files: &Files,
        folder: String,
        chapters: Option<Vec<ChapterRef>>,
    ) -> Result<Vec<WrittenFile>, ExtractAudioBibleError> {
        let project = self.state().project.clone();
        let chapters = chapters_or_all(&project, bible, chapters)?;
        export_to_folder(&project, bible, files, PathBuf::from(folder), &chapters)
    }
}

fn chapters_or_all(
    project: &eab::Project,
    bible: &Bible,
    chapters: Option<Vec<ChapterRef>>,
) -> Result<Vec<eab::ChapterRef>, ExtractAudioBibleError> {
    let chapters: Vec<eab::ChapterRef> = match chapters {
        Some(list) => list.iter().map(Into::into).collect(),
        None => eab::chapters_to_export(project, &bible.inner),
    };
    if chapters.is_empty() {
        return Err(invalid("no chapters have chosen verses yet: choose readings first"));
    }
    Ok(chapters)
}

#[cfg(not(target_arch = "wasm32"))]
fn export_to_folder(
    project: &eab::Project,
    bible: &Bible,
    files: &Files,
    folder: PathBuf,
    chapters: &[eab::ChapterRef],
) -> Result<Vec<WrittenFile>, ExtractAudioBibleError> {
    let written = eab::export_to_dir(project, &bible.inner, chapters, files, &folder, |_| {})?;
    Ok(written.into_iter().map(WrittenFile::from).collect())
}

#[cfg(target_arch = "wasm32")]
fn export_to_folder(
    project: &eab::Project,
    bible: &Bible,
    files: &Files,
    folder: PathBuf,
    chapters: &[eab::ChapterRef],
) -> Result<Vec<WrittenFile>, ExtractAudioBibleError> {
    let _ = (project, bible, files, folder, chapters);
    Err(ExtractAudioBibleError::Io {
        message: "no file access in WebAssembly: use export and save the bytes".into(),
    })
}

/// A reading found by the last search
fn reading(s: &State, id: &str) -> Result<eab::Match, ExtractAudioBibleError> {
    (s.matches.matches.iter())
        .find(|m| m.id == id)
        .cloned()
        .ok_or_else(|| not_found(format!("no reading {id}: search first")))
}

/// The choice for a verse, by its key
fn selection_of(project: &eab::Project, key: String) -> Result<Selection, ExtractAudioBibleError> {
    (project.selections.get(&key))
        .and_then(|sel| Selection::from_core(&key, sel))
        .ok_or_else(|| not_found(format!("nothing chosen for {key}")))
}
