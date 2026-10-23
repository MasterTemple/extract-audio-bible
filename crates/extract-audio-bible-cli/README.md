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

## Every verse read

`find` gives every verse read in transcripts, without a project, in one translation or several
(each reading of the one it fits best, and noting the others it fits), in the books you choose
by testament, genre (topos's book groups: `Pentateuch`, `Gospels`, `Pauline Epistles`…), and
book:

```sh
extract-audio-bible find --bible esv.json --translation nasb.json --translation kjv/ \
  --testament new --exclude-genre Gospels --csv sermons/*.json > readings.csv
```

Included testaments limit the search; included genres and books add up within them; what's
excluded is left out. With `--csv`, each reading is a row: the sermon, the reference, the
translation, the times, how it was read, and the words said.

To keep what's found, and look through it in the app, make it a project:

```sh
extract-audio-bible init readings --bible esv.json --translation nasb.json --translation kjv/ --testament new
extract-audio-bible add readings transcripts/        # .json or .json.gz
extract-audio-bible sermons readings --info info.json
extract-audio-bible search readings
extract-audio-bible table readings -o readings.csv   # or readings.json
```

The app opens the folder where it is (**Open folder** under Projects), and what it changes there
the command line sees.

## Audio downloaded by id

A sermon can be a transcript alone, its audio downloaded when it's first needed (rendering or
exporting) into the project's `downloads/` folder:

```sh
extract-audio-bible set 1thess 'audioUrl=https://example.com/audio/{id}.mp3'
extract-audio-bible add 1thess transcripts/          # transcripts alone, their audio from the URL
```

`{id}` is the sermon's id: the one its transcript gives (a Let's Church media id) once searched,
or its file's name (`sermons` lists them). `{uuid}` and `{shortId}` are that id as a UUID and as a
short id (base 58, as Let's Church and the `short-uuid` npm package write them), and
`{downloadId}` is a `downloadId` given in `sermons --info` (each is the id when it can't be had).

## A step at a time

A project is a folder holding `project.json` (its sermons, chosen readings, settings, and export
metadata), `readings.sqlite` (every reading found), and `bible.json` (with every verse read, the
other translations it searches are in `bibles/<id>.json`): the same formats the app keeps, so they
can be read by anything, and the app can open the folder as it is. A folder made before keeps its
readings in `matches.json`, which is moved into `readings.sqlite` the first time it's opened.

`readings.sqlite` is SQLite, so `sqlite3`, Python's `sqlite3`, DuckDB, and the like read it as it
is. `readings_table` has every reading as `table` gives them:

```sh
sqlite3 -header -csv readings/readings.sqlite \
  "select reference, translation, sermon, said from readings_table where speaker = 'Zac Poonen'"
```

Its tables are `readings` (each reading; `verse_key` is `book × 1000000 + chapter × 1000 + verse`,
`active` whether its sermon is still in the project), `runs`, `sermons`, `books`, `translations`,
and counts by verse, chapter, and sermon.

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
| `import PATHS… -o FILE` | Reads a Bible's text and saves it as JSON, noting on stderr what it left out (files not read, books passed over; `warnings` with `--json`) |
| `find --bible BIBLE [--translation BIBLE]… [--testament T] [--genre G]… [--book B]… [--exclude-…] [--csv] TRANSCRIPTS…` | The verses read in transcripts, with their times and translations (no project) |
| `init DIR --bible BIBLE [--book B] [--name N]` | Starts a project for a book (or, with no `--book`, the whole Bible), or some of its verses (`--book "2 Cor 2:14-17"`, or across chapters, `"Romans 7:24-8:2"`): a passage is exported as one file, and its verses as an album of their own |
| `init DIR --bible BIBLE --readings [--translation BIBLE]… [--testament T] [--genre G]… [--book B]… [--exclude-…] [--name N]` | Starts a project finding every verse read, in these translations and books (`--translation` alone means `--readings`) |
| `add DIR PATHS…` | Adds sermons from files or folders: audio files with the transcript of the same name, and transcripts alone (to search, or with `audioUrl` set, to download audio for); JSON that isn't a transcript is skipped |
| `sermons DIR [--info FILE]` | Lists the sermons with their ids, titles, passages, speakers, and series; `--info` first reads them by id from JSON (`{"<id>": {"title": "…", "passage": "…", "speaker": "…", "series": "…"}}`), field by field. A sermon's id is the one its transcript gives (a Let's Church media id) once searched, or its file's name |
| `transcript DIR SERMON` | A sermon's whole transcript (by its id or file name), in paragraphs with their times, the readings found marked where they begin (`[1Thess 5:15]` chosen, `<…>` not) |
| `grep DIR PHRASE [--prefix] [--near N [--in-order] [--keep-common]] [--context N] [--limit N] [--speaker S]… [--series S]…` | Where a phrase was said in the sermons (or only those by some speakers, in some series), with its words in order whatever their case, accents, or punctuation, numbers spelled out ("70 times 7" finds "seventy times seven"). With `--near N`, its words (or "quoted phrases") need only be said with at most N words between one and the next, in any order (`--in-order` for the order given), common words like "is" and "the" left out (`--keep-common` keeps them), and `forgiv*` for any word it begins: `grep 1thess 'Jesus God' --near 3` finds "Jesus is God" |
| `search DIR` | Finds every reading of the project's verses in its sermons |
| `table DIR [-o FILE]` | Every reading found, as CSV (JSON with `--json` or a `.json` file): the sermon, its speaker and series, the reference, the translation (and others it fits), the times, how it was read, and the words said |
| `choose DIR [--chapter C]… [--min-score S] [--replace]` | Chooses the best reading for each verse without one (0.9 exact, 0.75 exact and close, 0 anything) |
| `status DIR` | Each chapter's chosen and missing verses |
| `readings DIR VERSE` | A verse's readings (`5:15`, or `"1 Thess 5:15"`), numbered best first, marking those chosen |
| `select DIR VERSE READING…` | Chooses a verse's reading by number or id, or several to play one after another in the order given: for a verse read in pieces (its first half, then the rest minutes later) |
| `set DIR KEY=VALUE…` | Changes settings (`minAccuracy=0.8`, `gapBetweenVerses=0.3`) or metadata (`metadata.artist="…"`); values are JSON when they parse as JSON |
| `render DIR --chapter C [--verse V] -o FILE` | A chapter (or one verse) as it will be exported, as WAV (`--json` gives when each verse is heard) |
| `clip AUDIO START END -o FILE [--processed]` | A stretch of a sermon as WAV |
| `export DIR -o FOLDER [--chapter C]… [--book B]… [--mode chapter\|verse\|both] [--again]` | Tagged MP3s of the chosen readings (of some books alone, with `--book`). Files already in the folder as they'd be made are left as they are (the folder keeps what each was made from in `.extract-audio-bible-export.json`); `--again` makes them all again |
| `build --bible BIBLE [--book B] SERMONS… -o FOLDER [--project DIR]` | All of the above at once |

A chapter is a number in the project's book (`5`), or a book and a number (`"1 Thess 5"`). Books
go by any of their names (`1 Thessalonians`, `1 Thess`, `1Th`, `52`).

Every command prints JSON instead of text with `--json`, and only results and errors with
`--quiet`; progress goes to stderr. It exits non-zero on an error.
