//! `extract-audio-bible`: build an audio Bible from sermon readings, from the command line.
//!
//! A project is a folder: `project.json` (sermons, chosen readings, settings), `readings.sqlite`
//! (every reading found), and `bible.json` (the translation's text; others searched in
//! `bibles/<id>.json`), in the same formats the app uses.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use extract_audio_bible::{
    Bible, ChapterRef, ClipOptions, Disk, Downloading, ExportMode, MatchSet, Project, Transcript, audio, bible, db,
    export, matcher,
    project::{now, stable_id},
};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "extract-audio-bible", version, about, long_about = None)]
struct Cli {
    /// Print JSON instead of text (for scripts)
    #[arg(long, global = true)]
    json: bool,
    /// Print nothing but results and errors
    #[arg(short, long, global = true)]
    quiet: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read a Bible's text (USFM, USX, OSIS, ThML, GBF, JSON, CSV/TSV, or "John 3:16 text" lines)
    /// and save it as JSON
    Import {
        /// Files or folders of the translation
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Where to write it
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value = "")]
        name: String,
        #[arg(long, default_value = "")]
        abbreviation: String,
        #[arg(long)]
        language: Option<String>,
    },
    /// Find the verses read in transcripts, with their times, in one translation or several (no
    /// project needed)
    Find {
        /// A Bible's JSON (from `import`), or its files
        #[arg(long, required = true, num_args = 1..)]
        bible: Vec<PathBuf>,
        /// Another translation to search too (a Bible's JSON, or a folder or file of its text);
        /// each reading is of the translation it fits best
        #[arg(long = "translation")]
        translations: Vec<PathBuf>,
        /// Only these books ("John", "1 Thess"); with genres, they add up
        #[arg(long = "book")]
        books: Vec<String>,
        #[command(flatten)]
        scope: ScopeArgs,
        /// Print every reading as CSV (sermon, reference, translation, times, kind, words said…)
        #[arg(long)]
        csv: bool,
        /// How faithfully a verse must be read (0–1)
        #[arg(long, default_value_t = 0.85)]
        min_accuracy: f32,
        /// Transcripts (WhisperX or Let's Church JSON, gzipped or not)
        #[arg(required = true)]
        transcripts: Vec<PathBuf>,
    },
    /// Start a project in a folder: an audio Bible, or every verse read in its sermons
    /// (`--readings`, or with `--translation`). The app opens the folder as it is ("Open folder")
    Init {
        dir: PathBuf,
        /// A Bible's JSON (from `import`), or its files
        #[arg(long, required = true, num_args = 1..)]
        bible: Vec<PathBuf>,
        /// Find every verse read in the sermons rather than build an audio Bible
        #[arg(long)]
        readings: bool,
        /// Another translation to search too, for every verse read (a Bible's JSON, or a folder
        /// or file of its text); each reading is of the translation it fits best
        #[arg(long = "translation")]
        translations: Vec<PathBuf>,
        /// The book being built ("1 Thessalonians"; the whole Bible if left out), or some of its
        /// verses ("2 Cor 2:14-17", exported as one file, and its verses as an album of their own);
        /// for every verse read, only these books (again for others; with genres, they add up)
        #[arg(long = "book")]
        books: Vec<String>,
        #[command(flatten)]
        scope: ScopeArgs,
        #[arg(long, default_value = "")]
        name: String,
    },
    /// Add sermons: each audio file with the transcript (.json) of the same name, from files or
    /// folders
    Add {
        dir: PathBuf,
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// List the project's sermons with their file ids, titles, and passages
    Sermons {
        dir: PathBuf,
        /// First set titles, passages, speakers, and series by file id from JSON, field by field:
        /// `{"<file id>": {"title": "…", "passage": "1 Thess 5:12-15", "speaker": "…", "series": "…"}}`
        #[arg(long)]
        info: Option<PathBuf>,
    },
    /// Print a sermon's whole transcript in paragraphs, with their times and the readings found
    /// in it marked (`[1Thess 5:15]` where a chosen reading begins, `<…>` where another does)
    Transcript {
        dir: PathBuf,
        /// The sermon: its id or file name (as `sermons` lists them)
        sermon: String,
    },
    /// Find where a phrase was said in the project's sermons: its words in order, whatever their
    /// case, accents, or punctuation, numbers spelled out ("70 times 7" finds "seventy times
    /// seven")
    Grep {
        dir: PathBuf,
        phrase: String,
        /// Its last word may be the start of one ("forgiv" finds "forgive" and "forgiveness")
        #[arg(long)]
        prefix: bool,
        /// Words of context on each side
        #[arg(long, default_value_t = 10)]
        context: usize,
        /// The most places to give (0 for all)
        #[arg(long, default_value_t = 0)]
        limit: usize,
        /// Only sermons by this speaker (as their information names them; again for others)
        #[arg(long = "speaker")]
        speakers: Vec<String>,
        /// Only sermons in this series (again for others)
        #[arg(long = "series")]
        series: Vec<String>,
    },
    /// Find every reading of the project's verses in its sermons
    Search { dir: PathBuf },
    /// Every reading found (by `search`), as a table: CSV, or JSON with --json or to a `.json`
    /// file (the rows the app's readings export)
    Table {
        dir: PathBuf,
        /// Write it here instead of printing it (`.json` for JSON, else CSV)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Choose the best reading for each verse that has none
    Choose {
        dir: PathBuf,
        /// Only these chapters ("5", or "1 Thess 5" for a whole-Bible project); all if left out
        #[arg(long = "chapter")]
        chapters: Vec<String>,
        /// The least score a reading may have: 0.9 exact, 0.75 exact and close, 0 anything
        #[arg(long, default_value_t = 0.75)]
        min_score: f32,
        /// Choose again for verses that already have a reading
        #[arg(long)]
        replace: bool,
    },
    /// Show each chapter's chosen and missing verses
    Status { dir: PathBuf },
    /// List a verse's readings, numbered best first, marking those chosen
    Readings {
        dir: PathBuf,
        /// The verse ("5:3", or "1 Thess 5:3")
        verse: String,
    },
    /// Choose a verse's reading, or several to play one after another in the order given (a
    /// verse read in pieces: its first half, then the rest minutes later)
    Select {
        dir: PathBuf,
        /// The verse ("5:3", or "1 Thess 5:3")
        verse: String,
        /// Readings by their number in `readings`, or their id
        #[arg(required = true)]
        readings: Vec<String>,
    },
    /// Change settings: `minAccuracy=0.8`, `metadata.artist="Grace Church"`, `settings.gapBetweenVerses=0.3`
    Set {
        dir: PathBuf,
        /// `key=value` (a bare key is under `settings`, else `settings.` or `metadata.`; values are
        /// JSON when they parse as JSON, text otherwise)
        #[arg(required = true)]
        changes: Vec<String>,
    },
    /// Render a chapter (or one verse) as it will be exported, as WAV
    Render {
        dir: PathBuf,
        /// The chapter ("5", or "1 Thess 5")
        #[arg(long)]
        chapter: String,
        /// Only this verse of it, as its own file will be (every reading joined for it)
        #[arg(long)]
        verse: Option<u16>,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Write a stretch of a sermon as WAV
    Clip {
        /// The audio file
        audio: PathBuf,
        /// Start and end, in seconds
        start: f64,
        end: f64,
        #[arg(short, long)]
        output: PathBuf,
        /// Level and fade it as an export will
        #[arg(long)]
        processed: bool,
    },
    /// Export tagged MP3s of the chosen readings
    Export {
        dir: PathBuf,
        /// The folder to write to
        #[arg(short, long)]
        output: PathBuf,
        /// Only these chapters; every chapter with a chosen verse if left out
        #[arg(long = "chapter")]
        chapters: Vec<String>,
        /// A file per chapter, per verse, or both (the project's setting if left out)
        #[arg(long)]
        mode: Option<Mode>,
    },
    /// Everything at once: import the Bible, find every reading in the sermons, choose the best,
    /// and export (the project is kept in --project, or a temporary folder)
    Build {
        /// A Bible's JSON (from `import`), or its files
        #[arg(long, required = true, num_args = 1..)]
        bible: Vec<PathBuf>,
        #[arg(long)]
        book: Option<String>,
        /// Sermons: audio files with their transcripts, or folders of them
        #[arg(required = true)]
        sermons: Vec<PathBuf>,
        #[arg(short, long)]
        output: PathBuf,
        /// Keep the project here, to look at or change and export again
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long, default_value_t = 0.85)]
        min_accuracy: f32,
        #[arg(long, default_value_t = 0.75)]
        min_score: f32,
        #[arg(long)]
        mode: Option<Mode>,
    },
}

