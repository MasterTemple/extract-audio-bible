//! Putting chosen readings together, and writing them as tagged MP3s: a file per chapter, a file
//! per verse, or both.

use std::path::Path;

use id3::{
    Tag, TagLike, Version,
    frame::{Comment, Picture, PictureType},
};
use serde::{Deserialize, Serialize};

use crate::{
    audio::{self, Pcm},
    bible::{Bible, BibleBook},
    error::{Error, Result},
    files::Files,
    project::{ExportMode, Leveling, Project, Selection},
};

pub fn verse_key(book: u8, chapter: u16, verse: u16) -> String {
    format!("{book}.{chapter}.{verse}")
}

/// One stretch of one sermon: a verse, or verses read straight through
#[derive(Clone, Debug)]
pub struct Clip {
    pub path: String,
    pub start: f64,
    pub end: f64,
    /// The volume change chosen for it, in dB
    pub gain_db: f64,
}

/// What a file is made of
pub(crate) enum Piece {
    Silence(f64),
    Audio(Clip),
}

/// The verses of a chapter that have text but no chosen reading
pub fn missing_verses(project: &Project, book: &BibleBook, chapter: u16) -> Vec<u16> {
    let Some(verses) = book.chapters.get(chapter as usize - 1) else {
        return vec![];
    };
    (verses.iter().enumerate())
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(i, _)| i as u16 + 1)
        .filter(|v| !project.selections.contains_key(&verse_key(book.id, chapter, *v)))
        .collect()
}

/// A chapter's chosen verses, in order, with their clips (more than one for a verse read in
/// pieces)
fn chosen<'a>(project: &'a Project, book: &BibleBook, chapter: u16) -> Vec<(u16, &'a Selection, Vec<Clip>)> {
    let count = book.chapters.get(chapter as usize - 1).map_or(0, Vec::len) as u16;
    (1..=count)
        .filter_map(|v| {
            let sel = project.selections.get(&verse_key(book.id, chapter, v))?;
            let clips = clips(project, sel);
            (!clips.is_empty()).then_some((v, sel, clips))
        })
        .collect()
}

