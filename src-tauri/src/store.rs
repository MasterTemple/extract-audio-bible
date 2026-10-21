//! Where the app keeps everything: Bibles and projects as JSON in its data folder.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::SystemTime,
};

use extract_audio_bible::{Bible, MatchSet, Project, ProjectChanges, bible::BibleSummary, db, project::now};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result, file_error};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub kind: extract_audio_bible::ProjectKind,
    pub bible_id: String,
    pub book: Option<u8>,
    /// The Bibles searched, by id
    pub translations: Vec<String>,
    pub updated_at: u64,
    pub source_count: usize,
    pub selection_count: usize,
    /// The folder it's kept in, when it was opened from one (its own, as the command line
    /// keeps it) rather than made here
    pub folder: Option<PathBuf>,
}

/// Where everything is kept
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    /// Projects as last read or saved, by id: a project with thousands of sermons is megabytes of
    /// JSON, too slow to read for every command
    projects: Arc<Mutex<HashMap<String, Cached>>>,
    /// Held while changes are saved, so they're made one after another
    saving: Arc<Mutex<()>>,
}

/// A project as its file was when it was read or written (read again when the file changes, as
/// when the command line changes it)
struct Cached {
    path: PathBuf,
    modified: SystemTime,
    project: Arc<Project>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn modified(path: &Path) -> Result<SystemTime> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| file_error(path.display(), e))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written beside, then moved, so a crash never leaves half a file
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(value)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path).map_err(|e| file_error(path.display(), e))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn safe_id(id: &str) -> Result<&str> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::msg(format!("Bad id: {id}")));
    }
    Ok(id)
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            projects: Arc::default(),
            saving: Arc::default(),
        }
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Files from disk, with sermon audio given by a URL downloaded into the cache once
    pub fn audio_files(&self) -> extract_audio_bible::Downloading<'static> {
        extract_audio_bible::Downloading::new(&extract_audio_bible::Disk, self.cache_dir().join("audio"))
    }

    fn bible_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self.root.join("bibles").join(format!("{}.json", safe_id(id)?)))
    }

    fn project_dir(&self, id: &str) -> Result<PathBuf> {
        if let Some(folder) = self.linked().remove(id) {
            return Ok(folder);
        }
        Ok(self.root.join("projects").join(safe_id(id)?))
    }

    /// Projects opened from folders of their own, by id
    fn linked(&self) -> BTreeMap<String, PathBuf> {
        read_json(&self.root.join("linked-projects.json")).unwrap_or_default()
    }

    /**
    Opens a project kept in a folder of its own (as the command line keeps it: `project.json`,
    `readings.sqlite`, `bible.json`, and `bibles/<id>.json`), where it is: what's changed here is
    saved there. Its translations not here yet are imported
    */
    pub fn link_project(&self, folder: &Path) -> Result<Project> {
        let folder = std::path::absolute(folder)?;
        let project: Project = read_json(&folder.join("project.json"))?;
        safe_id(&project.id)?;
        if let Some(other) = self.linked().get(&project.id).filter(|f| **f != folder) {
            return Err(AppError::msg(format!(
                "A project with the same id is already open from {}",
                other.display()
            )));
        }
        if self.root.join("projects").join(&project.id).exists() {
            return Err(AppError::msg("A project with the same id is already here"));
        }
        for id in project.bibles_searched() {
            if self.bible_path(&id)?.exists() {
                continue;
            }
            let bible: Bible = read_json(&folder_bible(&folder, &project, &id))?;
            self.save_bible(&bible)?;
        }
        let mut linked = self.linked();
        linked.insert(project.id.clone(), folder);
        write_json(&self.root.join("linked-projects.json"), &linked)?;
        Ok(project)
    }

    pub fn save_bible(&self, bible: &Bible) -> Result<()> {
        write_json(&self.bible_path(&bible.id)?, bible)?;
        write_json(
            &self.root.join("bibles").join(format!("{}.summary", bible.id)),
            &bible.summary(),
        )
    }

    /**
    Renames a translation: its own file, the copies projects' folders keep of it (for the command
    line), and what projects' readings call it
    */
    pub fn rename_bible(
        &self,
        id: &str,
        name: &str,
        abbreviation: &str,
        language: Option<String>,
    ) -> Result<BibleSummary> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::msg("A translation needs a name"));
        }
        let mut bible = self.bible(id)?;
        bible.name = name.to_string();
        bible.abbreviation = abbreviation.trim().to_string();
        bible.language = language.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        self.save_bible(&bible)?;
        for summary in self.projects()? {
            if !summary.translations.iter().any(|t| t == id) {
                continue;
            }
            if let Some(folder) = &summary.folder {
                let copy = folder_bible(folder, &*self.project_ref(&summary.id)?, id);
                if copy.exists() {
                    write_json(&copy, &bible)?;
                }
            }
            if let Some(mut readings) = self.readings(&summary.id)? {
                readings.rename_translation(id, &bible.name, &bible.abbreviation)?;
            }
        }
        Ok(bible.summary())
    }

    pub fn bible(&self, id: &str) -> Result<Bible> {
        read_json(&self.bible_path(id)?)
    }

    pub fn delete_bible(&self, id: &str) -> Result<()> {
        let path = self.bible_path(id)?;
        let _ = std::fs::remove_file(path.with_extension("summary"));
        Ok(std::fs::remove_file(path)?)
    }

    pub fn bibles(&self) -> Result<Vec<BibleSummary>> {
        let dir = self.root.join("bibles");
        let mut out = vec![];
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "summary")
                && let Ok(s) = read_json::<BibleSummary>(&path)
            {
                out.push(s);
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Saves a project, and keeps what's read of its readings to its sermons
    pub fn save_project(&self, project: &Project) -> Result<()> {
        self.write_project(project)?;
        // What's read of its readings follows its sermons (taken out, put back, renamed)
        if let Some(mut readings) = self.readings(&project.id)? {
            readings.sync_sermons(project)?;
        }
        Ok(())
    }

    fn write_project(&self, project: &Project) -> Result<()> {
        let dir = self.project_dir(&project.id)?;
        // A folder of its own keeps its translations, for the command line
        if self.linked().contains_key(&project.id) {
            for id in project.bibles_searched() {
                let path = folder_bible(&dir, project, &id);
                if !path.exists() {
                    write_json(&path, &self.bible(&id)?)?;
                }
            }
        }
        let path = dir.join("project.json");
        write_json(&path, project)?;
        lock(&self.projects).insert(
            project.id.clone(),
            Cached {
                modified: modified(&path)?,
                path,
                project: Arc::new(project.clone()),
            },
        );
        Ok(())
    }

    /// A project, to change
    pub fn project(&self, id: &str) -> Result<Project> {
        Ok((*self.project_ref(id)?).clone())
    }

    /// A project, to read (as kept, unless its file has changed since)
    pub fn project_ref(&self, id: &str) -> Result<Arc<Project>> {
        let path = self.project_dir(id)?.join("project.json");
        let modified = modified(&path)?;
        if let Some(c) = lock(&self.projects).get(id)
            && c.path == path
            && c.modified == modified
        {
            return Ok(c.project.clone());
        }
        let project: Arc<Project> = Arc::new(read_json(&path)?);
        lock(&self.projects).insert(
            id.to_string(),
            Cached {
                path,
                modified,
                project: project.clone(),
            },
        );
        Ok(project)
    }

    /**
    A new project (made, not yet saved) given some of another's sermons: their transcripts and
    audio (searched again in it), what's known of them, and where its audio is downloaded from and
    how faithfully readings are found. Saved, and given back
    */
    pub fn project_from(&self, from_id: &str, source_ids: &[String], mut project: Project) -> Result<Project> {
        let from = self.project_ref(from_id)?;
        let wanted: std::collections::HashSet<&str> = source_ids.iter().map(String::as_str).collect();
        project.sources = (from.sources.iter())
            .filter(|s| wanted.contains(s.id.as_str()))
            .map(|s| extract_audio_bible::Source {
                word_count: 0,
                match_count: 0,
                ..s.clone()
            })
            .collect();
        if project.sources.is_empty() {
            return Err(AppError::msg("No sermons to make it from"));
        }
        let files: std::collections::HashSet<String> = project.sources.iter().map(|s| s.file_id()).collect();
        project.files = (from.files.iter())
            .filter(|(id, _)| files.contains(*id))
            .map(|(id, meta)| (id.clone(), meta.clone()))
            .collect();
        project.settings.audio_url = from.settings.audio_url.clone();
        project.settings.min_accuracy = from.settings.min_accuracy;
        self.save_project(&project)?;
        Ok(project)
    }

    /// Makes changes to a project and saves it (one save at a time); returns when it was changed
    pub fn save_changes(&self, id: &str, changes: ProjectChanges) -> Result<u64> {
        let _one_at_a_time = lock(&self.saving);
        let mut project = self.project(id)?;
        let sermons = changes.sources.is_some() || changes.files.is_some();
        project.apply(changes);
        project.updated_at = now();
        if sermons {
            self.save_project(&project)?;
        } else {
            self.write_project(&project)?;
        }
        Ok(project.updated_at)
    }

    /// Deletes a project made here; one opened from a folder is only closed (its folder kept)
    pub fn delete_project(&self, id: &str) -> Result<()> {
        lock(&self.projects).remove(id);
        let mut linked = self.linked();
        if linked.remove(id).is_some() {
            return write_json(&self.root.join("linked-projects.json"), &linked);
        }
        Ok(std::fs::remove_dir_all(self.project_dir(id)?)?)
    }

    pub fn projects(&self) -> Result<Vec<ProjectSummary>> {
        let mut out = vec![];
        let linked = self.linked();
        let ids = (std::fs::read_dir(self.root.join("projects"))
            .into_iter()
            .flatten()
            .flatten())
        .filter_map(|entry| Some((entry.file_name().to_str()?.to_string(), None)))
        .chain(linked.iter().map(|(id, folder)| (id.clone(), Some(folder.clone()))));
        for (id, folder) in ids {
            if let Ok(p) = self.project_ref(&id) {
                out.push(ProjectSummary {
                    folder,
                    translations: p.bibles_searched(),
                    id: p.id.clone(),
                    name: p.name.clone(),
                    kind: p.kind,
                    bible_id: p.bible_id.clone(),
                    book: p.book,
                    updated_at: p.updated_at,
                    source_count: p.sources.len(),
                    selection_count: p.selections.len(),
                });
            }
        }
        out.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        Ok(out)
    }

    /// Every reading a search found, written as the project's readings database (replacing
    /// what was there)
    pub fn save_matches(&self, project: &Project, matches: &MatchSet) -> Result<()> {
        let bibles = self.bibles_of(project)?;
        let path = db::path(&self.project_dir(&project.id)?);
        db::Readings::write(&path, project, &bibles.iter().collect::<Vec<_>>(), matches)?;
        let _ = std::fs::remove_file(path.with_file_name(db::LEGACY_FILE));
        Ok(())
    }

    /**
    Readies a project's readings database: made from the readings it kept as JSON before, if it
    has those, and its list of sermons kept as the project has them. Returns whether it has one
    (it's been searched)
    */
    pub fn prepare_readings(&self, project: &Project) -> Result<bool> {
        let dir = self.project_dir(&project.id)?;
        let bibles = || {
            (self.bibles_of(project)).map_err(|e| extract_audio_bible::Error::not_found(format!("A translation: {e}")))
        };
        Ok(db::open_in(&dir, project, &extract_audio_bible::Disk, bibles)?.is_some())
    }

    /// A project's readings database, as it was last readied ([`Store::prepare_readings`]), if it
    /// has one
    pub fn readings(&self, project_id: &str) -> Result<Option<db::Readings>> {
        let path = db::path(&self.project_dir(project_id)?);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(db::Readings::open(&path)?))
    }

    /// The translations a project searches
    pub fn bibles_of(&self, project: &Project) -> Result<Vec<Bible>> {
        project.bibles_searched().iter().map(|id| self.bible(id)).collect()
    }
}

