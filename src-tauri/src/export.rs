//! Putting chosen readings together, and writing them as tagged MP3s: a file per chapter, a file
//! per verse, or both.

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
    project::{ExportMode, Leveling, Project, Selection},
};

pub fn verse_key(book: u8, chapter: u16, verse: u16) -> String {
    format!("{book}.{chapter}.{verse}")
}

/// One stretch of one sermon: a verse, or verses read straight through
#[derive(Clone, Copy, Debug)]
pub struct Clip<'a> {
    pub path: &'a str,
    pub start: f64,
    pub end: f64,
    /// The volume change chosen for it, in dB
    pub gain_db: f64,
}

/// What a file is made of
enum Piece<'a> {
    Silence(f64),
    Audio(Clip<'a>),
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

/// A chapter's chosen verses, in order, with their clips
fn chosen<'a>(project: &'a Project, book: &BibleBook, chapter: u16) -> Vec<(u16, &'a Selection, Clip<'a>)> {
    let sources: HashMap<&str, &str> =
        project.sources.iter().map(|s| (s.id.as_str(), s.audio_path.as_str())).collect();
    let count = book.chapters.get(chapter as usize - 1).map_or(0, Vec::len) as u16;
    (1..=count)
        .filter_map(|v| {
            let sel = project.selections.get(&verse_key(book.id, chapter, v))?;
            let path = sources.get(sel.source_id.as_str())?;
            let (start, end) = sel.clip();
            Some((v, sel, Clip { path, start, end, gain_db: sel.gain_db }))
        })
        .collect()
}

/// Where a verse is heard in a chapter's audio, in seconds
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerseTime {
    pub verse: u16,
    pub start: f64,
    pub end: f64,
}

/// A chapter's audio as pieces, and when each verse is heard in it
fn chapter_pieces<'a>(project: &'a Project, book: &BibleBook, chapter: u16) -> (Vec<Piece<'a>>, Vec<VerseTime>) {
    let settings = &project.settings;
    let verses = chosen(project, book, chapter);
    let mut out = vec![Piece::Silence(settings.chapter_lead_in)];
    let mut times = vec![];
    let mut at = settings.chapter_lead_in.max(0.0);
    for (i, (verse, sel, clip)) in verses.iter().enumerate() {
        let gap = if i == 0 { 0.0 } else { settings.gap_between_verses };
        let after_last = i.checked_sub(1).map_or(0.0, |p| verses[p].1.pad_after);
        let silence = gap + sel.pad_before + after_last;
        let length = (clip.end - clip.start).max(0.0);
        // A verse that starts where the last one ended, in the same sermon at the same volume,
        // continues it
        if let (true, Some(Piece::Audio(last))) = (silence <= 0.0, out.last_mut()) {
            if last.path == clip.path && last.gain_db == clip.gain_db && (clip.start - last.end).abs() < 0.05 {
                last.end = clip.end;
                times.push(VerseTime { verse: *verse, start: at, end: at + length });
                at += length;
                continue;
            }
        }
        if silence > 0.0 {
            out.push(Piece::Silence(silence));
            at += silence;
        }
        out.push(Piece::Audio(*clip));
        times.push(VerseTime { verse: *verse, start: at, end: at + length });
        at += length;
    }
    if let Some((_, last, _)) = verses.last() {
        out.push(Piece::Silence(last.pad_after));
    }
    out.push(Piece::Silence(settings.chapter_tail));
    (out, times)
}

/// When each chosen verse is heard in a chapter's audio
pub fn chapter_timeline(project: &Project, book: &BibleBook, chapter: u16) -> Vec<VerseTime> {
    chapter_pieces(project, book, chapter).1
}

/// How each clip's volume is set
pub struct Leveler {
    leveling: Leveling,
    target_db: f64,
    fade_ms: f64,
    /// The volume change for everything (the project's, and the whole export's leveling)
    overall: f64,
}

impl Leveler {
    /// For a project, measuring `clips` when the whole export is leveled together
    pub fn new(project: &Project, clips: &[Clip]) -> Result<Self> {
        let s = &project.settings;
        let mut overall = audio::db_to_gain(s.output_gain_db);
        if s.leveling == Leveling::Export {
            overall *= export_gain(clips, s.target_db)?;
        }
        Ok(Self { leveling: s.leveling, target_db: s.target_db, fade_ms: s.fade_ms, overall })
    }

    /// One clip, ready to join others: resampled, its volume set, and faded at its ends
    pub fn render(&self, clip: &Clip, rate: u32) -> Result<Pcm> {
        let pcm = audio::decode_range(std::path::Path::new(clip.path), clip.start, clip.end)?;
        let mut pcm = audio::resample(&pcm, rate);
        let mut gain = audio::db_to_gain(clip.gain_db) * self.overall;
        if self.leveling == Leveling::Each {
            gain *= audio::leveling_gain(&pcm.samples, rate, self.target_db);
        }
        audio::apply_gain(&mut pcm.samples, gain);
        audio::fade(&mut pcm.samples, rate, self.fade_ms);
        Ok(pcm)
    }