/// A verse's clips, in the order they're played (leaving out any whose sermon is gone)
fn clips(project: &Project, sel: &Selection) -> Vec<Clip> {
    (sel.parts().iter())
        .filter_map(|part| {
            let path = project.audio_path(project.sources.iter().find(|s| s.id == part.source_id)?);
            let (start, end) = part.clip();
            Some(Clip {
                path,
                start,
                end,
                gain_db: sel.gain_db,
            })
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
pub(crate) fn chapter_pieces(project: &Project, book: &BibleBook, chapter: u16) -> (Vec<Piece>, Vec<VerseTime>) {
    let settings = &project.settings;
    let verses = chosen(project, book, chapter);
    let mut out = vec![Piece::Silence(settings.chapter_lead_in)];
    let mut times = vec![];
    let mut at = settings.chapter_lead_in.max(0.0);
    for (i, (verse, sel, clips)) in verses.iter().enumerate() {
        let gap = if i == 0 { 0.0 } else { settings.gap_between_verses };
        let after_last = i.checked_sub(1).map_or(0.0, |p| verses[p].1.pad_after);
        let silence = gap + sel.pad_before + after_last;
        if silence > 0.0 {
            out.push(Piece::Silence(silence));
            at += silence;
        }
        let start = at;
        for clip in clips {
            at += (clip.end - clip.start).max(0.0);
            // A clip that starts where the last one ended, in the same sermon at the same volume,
            // continues it
            if let Some(Piece::Audio(last)) = out.last_mut()
                && last.path == clip.path
                && last.gain_db == clip.gain_db
                && (clip.start - last.end).abs() < 0.05
            {
                last.end = clip.end;
                continue;
            }
            out.push(Piece::Audio(clip.clone()));
        }
        times.push(VerseTime {
            verse: *verse,
            start,
            end: at,
        });
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
pub struct Leveler<'f> {
    files: &'f dyn Files,
    leveling: Leveling,
    target_db: f64,
    fade_ms: f64,
    /// The volume change for everything (the project's, and the whole export's leveling)
    overall: f64,
}

impl<'f> Leveler<'f> {
    /// For a project, measuring `clips` when the whole export is leveled together
    pub fn new(project: &Project, clips: &[Clip], files: &'f dyn Files) -> Result<Self> {
        let s = &project.settings;
        let mut overall = audio::db_to_gain(s.output_gain_db);
        if s.leveling == Leveling::Export {
            overall *= export_gain(files, clips, s.target_db)?;
        }
        Ok(Self {
            files,
            leveling: s.leveling,
            target_db: s.target_db,
            fade_ms: s.fade_ms,
            overall,
        })
    }

    /// One clip, ready to join others: resampled, its volume set, and faded at its ends
    pub fn render(&self, clip: &Clip, rate: u32) -> Result<Pcm> {
        if clip.path.is_empty() {
            return Err(Error::not_found(
                "a chosen reading's sermon has no audio: add its audio file, or say where to download audio from",
            ));
        }
        let pcm = audio::decode_range(self.files, &clip.path, clip.start, clip.end)?;
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

/// How to render a stretch of a sermon to listen to ([`render_clip`])
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipOptions {
    /// Silence before and after it, in seconds
    pub pad_before: f64,
    pub pad_after: f64,
    /// Level and fade it as an export will (at 44.1 kHz)
    pub processed: bool,
    /// When processed, bring it to this level first, as leveling each reading does (dBFS RMS)
    pub target_db: Option<f64>,
    /// When processed, a volume change in dB
    pub gain_db: f64,
}

/// `start` to `end` seconds of a sermon's audio, mono, at its own rate unless processed
pub fn render_clip(files: &dyn Files, path: &str, start: f64, end: f64, options: &ClipOptions) -> Result<Pcm> {
    let mut pcm = audio::decode_range(files, path, start, end)?;
    if options.processed {
        pcm = audio::resample(&pcm, 44_100);
        let mut gain = audio::db_to_gain(options.gain_db);
        if let Some(db) = options.target_db {
            gain *= audio::leveling_gain(&pcm.samples, pcm.rate, db);
        }
        audio::apply_gain(&mut pcm.samples, gain);
        audio::fade(&mut pcm.samples, pcm.rate, 8.0);
        audio::limit(&mut pcm.samples);
    }
    let rate = pcm.rate as f64;
    let mut samples = vec![0.0; (options.pad_before.max(0.0) * rate) as usize];
    samples.extend(pcm.samples);
    samples.resize(samples.len() + (options.pad_after.max(0.0) * rate) as usize, 0.0);
    Ok(Pcm {
        rate: pcm.rate,
        samples,
    })
}

/// The one gain that brings all of `clips` (each with its own volume change) to the target
fn export_gain(files: &dyn Files, clips: &[Clip], target_db: f64) -> Result<f64> {
    let mut levels = vec![];
    for clip in clips {
        let pcm = audio::decode_range(files, &clip.path, clip.start, clip.end)?;
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
pub fn render_chapter(project: &Project, book: &BibleBook, chapter: u16, rate: u32, files: &dyn Files) -> Result<Pcm> {
    let clips: Vec<Clip> = chosen(project, book, chapter)
        .into_iter()
        .flat_map(|(_, _, c)| c)
        .collect();
    Leveler::new(project, &clips, files)?.render_pieces(&chapter_pieces(project, book, chapter).0, rate)
}

/// A verse's pieces: the silence before it, its readings, and the silence after
fn verse_pieces(sel: &Selection, clips: &[Clip]) -> Vec<Piece> {
    let mut pieces = vec![Piece::Silence(sel.pad_before)];
    pieces.extend(clips.iter().map(|c| Piece::Audio(c.clone())));
    pieces.push(Piece::Silence(sel.pad_after));
    pieces
}

/**
A verse as it will be exported in a file of its own, at `rate`: its readings one after another,
with the silence chosen around it (leveled on its own, when the export is leveled as a whole).
None if no reading is chosen for it
*/
pub fn render_verse(project: &Project, key: &str, rate: u32, files: &dyn Files) -> Result<Option<Pcm>> {
    let Some(sel) = project.selections.get(key) else {
        return Ok(None);
    };
    let clips = clips(project, sel);
    if clips.is_empty() {
        return Ok(None);
    }
    Leveler::new(project, &clips, files)?
        .render_pieces(&verse_pieces(sel, &clips), rate)
        .map(Some)
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
    /// Where it goes, under the folder exported to (`13 1 Thessalonians/52_1Thess_005.mp3`), or
    /// the whole path once written there
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
    for book in bible
        .books
        .iter()
        .filter(|b| project.book.is_none_or(|only| only == b.id))
    {
        for c in 1..=book.chapters.len() as u16 {
            let any = (1..=book.chapters[c as usize - 1].len() as u16)
                .any(|v| project.selections.contains_key(&verse_key(book.id, c, v)));
            if any && (!project.metadata.only_complete || missing_verses(project, book, c).is_empty()) {
                out.push(ChapterRef {
                    book: book.id,
                    chapter: c,
                });
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
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned.trim().trim_matches('.').to_string()
}

struct Tagger<'a> {
    project: &'a Project,
    bible: &'a Bible,
    cover: Option<(Vec<u8>, String)>,
}

impl Tagger<'_> {
    /// An MP3's ID3v2.4 tag, to put before its audio
    fn tag(&self, book: &BibleBook, chapter: u16, verse: Option<u16>, track: (u32, u32), disc: u32) -> Result<Vec<u8>> {
        let meta = &self.project.metadata;
        let fill = |t: &str| fill(t, self.bible, book, chapter, verse);
        let mut tag = Tag::new();
        tag.set_title(fill(if verse.is_some() {
            &meta.verse_title_template
        } else {
            &meta.title_template
        }));
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
            tag.add_frame(Comment {
                lang: "eng".into(),
                description: String::new(),
                text: fill(&meta.comment),
            });
        }
        if let Some((data, mime)) = &self.cover {
            tag.add_frame(Picture {
                mime_type: mime.clone(),
                picture_type: PictureType::CoverFront,
                description: "Cover".into(),
                data: data.clone(),
            });
        }
        let mut bytes = vec![];
        tag.write_to(&mut bytes, Version::Id3v24)?;
        Ok(bytes)
    }
}

/**
Exports chapters (see [`chapters_to_export`]) as the project's metadata says: a tagged MP3 per
chapter, per verse, or both. Each file is handed to `write` as it's made, with its path under
the folder exported to, so nothing has to be kept in memory; it returns what was written.
Natively, [`export_to_dir`] writes them to a folder
*/
pub fn export(
    project: &Project,
    bible: &Bible,
    chapters: &[ChapterRef],
    files: &dyn Files,
    mut progress: impl FnMut(ExportProgress),
    mut write: impl FnMut(&ExportedFile, Vec<u8>) -> Result<()>,
) -> Result<Vec<ExportedFile>> {
    let meta = &project.metadata;
    let cover = match meta.cover_path.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => {
            let data = files
                .read(p)
                .map_err(|e| Error::not_found(format!("Cover image {p}: {e}")))?;
            let mime = if p.to_ascii_lowercase().ends_with(".png") {
                "image/png"
            } else {
                "image/jpeg"
            };
            Some((data, mime.to_string()))
        }
        None => None,
    };
    let tagger = Tagger { project, bible, cover };
    let books: Vec<&BibleBook> = (chapters.iter())
        .map(|c| {
            bible
                .book(c.book)
                .ok_or_else(|| Error::not_found(format!("No book {}", c.book)))
        })
        .collect::<Result<_>>()?;

    let clips: Vec<Clip> = (chapters.iter().zip(&books))
        .flat_map(|(c, b)| {
            chosen(project, b, c.chapter)
                .into_iter()
                .flat_map(|(_, _, clips)| clips)
        })
        .collect();
    if project.settings.leveling == Leveling::Export {
        progress(ExportProgress {
            done: 0,
            total: chapters.len(),
            current: "Measuring loudness".into(),
        });
    }
    let leveler = Leveler::new(project, &clips, files)?;
    let (chapter_files, verse_files) = match meta.export_mode {
        ExportMode::Chapter => (true, false),
        ExportMode::Verse => (false, true),
        ExportMode::Both => (true, true),
    };
    let join = |dir: &str, name: String| if dir.is_empty() { name } else { format!("{dir}/{name}") };

    let mut done = vec![];
    for (i, (ch, book)) in chapters.iter().zip(&books).enumerate() {
        progress(ExportProgress {
            done: i,
            total: chapters.len(),
            current: format!("{} {}", book.name, ch.chapter),
        });
        let book_dir = if meta.folder_per_book {
            file_name(&format!("{:02} {}", book.id, book.name))
        } else {
            String::new()
        };
        let missing = missing_verses(project, book, ch.chapter);
        if chapter_files {
            let pcm = leveler.render_pieces(&chapter_pieces(project, book, ch.chapter).0, meta.sample_rate)?;
            let path = join(
                &book_dir,
                format!(
                    "{}.mp3",
                    file_name(&fill(&meta.filename_template, bible, book, ch.chapter, None))
                ),
            );
            let track = if meta.track_numbering == "sequence" {
                (i as u32 + 1, chapters.len() as u32)
            } else {
                (ch.chapter as u32, book.chapters.len() as u32)
            };
            let mut bytes = tagger.tag(book, ch.chapter, None, track, book.id as u32)?;
            bytes.extend(audio::mp3(&pcm, meta.bitrate)?);
            let file = ExportedFile {
                book: ch.book,
                chapter: ch.chapter,
                verse: None,
                path,
                duration: pcm.seconds(),
                missing: missing.clone(),
            };
            write(&file, bytes)?;
            done.push(file);
        }
        if verse_files {
            let dir = join(&book_dir, file_name(&format!("{} {}", book.name, ch.chapter)));
            let count = book.chapters[ch.chapter as usize - 1].len() as u32;
            for (v, sel, clips) in chosen(project, book, ch.chapter) {
                let pcm = leveler.render_pieces(&verse_pieces(sel, &clips), meta.sample_rate)?;
                let path = join(
                    &dir,
                    format!(
                        "{}.mp3",
                        file_name(&fill(&meta.verse_filename_template, bible, book, ch.chapter, Some(v)))
                    ),
                );
                let mut bytes = tagger.tag(book, ch.chapter, Some(v), (v as u32, count), ch.chapter as u32)?;
                bytes.extend(audio::mp3(&pcm, meta.bitrate)?);
                let file = ExportedFile {
                    book: ch.book,
                    chapter: ch.chapter,
                    verse: Some(v),
                    path,
                    duration: pcm.seconds(),
                    missing: vec![],
                };
                write(&file, bytes)?;
                done.push(file);
            }
        }
    }
    progress(ExportProgress {
        done: chapters.len(),
        total: chapters.len(),
        current: String::new(),
    });
    Ok(done)
}

/// [`export`] into a folder on disk (made if need be); the files' paths are where they were written
pub fn export_to_dir(
    project: &Project,
    bible: &Bible,
    chapters: &[ChapterRef],
    files: &dyn Files,
    dir: &Path,
    progress: impl FnMut(ExportProgress),
) -> Result<Vec<ExportedFile>> {
    let mut written = export(project, bible, chapters, files, progress, |file, bytes| {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
        Ok(())
    })?;
    for file in &mut written {
        file.path = dir.join(&file.path).to_string_lossy().into_owned();
    }
    Ok(written)
}
