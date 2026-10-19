# Extract Audio Bible

A desktop app (Tauri 2, Rust and React/TypeScript with shadcn/ui) that builds an audio Bible from the Scripture readings in your sermons. A project usually covers one book, the one a preacher is preaching through, and can cover the whole Bible.

1. **Projects**: each one has a translation and a book (or the whole Bible). Only that book is searched for in the sermons.
2. **Translations**: import the text the sermons read from. USFM, USX, OSIS, ThML and GBF are read with `logoxide-text` (book names and numbers come from `topos-bible`). Your own text also works as JSON, CSV/TSV (`book, chapter, verse, text`), or plain lines like `John 3:16 For God so loved…`.
3. **Sermons**: add audio files with their WhisperX JSON (or Let's Church transcript), or a whole folder. Files are paired by name (`sermon.mp3` goes with `sermon.json`).
4. **Search**: every verse is looked for in every transcript. Results are *exact*, *close* (a word changed, added or skipped) or *partial*, and must be at least as accurate as the project's minimum accuracy (85% by default). Verses read one after another are joined into runs.
5. **Verses**: the navigator shows each chapter's status (complete, partly chosen, needs trimming, has readings). For each verse you can listen to every reading with its context, see the transcript around it, choose a reading (or a whole run at once), trim it on the waveform, add silence before or after it, change its volume, or mark it as still needing a trim.
6. **Export**: tagged MP3s, one per chapter, one per verse, or both (ID3v2.4: title and album templates, artist, narrator, genre, year, copyright, cover art and more). Options include a gap between all verses, fades at cuts, an overall volume, and leveling. Leveling can bring each reading to a target level, or normalize the whole export with one change so readings keep their levels relative to each other.

Everything is saved as JSON in the app's data folder (`~/.local/share/extract-audio-bible` on Linux).

## Development

```sh
pnpm install
pnpm tauri dev
```

Tests: `cd src-tauri && cargo test`. Two ignored end-to-end tests run against a full Bible and a real recording:

```sh
KJV_USFM=/path/to/usfm cargo test --release e2e -- --ignored --nocapture
KJV_USFM=/path/to/usfm SERMON=/path/to/sermon cargo test --release export_e2e -- --ignored --nocapture  # sermon.mp3 + sermon.json
```

## How matching works

Readings are found with [`scripture-quotes`](../../rust/scripture-quotes) (`src-tauri/src/matcher.rs`
adapts it): a transcript's word trigrams vote for verses, each group of votes is aligned word by word
so transcription slips still count, the verses around every reading are looked for directly (which
catches very short verses like "Jesus wept"), and readings of verse after verse are joined into runs.
A reading counts when its words are at least as accurate as the project's **minimum accuracy**
(85% by default, 90% for part of a verse; a near miss like "saith"/"sayeth" counts most of a word, and a comment in the middle
of a verse counts half a word). This app gives the readings their times: cut points fall halfway into
the pause around a reading, and inside a run they meet so the reading plays back without a break.

Transcripts are WhisperX JSON or Let's Church transcripts (`paragraphs`); segments or paragraphs
without word times have their words spread evenly over their time.
