# Create Audio Bible

A desktop app (Tauri 2, Rust and React/TypeScript with shadcn/ui) that builds an audio Bible, one MP3 per chapter, from the Scripture readings in your sermons.

1. **Translations**: import the text the sermons read from. USFM, USX, OSIS, ThML and GBF are read with `logoxide-text` (book names and numbers come from `topos-bible`). Your own text also works as JSON, CSV/TSV (`book, chapter, verse, text`), or plain lines like `John 3:16 For God so loved…`.
2. **Sermons**: add audio files with their WhisperX JSON, or a whole folder. Files are paired by name (`sermon.mp3` goes with `sermon.json`).
3. **Search**: every verse is looked for in every transcript. Results are *exact*, *close* (a word changed, added or skipped) or *partial*. Verses read one after another are joined into runs.
4. **Verses**: the navigator shows each chapter's status (complete, partly chosen, needs trimming, has readings). For each verse you can listen to every reading with its context, see the transcript around it, choose a reading (or a whole run at once), trim it on the waveform, add silence before or after it, or mark it as still needing a trim.
5. **Export**: tagged MP3s (ID3v2.4: title and album templates, artist, narrator, genre, year, copyright, cover art and more), with an optional gap between all verses, leveling, and fades at cuts.

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

## How matching works (`src-tauri/src/matcher.rs`)

Each verse's three-word sequences are indexed, leaving out ones that are too common to be useful. The transcript's three-word sequences then vote for a verse and a position. Each group of votes is aligned word by word with Smith-Waterman, so small transcription errors still count as matches. That alignment gives the exact words read, the verse words that were skipped, and the words the speaker added. Around every reading the verses just before and after are searched for directly, which catches very short verses. Two-word verses ("Jesus wept") are also matched exactly on their own. Cut points fall halfway into the pause around a reading, and inside a run they meet so the reading plays back without a break.
