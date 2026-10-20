//! Where files come from. Everything the library reads (sermon audio, a cover image, Bible text)
//! goes through [`Files`], so it can work on the disk ([`Disk`]) or on bytes a host hands it
//! ([`Memory`]), as in WebAssembly, which has no files.

use std::{collections::HashMap, io::Cursor, path::PathBuf, sync::Arc};

pub use symphonia::core::io::MediaSource;

use crate::error::{Error, Result};

/// Files by path
pub trait Files: Send + Sync {
    /// A whole file
    fn read(&self, path: &str) -> Result<Vec<u8>>;

    /// A file to decode audio from, read as it's decoded
    fn open(&self, path: &str) -> Result<Box<dyn MediaSource>>;
}

/// The disk
#[derive(Clone, Copy, Debug, Default)]
pub struct Disk;

impl Files for Disk {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        std::fs::read(path).map_err(|e| with_path(e, path))
    }

    fn open(&self, path: &str) -> Result<Box<dyn MediaSource>> {
        let file = std::fs::File::open(path).map_err(|e| with_path(e, path))?;
        Ok(Box::new(file))
    }
}

fn with_path(e: std::io::Error, path: &str) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        Error::not_found(format!("{path}: no such file"))
    } else {
        Error::Io(std::io::Error::new(e.kind(), format!("{path}: {e}")))
    }
}

/// Files held in memory, by the paths they're asked for under
#[derive(Clone, Debug, Default)]
pub struct Memory {
    files: HashMap<String, Arc<[u8]>>,
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file (replacing one at the same path)
    pub fn insert(&mut self, path: impl Into<String>, bytes: impl Into<Arc<[u8]>>) {
        self.files.insert(path.into(), bytes.into());
    }

    pub fn remove(&mut self, path: &str) -> bool {
        self.files.remove(path).is_some()
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    fn get(&self, path: &str) -> Result<&Arc<[u8]>> {
        self.files
            .get(path)
            .ok_or_else(|| Error::not_found(format!("{path}: not given")))
    }
}

impl Files for Memory {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        Ok(self.get(path)?.to_vec())
    }

    fn open(&self, path: &str) -> Result<Box<dyn MediaSource>> {
        Ok(Box::new(Cursor::new(self.get(path)?.clone())))
    }
}

impl<F: Files + ?Sized> Files for &F {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        (**self).read(path)
    }

    fn open(&self, path: &str) -> Result<Box<dyn MediaSource>> {
        (**self).open(path)
    }
}

/// Every file under the given files and folders, sorted within each folder
pub fn gather(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = vec![];
    for path in paths {
        if path.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(path)?.flatten().map(|e| e.path()).collect();
            entries.sort();
            out.extend(gather(&entries)?);
        } else if path.is_file() {
            out.push(path.clone());
        } else {
            return Err(Error::not_found(format!("{}: no such file or folder", path.display())));
        }
    }
    Ok(out)
}
