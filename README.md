# Extract Audio Bible

A desktop app (Tauri 2, Rust and React/TypeScript with shadcn/ui) that builds an audio Bible from the Scripture readings in your sermons. A project usually covers one book, the one a preacher is preaching through, and can cover the whole Bible.

1. **Projects**: each one has a translation and a book (or the whole Bible), or some of a book's verses (2 Corinthians 2:14-17), exported as one file and an album of its verses. Only that book is searched for in the sermons. A project can be made from the sermons an every-verse project's readings show (a speaker's, say).
2. **Translations**: import the text the sermons read from. USFM, USX, OSIS, ThML and GBF are read with `logoxide-text` (book names and numbers come from `topos-bible`). Your own text also works as JSON, CSV/TSV (`book, chapter, verse, text`), or plain lines like `John 3:16 For God so loved…`.
3. **Sermons**: add audio files with their WhisperX JSON (or Let's Church transcript), or a whole folder. Files are paired by name (`sermon.mp3` goes with `sermon.json`), even when the audio and transcripts are in different folders and added one after the other, and transcripts may be gzipped (`sermon.json.gz`). Titles, passages, speakers, and series can be imported from a CSV or JSON table (keyed or a list, nested fields too), its columns matched to what they are.
4. **Search**: every verse is looked for in every transcript. Results are *exact*, *close* (a word changed, added or skipped) or *partial*, and must be at least as accurate as the project's minimum accuracy (85% by default). Verses read one after another are joined into runs.
5. **Verses**: the navigator shows each chapter's status (complete, partly chosen, needs trimming, has readings), and each verse chosen shows how good its reading is (exact, close, or partial, and its score), marked when a better one was found. For each verse you can listen to every reading with its context, see the transcript around it, choose a reading (or a whole run at once), join readings of a verse read in pieces, trim it on the waveform, add silence before or after it, change its volume, or mark it as still needing a trim. **Choose best** fills a chapter, or every chapter, with the best kind of reading, preferring readings read straight through; **Next to choose** opens the next verse still missing one; and a sermon's readings can be shown alone, to see what it reads through. A chapter plays as it will be exported, and can be paused and gone through verse by verse, or played from any verse.
6. **Search**: find where anything was said in the sermons (words in order, whatever their case or punctuation, numbers spelled out). A place found, here or in a transcript, can be used as a verse's reading (one the search missed, or a piece of a verse read in parts): it's compared with the verse, chosen or joined, and kept when the sermons are searched again.
7. **Export**: tagged MP3s, one per chapter, one per verse, or both (ID3v2.4: title and album templates, artist, narrator, genre, year, copyright, cover art and more). Options include a gap between all verses, fades at cuts, an overall volume, and leveling. Leveling can bring each reading to a target level, or normalize the whole export with one change so readings keep their levels relative to each other.

An **every-verse** project instead finds every verse read in its sermons, in several translations at once (each reading is of the one it fits best), and lists them by sermon or by verse, filtered by reference (completed as it's typed: `2 Co` → `2 Corinthians`), kind, translation, speaker, and series, and exported as CSV or JSON. An audio Bible can be made from the sermons it shows.

Everything is saved in the app's data folder (`~/.local/share/extract-audio-bible` on Linux), or a project's own folder (**Open folder**): Bibles and projects as JSON, and each project's readings in SQLite (`readings.sqlite`), which the app reads a chapter, a sermon, or a page at a time, and which `sqlite3`, Python, or DuckDB can read too.

## Without the app

The app is one front end to a library that does the work, and there are two more, so you can
build audio Bibles in your own workflows:

- **Command line** ([`crates/extract-audio-bible-cli`](crates/extract-audio-bible-cli)): the
  `extract-audio-bible` command, from `import` and `find` to `export`, or all at once with
  `build`. Its projects are folders of the same JSON the app keeps.
- **Python and TypeScript** ([`crates/extract-audio-bible-ffi`](crates/extract-audio-bible-ffi)):
  bindings made with BoltFFI, published as `extract-audio-bible` on PyPI (native) and npm
  (WebAssembly, for browsers, Node, and Bun). Swift and Kotlin are generated too.
- **Rust** ([`crates/extract-audio-bible`](crates/extract-audio-bible)): the library itself.

## Layout

| Folder | What it is |
|---|---|
| `crates/extract-audio-bible` | The library: Bible import, transcripts, matching, projects, choosing, rendering, MP3 export |
| `crates/extract-audio-bible-cli` | The command line |
| `crates/extract-audio-bible-ffi` | The Python and TypeScript bindings (BoltFFI), their README, examples, and pack scripts |
| `src-tauri` | The app's backend: its data folder, caches, and progress events, on top of the library |
| `src` | The app's interface (React, TypeScript, shadcn/ui) |

## Development

```sh
pnpm install
pnpm tauri dev
pnpm tauri build    # target/release/bundle (the AppImage bundles GStreamer, which needs patchelf)
cargo build --release -p extract-audio-bible-cli   # target/release/extract-audio-bible
```

Tests: `cargo test` (every crate). Two ignored end-to-end tests run against a full Bible and a real recording:

```sh
KJV_USFM=/path/to/usfm cargo test -p extract-audio-bible --release e2e -- --ignored --nocapture
KJV_USFM=/path/to/usfm SERMON=/path/to/sermon cargo test -p extract-audio-bible --release export_e2e -- --ignored --nocapture  # sermon.mp3 + sermon.json
```

The bindings are built and checked from their folder (see its README): `npm run pack:wasm`, `npm
run pack:python`, and `python scripts/readme.py check`.

## How matching works

Readings are found with [`scripture-quotes`](crates/scripture-quotes)
(`crates/extract-audio-bible/src/matcher.rs` adapts it): a transcript's word trigrams vote for
verses, each group of votes is aligned word by word so transcription slips still count, the verses
around every reading are looked for directly (which catches very short verses like "Jesus wept"),
and readings of verse after verse are joined into runs. A reading counts when its words are at
least as accurate as the project's **minimum accuracy** (85% by default, 90% for part of a verse; a
near miss like "saith"/"sayeth" counts most of a word, and a comment in the middle of a verse
counts half a word). Cut points fall halfway into the pause around a reading, and inside a run they
meet so the reading plays back without a break.

Transcripts are WhisperX JSON or Let's Church transcripts (`paragraphs`); segments or paragraphs
without word times have their words spread evenly over their time.

A sermon's audio can be a file or a URL (downloaded once, when it's first needed), or an HLS stream
(`.m3u8`, as Let's Church serves it), read a stretch at a time: a reading needs only the few
segments it's in.

MP3s are encoded in pure Rust (`rusty_mp3`), so export works in WebAssembly too.
