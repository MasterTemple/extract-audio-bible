# scripture-quotes

Find every quotation of Scripture in a transcript: any verse of any book, in any of the
translations you give it, and which translation it was quoted from. Built for sermons
transcribed by WhisperX or any other speech-to-text, so it expects transcription slips
("thee" heard as "the", "70" for "seventy") and speakers who add a word or skip one.

It works on words only. Times, audio, and file formats are yours: a quotation says which words
of the transcript it is (and which segments, for transcripts without word times), and you map
those to whatever you have.

```rust
use scripture_quotes::{Engine, Options, Transcript, Translation, VerseRef};

let mut kjv = Translation::new("KJV");
let john = 43; // topos's book numbers
kjv.verse(VerseRef::new(john, 11, 35), "Jesus wept.");
kjv.verse(VerseRef::new(john, 11, 36), "Then said the Jews, Behold how he loved him!");
let engine = Engine::new([kjv]);

let transcript = Transcript::from_text(
    "and then it says jesus wept then said the jews behold how he loved him and that's the point",
);
let found = engine.searcher().search(&transcript, &Options::default());
let refs: Vec<String> = found.quotations.iter().map(|q| engine.format(&q.reference.passage())).collect();
assert_eq!(refs, ["John 11:35", "John 11:36"]);
// Read one after another, they're a run
assert_eq!(engine.format(&found.runs[0].passage()), "John 11:35-36");
```

## Accuracy

`Options::min_accuracy` (default **0.85**) is how faithfully a verse's words must be said: of its
words from the first quoted to the last, the share said right. A near miss ("saith"/"sayeth")
counts 0.8 of a word, and each place the speaker added words (a comment in the middle of a
verse, however long) counts half a word against it. Quotations of less than
three quarters of a verse must also meet `min_partial_accuracy` (default **0.9**): a short stretch
with a word wrong is more often an everyday phrase than a quotation. When only part of a reading
is accurate enough (a verse read with a stretch left out or paraphrased), the quotation is the
longest part that is.

The defaults were chosen by reading what each threshold adds on 120 real sermons: from 90% down to
80%, the whole verses it adds are nearly all real (read from another translation, or with a word
misheard or changed), while the partial ones it adds are mostly everyday phrases.

Quoting part of a verse takes `min_words` of it (default 6), so everyday phrases ("at the end
of") aren't taken for verses; a whole verse always counts. When the transcript also quotes the
same chapter at that length, `min_words_in_context` (default 3) is enough, as long as the words
are rare enough together not to be anyone's (a sermon on 1 John 1 saying "we have no sin" again
is quoting it; "and the truth is" isn't). Rarity is measured against the engine's own text, so
it means most with whole translations. `min_coverage` (default 0) asks for that much of the
verse.

Each quotation has its `accuracy`, `coverage` (how much of the verse), `precision` (how much of
what was said is the verse), `information` (how rare its words are, in bits), a `kind` (`Exact`,
`Close`, `Partial`), which of the verse's words were heard, and the words the speaker added.

## Translations and scopes

An engine holds every translation you give it, and `engine.searcher()` looks for every verse of
every one. A quotation is reported once, in the translation it fits best, with the others that
fit as well as asked in `also_in`. When words fit several verses, a verse is left out only when
another clearly fits them better: as many words more accurately, as many and much more of its
verse, or clearly more words and no worse otherwise. So a verse read in full inside a run (the
commandments in Romans 13:9) isn't also reported as the verses it quotes, while parallel passages
worded alike are all kept. Verses worded the same in several translations are stored and
aligned once.

To look for less, build a scoped searcher (books take a few milliseconds) and keep it for as
many transcripts as you like. Passages are parsed and enumerated by
[topos-bible](https://crates.io/crates/topos-bible), with each translation's own chapter and
verse counts:

```rust
# use scripture_quotes::{Engine, Options, Scope, Transcript, Translation, VerseRef};
# let mut kjv = Translation::new("KJV");
# kjv.verse(VerseRef::new(43, 3, 16), "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.");
# let engine = Engine::new([kjv]);
let searcher = engine.scoped(&Scope::default().translation("kjv").passages("John 3:16-18; Rom 8"))?;
let found = searcher.search(&Transcript::from_text("for god so loved the world that he gave his only begotten son"), &Options::default());
assert_eq!(found.quotations.len(), 1);
# Ok::<(), scripture_quotes::ScopeError>(())
```

`Scope::book` and a bare book name (`passages("Jude")`) search a whole book.

## Transcripts

- `Transcript::from_words(words)`: words as said (WhisperX's words)
- `Transcript::from_segments(segments)`: text without word times; `segment_range` gives the
  segments a quotation spans
- `Transcript::from_text(text)`: plain text; `byte_range` gives a quotation's place in it
- `push_word` and `end_segment` to build one yourself

`find(phrase, prefix)` gives where anything else was said, as word ranges: the phrase's words in
order, compared the way quotations are (case, accents, and punctuation aside, numerals spelled
out), its last word the start of one with `prefix`.

## Performance

On one core of an i7-10750H, with 200 real sermons (WhisperX, 817,000 words): the whole ESV is
normalized and indexed in about 0.2 seconds, and each sermon is searched in **3.1 ms** (1.3
million words a second); with the ESV and NASB together, 4.6 ms. An engine and its searchers are
`Sync`, so transcripts can be searched in parallel.

## How it works

Every verse's word trigrams are indexed, leaving out trigrams in more than 48 verses (counted by
reference, so a phrase shared by translations counts once). Translations that word a verse the
same way share one copy of it. A transcript's trigrams vote for a verse and an offset; each group
of votes is aligned word by word (Smith-Waterman, in a band around the votes' diagonal), with
near-miss words counted, so transcription slips still match. Near misses are found once for the
whole vocabulary (symmetric deletion), and words not in it are looked up the same way and
remembered. Around every reading, the verses just before and after are looked for directly,
which finds very short verses, and two-word verses ("Jesus wept") are matched on their own.
Readings that fit one verse far better than another (only sharing a phrase) are only the better
one; parallel passages worded the same are both kept. Quotations of verse after verse are joined
into runs.

Words are compared lowercased, without accents or punctuation, with numerals spelled out in
English.

`cargo run --release --example bench -- SERMON_DIR BIBLE.json…` measures it on your own
transcripts.
