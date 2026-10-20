# extract-audio-bible

Build an audio Bible from the Scripture read in sermons. Give it a Bible's text and sermons
(audio with a transcript), and it finds every reading of every verse, chooses one per verse (or
lets you), and exports tagged MP3s by chapter or by verse.

This is the library behind the Extract Audio Bible app, its command line
(`extract-audio-bible-cli`), and its bindings for Python and TypeScript
(`extract-audio-bible-ffi`).

## Finding the verses read in a transcript

```rust
use extract_audio_bible::{Bible, Transcript, import, matcher};

let text = "John 11:35 Jesus wept.\nJohn 11:36 Then said the Jews, Behold how he loved him!";
let bible = Bible::new("kjv", "King James Version", "KJV", import(vec![("kjv.txt".into(), text.into())])?);
let transcript = Transcript::parse(
    r#"{"segments": [{"start": 0, "end": 6, "text": "Jesus wept. Then said the Jews, behold how he loved him."}]}"#,
)?;

let engine = matcher::engine(&bible)?;
let searcher = matcher::searcher(&engine, Some(43)).expect("John is a book");
let (readings, runs) = matcher::find_matches(&searcher, &transcript, "sunday", &Default::default());
let verses: Vec<u16> = readings.iter().map(|r| r.verse).collect();
assert_eq!(verses, [35, 36]);
assert_eq!(runs.len(), 1); // read one after the other
# Ok::<(), extract_audio_bible::Error>(())
```

Each reading has the transcript's words it spans, where to cut (in seconds, halfway into the
pauses around it), and how faithfully it was read (`accuracy`, `coverage`, and a `kind`: exact,
close, or partial).

## A whole audio Bible

```rust,no_run
use std::path::{Path, PathBuf};
use extract_audio_bible::{Bible, Disk, Project, analyze, chapters_to_export, export_to_dir, import_paths};

let bible = Bible::new("bsb", "Berean Standard Bible", "BSB", import_paths(&[PathBuf::from("bsb/")])?);
let mut project = Project::new("john", "", &bible, Some(43)); // John
project.add_sources(&["sermons/sunday.mp3".into(), "sermons/sunday.json".into()]);

// Every reading in every sermon (in parallel), then the best for each verse
let found = analyze(&mut project, &bible, &Disk, &|done, total, sermon| println!("[{done}/{total}] {sermon}"))?;
let john = bible.book(43).expect("John");
for chapter in 1..=john.chapters.len() as u16 {
    project.choose_best(john, chapter, &found.matches, 0.75);
}

let chapters = chapters_to_export(&project, &bible);
export_to_dir(&project, &bible, &chapters, &Disk, Path::new("audio-bible"), |_| {})?;
# Ok::<(), extract_audio_bible::Error>(())
```

## What's in it

- [`bible`]: Bibles from USFM, USX, OSIS, ThML, GBF (through `logoxide-text`), JSON, CSV/TSV, or
  `John 3:16 text` lines
- [`transcript`]: WhisperX JSON and Let's Church transcripts; segments without word times have
  their words spread over their time, and each transcript keeps where its paragraphs begin
- [`matcher`]: every reading of every verse, with cut points (verses are found by
  `scripture-quotes`), in one translation or several at once ([`matcher::engine_of`])
- A project can instead find every verse read in its sermons ([`Project::readings`]), in the
  books of a [`BookScope`] (topos's testaments, genres, and books), listed by [`reading_rows`]
- [`said`]: where anything else was said in sermons ([`find_in`], [`search_transcripts`])
- [`project`]: a project's sermons, chosen readings, settings, and export metadata, as plain data
  (serde), and [`analyze`] to search its sermons. A verse read in pieces (its first half, then the
  rest minutes later) is chosen as several readings played one after another
  ([`Project::join`], [`Selection::set_parts`]). What's known about each sermon (its title and
  passage) is kept by its file id, apart from the sermons ([`Project::apply_file_metadata`])
- [`export`]: a chapter as it will sound (and when each verse is heard in it), and tagged MP3s by
  chapter or by verse; [`audio`] decodes MP3, AAC/M4A, WAV, and more (symphonia) and encodes MP3
  (pure Rust)
- [`files`]: everything is read through [`Files`]: the disk ([`Disk`]), or files given as bytes
  ([`Memory`]), so it all works in WebAssembly too. A sermon can be a transcript alone, its audio
  given by a URL with its id ([`Settings::audio_url`]); with the `download` feature,
  `Downloading` fetches such audio once into a folder

Natively, sermons are searched in parallel (rayon); in WebAssembly, one at a time.
