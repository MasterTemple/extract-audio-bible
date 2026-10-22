//! Putting chosen readings together, and writing them as tagged MP3s: a file per chapter (or, for
//! a passage, one for it all), a file per verse, or both.

use std::{cell::OnceCell, collections::BTreeMap, hash::Hasher, path::Path};

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
    project::{Cut, ExportMode, Leveling, Project, Selection},
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
#[derive(Debug)]
pub(crate) enum Piece {
    Silence(f64),
    Audio(Clip),
}

/// The verses of a chapter that have text but no chosen reading (of the project's passage, if it has one)
pub fn missing_verses(project: &Project, book: &BibleBook, chapter: u16) -> Vec<u16> {
    let Some(verses) = book.chapters.get(chapter as usize - 1) else {
        return vec![];
    };
    (verses.iter().enumerate())
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(i, _)| i as u16 + 1)
        .filter(|v| project.covers(book.id, chapter, *v))
        .filter(|v| !project.selections.contains_key(&verse_key(book.id, chapter, *v)))
        .collect()
}

/// A chapter's chosen verses (of the project's passage, if it has one), in order, with their clips
/// (more than one for a verse read in pieces)
fn chosen<'a>(project: &'a Project, book: &BibleBook, chapter: u16) -> Vec<(u16, &'a Selection, Vec<Clip>)> {
    let count = book.chapters.get(chapter as usize - 1).map_or(0, Vec::len) as u16;
    (1..=count)
        .filter(|&v| project.covers(book.id, chapter, v))
        .filter_map(|v| {
            let sel = project.selections.get(&verse_key(book.id, chapter, v))?;
            let clips = clips(project, sel);
            (!clips.is_empty()).then_some((v, sel, clips))
        })
        .collect()
}

/// A verse's clips, in the order they're played (leaving out any whose sermon is gone): a clip for
/// each reading, or for each piece of one with stretches cut out of it
fn clips(project: &Project, sel: &Selection) -> Vec<Clip> {
    (sel.parts().iter())
        .filter_map(|part| {
            let path = project.audio_path(project.sources.iter().find(|s| s.id == part.source_id)?);
            Some((part.spans().into_iter()).map(move |(start, end)| Clip {
                path: path.clone(),
                start,
                end,
                gain_db: sel.gain_db,
            }))
        })
        .flatten()
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
    pieces(project, chosen(project, book, chapter))
}

/// A passage's audio as pieces: its chapters' chosen verses one after another, with one lead-in and tail
fn passage_pieces(project: &Project, chapters: &[(&ChapterRef, &BibleBook)]) -> Vec<Piece> {
    let verses = (chapters.iter())
        .flat_map(|(c, book)| chosen(project, book, c.chapter))
        .collect();
    pieces(project, verses).0
}

/// Chosen verses' audio as pieces (with the silences between them), and when each is heard
fn pieces(project: &Project, verses: Vec<(u16, &Selection, Vec<Clip>)>) -> (Vec<Piece>, Vec<VerseTime>) {
    let settings = &project.settings;
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
    /// Stretches left out (cut out of a reading), in the sermon's seconds; each join is faded
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<Cut>,
}

