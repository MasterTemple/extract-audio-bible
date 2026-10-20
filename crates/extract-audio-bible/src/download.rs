//! Sermon audio given by a URL ([`Settings::audio_url`](crate::Settings::audio_url)), downloaded
//! the first time it's read and kept in a folder.

use std::path::{Path, PathBuf};

use crate::{
    error::{Error, Result},
    files::{Files, MediaSource},
    project::stable_id,
};

/// Files read through `files`, except URLs (`http://`, `https://`), which are downloaded once
/// into `cache` and read from there
pub struct Downloading<'f> {
    files: &'f dyn Files,
    cache: PathBuf,
}

impl<'f> Downloading<'f> {
    pub fn new(files: &'f dyn Files, cache: impl Into<PathBuf>) -> Self {
        Downloading { files, cache: cache.into() }
    }

    /// Where a path is read from: itself, or a URL's download (fetched if it hasn't been)
    pub fn local(&self, path: &str) -> Result<String> {
        if !is_url(path) {
            return Ok(path.to_string());
        }
        let file = self.cache.join(cache_name(path));
        if !file.exists() {
            download(path, &file)?;
        }
        Ok(file.to_string_lossy().into_owned())
    }
}

impl Files for Downloading<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.files.read(&self.local(path)?)
    }

    fn open(&self, path: &str) -> Result<Box<dyn MediaSource>> {
        self.files.open(&self.local(path)?)
    }
}

/// Whether a path is a URL to download
pub fn is_url(path: &str) -> bool {
    path.starts_with("https://") || path.starts_with("http://")
}

/// A URL's download's name: a hash of it, with its file's extension (so its kind of audio is known)
fn cache_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = (path.rsplit_once('/').map_or(path, |(_, name)| name).rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()));
    match ext {
        Some(ext) => format!("{}.{ext}", stable_id(url)),
        None => stable_id(url),
    }
}

/// Downloads `url` to `file`, whole or not at all
fn download(url: &str, file: &Path) -> Result<()> {
    let failed = |message: String| Error::Download { url: url.to_string(), message };
    let mut response = ureq::get(url).call().map_err(|e| failed(e.to_string()))?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = file.with_extension("part");
    let mut out = std::fs::File::create(&partial)?;
    let copied = std::io::copy(&mut response.body_mut().as_reader(), &mut out);
    drop(out);
    match copied {
        Ok(0) => {
            let _ = std::fs::remove_file(&partial);
            Err(failed("it was empty".into()))
        }
        Ok(_) => Ok(std::fs::rename(&partial, file)?),
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(failed(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_downloads_by_url() {
        assert!(cache_name("https://example.com/a/b.MP3?x=1").ends_with(".mp3"));
        assert!(!cache_name("https://example.com/media/abc").contains('.'));
        assert_ne!(cache_name("https://example.com/1.mp3"), cache_name("https://example.com/2.mp3"));
        let d = Downloading::new(&crate::Disk, "/nowhere");
        assert_eq!(d.local("sermons/a.mp3").unwrap(), "sermons/a.mp3");
        assert!(matches!(
            d.local("http://127.0.0.1:9/a.mp3").unwrap_err().kind(),
            crate::ErrorKind::Network
        ));
    }
}