    fn render_pieces(&self, pieces: &[Piece], rate: u32) -> Result<Pcm> {
        let mut samples = vec![];
        for piece in pieces {
            match piece {
                Piece::Silence(s) => samples.resize(samples.len() + (s.max(0.0) * rate as f64) as usize, 0.0),
                Piece::Audio(clip) => samples.extend(self.render(clip, rate)?.samples),
            }
        }
        audio::limit(&mut samples);
        Ok(Pcm { rate, samples })
    }
}

/// The one gain that brings all of `clips` (each with its own volume change) to the target
fn export_gain(clips: &[Clip], target_db: f64) -> Result<f64> {
    let mut levels = vec![];
    for clip in clips {
        let pcm = audio::decode_range(std::path::Path::new(clip.path), clip.start, clip.end)?;
        let gain = audio::db_to_gain(clip.gain_db);
        levels.extend(audio::levels(&pcm.samples, pcm.rate).into_iter().map(|l| l * gain));
    }
    Ok(match audio::speech_rms(levels) {
        Some(rms) => (audio::db_to_gain(target_db) / rms).clamp(0.1, 10.0),
        None => 1.0,
    })
}

/// A chapter as it will be exported, at `rate` (leveled as a whole on its own, when the export
/// is leveled as a whole)
pub fn render_chapter(project: &Project, book: &BibleBook, chapter: u16, rate: u32) -> Result<Pcm> {
    let clips: Vec<Clip> = chosen(project, book, chapter).into_iter().map(|(_, _, c)| c).collect();
    Leveler::new(project, &clips)?.render_pieces(&chapter_pieces(project, book, chapter).0, rate)
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
    /// For a verse's own file
    pub verse: Option<u16>,
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

/// The chapters that would be exported: those of the project's book with a chosen verse (only
/// full ones, if asked)
pub fn chapters_to_export(project: &Project, bible: &Bible) -> Vec<ChapterRef> {
    let mut out = vec![];
    for book in bible.books.iter().filter(|b| project.book.is_none_or(|only| only == b.id)) {
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

fn fill(template: &str, bible: &Bible, book: &BibleBook, chapter: u16, verse: Option<u16>) -> String {
    let verse_count = book.chapters.get(chapter as usize - 1).map_or(0, Vec::len);
    let v = |f: &dyn Fn(u16) -> String| verse.map(f).unwrap_or_default();
    template
        .replace("{book}", &book.name)
        .replace("{osis}", &book.osis)
        .replace("{bookNumber2}", &format!("{:02}", book.id))
        .replace("{bookNumber}", &book.id.to_string())
        .replace("{chapter3}", &format!("{chapter:03}"))
        .replace("{chapter2}", &format!("{chapter:02}"))
        .replace("{chapter}", &chapter.to_string())
        .replace("{chapters}", &book.chapters.len().to_string())
        .replace("{verse3}", &v(&|n| format!("{n:03}")))
        .replace("{verse2}", &v(&|n| format!("{n:02}")))
        .replace("{verses}", &verse_count.to_string())
        .replace("{verse}", &v(&|n| n.to_string()))
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

struct Tagger<'a> {
    project: &'a Project,
    bible: &'a Bible,
    cover: Option<(Vec<u8>, String)>,
}

impl Tagger<'_> {
    fn write(&self, path: &PathBuf, book: &BibleBook, chapter: u16, verse: Option<u16>, track: (u32, u32), disc: u32) -> Result<()> {
        let meta = &self.project.metadata;
        let fill = |t: &str| fill(t, self.bible, book, chapter, verse);
        let mut tag = Tag::new();
        tag.set_title(fill(if verse.is_some() { &meta.verse_title_template } else { &meta.title_template }));
        tag.set_album(fill(&meta.album_template));
        for (id, value) in [
            ("TPE1", &meta.artist),
            ("TPE2", &meta.album_artist),
            ("TCOM", &meta.narrator),
            ("TCON", &meta.genre),
            ("TCOP", &meta.copyright),
            ("TPUB", &meta.publisher),
            ("TLAN", &meta.language),
        ] {
            if !value.trim().is_empty() {
                tag.set_text(id, fill(value));
            }
        }
        if let Ok(year) = meta.year.trim().parse::<i32>() {
            tag.set_year(year);
        }
        tag.set_track(track.0);
        tag.set_total_tracks(track.1);
        tag.set_disc(disc);
        if !meta.comment.trim().is_empty() {
            tag.add_frame(Comment { lang: "eng".into(), description: String::new(), text: fill(&meta.comment) });
        }
        if let Some((data, mime)) = &self.cover {
            tag.add_frame(Picture {
                mime_type: mime.clone(),
                picture_type: PictureType::CoverFront,
                description: "Cover".into(),
                data: data.clone(),
            });
        }
        tag.write_to_path(path, Version::Id3v24)?;
        Ok(())
    }
}

pub fn export(
    project: &Project,
    bible: &Bible,
    chapters: &[ChapterRef],
    mut progress: impl FnMut(ExportProgress),
) -> Result<Vec<ExportedFile>> {
    let meta = &project.metadata;
    let out_dir = PathBuf::from(
        meta.output_dir.as_deref().filter(|d| !d.is_empty()).ok_or_else(|| AppError::msg("Choose a folder to export to"))?,
    );
    let cover = match meta.cover_path.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => {
            let data = std::fs::read(p).map_err(|e| AppError::msg(format!("Cover image {p}: {e}")))?;
            let mime = if p.to_ascii_lowercase().ends_with(".png") { "image/png" } else { "image/jpeg" };
            Some((data, mime.to_string()))
        }
        None => None,
    };
    let tagger = Tagger { project, bible, cover };
    let books: Vec<&BibleBook> = (chapters.iter())
        .map(|c| bible.book(c.book).ok_or_else(|| AppError::msg(format!("No book {}", c.book))))
        .collect::<Result<_>>()?;

    let clips: Vec<Clip> = (chapters.iter().zip(&books))
        .flat_map(|(c, b)| chosen(project, b, c.chapter).into_iter().map(|(_, _, clip)| clip))
        .collect();
    if project.settings.leveling == Leveling::Export {
        progress(ExportProgress { done: 0, total: chapters.len(), current: "Measuring loudness".into() });
    }
    let leveler = Leveler::new(project, &clips)?;
    let (chapter_files, verse_files) = match meta.export_mode {
        ExportMode::Chapter => (true, false),
        ExportMode::Verse => (false, true),
        ExportMode::Both => (true, true),
    };

    let mut done = vec![];
    for (i, (ch, book)) in chapters.iter().zip(&books).enumerate() {
        progress(ExportProgress { done: i, total: chapters.len(), current: format!("{} {}", book.name, ch.chapter) });
        let book_dir = if meta.folder_per_book {
            out_dir.join(file_name(&format!("{:02} {}", book.id, book.name)))
        } else {
            out_dir.clone()
        };
        let missing = missing_verses(project, book, ch.chapter);
        if chapter_files {
            let pcm = leveler.render_pieces(&chapter_pieces(project, book, ch.chapter).0, meta.sample_rate)?;
            std::fs::create_dir_all(&book_dir)?;
            let path = book_dir.join(format!("{}.mp3", file_name(&fill(&meta.filename_template, bible, book, ch.chapter, None))));
            audio::write_mp3(&pcm, meta.bitrate, &path)?;
            let track = if meta.track_numbering == "sequence" {
                (i as u32 + 1, chapters.len() as u32)
            } else {
                (ch.chapter as u32, book.chapters.len() as u32)
            };
            tagger.write(&path, book, ch.chapter, None, track, book.id as u32)?;
            done.push(ExportedFile {
                book: ch.book,
                chapter: ch.chapter,
                verse: None,
                path: path.to_string_lossy().into_owned(),
                duration: pcm.seconds(),
                missing: missing.clone(),
            });
        }
        if verse_files {
            let dir = book_dir.join(file_name(&format!("{} {}", book.name, ch.chapter)));
            std::fs::create_dir_all(&dir)?;
            let count = book.chapters[ch.chapter as usize - 1].len() as u32;
            for (v, sel, clip) in chosen(project, book, ch.chapter) {
                let pieces = [Piece::Silence(sel.pad_before), Piece::Audio(clip), Piece::Silence(sel.pad_after)];
                let pcm = leveler.render_pieces(&pieces, meta.sample_rate)?;
                let path = dir.join(format!(
                    "{}.mp3",
                    file_name(&fill(&meta.verse_filename_template, bible, book, ch.chapter, Some(v)))
                ));
                audio::write_mp3(&pcm, meta.bitrate, &path)?;
                tagger.write(&path, book, ch.chapter, Some(v), (v as u32, count), ch.chapter as u32)?;
                done.push(ExportedFile {
                    book: ch.book,
                    chapter: ch.chapter,
                    verse: Some(v),
                    path: path.to_string_lossy().into_owned(),
                    duration: pcm.seconds(),
                    missing: vec![],
                });
            }
        }
    }
    progress(ExportProgress { done: chapters.len(), total: chapters.len(), current: String::new() });
    Ok(done)
}
