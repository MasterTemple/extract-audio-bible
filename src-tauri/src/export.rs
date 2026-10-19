//! Putting a chapter's chosen readings together, and writing them as tagged MP3s.

use std::{collections::HashMap, path::PathBuf};

use id3::{
    Tag, TagLike, Version,
    frame::{Comment, Picture, PictureType},
};
use serde::{Deserialize, Serialize};

use crate::{
    audio::{self, Pcm},
    bible::{Bible, BibleBook},
    error::{AppError, Result},
    project::{Project, Selection},
};

pub fn verse_key(book: u8, chapter: u16, verse: u16) -> String {
    format!("{book}.{chapter}.{verse}")
}

/// What a chapter's file is made of
enum Piece<'a> {
    Silence(f64),
    /// One stretch of one sermon: a verse, or verses read straight through
    Audio { path: &'a str, start: f64, end: f64 },
}

/// The verses of a chapter that have text but no chosen reading
pub fn missing_verses(project: &Project, book: &BibleBook, chapter: u16) -> Vec<u16> {
    let Some(verses) = book.chapters.get(chapter as usize - 1) else { return vec![] };
    (verses.iter().enumerate())
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(i, _)| i as u16 + 1)
        .filter(|v| !project.selections.contains_key(&verse_key(book.id, chapter, *v)))
        .collect()
}

fn pieces<'a>(project: &'a Project, book: &BibleBook, chapter: u16) -> Vec<Piece<'a>> {
    let settings = &project.settings;
    let sources: HashMap<&str, &str> =
        project.sources.iter().map(|s| (s.id.as_str(), s.audio_path.as_str())).collect();
    let count = book.chapters.get(chapter as usize - 1).map_or(0, Vec::len) as u16;
    let chosen: Vec<&Selection> = (1..=count)
        .filter_map(|v| project.selections.get(&verse_key(book.id, chapter, v)))
        .filter(|s| sources.contains_key(s.source_id.as_str()))
        .collect();

    let mut out = vec![Piece::Silence(settings.chapter_lead_in)];
    for (i, sel) in chosen.iter().enumerate() {
        let (start, end) = sel.clip();
        let path = sources[sel.source_id.as_str()];
        let gap = if i == 0 { 0.0 } else { settings.gap_between_verses };
        let silence = gap + sel.pad_before + chosen.get(i.wrapping_sub(1)).map_or(0.0, |p| p.pad_after);
        // A verse that starts where the last one ended, in the same sermon, continues it
        if let (true, Some(Piece::Audio { path: p, end: e, .. })) = (silence <= 0.0, out.last_mut()) {
            if *p == path && (start - *e).abs() < 0.05 {
                *e = end;
                continue;
            }
        }
        if silence > 0.0 {
            out.push(Piece::Silence(silence));
        }
        out.push(Piece::Audio { path, start, end });
    }
    if let Some(last) = chosen.last() {
        out.push(Piece::Silence(last.pad_after));
    }
    out.push(Piece::Silence(settings.chapter_tail));
    out
}

/// A chapter as it will be exported, at `rate`
pub fn render_chapter(project: &Project, book: &BibleBook, chapter: u16, rate: u32) -> Result<Pcm> {
    let settings = &project.settings;
    let mut samples = vec![];
    for piece in pieces(project, book, chapter) {
        match piece {
            Piece::Silence(s) => samples.resize(samples.len() + (s.max(0.0) * rate as f64) as usize, 0.0),
            Piece::Audio { path, start, end } => {
                let clip = render_clip(path, start, end, rate, settings.normalize.then_some(settings.target_db), settings.fade_ms)?;
                samples.extend(clip.samples);
            }
        }
    }
    Ok(Pcm { rate, samples })
}

