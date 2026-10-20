//! Where the app keeps everything: Bibles and projects as JSON in its data folder.

use std::path::{Path, PathBuf};

use extract_audio_bible::{
    Bible, MatchSet, Project,
    bible::BibleSummary,
};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub bible_id: String,
    pub book: Option<u8>,
    pub updated_at: u64,
    pub source_count: usize,
    pub selection_count: usize,
}

/// Where everything is kept
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
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
    let bytes = std::fs::read(path).map_err(|e| AppError::msg(format!("{}: {e}", path.display())))?;
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
        Self { root }
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
        Ok(self.root.join("projects").join(safe_id(id)?))
    }

    pub fn save_bible(&self, bible: &Bible) -> Result<()> {
        write_json(&self.bible_path(&bible.id)?, bible)?;
        write_json(&self.root.join("bibles").join(format!("{}.summary", bible.id)), &bible.summary())
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
            if path.extension().is_some_and(|e| e == "summary") {
                if let Ok(s) = read_json::<BibleSummary>(&path) {
                    out.push(s);
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn save_project(&self, project: &Project) -> Result<()> {
        write_json(&self.project_dir(&project.id)?.join("project.json"), project)
    }

    pub fn project(&self, id: &str) -> Result<Project> {
        read_json(&self.project_dir(id)?.join("project.json"))
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        Ok(std::fs::remove_dir_all(self.project_dir(id)?)?)
    }

    pub fn projects(&self) -> Result<Vec<ProjectSummary>> {
        let mut out = vec![];
        for entry in std::fs::read_dir(self.root.join("projects")).into_iter().flatten().flatten() {
            if let Ok(p) = read_json::<Project>(&entry.path().join("project.json")) {
                out.push(ProjectSummary {
                    id: p.id,
                    name: p.name,
                    bible_id: p.bible_id,
                    book: p.book,
                    updated_at: p.updated_at,
                    source_count: p.sources.len(),
                    selection_count: p.selections.len(),
                });
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub fn save_matches(&self, project_id: &str, matches: &MatchSet) -> Result<()> {
        write_json(&self.project_dir(project_id)?.join("matches.json"), matches)
    }

    pub fn matches(&self, project_id: &str) -> Result<MatchSet> {
        let path = self.project_dir(project_id)?.join("matches.json");
        if !path.exists() {
            return Ok(MatchSet::default());
        }
        read_json(&path)
    }
}
