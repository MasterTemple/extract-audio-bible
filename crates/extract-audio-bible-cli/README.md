# extract-audio-bible (command line)

Build an audio Bible from the Scripture read in sermons, from the command line: what the Extract
Audio Bible app does, for scripts and workflows of your own.

```sh
cargo install --path crates/extract-audio-bible-cli   # the `extract-audio-bible` command
```

## All at once

```sh
extract-audio-bible build --bible bsb/ --book "1 Thessalonians" sermons/ -o audio-bible/ --project 1thess/
```

reads the Bible (a folder of USFM, USX, OSIS, ThML, or GBF, a JSON, CSV, or TSV file, or lines
like `John 3:16 For God so loved…`), pairs each sermon's audio with its transcript (`sunday.mp3`
with `sunday.json`, WhisperX or Let's Church), finds every reading of the book's verses, chooses
the best for each verse, and exports a tagged MP3 per chapter. With `--project`, the project is
kept to look at, change, and export again.

## A step at a time

A project is a folder holding `project.json` (its sermons, chosen readings, settings, and export
metadata), `matches.json` (every reading found), and `bible.json`: the same formats the app keeps,
so they can be read and edited by anything.

```sh
extract-audio-bible import bsb/ -o bsb.json --name "Berean Standard Bible" --abbreviation BSB
extract-audio-bible init 1thess --bible bsb.json --book "1 Thess"
extract-audio-bible add 1thess sermons/              # audio files with their transcripts
extract-audio-bible sermons 1thess --info info.json  # titles and passages by sermon id
extract-audio-bible search 1thess                    # every reading of every verse
extract-audio-bible choose 1thess --min-score 0.75   # the best for each verse without one
extract-audio-bible status 1thess                    # each chapter's chosen and missing verses
extract-audio-bible readings 1thess 5:15             # a verse's readings, numbered
extract-audio-bible select 1thess 5:15 4 3           # its first half, then the rest read later
extract-audio-bible set 1thess gapBetweenVerses=0.3 metadata.artist="Grace Church" metadata.exportMode='"both"'
extract-audio-bible render 1thess --chapter 5 -o 5.wav   # hear a chapter before exporting
extract-audio-bible export 1thess -o audio-bible/
```

| Command | What it does |
|---|---|
| `import PATHS… -o FILE` | Reads a Bible's text and saves it as JSON |
| `find --bible BIBLE [--book B] TRANSCRIPTS…` | The verses read in transcripts, with their times (no project) |
| `init DIR --bible BIBLE [--book B] [--name N]` | Starts a project for a book (or, with no `--book`, the whole Bible) |
| `add DIR PATHS…` | Adds sermons: audio files with the transcript of the same name, from files or folders |
| `sermons DIR [--info FILE]` | Lists the sermons with their ids, titles, and passages; `--info` first reads titles and passages by id from JSON (`{"<id>": {"title": "…", "passage": "…"}}`), field by field. A sermon's id is the one its transcript gives (a Let's Church media id) once searched, or its file's name |
| `search DIR` | Finds every reading of the project's verses in its sermons |
| `choose DIR [--chapter C]… [--min-score S] [--replace]` | Chooses the best reading for each verse without one (0.9 exact, 0.75 exact and close, 0 anything) |
| `status DIR` | Each chapter's chosen and missing verses |
| `readings DIR VERSE` | A verse's readings (`5:15`, or `"1 Thess 5:15"`), numbered best first, marking those chosen |
| `select DIR VERSE READING…` | Chooses a verse's reading by number or id, or several to play one after another in the order given: for a verse read in pieces (its first half, then the rest minutes later) |
| `set DIR KEY=VALUE…` | Changes settings (`minAccuracy=0.8`, `gapBetweenVerses=0.3`) or metadata (`metadata.artist="…"`); values are JSON when they parse as JSON |
| `render DIR --chapter C [--verse V] -o FILE` | A chapter (or one verse) as it will be exported, as WAV (`--json` gives when each verse is heard) |
| `clip AUDIO START END -o FILE [--processed]` | A stretch of a sermon as WAV |
| `export DIR -o FOLDER [--chapter C]… [--mode chapter\|verse\|both]` | Tagged MP3s of the chosen readings |
| `build --bible BIBLE [--book B] SERMONS… -o FOLDER [--project DIR]` | All of the above at once |

A chapter is a number in the project's book (`5`), or a book and a number (`"1 Thess 5"`). Books
go by any of their names (`1 Thessalonians`, `1 Thess`, `1Th`, `52`).

Every command prints JSON instead of text with `--json`, and only results and errors with
`--quiet`; progress goes to stderr. It exits non-zero on an error.