/// One stretch of a sermon, ready to join others: resampled, leveled, and faded at its ends
pub fn render_clip(path: &str, start: f64, end: f64, rate: u32, target_db: Option<f64>, fade_ms: f64) -> Result<Pcm> {
    let pcm = audio::decode_range(std::path::Path::new(path), start, end)?;
    let mut pcm = audio::resample(&pcm, rate);
    if let Some(db) = target_db {
        audio::normalize(&mut pcm.samples, rate, db);
    }
    audio::fade(&mut pcm.samples, rate, fade_ms);
    Ok(pcm)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterRef {
    pub book: u8,
    pub chapter: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedFile {
    pub book: u8,
    pub chapter: u16,
    pub path: String,
    pub duration: f64,
    pub missing: Vec<u16>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

/// The chapters that would be exported: any with a chosen verse (only full ones, if asked)
pub fn chapters_to_export(project: &Project, bible: &Bible) -> Vec<ChapterRef> {
    let mut out = vec![];
    for book in &bible.books {
        for c in 1..=book.chapters.len() as u16 {
            let any = (1..=book.chapters[c as usize - 1].len() as u16)
                .any(|v| project.selections.contains_key(&verse_key(book.id, c, v)));
            if any && (!project.metadata.only_complete || missing_verses(project, book, c).is_empty()) {
                out.push(ChapterRef { book: book.id, chapter: c });
            }
        }
    }
    out
}

fn fill(template: &str, bible: &Bible, book: &BibleBook, chapter: u16) -> String {
    template
        .replace("{book}", &book.name)
        .replace("{osis}", &book.osis)
        .replace("{bookNumber2}", &format!("{:02}", book.id))
        .replace("{bookNumber}", &book.id.to_string())
        .replace("{chapter3}", &format!("{chapter:03}"))
        .replace("{chapter2}", &format!("{chapter:02}"))
        .replace("{chapter}", &chapter.to_string())
        .replace("{chapters}", &book.chapters.len().to_string())
        .replace("{translation}", &bible.name)
        .replace("{abbreviation}", &bible.abbreviation)
}

fn file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c })
        .collect();
    cleaned.trim().trim_matches('.').to_string()
}

pub fn export(
    project: &Project,
    bible: &Bible,
    chapters: &[ChapterRef],
    mut progress: impl FnMut(ExportProgress),
) -> Result<Vec<ExportedFile>> {
    let meta = &project.metadata;
    let out_dir = PathBuf::from(meta.output_dir.as_deref().filter(|d| !d.is_empty()).ok_or_else(|| AppError::msg("Choose a folder to export to"))?);
    let cover = match meta.cover_path.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => {
            let data = std::fs::read(p).map_err(|e| AppError::msg(format!("Cover image {p}: {e}")))?;
            let mime = if p.to_ascii_lowercase().ends_with(".png") { "image/png" } else { "image/jpeg" };
            Some((data, mime.to_string()))
        }
        None => None,
    };
    let mut done = vec![];
    for (i, ch) in chapters.iter().enumerate() {
        let book = bible.book(ch.book).ok_or_else(|| AppError::msg(format!("No book {}", ch.book)))?;
        progress(ExportProgress { done: i, total: chapters.len(), current: format!("{} {}", book.name, ch.chapter) });
        let pcm = render_chapter(project, book, ch.chapter, meta.sample_rate)?;
        let dir = if meta.folder_per_book {
            out_dir.join(file_name(&format!("{:02} {}", book.id, book.name)))
        } else {
            out_dir.clone()
        };
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.mp3", file_name(&fill(&meta.filename_template, bible, book, ch.chapter))));
        audio::write_mp3(&pcm, meta.bitrate, &path)?;

        let mut tag = Tag::new();
        tag.set_title(fill(&meta.title_template, bible, book, ch.chapter));
        tag.set_album(fill(&meta.album_template, bible, book, ch.chapter));
        let set = |tag: &mut Tag, id: &str, value: &str| {
            if !value.trim().is_empty() {
                tag.set_text(id, fill(value, bible, book, ch.chapter));
            }
        };
        set(&mut tag, "TPE1", &meta.artist);
        set(&mut tag, "TPE2", &meta.album_artist);
        set(&mut tag, "TCOM", &meta.narrator);
        set(&mut tag, "TCON", &meta.genre);
        set(&mut tag, "TCOP", &meta.copyright);
        set(&mut tag, "TPUB", &meta.publisher);
        set(&mut tag, "TLAN", &meta.language);
        if let Ok(year) = meta.year.trim().parse::<i32>() {
            tag.set_year(year);
        }
        if meta.track_numbering == "sequence" {
            tag.set_track(i as u32 + 1);
            tag.set_total_tracks(chapters.len() as u32);
        } else {
            tag.set_track(ch.chapter as u32);
            tag.set_total_tracks(book.chapters.len() as u32);
        }
        tag.set_disc(book.id as u32);
        if !meta.comment.trim().is_empty() {
            tag.add_frame(Comment {
                lang: "eng".into(),
                description: String::new(),
                text: fill(&meta.comment, bible, book, ch.chapter),
            });
        }
        if let Some((data, mime)) = &cover {
            tag.add_frame(Picture {
                mime_type: mime.clone(),
                picture_type: PictureType::CoverFront,
                description: "Cover".into(),
                data: data.clone(),
            });
        }
        tag.write_to_path(&path, Version::Id3v24)?;
        done.push(ExportedFile {
            book: ch.book,
            chapter: ch.chapter,
            path: path.to_string_lossy().into_owned(),
            duration: pcm.seconds(),
            missing: missing_verses(project, book, ch.chapter),
        });
    }
    progress(ExportProgress { done: chapters.len(), total: chapters.len(), current: String::new() });
    Ok(done)
}