/// Where a project's folder keeps a translation: `bible.json` its own, `bibles/<id>.json` the others
fn folder_bible(folder: &Path, project: &Project, id: &str) -> PathBuf {
    if id == project.bible_id {
        folder.join("bible.json")
    } else {
        folder.join("bibles").join(format!("{id}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bible(id: &str, line: &str) -> Bible {
        let imported = extract_audio_bible::import(vec![(format!("{id}.txt"), line.as_bytes().to_vec())]).unwrap();
        Bible::new(id, id, id.to_uppercase(), imported)
    }

    #[test]
    fn a_project_from_another_s_sermons() {
        let tmp = std::env::temp_dir().join(format!("eab-store-from-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::new(tmp.clone());
        let kjv = bible(
            "kjv",
            "2 Corinthians 2:14 Now thanks be unto God.\n2 Corinthians 2:15 For we are unto God a sweet savour.",
        );
        store.save_bible(&kjv).unwrap();
        let mut every = Project::readings("every", "", &[&kjv], Default::default()).unwrap();
        every.add_sources(&["a.json".into(), "b.json".into(), "c.json".into()]);
        every.settings.audio_url = "https://example.com/{id}.mp3".into();
        every.settings.min_accuracy = 0.8;
        for (id, speaker) in [("a", "Paul Washer"), ("b", "Paul Washer"), ("c", "Someone else")] {
            every.files.insert(
                id.into(),
                extract_audio_bible::FileMetadata {
                    speaker: Some(speaker.into()),
                    ..Default::default()
                },
            );
        }
        every.sources[0].word_count = 100;
        store.save_project(&every).unwrap();

        let washer: Vec<String> = every.sources[..2].iter().map(|s| s.id.clone()).collect();
        let passage = extract_audio_bible::Passage::parse("2:14-15", kjv.book(47).unwrap()).unwrap();
        let made = store
            .project_from("every", &washer, Project::passage("made", "", &kjv, 47, passage))
            .unwrap();
        assert_eq!(made.sources.iter().map(|s| s.file_id()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(made.sources[0].word_count, 0, "searched again in it");
        assert_eq!(made.files.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(
            (made.settings.audio_url.as_str(), made.settings.min_accuracy),
            ("https://example.com/{id}.mp3", 0.8)
        );
        assert_eq!(store.project("made").unwrap().passage, Some(passage));
        assert!(
            store
                .project_from("every", &[], Project::new("none", "", &kjv, None))
                .is_err()
        );
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn changes_are_saved_and_read_back() {
        let tmp = std::env::temp_dir().join(format!("eab-store-changes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let store = Store::new(tmp.clone());
        let kjv = bible("kjv", "John 11:35 Jesus wept.");
        store.save_bible(&kjv).unwrap();
        let project = Project::new("changing", "", &kjv, None);
        store.save_project(&project).unwrap();

        let changes: ProjectChanges =
            serde_json::from_str(r#"{"name": "John", "files": {"set": {"a": {"speaker": "Paul"}}}}"#).unwrap();
        let at = store.save_changes("changing", changes).unwrap();
        let saved = store.project_ref("changing").unwrap();
        assert_eq!((saved.name.as_str(), saved.updated_at), ("John", at));
        assert_eq!(saved.files["a"].speaker.as_deref(), Some("Paul"));
        // Read as kept, until its file changes
        assert!(Arc::ptr_eq(&saved, &store.project_ref("changing").unwrap()));
        let on_disk: Project = read_json(&tmp.join("projects/changing/project.json")).unwrap();
        assert_eq!(on_disk.name, "John");

        // Changed outside the app (as the command line does), it's read again
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut edited = on_disk;
        edited.name = "Edited".into();
        std::fs::write(
            tmp.join("projects/changing/project.json"),
            serde_json::to_vec(&edited).unwrap(),
        )
        .unwrap();
        assert_eq!(store.project("changing").unwrap().name, "Edited");
        assert_eq!(store.projects().unwrap()[0].name, "Edited");

        store.delete_project("changing").unwrap();
        assert!(store.project("changing").is_err());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_project_folder_is_opened_where_it_is() {
        let tmp = std::env::temp_dir().join(format!("eab-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let (root, folder) = (tmp.join("data"), tmp.join("folder"));
        let (kjv, web) = (
            bible("kjv", "John 11:35 Jesus wept."),
            bible("web", "John 11:35 Jesus wept."),
        );
        // A folder as the command line keeps it
        let mut project = Project::readings(
            "folderproject",
            "",
            &[&kjv, &web],
            extract_audio_bible::BookScope::default(),
        )
        .unwrap();
        write_json(&folder.join("project.json"), &project).unwrap();
        write_json(&folder.join("bible.json"), &kjv).unwrap();
        write_json(&folder.join("bibles/web.json"), &web).unwrap();

        let store = Store::new(root.clone());
        store.link_project(&folder).unwrap();
        assert_eq!(store.bible("web").unwrap().abbreviation, "WEB");
        let listed = store.projects().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].folder.as_deref(),
            Some(std::path::absolute(&folder).unwrap().as_path())
        );

        // Saved in its folder, and its matches read from there
        project.name = "Renamed".into();
        store.save_project(&project).unwrap();
        assert_eq!(
            read_json::<Project>(&folder.join("project.json")).unwrap().name,
            "Renamed"
        );
        assert!(!root.join("projects/folderproject").exists());
        store.save_matches(&project, &MatchSet::default()).unwrap();
        assert!(folder.join(db::FILE).exists());
        assert!(store.readings(&project.id).unwrap().is_some());

        // A translation it lost is written back for the command line
        std::fs::remove_file(folder.join("bibles/web.json")).unwrap();
        store.save_project(&project).unwrap();
        assert!(folder.join("bibles/web.json").exists());

        // Renamed, it's renamed in the folder's copy (for the command line) and its readings
        store.rename_bible("web", "World English Bible", "WEBUS", None).unwrap();
        assert_eq!(store.bible("web").unwrap().abbreviation, "WEBUS");
        assert_eq!(
            read_json::<Bible>(&folder.join("bibles/web.json")).unwrap().name,
            "World English Bible"
        );
        let readings = store.readings(&project.id).unwrap().unwrap();
        assert!(readings.overview(&Default::default()).is_ok());
        assert!(store.rename_bible("web", "  ", "", None).is_err());

        // Closing keeps the folder
        store.delete_project(&project.id).unwrap();
        assert!(store.projects().unwrap().is_empty());
        assert!(folder.join("project.json").exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