/// `start` to `end` seconds of a sermon's audio (without its cuts), mono, at its own rate unless processed
pub fn render_clip(files: &dyn Files, path: &str, start: f64, end: f64, options: &ClipOptions) -> Result<Pcm> {
    let spans = crate::Part {
        match_id: String::new(),
        source_id: String::new(),
        start,
        end,
        trim_start: 0.0,
        trim_end: 0.0,
        cuts: options.cuts.clone(),
    }
    .spans();
    let mut pcm = audio::decode_range(files, path, spans[0].0, spans[0].1)?;
    for &(from, to) in &spans[1..] {
        let mut next = audio::decode_range(files, path, from, to)?;
        if next.rate != pcm.rate {
            next = audio::resample(&next, pcm.rate);
        }
        audio::fade(&mut pcm.samples, pcm.rate, 8.0);
        audio::fade(&mut next.samples, next.rate, 8.0);
        pcm.samples.extend(next.samples);
    }
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
    /// What it's made from, as a short hash: the same when it would come out the same
    #[serde(default)]
    pub fingerprint: String,
    /// Not made again, since the file there was made from the same ([`export_with`])
    #[serde(default)]
    pub unchanged: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

/// The chapters that would be exported: those of the project's book (or passage) with a chosen
/// verse (only full ones, if asked)
pub fn chapters_to_export(project: &Project, bible: &Bible) -> Vec<ChapterRef> {
    let mut out = vec![];
    for book in bible
        .books
        .iter()
        .filter(|b| project.book.is_none_or(|only| only == b.id))
    {
        for c in 1..=book.chapters.len() as u16 {
            let any = (1..=book.chapters[c as usize - 1].len() as u16)
                .filter(|&v| project.covers(book.id, c, v))
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

/// A template filled in: `{passage}` is the project's passage ("2 Corinthians 2:14-17"), or the
/// book and chapter when it has none
fn fill(
    template: &str,
    project: &Project,
    bible: &Bible,
    book: &BibleBook,
    chapter: u16,
    verse: Option<u16>,
) -> String {
    let verse_count = book.chapters.get(chapter as usize - 1).map_or(0, Vec::len);
    let v = |f: &dyn Fn(u16) -> String| verse.map(f).unwrap_or_default();
    let passage = match project.passage {
        Some(p) => format!("{} {}", book.name, p.label()),
        None => format!("{} {chapter}", book.name),
    };
    template
        .replace("{passage}", &passage)
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
        let fill = |t: &str| fill(t, self.project, self.bible, book, chapter, verse);
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

/// Bumped when what's made from the same pieces changes, so files made before are made again
const FORMAT: u32 = 1;

/// FNV-1a, 64 bits: the same everywhere, in every version (unlike the standard library's hashers)
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn of(parts: &[&[u8]]) -> String {
        let mut h = Fnv::new();
        for part in parts {
            h.write(&(part.len() as u64).to_le_bytes());
            h.write(part);
        }
        format!("{:016x}", h.finish())
    }
}

/// How long pieces play, in seconds
fn seconds(pieces: &[Piece]) -> f64 {
    (pieces.iter())
        .map(|p| match p {
            Piece::Silence(s) => s.max(0.0),
            Piece::Audio(c) => (c.end - c.start).max(0.0),
        })
        .sum()
}

/// The leveler, made (measuring the whole export, if it's leveled as a whole) the first time it's needed
fn leveler<'c, 'f>(
    cell: &'c OnceCell<Leveler<'f>>,
    project: &Project,
    clips: &[Clip],
    files: &'f dyn Files,
    progress: &mut dyn FnMut(ExportProgress),
    done: usize,
    total: usize,
) -> Result<&'c Leveler<'f>> {
    if cell.get().is_none() {
        if project.settings.leveling == Leveling::Export {
            progress(ExportProgress {
                done,
                total,
                current: "Measuring loudness".into(),
            });
        }
        let _ = cell.set(Leveler::new(project, clips, files)?);
    }
    Ok(cell.get().expect("just set"))
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
    progress: impl FnMut(ExportProgress),
    write: impl FnMut(&ExportedFile, Vec<u8>) -> Result<()>,
) -> Result<Vec<ExportedFile>> {
    export_with(project, bible, chapters, files, &|_, _| false, progress, write)
}

/**
[`export`], leaving out the files `unchanged` says are there already as they'd be made (given
each file's path and [`ExportedFile::fingerprint`]): they aren't made or handed to `write`, but
are returned, [`ExportedFile::unchanged`]. A file's fingerprint is of everything it's made from
(its readings and where they're cut, the silence, the sound settings, its tags, and the MP3's
rate), though not of the sermons' audio itself. When the export is leveled as a whole, its
loudness is measured only if a file is made
*/
#[allow(clippy::too_many_arguments)]
pub fn export_with(
    project: &Project,
    bible: &Bible,
    chapters: &[ChapterRef],
    files: &dyn Files,
    unchanged: &dyn Fn(&str, &str) -> bool,
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
    let levelers = OnceCell::new();
    // How every file's sound is made, beyond its own pieces: leveled as a whole, that's every
    // clip exported too
    let s = &project.settings;
    let sound = format!(
        "{FORMAT} {:?} {} {} {} {} {}",
        s.leveling, s.target_db, s.output_gain_db, s.fade_ms, meta.sample_rate, meta.bitrate,
    );
    let sound = match s.leveling {
        Leveling::Export => format!("{sound} {}", Fnv::of(&[format!("{clips:?}").as_bytes()])),
        _ => sound,
    };
    let (chapter_files, verse_files) = match meta.export_mode {
        ExportMode::Chapter => (true, false),
        ExportMode::Verse => (false, true),
        ExportMode::Both => (true, true),
    };
    let join = |dir: &str, name: String| if dir.is_empty() { name } else { format!("{dir}/{name}") };
    let book_dir = |book: &BibleBook| {
        if meta.folder_per_book {
            file_name(&format!("{:02} {}", book.id, book.name))
        } else {
            String::new()
        }
    };
    // A passage is one file, not a file per chapter, and its verses an album numbered in its order
    let passage = project.passage.is_some();
    let passage_verses: u32 = (chapters.iter().zip(&books))
        .map(|(c, b)| chosen(project, b, c.chapter).len() as u32)
        .sum();
    let mut passage_track = 0;

    // Each file: left as it is if it's unchanged, or made and written
    let mut make = |mut file: ExportedFile,
                    pieces: &[Piece],
                    tag: Vec<u8>,
                    done: usize,
                    progress: &mut dyn FnMut(ExportProgress)|
     -> Result<ExportedFile> {
        file.fingerprint = Fnv::of(&[sound.as_bytes(), &tag, format!("{pieces:?}").as_bytes()]);
        if unchanged(&file.path, &file.fingerprint) {
            file.duration = seconds(pieces);
            file.unchanged = true;
            return Ok(file);
        }
        let leveler = leveler(&levelers, project, &clips, files, progress, done, chapters.len())?;
        let pcm = leveler.render_pieces(pieces, meta.sample_rate)?;
        file.duration = pcm.seconds();
        let mut bytes = tag;
        bytes.extend(audio::mp3(&pcm, meta.bitrate)?);
        write(&file, bytes)?;
        Ok(file)
    };

    let mut done = vec![];
    if passage
        && chapter_files
        && let (Some(first), Some(book)) = (chapters.first(), books.first())
    {
        progress(ExportProgress {
            done: 0,
            total: chapters.len(),
            current: project.reference(bible).unwrap_or_default(),
        });
        let parts: Vec<(&ChapterRef, &BibleBook)> = chapters.iter().zip(books.iter().copied()).collect();
        let pieces = passage_pieces(project, &parts);
        let path = join(
            &book_dir(book),
            format!(
                "{}.mp3",
                file_name(&fill(
                    &meta.filename_template,
                    project,
                    bible,
                    book,
                    first.chapter,
                    None
                ))
            ),
        );
        let file = ExportedFile {
            book: first.book,
            chapter: first.chapter,
            verse: None,
            path,
            duration: 0.0,
            missing: (parts.iter())
                .flat_map(|(c, b)| missing_verses(project, b, c.chapter))
                .collect(),
            fingerprint: String::new(),
            unchanged: false,
        };
        let tag = tagger.tag(book, first.chapter, None, (1, 1), book.id as u32)?;
        done.push(make(file, &pieces, tag, 0, &mut progress)?);
    }
    for (i, (ch, book)) in chapters.iter().zip(&books).enumerate() {
        progress(ExportProgress {
            done: i,
            total: chapters.len(),
            current: format!("{} {}", book.name, ch.chapter),
        });
        let book_dir = book_dir(book);
        let missing = missing_verses(project, book, ch.chapter);
        if chapter_files && !passage {
            let pieces = chapter_pieces(project, book, ch.chapter).0;
            let path = join(
                &book_dir,
                format!(
                    "{}.mp3",
                    file_name(&fill(&meta.filename_template, project, bible, book, ch.chapter, None))
                ),
            );
            let track = if meta.track_numbering == "sequence" {
                (i as u32 + 1, chapters.len() as u32)
            } else {
                (ch.chapter as u32, book.chapters.len() as u32)
            };
            let tag = tagger.tag(book, ch.chapter, None, track, book.id as u32)?;
            let file = ExportedFile {
                book: ch.book,
                chapter: ch.chapter,
                verse: None,
                path,
                duration: 0.0,
                missing: missing.clone(),
                fingerprint: String::new(),
                unchanged: false,
            };
            done.push(make(file, &pieces, tag, i, &mut progress)?);
        }
        if verse_files {
            let dir = join(
                &book_dir,
                file_name(&match project.reference(bible).filter(|_| passage) {
                    Some(reference) => reference,
                    None => format!("{} {}", book.name, ch.chapter),
                }),
            );
            let count = book.chapters[ch.chapter as usize - 1].len() as u32;
            for (v, sel, clips) in chosen(project, book, ch.chapter) {
                let pieces = verse_pieces(sel, &clips);
                let path = join(
                    &dir,
                    format!(
                        "{}.mp3",
                        file_name(&fill(
                            &meta.verse_filename_template,
                            project,
                            bible,
                            book,
                            ch.chapter,
                            Some(v)
                        ))
                    ),
                );
                passage_track += 1;
                let (track, disc) = if passage {
                    ((passage_track, passage_verses), 1)
                } else {
                    ((v as u32, count), ch.chapter as u32)
                };
                let tag = tagger.tag(book, ch.chapter, Some(v), track, disc)?;
                let file = ExportedFile {
                    book: ch.book,
                    chapter: ch.chapter,
                    verse: Some(v),
                    path,
                    duration: 0.0,
                    missing: vec![],
                    fingerprint: String::new(),
                    unchanged: false,
                };
                done.push(make(file, &pieces, tag, i, &mut progress)?);
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

/// Where an export folder keeps what each file it has was made from ([`export_to_dir`])
pub const EXPORTED: &str = ".extract-audio-bible-export.json";

/// What's kept in an export folder's [`EXPORTED`]: each file's fingerprint, by its path in the folder
#[derive(Default, Serialize, Deserialize)]
struct Exported {
    files: BTreeMap<String, String>,
}

/**
[`export_with`] into a folder on disk (made if need be); the files' paths are where they were
written. The folder keeps what each file was made from ([`EXPORTED`]), so a file that would come
out the same as the one there isn't made again, unless `again` is set (every file is made again)
*/
pub fn export_to_dir(
    project: &Project,
    bible: &Bible,
    chapters: &[ChapterRef],
    files: &dyn Files,
    dir: &Path,
    again: bool,
    progress: impl FnMut(ExportProgress),
) -> Result<Vec<ExportedFile>> {
    let kept = dir.join(EXPORTED);
    let before: Exported = (std::fs::read(&kept).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let unchanged = |path: &str, fingerprint: &str| {
        !again && before.files.get(path).is_some_and(|f| f == fingerprint) && dir.join(path).is_file()
    };
    let mut after = Exported {
        files: before.files.clone(),
    };
    let result = export_with(project, bible, chapters, files, &unchanged, progress, |file, bytes| {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
        after.files.insert(file.path.clone(), file.fingerprint.clone());
        Ok(())
    });
    // What was written is kept, even when the export stopped part way
    if after.files != before.files {
        std::fs::write(&kept, serde_json::to_vec_pretty(&after)?)?;
    }
    let mut written = result?;
    for file in &mut written {
        file.path = dir.join(&file.path).to_string_lossy().into_owned();
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Memory, Passage, Selection};

    #[test]
    fn a_passage_is_one_file_and_its_verses_an_album() {
        let mut lines = String::new();
        for (chapter, verses) in [(7, 25), (8, 39)] {
            for v in 1..=verses {
                lines += &format!("Romans {chapter}:{v} Verse {v}.\n");
            }
        }
        let bible = Bible::new(
            "b",
            "Test",
            "T",
            crate::import(vec![("b.txt".into(), lines.into_bytes())]).unwrap(),
        );
        let romans = bible.book(45).unwrap();
        let passage = Passage::parse("7:24-8:2", romans).unwrap();
        let mut project = Project::passage("p", "", &bible, 45, passage);
        project.metadata.export_mode = ExportMode::Both;
        project.settings.chapter_lead_in = 0.0;
        project.settings.chapter_tail = 0.0;
        project.add_sources(&["s.wav".into(), "s.json".into()]);
        let source = project.sources[0].id.clone();
        // A second of the sermon for each verse, 7:23 (before the passage) too
        for (i, (chapter, verse)) in [(7, 23), (7, 24), (7, 25), (8, 1), (8, 2)].into_iter().enumerate() {
            let at = i as f64;
            project.selections.insert(
                verse_key(45, chapter, verse),
                Selection {
                    match_id: format!("m{i}"),
                    source_id: source.clone(),
                    start: at,
                    end: at + 1.0,
                    trim_start: 0.0,
                    trim_end: 0.0,
                    pad_before: 0.0,
                    pad_after: 0.0,
                    gain_db: 0.0,
                    needs_trim: false,
                    note: String::new(),
                    cuts: vec![],
                    joined: vec![],
                },
            );
        }
        let mut files = Memory::new();
        let tone: Vec<f32> = (0..16_000 * 6).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        files.insert(
            "s.wav",
            audio::wav(&Pcm {
                rate: 16_000,
                samples: tone,
            }),
        );

        let chapters = chapters_to_export(&project, &bible);
        assert_eq!(chapters.iter().map(|c| c.chapter).collect::<Vec<_>>(), [7, 8]);
        assert!(missing_verses(&project, romans, 7).is_empty() && missing_verses(&project, romans, 8).is_empty());
        let mut written = vec![];
        let exported = export(
            &project,
            &bible,
            &chapters,
            &files,
            |_| {},
            |file, bytes| {
                let tag = Tag::read_from2(std::io::Cursor::new(&bytes)).unwrap();
                written.push((
                    file.path.clone(),
                    tag.title().unwrap_or_default().to_string(),
                    tag.album().unwrap_or_default().to_string(),
                    tag.track(),
                    tag.total_tracks(),
                ));
                Ok(())
            },
        )
        .unwrap();

        // One file for the passage (its four verses, without 7:23), not one for each chapter
        let (path, title, album, ..) = &written[0];
        assert_eq!(path, "45 Romans/Romans 7_24-8_2.mp3");
        assert_eq!(
            (title.as_str(), album.as_str()),
            ("Romans 7:24-8:2", "Romans 7:24-8:2 (T)")
        );
        assert!((exported[0].duration - 4.0).abs() < 0.2, "{}", exported[0].duration);
        // Its verses, an album numbered in the passage's order
        let verses: Vec<_> = written[1..]
            .iter()
            .map(|(path, title, _, track, total)| (path.as_str(), title.as_str(), *track, *total))
            .collect();
        assert_eq!(
            verses,
            [
                (
                    "45 Romans/Romans 7_24-8_2/45_Rom_007_024.mp3",
                    "Romans 7:24",
                    Some(1),
                    Some(4)
                ),
                (
                    "45 Romans/Romans 7_24-8_2/45_Rom_007_025.mp3",
                    "Romans 7:25",
                    Some(2),
                    Some(4)
                ),
                (
                    "45 Romans/Romans 7_24-8_2/45_Rom_008_001.mp3",
                    "Romans 8:1",
                    Some(3),
                    Some(4)
                ),
                (
                    "45 Romans/Romans 7_24-8_2/45_Rom_008_002.mp3",
                    "Romans 8:2",
                    Some(4),
                    Some(4)
                ),
            ]
        );
        assert!(written[1..].iter().all(|w| w.2 == "Romans 7:24-8:2 (T)"));

        // Its status is of the passage's verses alone
        let status: Vec<_> = (project.status(&bible, &crate::MatchSet::default()).into_iter())
            .map(|c| (c.chapter, c.verses, c.chosen))
            .collect();
        assert_eq!(status, [(7, 2, 2), (8, 2, 2)]);
    }

    #[test]
    fn cuts_leave_out_stretches_of_a_reading() {
        let part = |cuts: &[(f64, f64)]| crate::Part {
            match_id: "m".into(),
            source_id: "s".into(),
            start: 10.0,
            end: 20.0,
            trim_start: 1.0,
            trim_end: 0.0,
            cuts: cuts.iter().map(|&(start, end)| Cut { start, end }).collect(),
        };
        assert_eq!(part(&[]).spans(), [(11.0, 20.0)]);
        // Out of order, overlapping, reversed, and partly outside the clip
        assert_eq!(
            part(&[(15.0, 16.0), (12.0, 13.0), (12.5, 14.0), (19.5, 25.0), (9.0, 10.5)]).spans(),
            [(11.0, 12.0), (14.0, 15.0), (16.0, 19.5)]
        );
        assert_eq!(part(&[(17.0, 16.0)]).spans(), [(11.0, 16.0), (17.0, 20.0)]);
        assert!((part(&[(12.0, 13.0)]).duration() - 8.0).abs() < 1e-9);
        // Cutting it all out leaves it whole
        assert_eq!(part(&[(0.0, 30.0)]).spans(), [(11.0, 20.0)]);
    }

    /// A project of John 3:16-17, a second of a tone each, with the tone in memory
    fn two_verses() -> (Bible, Project, Memory) {
        let bible = Bible::new(
            "b",
            "Test",
            "T",
            crate::import(vec![(
                "b.txt".into(),
                b"John 3:16 For God so loved.\nJohn 3:17 For God sent.\nRomans 1:1 Paul.\n".to_vec(),
            )])
            .unwrap(),
        );
        let mut project = Project::new("p", "", &bible, None);
        project.settings.chapter_lead_in = 0.0;
        project.settings.chapter_tail = 0.0;
        project.add_sources(&["s.wav".into(), "s.json".into()]);
        let source = project.sources[0].id.clone();
        for (key, at) in [("43.3.16", 0.0), ("43.3.17", 1.0), ("45.1.1", 2.0)] {
            let sel = Selection {
                match_id: key.into(),
                source_id: source.clone(),
                start: at,
                end: at + 1.0,
                trim_start: 0.0,
                trim_end: 0.0,
                pad_before: 0.0,
                pad_after: 0.0,
                gain_db: 0.0,
                needs_trim: false,
                note: String::new(),
                cuts: vec![],
                joined: vec![],
            };
            project.selections.insert(key.into(), sel);
        }
        let mut files = Memory::new();
        let tone: Vec<f32> = (0..16_000 * 4).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        files.insert(
            "s.wav",
            audio::wav(&Pcm {
                rate: 16_000,
                samples: tone,
            }),
        );
        (bible, project, files)
    }

    #[test]
    fn a_cut_shortens_the_verse_exported() {
        let (bible, mut project, files) = two_verses();
        project.selections.get_mut("43.3.16").unwrap().cuts = vec![Cut { start: 0.25, end: 0.75 }];
        let john = bible.book(43).unwrap();
        let (pieces, times) = chapter_pieces(&project, john, 3);
        let clips: Vec<(f64, f64)> = (pieces.iter())
            .filter_map(|p| match p {
                Piece::Audio(c) => Some((c.start, c.end)),
                Piece::Silence(_) => None,
            })
            .collect();
        // Verse 17 carries straight on from 16's last piece
        assert_eq!(clips, [(0.0, 0.25), (0.75, 2.0)]);
        assert_eq!((times[0].end, times[1].start), (0.5, 0.5));
        let pcm = render_chapter(&project, john, 3, 16_000, &files).unwrap();
        assert!((pcm.seconds() - 1.5).abs() < 0.01, "{}", pcm.seconds());
    }

    #[test]
    fn files_unchanged_are_left_as_they_are() {
        let (bible, mut project, files) = two_verses();
        let dir = std::env::temp_dir().join(format!("eab-unchanged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let chapters = chapters_to_export(&project, &bible);
        let export = |project: &Project, again| {
            export_to_dir(project, &bible, &chapters, &files, &dir, again, |_| {})
                .unwrap()
                .into_iter()
                .map(|f| (f.path.rsplit('/').next().unwrap().to_string(), f.unchanged))
                .collect::<Vec<_>>()
        };
        let made = |names: &[(&str, bool)]| names.iter().map(|(n, u)| (n.to_string(), *u)).collect::<Vec<_>>();
        assert_eq!(
            export(&project, false),
            made(&[("43_John_003.mp3", false), ("45_Rom_001.mp3", false)])
        );
        assert!(dir.join(EXPORTED).is_file());
        // Nothing changed: nothing made
        assert_eq!(
            export(&project, false),
            made(&[("43_John_003.mp3", true), ("45_Rom_001.mp3", true)])
        );
        // A verse cut differently: only its chapter is made again
        project.selections.get_mut("43.3.17").unwrap().trim_end = -0.2;
        assert_eq!(
            export(&project, false),
            made(&[("43_John_003.mp3", false), ("45_Rom_001.mp3", true)])
        );
        // A file taken away is made again, and every file when asked
        std::fs::remove_file(dir.join("45 Romans/45_Rom_001.mp3")).unwrap();
        assert_eq!(
            export(&project, false),
            made(&[("43_John_003.mp3", true), ("45_Rom_001.mp3", false)])
        );
        assert_eq!(
            export(&project, true),
            made(&[("43_John_003.mp3", false), ("45_Rom_001.mp3", false)])
        );
        // A tag that changes every file
        project.metadata.artist = "Someone".into();
        assert_eq!(
            export(&project, false),
            made(&[("43_John_003.mp3", false), ("45_Rom_001.mp3", false)])
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
