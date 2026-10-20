<!-- source -->
# extract-audio-bible (bindings)
<!-- /source -->
<!-- ts:
# extract-audio-bible for TypeScript and JavaScript
-->
<!-- python:
# extract-audio-bible for Python
-->

Build an audio Bible from the Scripture read in sermons. Give it a Bible's text and sermons
(audio with a transcript), and it finds every reading of every verse, chooses one per verse (or
lets you), and exports tagged MP3s by chapter or by verse.

These bindings are the
[`crates/extract-audio-bible-ffi`](https://github.com/MasterTemple/extract-audio-bible/tree/main/crates/extract-audio-bible-ffi)
of [Extract Audio Bible](https://github.com/MasterTemple/extract-audio-bible) (a desktop app, a
command line, and the Rust library they share), made through
[BoltFFI](https://github.com/boltffi/boltffi): one API for <!-- source -->TypeScript/WebAssembly (npm: `extract-audio-bible`), Python (PyPI: `extract-audio-bible`), Swift, and Kotlin. npm's and PyPI's READMEs are this one in their own language (`scripts/readme.py` writes them).<!-- /source --><!-- ts: TypeScript and JavaScript (this package, WebAssembly in browsers, Node, and Bun), Python (PyPI: `extract-audio-bible`), Swift, and Kotlin. --><!-- python: Python (this package, a native module), TypeScript and JavaScript (npm: `extract-audio-bible`), Swift, and Kotlin. -->

Files come from a `Files` object: the disk, natively, or files the host adds as bytes, by the
paths a project names them by. WebAssembly has no files, so there the host reads them (and saves
what an export makes). Everything else, MP3 export included, works the same in every language.

## The examples

Each feature below has an example in <!-- source -->TypeScript and in Python<!-- /source --><!-- ts: TypeScript --><!-- python: Python -->. They run in order, as one program: each uses what the ones before it made (`eab`, `bsb`, `files`, `project`, ...). What a line prints is written after it, following `→` (`…` stands for text left out).

They're checked: <!-- source -->`examples/node/readme.ts` and `examples/python/readme.py` run every example here and compare what it prints with its `→` comments.<!-- /source --><!-- ts: [`examples/node/readme.ts`](https://github.com/MasterTemple/extract-audio-bible/blob/main/crates/extract-audio-bible-ffi/examples/node/readme.ts) runs every example here and compares what it prints with its `→` comments. --><!-- python: [`examples/python/readme.py`](https://github.com/MasterTemple/extract-audio-bible/blob/main/crates/extract-audio-bible-ffi/examples/python/readme.py) runs every example here and compares what it prints with its `→` comments. -->
They run in a folder holding `examples/fixtures`: John 11 from the Berean Standard Bible (public
domain) as lines of text, and a short made-up sermon that reads John 11:33-36, as a WhisperX
transcript and an MP3 (a tone where each word is said).

- [Loading](#loading)
- [A Bible](#a-bible)
- [Finding verses in a transcript](#finding-verses-in-a-transcript)
- [A project](#a-project)
- [Choosing readings](#choosing-readings)
- [Settings and metadata](#settings-and-metadata)
- [Listening](#listening)
- [Exporting](#exporting)
- [Saving and opening](#saving-and-opening)
- [Reading natively](#reading-natively)
- [Errors](#errors)
- [Every export](#every-export)
- <!-- source -->[Building](#building)<!-- /source --><!-- ts: [Source and building](#source-and-building) --><!-- python: [Source and building](#source-and-building) -->

## Loading

<!-- source -->
In Node and Bun the WebAssembly module is ready on import (in a browser, `await initialized`
first). Names are camelCase in TypeScript and snake_case in Python. Records are plain objects in
TypeScript and frozen dataclasses in Python, every field given; enums are numbers in TypeScript
(`ReadingKind.Exact` is `0`) and `IntEnum`s in Python (`ReadingKind.EXACT`). Bytes are
`Uint8Array` in TypeScript and `bytes` in Python.
<!-- /source -->
<!-- ts:
In Node and Bun the WebAssembly module is ready on import (in a browser, `await initialized`
first). Records are plain objects, every field given; enums are numbers (`ReadingKind.Exact` is
`0`); bytes are `Uint8Array`.
-->
<!-- python:
Records are frozen dataclasses, every field given (`dataclasses.replace` makes a changed copy);
enums are `IntEnum`s (`ReadingKind.EXACT`); bytes are `bytes`.
-->

`ExtractAudioBible` is where everything starts.

```ts
import { ExtractAudioBible } from "extract-audio-bible";

const eab = ExtractAudioBible.new();
console.log(eab.version()); // → 0.1.0
```

```python
from extract_audio_bible import ExtractAudioBible

eab = ExtractAudioBible()
print(eab.version())  # → 0.1.0
```

## A Bible

`importBible` reads a translation from its files, given as bytes: USFM, USX, OSIS, ThML, GBF,
JSON, CSV/TSV (`book, chapter, verse, text`), or lines like `John 3:16 For God so loved…`. Each
file's name says what it is. `toJson` saves it, and `readBible` reads it again (or a Bible the
Extract Audio Bible app imported). `importWarnings` says what the import left out, a sentence
each: files not read (of another format than the one read, or with no verses found), books that
couldn't be read, and books passed over. Books are numbered 1 (Genesis) to 66 (Revelation); `bookId`
reads any of a book's names.

```ts
import { readFileSync } from "node:fs";

const bsb = eab.importBible(
  [{ path: "bsb-john-11.txt", bytes: readFileSync("bsb-john-11.txt") }],
  "Berean Standard Bible",
  "BSB",
);
const john = bsb.books()[0];
console.log(john.name, john.id, john.verseCounts.length, john.verseCounts[10]); // → John 43 11 57
console.log(bsb.verse(43, 11, 35)); // → Jesus wept.
console.log(bsb.importWarnings()); // → []
console.log(bsb.chapter(43, 11).length); // → 57
console.log(eab.bookId("1 Thess")); // → 52
console.log(eab.readBible(bsb.toJson()).abbreviation()); // → BSB
```

```python
from pathlib import Path
from extract_audio_bible import InputFile

bsb = eab.import_bible(
    [InputFile(path="bsb-john-11.txt", bytes=Path("bsb-john-11.txt").read_bytes())],
    "Berean Standard Bible",
    "BSB",
)
john = bsb.books()[0]
print(john.name, john.id, len(john.verse_counts), john.verse_counts[10])  # → John 43 11 57
print(bsb.verse(43, 11, 35))  # → Jesus wept.
print(bsb.import_warnings())  # → []
print(len(bsb.chapter(43, 11)))  # → 57
print(eab.book_id("1 Thess"))  # → 52
print(eab.read_bible(bsb.to_json()).abbreviation())  # → BSB
```

## Finding verses in a transcript

`readTranscript` reads WhisperX JSON, or a Let's Church transcript (paragraphs, with or without
word times: a segment without them has its words spread over its time). A `searcher` finds the
verses read in a transcript, without a project: one book's (any of its names) or, with `null`,
the whole Bible's, read at least `minAccuracy` faithfully (0.85 is a good start). Each reading has
its verse, the transcript's words it spans, where to cut (in seconds), and how well it was read;
verses read one after another are a run. A transcript's `paragraphStarts` say where each of its
paragraphs (or segments) begins, by word, to show it whole; `id` and `title` are what it says of
its sermon (a Let's Church transcript's media id and title), if anything. `find` gives where a
phrase was said, with words of context: its words in order, compared as verses are (case,
accents, and punctuation aside, numerals spelled out), the last one the start of a word if
`prefix`.

```ts
import { ReadingKind } from "extract-audio-bible";

const transcript = eab.readTranscript(readFileSync("sermon.json", "utf8"));
console.log(transcript.wordCount(), transcript.duration().toFixed(1)); // → 72 39.3
const first = transcript.words()[0];
console.log(first.text, first.start, first.end, first.timed); // → good 0.8 1.2 true
console.log(transcript.paragraphStarts().slice(0, 3).join(" "), transcript.id(), transcript.title()); // → 0 12 26 null null

const found = eab.searcher(bsb, "John", 0.85).find(transcript, "sunday");
for (const r of found.readings) {
  console.log(`${r.chapter}:${r.verse}`, r.kind === ReadingKind.Exact, r.accuracy, r.start.toFixed(2), r.end.toFixed(2));
}
// → 11:33 true 1 6.93 19.09
// → 11:34 true 1 19.09 26.49
// → 11:35 true 1 26.49 27.99
// → 11:36 true 1 27.99 32.54
// → 11:35 true 1 34.54 35.52
const run = found.runs[0];
console.log(`${run.startChapter}:${run.startVerse}-${run.endVerse}`, run.readingIds.length); // → 11:33-36 4

const said = transcript.find("Jesus wept", "sunday", { prefix: false, context: 3, limit: 0 });
console.log(said.length, said[1].before, "|", said[1].text, "|", said[1].after, said[1].wordStart); // → 2 amen. notice that | jesus wept | even though he 62
console.log(transcript.find("John 11", "sunday", { prefix: false, context: 0, limit: 0 })[0].text); // → john eleven
```

```python
from extract_audio_bible import FindOptions, ReadingKind

transcript = eab.read_transcript(Path("sermon.json").read_text())
print(transcript.word_count(), f"{transcript.duration():.1f}")  # → 72 39.3
first = transcript.words()[0]
print(first.text, first.start, first.end, first.timed)  # → good 0.8 1.2 True
print(transcript.paragraph_starts()[:3], transcript.id(), transcript.title())  # → [0, 12, 26] None None

found = eab.searcher(bsb, "John", 0.85).find(transcript, "sunday")
for r in found.readings:
    print(f"{r.chapter}:{r.verse}", r.kind == ReadingKind.EXACT, r.accuracy, f"{r.start:.2f}", f"{r.end:.2f}")
# → 11:33 True 1.0 6.93 19.09
# → 11:34 True 1.0 19.09 26.49
# → 11:35 True 1.0 26.49 27.99
# → 11:36 True 1.0 27.99 32.54
# → 11:35 True 1.0 34.54 35.52
run = found.runs[0]
print(f"{run.start_chapter}:{run.start_verse}-{run.end_verse}", len(run.reading_ids))  # → 11:33-36 4

said = transcript.find("Jesus wept", "sunday", FindOptions(prefix=False, context=3, limit=0))
print(len(said), said[1].before, "|", said[1].text, "|", said[1].after, said[1].word_start)  # → 2 amen. notice that | jesus wept | even though he 62
print(transcript.find("John 11", "sunday", FindOptions(prefix=False, context=0, limit=0))[0].text)  # → john eleven
```

The last reading is the sermon saying "Jesus wept" again after reading the passage.

## A project

A project builds one book (or, with `null`, the whole Bible) from sermons. `addSources` takes the
sermons' files by path, pairing each audio file (mp3, m4a, wav, flac, ogg, aac, or mp4) with the
transcript (`.json`) of the same name; a transcript without audio is a sermon too, to search and
find readings in. `addSourcesFrom` does the same reading each JSON file through `files` first,
leaving out (`skipped`) any that isn't a transcript. `search` reads the transcripts through
`files` and finds every reading. Here the files are in memory, by the paths the project names
them by.

Each sermon has a file id: the id its transcript gives (a Let's Church transcript's media id) once
it's searched, or its file's name. What's known about a sermon (its title, the passage it
preaches, its speaker, and its series) is kept by file id, apart from the sermons: `setFileMetadata` sets it for one (even one
not added yet), and `applyFileMetadataJson` for many, field by field. A transcript's own title is
used when none is given.

A sermon that's a transcript alone can have its audio downloaded when it's needed: the setting
`audioUrl` says where from, with `{id}` for the file id (`https://example.com/audio/{id}.mp3`),
and with one set its `audioPath` is the URL (it's empty without one). Natively,
`Files.downloading(folder)` downloads it once into a folder (failing with a `Network` error); in
WebAssembly, fetch it and `add` it to memory files under that URL.

`searchTranscripts` finds a phrase in every sermon of the project (as `Transcript.find` does in
one), reading their transcripts through `files`.

```ts
import { Files } from "extract-audio-bible";

const files = Files.memory();
files.add("sermons/sunday.mp3", readFileSync("sermon.mp3"));
files.add("sermons/sunday.json", readFileSync("sermon.json"));
console.log(files.paths().join(", ")); // → sermons/sunday.json, sermons/sunday.mp3

const project = eab.newProject(bsb, "", "John");
console.log(project.name(), project.book()); // → John (BSB) 43
const added = project.addSourcesFrom(files, files.paths());
console.log(added.added, added.unpaired.length, added.skipped.length); // → 1 0 0
const summary = project.search(bsb, files);
console.log(summary.sermons, summary.readings, summary.errors.length); // → 1 5 0
const sermon = project.sources()[0];
console.log(sermon.label, sermon.wordCount, sermon.readingCount); // → sunday 72 5
console.log(project.runs().length, project.readings().length); // → 1 5

project.applyFileMetadataJson('{"sunday": {"title": "Jesus Wept", "passage": "John 11:33-36", "speaker": "Pastor Jim", "series": "John"}}');
const info = project.sources()[0];
console.log(info.fileId, info.title, info.passage); // → sunday Jesus Wept John 11:33-36
console.log(info.speaker, info.series); // → Pastor Jim John
project.setFileMetadata("sunday", { ...project.fileMetadata("sunday"), title: null });
console.log(project.sources()[0].title, project.fileMetadata("sunday").passage); // → null John 11:33-36

project.applyJson('{"settings": {"audioUrl": "https://example.com/audio/{id}.mp3"}}');
console.log(project.addSources(["sermons/later.json"]).added, project.sources()[1].audioPath); // → 1 https://example.com/audio/later.mp3
project.removeSource(project.sources()[1].id);
project.applyJson('{"settings": {"audioUrl": ""}}');

const where = project.searchTranscripts(files, "deeply mov", { prefix: true, context: 0, limit: 0 });
console.log(where.found.length, where.found[0].text, where.found[0].start.toFixed(2), where.errors.length); // → 1 deeply moved 15.86 0
```

```python
import dataclasses

from extract_audio_bible import Files

files = Files.memory()
files.add("sermons/sunday.mp3", Path("sermon.mp3").read_bytes())
files.add("sermons/sunday.json", Path("sermon.json").read_bytes())
print(", ".join(files.paths()))  # → sermons/sunday.json, sermons/sunday.mp3

project = eab.new_project(bsb, "", "John")
print(project.name(), project.book())  # → John (BSB) 43
added = project.add_sources_from(files, files.paths())
print(added.added, len(added.unpaired), len(added.skipped))  # → 1 0 0
summary = project.search(bsb, files)
print(summary.sermons, summary.readings, len(summary.errors))  # → 1 5 0
sermon = project.sources()[0]
print(sermon.label, sermon.word_count, sermon.reading_count)  # → sunday 72 5
print(len(project.runs()), len(project.readings()))  # → 1 5

project.apply_file_metadata_json('{"sunday": {"title": "Jesus Wept", "passage": "John 11:33-36", "speaker": "Pastor Jim", "series": "John"}}')
info = project.sources()[0]
print(info.file_id, info.title, info.passage)  # → sunday Jesus Wept John 11:33-36
print(info.speaker, info.series)  # → Pastor Jim John
project.set_file_metadata("sunday", dataclasses.replace(project.file_metadata("sunday"), title=None))
print(project.sources()[0].title, project.file_metadata("sunday").passage)  # → None John 11:33-36

project.apply_json('{"settings": {"audioUrl": "https://example.com/audio/{id}.mp3"}}')
print(project.add_sources(["sermons/later.json"]).added, project.sources()[1].audio_path)  # → 1 https://example.com/audio/later.mp3
project.remove_source(project.sources()[1].id)
project.apply_json('{"settings": {"audioUrl": ""}}')

where = project.search_transcripts(files, "deeply mov", FindOptions(prefix=True, context=0, limit=0))
print(len(where.found), where.found[0].text, f"{where.found[0].start:.2f}", len(where.errors))  # → 1 deeply moved 15.86 0
```

## Choosing readings

`chooseBest` chooses, for every verse without one, its best reading scoring at least `minScore`
(0.9 exact, 0.75 exact and close, 0 anything), preferring one that carries on from the verse
before; `chooseBestIn` does one chapter. `select` chooses a reading by id, and `updateSelection`
changes a verse's trims (seconds added to each cut), silence around it, volume, or note. `status`
says how far along each chapter is.

A verse read in pieces (its first half, then the rest minutes later) is chosen as several
readings, played one after another: `join` adds a reading to its verse's choice (before any part
that begins later in the verse), `arrange` puts them in another order, and `unjoin` takes one out.
The parts after the first are the choice's `joined`. Here the sermon's second "Jesus wept" shows
how.

```ts
const choices = project.chooseBest(bsb, 0.75);
console.log(choices.map((c) => `${c.chapter}: ${c.verses.join(" ")}`).join("; ")); // → 11: 33 34 35 36

const status = project.status(bsb).find((s) => s.chapter === 11)!;
console.log(status.verses, status.chosen, status.withReadings, status.missing.length); // → 57 4 4 53

const best = project.verseReadings(43, 11, 35)[0];
const chosen = project.selection(43, 11, 35)!;
console.log(chosen.readingId === best.id, chosen.start.toFixed(2), chosen.end.toFixed(2)); // → true 26.49 27.99
project.updateSelection({ ...chosen, padBefore: 0.5, gainDb: 2 });
console.log(project.selection(43, 11, 35)!.padBefore); // → 0.5

console.log(project.unselect(43, 11, 36), project.selections().length); // → true 3
console.log(project.select(project.verseReadings(43, 11, 36)[0].id).verse); // → 36
console.log(project.chooseBestIn(bsb, 43, 11, 0.75).length); // → 0

const again = project.verseReadings(43, 11, 35)[1];
const joined = project.join(again.id);
console.log(joined.joined.length, joined.joined[0].start.toFixed(2)); // → 1 34.54
console.log(project.arrange(43, 11, 35, [again.id, best.id]).readingId === again.id); // → true
console.log(project.unjoin(again.id)?.joined.length); // → 0
```

```python
choices = project.choose_best(bsb, 0.75)
print("; ".join(f"{c.chapter}: {' '.join(map(str, c.verses))}" for c in choices))  # → 11: 33 34 35 36

status = next(s for s in project.status(bsb) if s.chapter == 11)
print(status.verses, status.chosen, status.with_readings, len(status.missing))  # → 57 4 4 53

best = project.verse_readings(43, 11, 35)[0]
chosen = project.selection(43, 11, 35)
print(chosen.reading_id == best.id, round(chosen.start, 2), round(chosen.end, 2))  # → True 26.49 27.99
project.update_selection(dataclasses.replace(chosen, pad_before=0.5, gain_db=2.0))
print(project.selection(43, 11, 35).pad_before)  # → 0.5

print(project.unselect(43, 11, 36), len(project.selections()))  # → True 3
print(project.select(project.verse_readings(43, 11, 36)[0].id).verse)  # → 36
print(len(project.choose_best_in(bsb, 43, 11, 0.75)))  # → 0

again = project.verse_readings(43, 11, 35)[1]
joined = project.join(again.id)
print(len(joined.joined), f"{joined.joined[0].start:.2f}")  # → 1 34.54
print(project.arrange(43, 11, 35, [again.id, best.id]).reading_id == again.id)  # → True
print(len(project.unjoin(again.id).joined))  # → 0
```

## Settings and metadata

`settings` are how a project searches and puts verses together (minimum accuracy, silence,
leveling); `metadata` is how an export is named and tagged. Templates fill in `{book}`, `{osis}`,
`{bookNumber}`, `{bookNumber2}`, `{chapter}`, `{chapter2}`, `{chapter3}`, `{chapters}`,
`{verse}`, `{verse2}`, `{verse3}`, `{verses}`, `{translation}`, and `{abbreviation}`.
`applyJson` changes either by the names the project's JSON uses, so hosts can pass options
through; an unknown name is an error.

```ts
import { ExportMode, Leveling } from "extract-audio-bible";

const settings = project.settings();
console.log(settings.minAccuracy, settings.leveling === Leveling.EachReading, settings.chapterLeadIn); // → 0.85 true 0.5
project.setSettings({ ...settings, gapBetweenVerses: 0.25, leveling: Leveling.WholeExport });

const metadata = project.metadata();
console.log(metadata.titleTemplate, "|", metadata.filenameTemplate, "|", metadata.bitrate); // → {book} {chapter} | {bookNumber2}_{osis}_{chapter3} | 96
project.setMetadata({ ...metadata, artist: "Grace Church", exportMode: ExportMode.Both });

project.applyJson('{"metadata": {"year": "2026"}, "settings": {"chapterTail": 2}}');
console.log(project.metadata().year, project.settings().chapterTail); // → 2026 2
```

```python
from extract_audio_bible import ExportMode, Leveling

settings = project.settings()
print(settings.min_accuracy, settings.leveling == Leveling.EACH_READING, settings.chapter_lead_in)  # → 0.85 True 0.5
project.set_settings(dataclasses.replace(settings, gap_between_verses=0.25, leveling=Leveling.WHOLE_EXPORT))

metadata = project.metadata()
print(metadata.title_template, "|", metadata.filename_template, "|", metadata.bitrate)  # → {book} {chapter} | {bookNumber2}_{osis}_{chapter3} | 96
project.set_metadata(dataclasses.replace(metadata, artist="Grace Church", export_mode=ExportMode.BOTH))

project.apply_json('{"metadata": {"year": "2026"}, "settings": {"chapterTail": 2}}')
print(project.metadata().year, project.settings().chapter_tail)  # → 2026 2.0
```

## Listening

`renderClip` gives a stretch of a sermon as WAV (mono, at the file's own rate, unless `processed`
levels and fades it as an export will). `renderChapter` gives a chapter as it will be exported,
and `chapterTimeline` when each verse is heard in it, to follow along; `renderVerse` gives a verse
as its own file will be (every part of it, with its silence), or `null` if nothing is chosen.

```ts
import type { ClipOptions } from "extract-audio-bible";

const options: ClipOptions = { padBefore: 0, padAfter: 0, processed: true, targetDb: -20, gainDb: 0 };
const clip = eab.renderClip(files, "sermons/sunday.mp3", best.start, best.end, options);
console.log(new TextDecoder().decode(clip.slice(0, 4)), clip.length > 44); // → RIFF true

for (const t of project.chapterTimeline(bsb, 43, 11)) console.log(t.verse, t.start.toFixed(2), t.end.toFixed(2));
// → 33 0.50 12.66
// → 34 12.91 20.31
// → 35 21.06 22.56
// → 36 22.81 27.36
const wav = project.renderChapter(bsb, files, 43, 11);
console.log(new TextDecoder().decode(wav.slice(0, 4))); // → RIFF
console.log(new TextDecoder().decode(project.renderVerse(files, 43, 11, 35)!.slice(0, 4))); // → RIFF
```

```python
from extract_audio_bible import ClipOptions

options = ClipOptions(pad_before=0.0, pad_after=0.0, processed=True, target_db=-20.0, gain_db=0.0)
clip = eab.render_clip(files, "sermons/sunday.mp3", best.start, best.end, options)
print(clip[:4].decode(), len(clip) > 44)  # → RIFF True

for t in project.chapter_timeline(bsb, 43, 11):
    print(t.verse, f"{t.start:.2f}", f"{t.end:.2f}")
# → 33 0.50 12.66
# → 34 12.91 20.31
# → 35 21.06 22.56
# → 36 22.81 27.36
wav = project.render_chapter(bsb, files, 43, 11)
print(wav[:4].decode())  # → RIFF
print(project.render_verse(files, 43, 11, 35)[:4].decode())  # → RIFF
```

## Exporting

`export` makes tagged MP3s of the given chapters, or (with `null`) of every chapter with a chosen
verse (`chaptersToExport`), by chapter, by verse, or both, as the metadata says, and returns them
with their bytes and their paths under the folder they'd go in.

```ts
console.log(project.chaptersToExport(bsb).map((c) => `${c.book}.${c.chapter}`).join(" ")); // → 43.11
const exported = project.export(bsb, files, null);
for (const f of exported) console.log(f.path, f.verse, f.duration.toFixed(1), new TextDecoder().decode(f.bytes.slice(0, 3)));
// → 43 John/43_John_011.mp3 null 29.4 ID3
// → 43 John/John 11/43_John_011_033.mp3 33 12.2 ID3
// → 43 John/John 11/43_John_011_034.mp3 34 7.4 ID3
// → 43 John/John 11/43_John_011_035.mp3 35 2.0 ID3
// → 43 John/John 11/43_John_011_036.mp3 36 4.5 ID3
console.log(exported[0].missing.slice(0, 5).join(" ")); // → 1 2 3 4 5
```

```python
print(" ".join(f"{c.book}.{c.chapter}" for c in project.chapters_to_export(bsb)))  # → 43.11
exported = project.export(bsb, files, None)
for f in exported:
    print(f.path, f.verse, f"{f.duration:.1f}", f.bytes[:3].decode())
# → 43 John/43_John_011.mp3 None 29.4 ID3
# → 43 John/John 11/43_John_011_033.mp3 33 12.2 ID3
# → 43 John/John 11/43_John_011_034.mp3 34 7.4 ID3
# → 43 John/John 11/43_John_011_035.mp3 35 2.0 ID3
# → 43 John/John 11/43_John_011_036.mp3 36 4.5 ID3
print(" ".join(map(str, exported[0].missing[:5])))  # → 1 2 3 4 5
```

## Every verse read

A project can find every verse read in its sermons instead of building an audio Bible: in any of
several translations, each reading of the one it fits best (`translation`) and noting the others
it fits as faithfully (`alsoIn`), in the books of a scope. A scope is topos's filters: included
testaments limit it, included genres (`genres` lists them) and books add up within them, and what
it excludes is left out; `scopeBooks` says which books it searches (`null` for every book).
`translations()` starts a set of translations, `withBible` adds to it, and its `searcher` finds
verses without a project. `readingTable` and `readingsCsv`
give every reading with the words said.

Words that several verses share are one reading at most. A verse whose chapter the sermon quotes
elsewhere is the one; otherwise a whole verse worded the same as others (Psalm 14:1 and 53:1), or
a long enough part of one in rare enough words, is kept once with the others in `alsoAt`, and a
shorter, commoner phrase ("according to the will of God") isn't a reading at all.

```ts
import { ProjectKind, Testament, type BookScope } from "extract-audio-bible";

const kjv = eab.importBible([{ path: "kjv-john-11.txt", bytes: readFileSync("kjv-john-11.txt") }], "King James Version", "KJV");
const both = eab.translations().withBible(bsb).withBible(kjv);
const gospels: BookScope = { testaments: [], genres: ["Gospels"], books: [], excludeTestaments: [], excludeGenres: [], excludeBooks: ["Mark"] };
console.log(eab.scopeBooks(gospels)?.join(" "), eab.genres()[0].name); // → 40 42 43 Pentateuch
console.log(eab.scopeBooks({ ...gospels, testaments: [Testament.New], genres: [], excludeBooks: [] })?.length); // → 27

const everything = eab.newReadingsProject(both, "", gospels);
console.log(everything.name(), everything.addSources(["sermons/sunday.json"]).added); // → Verses read (BSB, KJV) 1
console.log(everything.kind() === ProjectKind.Readings, everything.translations().length, both.ids().length); // → true 2 2
everything.setScope({ ...gospels, excludeBooks: [] });
console.log(everything.scope().genres.join(), everything.scope().excludeBooks.length); // → Gospels 0
console.log(everything.searchIn(both, files).readings); // → 5
for (const r of everything.readingTable(both, files).slice(1, 4)) console.log(r.reference, r.translation, r.alsoIn.join(" "), "|", r.said);
// → John 11:34 BSB  | “Where have you put him?” He asked. “Come and see, Lord,” they answered.
// → John 11:35 BSB KJV | Jesus wept.
// → John 11:36 BSB  | Then the Jews said, “See how He loved him!”
console.log(everything.readingsCsv(both, files).split("\n")[0]); // → sermon,sermon id,speaker,series,reference,book,chapter,verse,also at,translation,also in,start,end,kind,accuracy,coverage,said
```

```python
from extract_audio_bible import BookScope, ProjectKind, Testament

kjv = eab.import_bible([InputFile(path="kjv-john-11.txt", bytes=Path("kjv-john-11.txt").read_bytes())], "King James Version", "KJV")
both = eab.translations().with_bible(bsb).with_bible(kjv)
gospels = BookScope(testaments=[], genres=["Gospels"], books=[], exclude_testaments=[], exclude_genres=[], exclude_books=["Mark"])
print(*eab.scope_books(gospels), eab.genres()[0].name)  # → 40 42 43 Pentateuch
print(len(eab.scope_books(dataclasses.replace(gospels, testaments=[Testament.NEW], genres=[], exclude_books=[]))))  # → 27

everything = eab.new_readings_project(both, "", gospels)
print(everything.name(), everything.add_sources(["sermons/sunday.json"]).added)  # → Verses read (BSB, KJV) 1
print(everything.kind() == ProjectKind.READINGS, len(everything.translations()), len(both.ids()))  # → True 2 2
everything.set_scope(dataclasses.replace(gospels, exclude_books=[]))
print(",".join(everything.scope().genres), len(everything.scope().exclude_books))  # → Gospels 0
print(everything.search_in(both, files).readings)  # → 5
for r in everything.reading_table(both, files)[1:4]:
    print(r.reference, r.translation, " ".join(r.also_in), "|", r.said)
# → John 11:34 BSB  | “Where have you put him?” He asked. “Come and see, Lord,” they answered.
# → John 11:35 BSB KJV | Jesus wept.
# → John 11:36 BSB  | Then the Jews said, “See how He loved him!”
print(everything.readings_csv(both, files).splitlines()[0])  # → sermon,sermon id,speaker,series,reference,book,chapter,verse,also at,translation,also in,start,end,kind,accuracy,coverage,said
```

## Saving and opening

A project is saved as two strings: `toJson` (its sermons, choices, settings, and metadata, the same
as the Extract Audio Bible app keeps) and `readingsJson` (what `search` found).
`readProject` opens them again; without the readings, search again.

```ts
const reopened = eab.readProject(project.toJson(), project.readingsJson());
console.log(reopened.name(), reopened.selections().length, reopened.readings().length); // → John (BSB) 4 5
console.log(reopened.metadata().artist, reopened.bibleId() === bsb.id()); // → Grace Church true
reopened.rename("John, read aloud");
console.log(reopened.name(), reopened.id() === project.id()); // → John, read aloud true
console.log(reopened.removeSource(sermon.id), reopened.sources().length, reopened.selections().length); // → true 0 0
```

```python
reopened = eab.read_project(project.to_json(), project.readings_json())
print(reopened.name(), len(reopened.selections()), len(reopened.readings()))  # → John (BSB) 4 5
print(reopened.metadata().artist, reopened.bible_id() == bsb.id())  # → Grace Church True
reopened.rename("John, read aloud")
print(reopened.name(), reopened.id() == project.id())  # → John, read aloud True
print(reopened.remove_source(sermon.id), len(reopened.sources()), len(reopened.selections()))  # → True 0 0
```

## Reading natively

<!-- source -->In Python (and Swift and Kotlin),<!-- /source --><!-- ts: In the native packages (Python, Swift, and Kotlin), --><!-- python: In this package (and in Swift and Kotlin), -->
`Files.disk` reads files from disk (`Files.downloading` too, downloading audio given by URL into a
folder), `importBibleFromPaths` reads a Bible from files and folders,
`openTranscript` reads a transcript through any `Files`, and `exportToFolder` writes an export
into a folder.
<!-- source -->
In WebAssembly (the npm package, in Node too) the disk's files can't be read, and those are an
`Io` error: read the files and give them as bytes, and save what `export` returns.
<!-- /source -->
<!-- ts:
In WebAssembly (this package, in Node too) the disk's files can't be read, and those are an `Io`
error: read the files and give them as bytes, and save what `export` returns.
-->

```ts
import { ExtractAudioBibleErrorException } from "extract-audio-bible";

try {
  eab.importBibleFromPaths(["bsb-john-11.txt"], "Berean Standard Bible", "BSB");
} catch (error) {
  console.log((error as ExtractAudioBibleErrorException).value); // → { tag: 'Io', message: 'no file access in WebAssembly: read the files and use importBible' }
}
try {
  eab.openTranscript(Files.disk(), "sermon.json");
} catch (error) {
  console.log((error as ExtractAudioBibleErrorException).value.tag); // → Io
}
try {
  eab.openTranscript(Files.downloading("downloads"), "sermon.json");
} catch (error) {
  console.log((error as ExtractAudioBibleErrorException).value.tag); // → Io
}
try {
  project.exportToFolder(bsb, files, "audio-bible", null);
} catch (error) {
  console.log((error as ExtractAudioBibleErrorException).value.tag); // → Io
}
console.log(eab.openTranscript(files, "sermons/sunday.json").wordCount()); // → 72
```

```python
disk = Files.disk()
from_disk = eab.import_bible_from_paths(["bsb-john-11.txt"], "Berean Standard Bible", "BSB")
print(from_disk.verse(43, 11, 35), disk.has("sermon.json"))  # → Jesus wept. True
print(eab.open_transcript(disk, "sermon.json").word_count())  # → 72
print(eab.open_transcript(Files.downloading("downloads"), "sermon.json").word_count())  # → 72
written = project.export_to_folder(bsb, files, "audio-bible", None)
print(len(written), written[0].path.endswith("audio-bible/43 John/43_John_011.mp3"))  # → 5 True
print(eab.open_transcript(files, "sermons/sunday.json").word_count())  # → 72
print(files.remove("sermons/sunday.json"), files.has("sermons/sunday.json"))  # → True False
```

## Errors

<!-- source -->
Errors are thrown as `ExtractAudioBibleErrorException` in TypeScript (`error.value` is
`{ tag, message }`) and raised as `ExtractAudioBibleErrorException` in Python (`error.error` is
an `ExtractAudioBibleErrorNotFound`, `ExtractAudioBibleErrorIo`, `ExtractAudioBibleErrorInvalid`,
`ExtractAudioBibleErrorAudio`, or `ExtractAudioBibleErrorNetwork`, with its `message`).
<!-- /source -->
<!-- ts:
Errors are thrown as `ExtractAudioBibleErrorException` (`error.value` is `{ tag, message }`).
-->
<!-- python:
Errors are raised as `ExtractAudioBibleErrorException` (`error.error` is an
`ExtractAudioBibleErrorNotFound`, `ExtractAudioBibleErrorIo`, `ExtractAudioBibleErrorInvalid`,
`ExtractAudioBibleErrorAudio`, or `ExtractAudioBibleErrorNetwork`, with its `message`).
-->
The <!-- source -->tag<!-- /source --><!-- ts: tag --><!-- python: kind --> says what went wrong: `NotFound` (a file, book, chapter, or reading that isn't there),
`Io` (reading or writing failed), `Invalid` (input that can't be used), `Audio` (audio that
can't be decoded), or `Network` (audio given by a URL that couldn't be downloaded).

```ts
try {
  eab.newProject(bsb, "", "Romans");
} catch (error) {
  if (!(error instanceof ExtractAudioBibleErrorException)) throw error;
  console.log(error.value); // → { tag: 'NotFound', message: "Berean Standard Bible doesn't have book 45" }
}
try {
  eab.readTranscript("not json");
} catch (error) {
  console.log((error as ExtractAudioBibleErrorException).value.tag); // → Invalid
}
files.add("noise.mp3", new Uint8Array([1, 2, 3, 4]));
try {
  eab.renderClip(files, "noise.mp3", 0, 1, options);
} catch (error) {
  console.log((error as ExtractAudioBibleErrorException).value.tag); // → Audio
}
```

```python
from extract_audio_bible import (
    ExtractAudioBibleErrorAudio,
    ExtractAudioBibleErrorException,
    ExtractAudioBibleErrorInvalid,
    ExtractAudioBibleErrorNotFound,
)

try:
    eab.new_project(bsb, "", "Romans")
except ExtractAudioBibleErrorException as error:
    print(isinstance(error.error, ExtractAudioBibleErrorNotFound), error.error.message)  # → True Berean Standard Bible doesn't have book 45
try:
    eab.read_transcript("not json")
except ExtractAudioBibleErrorException as error:
    print(isinstance(error.error, ExtractAudioBibleErrorInvalid))  # → True
files.add("noise.mp3", b"\x01\x02\x03\x04")
try:
    eab.render_clip(files, "noise.mp3", 0.0, 1.0, options)
except ExtractAudioBibleErrorException as error:
    print(isinstance(error.error, ExtractAudioBibleErrorAudio))  # → True
```

## Every export

Each class, record, and enum <!-- source -->(by their TypeScript names)<!-- /source --><!-- ts: (by their TypeScript names) --><!-- python: (by its Python name, as the table's TypeScript names snake_cased) -->, and where an example uses it.

| Export | Example |
|---|---|
| `ExtractAudioBible.new`, `version` | [Loading](#loading) |
| `ExtractAudioBible.importBible`, `readBible`, `bookId` | [A bible](#a-bible) |
| `ExtractAudioBible.readTranscript`, `searcher` | [Finding verses in a transcript](#finding-verses-in-a-transcript) |
| `ExtractAudioBible.newProject` | [A project](#a-project) |
| `ExtractAudioBible.renderClip` | [Listening](#listening) |
| `ExtractAudioBible.translations`, `genres`, `scopeBooks`, `newReadingsProject` | [Every verse read](#every-verse-read) |
| `Translations.withBible`, `ids`, `searcher` | [Every verse read](#every-verse-read) |
| `Project.kind`, `translations`, `scope`, `setScope`, `searchIn`, `readingTable`, `readingsCsv` | [Every verse read](#every-verse-read) |
| `BookScope`, `Testament`, `GenreInfo`, `ReadingRow`, `ProjectKind` | [Every verse read](#every-verse-read) |
| `ExtractAudioBible.readProject` | [Saving and opening](#saving-and-opening) |
| `ExtractAudioBible.importBibleFromPaths`, `openTranscript` | [Reading natively](#reading-natively) |
| `Bible.id`, `name`, `abbreviation`, `importWarnings`, `books`, `verse`, `chapter`, `toJson` | [A Bible](#a-bible), [Saving and opening](#saving-and-opening) |
| `Transcript.words`, `wordCount`, `duration`, `paragraphStarts`, `id`, `title`, `find` | [Finding verses in a transcript](#finding-verses-in-a-transcript) |
| `Searcher.find` | [Finding verses in a transcript](#finding-verses-in-a-transcript) |
| `Files.memory`, `add`, `paths` | [A project](#a-project) |
| `Files.disk`, `downloading`, `has`, `remove` | [Reading natively](#reading-natively) |
| `Project.name`, `book`, `addSources`, `addSourcesFrom`, `search`, `sources`, `runs`, `readings`, `fileMetadata`, `setFileMetadata`, `applyFileMetadataJson`, `searchTranscripts` | [A project](#a-project) |
| `Project.chooseBest`, `chooseBestIn`, `status`, `verseReadings`, `selection`, `selections`, `updateSelection`, `select`, `unselect`, `join`, `arrange`, `unjoin` | [Choosing readings](#choosing-readings) |
| `Project.settings`, `setSettings`, `metadata`, `setMetadata`, `applyJson` | [Settings and metadata](#settings-and-metadata) |
| `Project.chapterTimeline`, `renderChapter`, `renderVerse` | [Listening](#listening) |
| `Project.chaptersToExport`, `export` | [Exporting](#exporting) |
| `Project.exportToFolder` | [Reading natively](#reading-natively) |
| `Project.toJson`, `readingsJson`, `id`, `bibleId`, `rename`, `removeSource` | [Saving and opening](#saving-and-opening) |
| `InputFile`, `BookInfo` | [A Bible](#a-bible) |
| `Word`, `Readings`, `Reading`, `ReadingRun`, `ReadingKind`, `Said`, `FindOptions` | [Finding verses in a transcript](#finding-verses-in-a-transcript) |
| `SourceInfo`, `FileMetadata`, `Added`, `SearchSummary`, `SaidInSermons` | [A project](#a-project) |
| `ChapterChoice`, `ChapterStatus`, `Selection`, `SelectionPart` | [Choosing readings](#choosing-readings) |
| `Settings`, `Leveling`, `ExportMetadata`, `ExportMode`, `TrackNumbering` | [Settings and metadata](#settings-and-metadata) |
| `ClipOptions`, `VerseTime` | [Listening](#listening) |
| `ChapterRef`, `ExportedFile` | [Exporting](#exporting) |
| `WrittenFile` | [Reading natively](#reading-natively) |
| `ExtractAudioBibleErrorException`, `ExtractAudioBibleError` | [Errors](#errors) |

<!-- ts:
## Source and building

The source, its issues, and the checks of these examples are at
[github.com/MasterTemple/extract-audio-bible](https://github.com/MasterTemple/extract-audio-bible)
(`crates/extract-audio-bible-ffi`; its README says how to build the package and run them).
-->
<!-- python:
## Source and building

The source, its issues, and the checks of these examples are at
[github.com/MasterTemple/extract-audio-bible](https://github.com/MasterTemple/extract-audio-bible)
(`crates/extract-audio-bible-ffi`; its README says how to build the package and run them).
-->
<!-- source -->
## Building

```sh
npm install && npm run pack:wasm   # dist/wasm/pkg (npm: extract-audio-bible)
npm run pack:python                # dist/python/wheelhouse (PyPI: extract-audio-bible)
python scripts/readme.py check     # this README's examples and names, against both builds
```

Each pack script writes its package's README with `scripts/readme.py`, which first checks this
one, without running anything: its marks, its examples in pairs (TypeScript, then Python), the
links to its headings, each language's examples against the package just built (TypeScript's by
`tsc`, Python's against its `__init__.pyi`), and the API's names in the prose. A problem stops
the build, with README.md's line. To check against bindings generated rather than built:
`boltffi generate typescript -o dist/api/ts`, `boltffi generate python -o dist/api/python`, then
`python scripts/readme.py check --wasm dist/api/ts --python dist/api/python/extract_audio_bible`.

```sh
cd examples/node && npm install && node readme.ts   # every TypeScript example above (--show prints what each printed)
cd ../.. && pip install dist/python/wheelhouse/*.whl
python examples/python/readme.py                    # every Python example above
python scripts/smoke-test.py                        # the installed wheel imports and answers
```

The library this wraps is `crates/extract-audio-bible`; it finds verses with
`crates/scripture-quotes`.
<!-- /source -->