/// The books searched for every verse read, besides those named (`--book`)
#[derive(clap::Args)]
struct ScopeArgs {
    /// Only these genres, topos's book groups ("Gospels", "Pauline Epistles", "pentateuch")
    #[arg(long = "genre")]
    genres: Vec<String>,
    /// Only this testament (the books and genres given are then within it)
    #[arg(long = "testament", value_enum)]
    testaments: Vec<TestamentArg>,
    #[arg(long = "exclude-book")]
    exclude_books: Vec<String>,
    #[arg(long = "exclude-genre")]
    exclude_genres: Vec<String>,
    #[arg(long = "exclude-testament", value_enum)]
    exclude_testaments: Vec<TestamentArg>,
}

impl ScopeArgs {
    fn is_empty(&self) -> bool {
        self.genres.is_empty()
            && self.testaments.is_empty()
            && self.exclude_books.is_empty()
            && self.exclude_genres.is_empty()
            && self.exclude_testaments.is_empty()
    }

    fn with_books(self, books: Vec<String>) -> extract_audio_bible::BookScope {
        extract_audio_bible::BookScope {
            testaments: self.testaments.into_iter().map(Into::into).collect(),
            genres: self.genres,
            books,
            exclude_testaments: self.exclude_testaments.into_iter().map(Into::into).collect(),
            exclude_genres: self.exclude_genres,
            exclude_books: self.exclude_books,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum TestamentArg {
    Old,
    New,
}

impl From<TestamentArg> for extract_audio_bible::Testament {
    fn from(t: TestamentArg) -> Self {
        match t {
            TestamentArg::Old => extract_audio_bible::Testament::Old,
            TestamentArg::New => extract_audio_bible::Testament::New,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Chapter,
    Verse,
    Both,
}

impl From<Mode> for ExportMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Chapter => ExportMode::Chapter,
            Mode::Verse => ExportMode::Verse,
            Mode::Both => ExportMode::Both,
        }
    }
}

/// What prints the results
struct Out {
    json: bool,
    quiet: bool,
}

impl Out {
    /// Progress and notes, on stderr
    fn note(&self, message: impl AsRef<str>) {
        if !self.quiet && !self.json {
            eprintln!("{}", message.as_ref());
        }
    }

    /// The result: `text` for people, `value` as JSON for scripts
    fn result<T: Serialize>(&self, value: &T, text: impl FnOnce() -> String) -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        if self.json {
            serde_json::to_writer_pretty(&mut stdout, value)?;
            writeln!(stdout)?;
        } else {
            let text = text();
            if !text.is_empty() {
                writeln!(stdout, "{text}")?;
            }
        }
        Ok(())
    }
}

fn main() {
    let cli = Cli::parse();
    let out = Out {
        json: cli.json,
        quiet: cli.quiet,
    };
    if let Err(e) = run(cli.command, &out) {
        // The reader stopped early (`| head`): not an error
        let broken_pipe = |c: &(dyn std::error::Error + 'static)| {
            let kind = (c.downcast_ref::<std::io::Error>().map(std::io::Error::kind)).or_else(|| {
                c.downcast_ref::<serde_json::Error>()
                    .and_then(serde_json::Error::io_error_kind)
            });
            kind == Some(std::io::ErrorKind::BrokenPipe)
        };
        if e.chain().any(broken_pipe) {
            return;
        }
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run(command: Command, out: &Out) -> Result<()> {
    match command {
        Command::Import {
            paths,
            output,
            name,
            abbreviation,
            language,
        } => {
            let (mut bible, warnings) = import_bible(&paths, &name, &abbreviation)?;
            for warning in &warnings {
                out.note(warning);
            }
            bible.language = language;
            write_json(&output, &bible)?;
            let imported = Imported {
                summary: bible.summary(),
                warnings,
            };
            let summary = &imported.summary;
            out.result(&imported, || {
                format!(
                    "{} ({}): {} books, {} verses → {}",
                    summary.name,
                    summary.format,
                    summary.book_count,
                    summary.verse_count,
                    output.display()
                )
            })
        }
        Command::Find {
            bible,
            translations,
            books,
            scope,
            csv,
            min_accuracy,
            transcripts,
        } => {
            let mut bibles = vec![load_bible(&bible, out)?];
            for path in &translations {
                bibles.push(load_bible(std::slice::from_ref(path), out)?);
            }
            let scope = scope.with_books(books);
            // A project of these transcripts alone, searched as the app searches
            let mut project = Project::readings("find", "", &bibles.iter().collect::<Vec<_>>(), scope)?;
            project.settings.min_accuracy = min_accuracy;
            let names: Vec<String> = transcripts.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            project.add_sources(&names);
            let engine = matcher::engine_of(&bibles)?;
            let analysis = extract_audio_bible::analyze_with(&mut project, &engine, &Disk, &|_, _, _| {})?;
            if let Some((name, e)) = analysis.errors.into_iter().next() {
                bail!("{name}: {e}");
            }
            let rows = extract_audio_bible::reading_rows(
                &project,
                &analysis.matches,
                &bibles.iter().collect::<Vec<_>>(),
                &Disk,
            );
            if csv {
                print!("{}", extract_audio_bible::readings::to_csv(&rows));
                return Ok(());
            }
            let found: Vec<Found> = (project.sources.iter())
                .map(|s| Found {
                    transcript: s.transcript_path.clone(),
                    matches: analysis
                        .matches
                        .matches
                        .iter()
                        .filter(|m| m.source_id == s.id)
                        .cloned()
                        .collect(),
                    runs: analysis
                        .matches
                        .runs
                        .iter()
                        .filter(|r| r.source_id == s.id)
                        .cloned()
                        .collect(),
                })
                .collect();
            let several = bibles.len() > 1;
            out.result(&found, || {
                let mut text = String::new();
                for f in &found {
                    text += &format!("{}\n", f.transcript);
                    for r in rows.iter().filter(|r| f.matches.iter().any(|m| m.id == r.id)) {
                        let translation = if several {
                            let also = if r.also_in.is_empty() {
                                String::new()
                            } else {
                                format!(" (also {})", r.also_in.join(", "))
                            };
                            format!("  {}{also}", r.translation)
                        } else {
                            String::new()
                        };
                        text += &format!(
                            "  {:<24} {:>8}–{:<8} {:<7} {:>3}% accurate{translation}{}\n",
                            r.reference,
                            clock(r.start),
                            clock(r.end),
                            format!("{:?}", r.kind).to_lowercase(),
                            (r.accuracy * 100.0).round(),
                            if r.also_at.is_empty() {
                                String::new()
                            } else {
                                format!("  (or {})", r.also_at.join(", "))
                            }
                        );
                    }
                }
                text.trim_end().to_string()
            })
        }
        Command::Init {
            dir,
            bible,
            readings,
            translations,
            books,
            scope,
            name,
        } => {
            if dir.join("project.json").exists() {
                bail!("{} already has a project", dir.display());
            }
            let mut bibles = vec![load_bible(&bible, out)?];
            for path in &translations {
                bibles.push(load_bible(std::slice::from_ref(path), out)?);
            }
            let id = stable_id(&format!("{}{}", dir.display(), now()));
            let project = if readings || !translations.is_empty() {
                Project::readings(id, &name, &bibles.iter().collect::<Vec<_>>(), scope.with_books(books))?
            } else {
                if books.len() > 1 || !scope.is_empty() {
                    bail!(
                        "an audio Bible is of one book (or the whole Bible): give --readings to find every verse read in some"
                    );
                }
                audio_bible(id, &name, &bibles[0], books.first().map(String::as_str))?
            };
            std::fs::create_dir_all(&dir)?;
            write_json(&dir.join("bible.json"), &bibles[0])?;
            for other in &bibles[1..] {
                write_json(&dir.join("bibles").join(format!("{}.json", other.id)), other)?;
            }
            write_json(&dir.join("project.json"), &project)?;
            out.result(&project, || format!("Started {} in {}", project.name, dir.display()))
        }
        Command::Add { dir, paths } => {
            let mut p = Folder::open(&dir)?;
            let added = add_sermons(&mut p.project, &paths)?;
            p.save()?;
            for f in &added.unpaired {
                out.note(format!("no transcript: {f}"));
            }
            for f in &added.skipped {
                out.note(format!("not a transcript: {f}"));
            }
            out.result(&added, || {
                let paired = match added.paired {
                    0 => String::new(),
                    n => format!(", and gave {} its audio", plural(n, "sermon")),
                };
                format!(
                    "Added {}{paired} ({} in the project)",
                    plural(added.added, "sermon"),
                    p.project.sources.len()
                )
            })
        }
        Command::Sermons { dir, info } => {
            let mut p = Folder::open(&dir)?;
            if let Some(info) = info {
                let given = read_json(&info)?;
                let count = p.project.apply_file_metadata(given);
                p.save()?;
                out.note(format!("Set the information of {}", plural(count, "sermon")));
            }
            let sermons: Vec<Sermon> = (p.project.sources.iter())
                .map(|s| {
                    let meta = p.project.file_metadata(s);
                    Sermon {
                        id: s.id.clone(),
                        file_id: s.file_id(),
                        label: s.label.clone(),
                        title: meta.title,
                        passage: meta.passage,
                        speaker: meta.speaker,
                        series: meta.series,
                        audio_path: p.project.audio_path(s),
                        transcript_path: s.transcript_path.clone(),
                        readings: s.match_count,
                    }
                })
                .collect();
            out.result(&sermons, || {
                let mut text = String::new();
                for s in &sermons {
                    let title = s.title.as_deref().unwrap_or(&s.label);
                    let passage = s.passage.as_deref().map(|p| format!(" ({p})")).unwrap_or_default();
                    let by: Vec<&str> = [s.speaker.as_deref(), s.series.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect();
                    let by = if by.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", by.join(" · "))
                    };
                    text += &format!(
                        "{:<24} {title}{passage}{by}  {}\n",
                        s.file_id,
                        plural(s.readings, "reading")
                    );
                }
                text.trim_end().to_string()
            })
        }
        Command::Transcript { dir, sermon } => {
            let p = Folder::open(&dir)?;
            let source = (p.project.sources.iter())
                .find(|s| s.file_id() == sermon || s.label == sermon || s.id == sermon)
                .ok_or_else(|| anyhow!("{sermon}: no such sermon (`sermons` lists them)"))?;
            let t = Transcript::open(&Disk, &source.transcript_path)?;
            let chosen: std::collections::HashSet<&str> = (p.project.selections.values())
                .flat_map(|s| std::iter::once(s.match_id.as_str()).chain(s.joined.iter().map(|j| j.match_id.as_str())))
                .collect();
            // Where each reading begins, by word
            let mut begins: std::collections::HashMap<usize, Vec<String>> = Default::default();
            let found = match p.database()? {
                Some(readings) => readings.sermon(&source.id)?.matches,
                None => vec![],
            };
            for m in &found {
                let osis = p
                    .bible
                    .book(m.book)
                    .map_or_else(|| m.book.to_string(), |b| b.osis.clone());
                let (open, close) = if chosen.contains(m.id.as_str()) {
                    ('[', ']')
                } else {
                    ('<', '>')
                };
                begins
                    .entry(m.word_start as usize)
                    .or_default()
                    .push(format!("{open}{osis} {}:{}{close}", m.chapter, m.verse));
            }
            let paragraphs: Vec<Paragraph> = (t.paragraphs.iter().enumerate())
                .map(|(k, &from)| {
                    let to = t.paragraphs.get(k + 1).copied().unwrap_or(t.words.len());
                    let mut text = String::new();
                    for i in from..to {
                        for mark in begins.get(&i).into_iter().flatten() {
                            text += mark;
                            text.push(' ');
                        }
                        text += &t.words[i].text;
                        text.push(' ');
                    }
                    Paragraph {
                        start: t.words[from].start,
                        end: t.words[to - 1].end,
                        text: text.trim_end().to_string(),
                    }
                })
                .collect();
            let title = p.project.source_title(source);
            out.result(&paragraphs, || {
                let mut text = format!("{title}\n");
                for para in &paragraphs {
                    text += &format!("\n[{}] {}\n", clock(para.start), para.text);
                }
                text.trim_end().to_string()
            })
        }
        Command::Grep {
            dir,
            phrase,
            prefix,
            context,
            limit,
            speakers,
            series,
        } => {
            let mut p = Folder::open(&dir)?;
            // Only the sermons of the speakers and series asked for (ignoring case)
            let among = |value: Option<String>, wanted: &[String]| {
                wanted.is_empty() || value.is_some_and(|v| wanted.iter().any(|w| w.eq_ignore_ascii_case(v.trim())))
            };
            let kept: Vec<_> = (p.project.sources.iter())
                .filter(|s| {
                    let meta = p.project.file_metadata(s);
                    among(meta.speaker, &speakers) && among(meta.series, &series)
                })
                .cloned()
                .collect();
            p.project.sources = kept;
            let options = extract_audio_bible::FindOptions { prefix, context, limit };
            let (found, errors) = extract_audio_bible::search_transcripts(&p.project, &Disk, &phrase, &options);
            for (sermon, e) in &errors {
                out.note(format!("couldn't read {sermon}: {e}"));
            }
            out.result(&found, || {
                let mut text = String::new();
                let mut sermon = "";
                for s in &found {
                    if s.source_id != sermon {
                        sermon = &s.source_id;
                        let source = p.project.sources.iter().find(|x| x.id == s.source_id);
                        let title = source.map_or_else(|| s.source_id.clone(), |x| p.project.source_title(x));
                        text += &format!("{}{title}\n", if text.is_empty() { "" } else { "\n" });
                    }
                    let before = if s.before.is_empty() {
                        String::new()
                    } else {
                        format!("…{} ", s.before)
                    };
                    let after = if s.after.is_empty() {
                        String::new()
                    } else {
                        format!(" {}…", s.after)
                    };
                    text += &format!("  [{}] {before}«{}»{after}\n", clock(s.start), s.text);
                }
                if found.is_empty() {
                    text = format!("\"{phrase}\" wasn't found");
                }
                text.trim_end().to_string()
            })
        }
        Command::Search { dir } => {
            let mut p = Folder::open(&dir)?;
            let summary = search(&mut p, out)?;
            p.save()?;
            out.result(&summary, || {
                format!(
                    "{} in {}",
                    plural(summary.readings, "reading"),
                    plural(summary.sermons, "sermon")
                )
            })
        }
        Command::Table { dir, output } => {
            let p = Folder::open(&dir)?;
            let readings = p
                .database()?
                .ok_or_else(|| anyhow!("no readings yet: run `search` first"))?;
            let (filter, all) = (db::RowFilter::default(), db::Group::All);
            let json = match &output {
                Some(output) => output.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")),
                None => out.json,
            };
            if json {
                let rows = readings.rows(&filter, &all)?;
                return match output {
                    Some(output) => {
                        write_json(&output, &rows)?;
                        out.result(&serde_json::json!({ "rows": rows.len(), "output": output }), || {
                            format!("Wrote {} to {}", plural(rows.len(), "reading"), output.display())
                        })
                    }
                    None => out.result(&rows, String::new),
                };
            }
            // CSV is written as it's read
            let mut sink: Box<dyn Write> = match &output {
                Some(output) => Box::new(std::io::BufWriter::new(
                    std::fs::File::create(output).with_context(|| format!("writing {}", output.display()))?,
                )),
                None => Box::new(std::io::BufWriter::new(std::io::stdout().lock())),
            };
            sink.write_all(extract_audio_bible::readings::CSV_HEADER.as_bytes())?;
            let (mut count, mut failed) = (0, None);
            readings.rows_each(&filter, &all, |row| {
                if failed.is_none()
                    && let Err(e) = sink.write_all(extract_audio_bible::readings::csv_line(&row).as_bytes())
                {
                    failed = Some(e);
                }
                count += 1;
            })?;
            if let Some(e) = failed {
                return Err(e.into());
            }
            sink.flush()?;
            if let Some(output) = output {
                out.note(format!("Wrote {} to {}", plural(count, "reading"), output.display()));
            }
            Ok(())
        }
        Command::Choose {
            dir,
            chapters,
            min_score,
            replace,
        } => {
            let mut p = Folder::open(&dir)?.with_matches()?;
            let chapters = p.chapters(&chapters, true)?;
            let chosen = choose(&mut p, &chapters, min_score, replace)?;
            p.save()?;
            let count: usize = chosen.iter().map(|c| c.verses.len()).sum();
            out.result(&chosen, || format!("Chose readings for {}", plural(count, "verse")))
        }
        Command::Status { dir } => {
            let p = Folder::open(&dir)?.with_matches()?;
            let status = status(&p);
            out.result(&status, || {
                let mut text = String::new();
                for ChapterStatus { name, status: c } in &status {
                    let missing = if c.missing.is_empty() {
                        "complete".to_string()
                    } else {
                        format!("missing {}", ranges(&c.missing))
                    };
                    text += &format!(
                        "{name:<24} {:>3}/{:<3} chosen, {:>3} with readings  {missing}\n",
                        c.chosen, c.verses, c.with_readings
                    );
                }
                text.trim_end().to_string()
            })
        }
        Command::Readings { dir, verse } => {
            let p = Folder::open(&dir)?.with_matches()?;
            let (c, v) = p.verse(&verse)?;
            let key = export::verse_key(c.book, c.chapter, v);
            let chosen = p.project.selections.get(&key).map(|s| s.parts()).unwrap_or_default();
            let listed: Vec<Listed> = (p.readings(&c, v).into_iter().enumerate())
                .map(|(i, reading)| Listed {
                    number: i + 1,
                    chosen: chosen
                        .iter()
                        .position(|part| part.match_id == reading.id)
                        .map(|n| n + 1),
                    reading: reading.clone(),
                })
                .collect();
            out.result(&listed, || {
                let mut text = reference(&p.bible, c.book, c.chapter, Some(v));
                if listed.is_empty() {
                    text += ": no readings found";
                }
                for l in &listed {
                    let r = &l.reading;
                    let sermon = (p.project.sources.iter())
                        .find(|s| s.id == r.source_id)
                        .map_or_else(|| "?".to_string(), |s| p.project.source_title(s));
                    let mark = l.chosen.map_or(String::new(), |n| {
                        if chosen.len() > 1 {
                            format!("chosen ({n} of {})", chosen.len())
                        } else {
                            "chosen".into()
                        }
                    });
                    text += &format!(
                        "\n{:>3}  {:<8} {:>3}% of the verse, {:>3}%  {}–{}  {sermon}  {mark}",
                        l.number,
                        format!("{:?}", r.kind).to_lowercase(),
                        (r.coverage * 100.0).round(),
                        (r.score * 100.0).round(),
                        clock(r.start),
                        clock(r.end),
                    );
                }
                text
            })
        }
        Command::Select { dir, verse, readings } => {
            let mut p = Folder::open(&dir)?.with_matches()?;
            let (c, v) = p.verse(&verse)?;
            let listed = p.readings(&c, v);
            let mut picked: Vec<extract_audio_bible::Match> = vec![];
            for name in &readings {
                let m = match name.parse::<usize>() {
                    Ok(n) => listed
                        .get(n.wrapping_sub(1))
                        .ok_or_else(|| anyhow!("{name}: the verse has {}", plural(listed.len(), "reading")))?,
                    Err(_) => listed
                        .iter()
                        .find(|m| &m.id == name)
                        .ok_or_else(|| anyhow!("{name}: not a reading of this verse"))?,
                };
                if picked.iter().any(|x| x.id == m.id) {
                    bail!("{name} is given twice");
                }
                picked.push((*m).clone());
            }
            let key = export::verse_key(c.book, c.chapter, v);
            p.project.select(&picked[0]);
            let selection = p.project.selections.get_mut(&key).expect("just chosen");
            selection.set_parts(picked.iter().map(extract_audio_bible::Part::of).collect());
            let selection = selection.clone();
            p.save()?;
            out.result(&selection, || {
                format!(
                    "Chose {} for {}",
                    plural(picked.len(), "reading"),
                    reference(&p.bible, c.book, c.chapter, Some(v))
                )
            })
        }
        Command::Set { dir, changes } => {
            let mut p = Folder::open(&dir)?;
            let mut value = serde_json::to_value(&p.project)?;
            for change in &changes {
                let (key, raw) = change
                    .split_once('=')
                    .ok_or_else(|| anyhow!("{change}: expected key=value"))?;
                let key = if key.starts_with("settings.") || key.starts_with("metadata.") {
                    key.to_string()
                } else {
                    format!("settings.{key}")
                };
                let parsed = serde_json::from_str(raw).unwrap_or_else(|_| serde_json::Value::String(raw.to_string()));
                let (group, field) = key.split_once('.').unwrap_or(("settings", &key));
                let object = value[group].as_object_mut().ok_or_else(|| anyhow!("no {group}"))?;
                if !object.contains_key(field) {
                    bail!("{key}: no such setting (see `{}` in project.json)", group);
                }
                object.insert(field.to_string(), parsed);
            }
            p.project = serde_json::from_value(value).context("a setting has the wrong kind of value")?;
            p.save()?;
            out.result(&p.project.settings, || format!("Changed {}", changes.len()))
        }
        Command::Render {
            dir,
            chapter,
            verse,
            output,
        } => {
            let p = Folder::open(&dir)?;
            let c = p.chapters(&[chapter], false)?.remove(0);
            let book = p.bible.book(c.book).ok_or_else(|| anyhow!("no book {}", c.book))?;
            let rate = p.project.metadata.sample_rate;
            let (pcm, timeline) = match verse {
                Some(v) => {
                    let key = export::verse_key(c.book, c.chapter, v);
                    let pcm = export::render_verse(&p.project, &key, rate, &p.audio())?.ok_or_else(|| {
                        anyhow!(
                            "nothing is chosen for {}",
                            reference(&p.bible, c.book, c.chapter, Some(v))
                        )
                    })?;
                    let sel = &p.project.selections[&key];
                    let heard = export::VerseTime {
                        verse: v,
                        start: sel.pad_before,
                        end: pcm.seconds() - sel.pad_after,
                    };
                    (pcm, vec![heard])
                }
                None => (
                    export::render_chapter(&p.project, book, c.chapter, rate, &p.audio())?,
                    export::chapter_timeline(&p.project, book, c.chapter),
                ),
            };
            std::fs::write(&output, audio::wav(&pcm))?;
            out.result(&timeline, || {
                format!("{:.1} seconds → {}", pcm.seconds(), output.display())
            })
        }
        Command::Clip {
            audio: path,
            start,
            end,
            output,
            processed,
        } => {
            let options = ClipOptions {
                processed,
                ..Default::default()
            };
            // A URL is downloaded to a temporary folder
            let files = Downloading::new(&Disk, std::env::temp_dir().join("extract-audio-bible-downloads"));
            let pcm = extract_audio_bible::render_clip(&files, &path.to_string_lossy(), start, end, &options)?;
            std::fs::write(&output, audio::wav(&pcm))?;
            out.result(&pcm.seconds(), || {
                format!("{:.1} seconds → {}", pcm.seconds(), output.display())
            })
        }
        Command::Export {
            dir,
            output,
            chapters,
            mode,
        } => {
            let mut p = Folder::open(&dir)?;
            if let Some(mode) = mode {
                p.project.metadata.export_mode = mode.into();
            }
            let chapters = if chapters.is_empty() {
                export::chapters_to_export(&p.project, &p.bible)
            } else {
                p.chapters(&chapters, false)?
            };
            let files = export_to(&p, &chapters, &output, out)?;
            out.result(&files, || {
                format!("Exported {} to {}", plural(files.len(), "file"), output.display())
            })
        }
        Command::Build {
            bible,
            book,
            sermons,
            output,
            project: keep,
            min_accuracy,
            min_score,
            mode,
        } => {
            let dir = match &keep {
                Some(dir) => dir.clone(),
                None => std::env::temp_dir().join(format!("extract-audio-bible-{}", std::process::id())),
            };
            let bible = load_bible(&bible, out)?;
            let mut project = if dir.join("project.json").exists() {
                Folder::open(&dir)?.project
            } else {
                audio_bible(
                    stable_id(&format!("{}{}", dir.display(), now())),
                    "",
                    &bible,
                    book.as_deref(),
                )?
            };
            project.settings.min_accuracy = min_accuracy;
            if let Some(mode) = mode {
                project.metadata.export_mode = mode.into();
            }
            std::fs::create_dir_all(&dir)?;
            write_json(&dir.join("bible.json"), &bible)?;
            let mut p = Folder {
                dir: dir.clone(),
                project,
                bible,
                others: vec![],
                matches: MatchSet::default(),
                searched: false,
            };
            let added = add_sermons(&mut p.project, &sermons)?;
            out.note(plural(p.project.sources.len(), "sermon"));
            for f in &added.unpaired {
                out.note(format!("no transcript: {f}"));
            }
            for f in &added.skipped {
                out.note(format!("not a transcript: {f}"));
            }
            search(&mut p, out)?;
            let all = p.chapters(&[], true)?;
            let chosen = choose(&mut p, &all, min_score, false)?;
            out.note(format!(
                "Chose readings for {}",
                plural(chosen.iter().map(|c| c.verses.len()).sum(), "verse")
            ));
            p.save()?;
            let chapters = export::chapters_to_export(&p.project, &p.bible);
            let files = export_to(&p, &chapters, &output, out)?;
            if keep.is_none() {
                let _ = std::fs::remove_dir_all(&dir);
            }
            out.result(&files, || {
                format!("Exported {} to {}", plural(files.len(), "file"), output.display())
            })
        }
    }
}

/// A project folder
struct Folder {
    dir: PathBuf,
    project: Project,
    bible: Bible,
    /// The other translations it searches, for every verse read (`bibles/<id>.json`)
    others: Vec<Bible>,
    /// Its readings, once read ([`Folder::with_matches`]) or found ([`search`])
    matches: MatchSet,
    /// Whether the sermons were searched again, so `save` writes every reading
    searched: bool,
}

impl Folder {
    /// Files from disk, with sermon audio given by a URL downloaded once into `downloads/`
    fn audio(&self) -> Downloading<'static> {
        Downloading::new(&Disk, self.dir.join("downloads"))
    }

    fn open(dir: &Path) -> Result<Self> {
        let project: Project = read_json(&dir.join("project.json"))
            .with_context(|| format!("{} isn't a project folder (start one with `init`)", dir.display()))?;
        let bible: Bible = read_json(&dir.join("bible.json"))?;
        let others = (project.bibles_searched().iter())
            .filter(|id| **id != bible.id)
            .map(|id| read_json(&dir.join("bibles").join(format!("{id}.json"))))
            .collect::<Result<Vec<Bible>>>()?;
        Ok(Folder {
            dir: dir.to_path_buf(),
            project,
            bible,
            others,
            matches: MatchSet::default(),
            searched: false,
        })
    }

    /// With every reading read, for what goes through them all
    fn with_matches(mut self) -> Result<Self> {
        if let Some(readings) = self.database()? {
            self.matches = readings.all()?;
        }
        Ok(self)
    }

    /// Its readings database (made from `matches.json` the first time, if it has that), if it's
    /// been searched
    fn database(&self) -> Result<Option<db::Readings>> {
        let bibles = || Ok(self.translations().into_iter().cloned().collect());
        Ok(db::open_in(&self.dir, &self.project, &Disk, bibles)?)
    }

    /// Every translation it searches, its own first
    fn translations(&self) -> Vec<&Bible> {
        std::iter::once(&self.bible).chain(&self.others).collect()
    }

    /// Writes the project, and its readings when the sermons were searched (else the readings
    /// database's list of sermons, if it's changed)
    fn save(&mut self) -> Result<()> {
        self.project.updated_at = now();
        write_json(&self.dir.join("project.json"), &self.project)?;
        if self.searched {
            db::Readings::write(&db::path(&self.dir), &self.project, &self.translations(), &self.matches)?;
            let _ = std::fs::remove_file(self.dir.join(db::LEGACY_FILE));
            self.searched = false;
        } else if let Some(mut readings) = self.database()? {
            readings.sync_sermons(&self.project)?;
        }
        Ok(())
    }

    /// Chapters named like "5" (in the project's book) or "1 Thess 5"; with none, every chapter
    /// of the project's book (or the Bible) when `all` is allowed
    fn chapters(&self, names: &[String], all: bool) -> Result<Vec<ChapterRef>> {
        if names.is_empty() {
            if !all {
                bail!("name a chapter");
            }
            return Ok((self.bible.books.iter())
                .filter(|b| self.project.book.is_none_or(|only| only == b.id))
                .flat_map(|b| (1..=b.chapters.len() as u16).map(|chapter| ChapterRef { book: b.id, chapter }))
                .collect());
        }
        names.iter().map(|name| self.chapter(name)).collect()
    }

    /// A verse named like "5:3" (in the project's book) or "1 Thess 5:3"
    fn verse(&self, name: &str) -> Result<(ChapterRef, u16)> {
        let (chapter, verse) = name
            .trim()
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("{name}: not a verse (\"5:3\")"))?;
        let c = self.chapter(chapter)?;
        let count = self
            .bible
            .book(c.book)
            .map_or(0, |b| b.chapters[c.chapter as usize - 1].len());
        match verse.parse::<u16>() {
            Ok(v) if v >= 1 && v as usize <= count => Ok((c, v)),
            _ => bail!("{name}: the chapter has {count} verses"),
        }
    }

    /// A verse's readings, best first
    fn readings(&self, c: &ChapterRef, verse: u16) -> Vec<&extract_audio_bible::Match> {
        let mut found: Vec<_> = (self.matches.matches.iter())
            .filter(|m| (m.book, m.chapter, m.verse) == (c.book, c.chapter, verse))
            .collect();
        found.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.start.total_cmp(&b.start)));
        found
    }

    fn chapter(&self, name: &str) -> Result<ChapterRef> {
        let name = name.trim();
        let (book, number) = match name.rsplit_once([' ', ':']) {
            Some((book, number)) => (Some(book_id(&self.bible, book)?), number),
            None => (self.project.book, name),
        };
        let book =
            book.ok_or_else(|| anyhow!("{name}: name the book too (\"John 3\"), as the project is the whole Bible"))?;
        let chapter: u16 = number.parse().map_err(|_| anyhow!("{name}: not a chapter"))?;
        let b = self
            .bible
            .book(book)
            .ok_or_else(|| anyhow!("{name}: the Bible has no such book"))?;
        if chapter == 0 || chapter as usize > b.chapters.len() {
            bail!("{name}: {} has {} chapters", b.name, b.chapters.len());
        }
        Ok(ChapterRef { book, chapter })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Found {
    transcript: String,
    matches: Vec<extract_audio_bible::Match>,
    runs: Vec<extract_audio_bible::Run>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Searched {
    sermons: usize,
    readings: usize,
    errors: Vec<String>,
}

fn search(p: &mut Folder, out: &Out) -> Result<Searched> {
    let engine = matcher::engine_of(p.translations())?;
    let analysis = extract_audio_bible::analyze_with(&mut p.project, &engine, &Disk, &|done, total, label| {
        out.note(format!("[{done}/{total}] {label}"));
    })?;
    let errors: Vec<String> = analysis
        .errors
        .iter()
        .map(|(label, e)| format!("{label}: {e}"))
        .collect();
    for e in &errors {
        out.note(format!("couldn't search {e}"));
    }
    p.matches = analysis.matches;
    p.searched = true;
    Ok(Searched {
        sermons: p.project.sources.len(),
        readings: p.matches.matches.len(),
        errors,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Paragraph {
    start: f64,
    end: f64,
    /// Its words, with the readings that begin in it marked
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Sermon {
    id: String,
    file_id: String,
    label: String,
    title: Option<String>,
    passage: Option<String>,
    speaker: Option<String>,
    series: Option<String>,
    audio_path: String,
    transcript_path: String,
    readings: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    number: usize,
    /// Where it's played among the verse's chosen readings (1 first), if chosen
    chosen: Option<usize>,
    reading: extract_audio_bible::Match,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Chosen {
    book: u8,
    chapter: u16,
    verses: Vec<u16>,
}

fn choose(p: &mut Folder, chapters: &[ChapterRef], min_score: f32, replace: bool) -> Result<Vec<Chosen>> {
    if p.matches.matches.is_empty() {
        bail!("no readings yet: run `search` first");
    }
    let mut chosen = vec![];
    for c in chapters {
        let book = p.bible.book(c.book).ok_or_else(|| anyhow!("no book {}", c.book))?;
        if replace {
            let prefix = format!("{}.{}.", c.book, c.chapter);
            p.project.selections.retain(|k, _| !k.starts_with(&prefix));
        }
        let verses = p.project.choose_best(book, c.chapter, &p.matches, min_score);
        if !verses.is_empty() {
            chosen.push(Chosen {
                book: c.book,
                chapter: c.chapter,
                verses,
            });
        }
    }
    Ok(chosen)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChapterStatus {
    /// "John 3"
    name: String,
    #[serde(flatten)]
    status: extract_audio_bible::ChapterStatus,
}

fn status(p: &Folder) -> Vec<ChapterStatus> {
    (p.project.status(&p.bible, &p.matches).into_iter())
        .map(|status| ChapterStatus {
            name: reference(&p.bible, status.book, status.chapter, None),
            status,
        })
        .collect()
}

fn export_to(
    p: &Folder,
    chapters: &[ChapterRef],
    output: &Path,
    out: &Out,
) -> Result<Vec<extract_audio_bible::ExportedFile>> {
    if chapters.is_empty() {
        bail!("no chapters have chosen verses yet: run `choose` first");
    }
    Ok(export::export_to_dir(
        &p.project,
        &p.bible,
        chapters,
        &p.audio(),
        output,
        |progress| {
            if !progress.current.is_empty() {
                out.note(format!(
                    "[{}/{}] {}",
                    progress.done + 1,
                    progress.total,
                    progress.current
                ));
            }
        },
    )?)
}

fn add_sermons(project: &mut Project, paths: &[PathBuf]) -> Result<extract_audio_bible::Added> {
    let files: Vec<String> = (extract_audio_bible::files::gather(paths)?.into_iter())
        .map(|p| std::path::absolute(&p).unwrap_or(p).to_string_lossy().into_owned())
        .collect();
    Ok(project.add_sources_from(&Disk, &files))
}

/// A Bible imported, and what was left out of it
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Imported {
    #[serde(flatten)]
    summary: bible::BibleSummary,
    warnings: Vec<String>,
}

/// A Bible from its JSON (one `.json` file that `import` wrote) or its files, noting what its
/// files' import left out
fn load_bible(paths: &[PathBuf], out: &Out) -> Result<Bible> {
    if let [path] = paths
        && path.extension().is_some_and(|e| e == "json")
        && let Ok(bible) = read_json::<Bible>(path)
    {
        return Ok(bible);
    }
    let (bible, warnings) = import_bible(paths, "", "")?;
    for warning in &warnings {
        out.note(warning);
    }
    Ok(bible)
}

/// A Bible from its files, and what was left out of it
fn import_bible(paths: &[PathBuf], name: &str, abbreviation: &str) -> Result<(Bible, Vec<String>)> {
    let mut imported = bible::import_paths(paths)?;
    let warnings = std::mem::take(&mut imported.warnings);
    let fallback = paths
        .first()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = if name.trim().is_empty() {
        fallback.as_str()
    } else {
        name.trim()
    };
    let abbreviation = if abbreviation.trim().is_empty() {
        name
    } else {
        abbreviation.trim()
    };
    Ok((Bible::new(stable_id(name), name, abbreviation, imported), warnings))
}

/// An audio Bible of a book ("1 Thessalonians"), some of its verses ("2 Cor 2:14-17"), or (with
/// none) the whole Bible
fn audio_bible(id: String, name: &str, bible: &Bible, reference: Option<&str>) -> Result<Project> {
    Ok(match reference {
        None => Project::new(id, name, bible, None),
        Some(reference) => match extract_audio_bible::parse_reference(reference, bible)? {
            (book, Some(passage)) => Project::passage(id, name, bible, book, passage),
            (book, None) => Project::new(id, name, bible, Some(book)),
        },
    })
}

fn book_id(bible: &Bible, name: &str) -> Result<u8> {
    let id = bible::find_book(name)
        .ok_or_else(|| anyhow!("{name}: not a book of the Bible"))?
        .0;
    if bible.book(id).is_none() {
        bail!("{name}: this translation doesn't have it");
    }
    Ok(id)
}

fn reference(bible: &Bible, book: u8, chapter: u16, verse: Option<u16>) -> String {
    let name = bible.book(book).map_or_else(|| book.to_string(), |b| b.name.clone());
    match verse {
        Some(v) => format!("{name} {chapter}:{v}"),
        None => format!("{name} {chapter}"),
    }
}

/// "1 sermon", "2 sermons"
fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// `1:02:03.4` or `2:03.4`
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let (h, m, rest) = ((s / 3600.0) as u64, ((s % 3600.0) / 60.0) as u64, s % 60.0);
    if h > 0 {
        format!("{h}:{m:02}:{rest:04.1}")
    } else {
        format!("{m}:{rest:04.1}")
    }
}

/// `1-3, 7, 9-12`
fn ranges(verses: &[u16]) -> String {
    let mut parts: Vec<String> = vec![];
    let mut i = 0;
    while i < verses.len() {
        let mut j = i;
        while j + 1 < verses.len() && verses[j + 1] == verses[j] + 1 {
            j += 1;
        }
        parts.push(if i == j {
            verses[i].to_string()
        } else {
            format!("{}-{}", verses[i], verses[j])
        });
        i = j + 1;
    }
    parts.join(", ")
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("reading {}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    // Written beside, then moved, so a crash never leaves half a file
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(value)?)?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}
