//! The Bible text the engine knows: every translation's verses, normalized once and shared.

use std::{
    borrow::Cow,
    collections::BTreeMap,
    sync::{OnceLock, RwLock},
};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use topos_bible::{
    data::{BibleData, books::BookId, chapter_verses::ChapterVerses},
    segments::{Passage, Segment},
};

use crate::{
    index::Index,
    near::NearIndex,
    scope::{Scope, ScopeError},
    search::Searcher,
    text::{Shape, tokenize_into},
};

/// One verse, as topos numbers it
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VerseRef {
    pub book: BookId,
    pub chapter: u8,
    pub verse: u8,
}

impl VerseRef {
    pub fn new(book: impl Into<BookId>, chapter: u8, verse: u8) -> Self {
        Self {
            book: book.into(),
            chapter,
            verse,
        }
    }

    /// The verse as a topos passage, to format or compare
    pub fn passage(&self) -> Passage {
        Segment::chapter_verse(self.chapter, self.verse)
            .as_segments()
            .with_book(self.book)
    }
}

/// A translation's index in its [`Engine`], in the order they were added
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TranslationId(pub u16);

/// A translation's verses, to give to [`EngineBuilder::translation`]
#[derive(Clone, Debug, Default)]
pub struct Translation {
    name: String,
    verses: BTreeMap<VerseRef, String>,
}

impl Translation {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            verses: BTreeMap::new(),
        }
    }

    /// Adds a verse's text (added to what it has, if the verse is given twice). A verse with no
    /// text of its own (the later verses of a bridge, `4-5`) can be added empty: it counts in the
    /// chapter's length but is never found
    pub fn verse(&mut self, reference: VerseRef, text: impl AsRef<str>) -> &mut Self {
        let slot = self.verses.entry(reference).or_default();
        let text = text.as_ref().trim();
        if !slot.is_empty() && !text.is_empty() {
            slot.push(' ');
        }
        slot.push_str(text);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn len(&self) -> usize {
        self.verses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.verses.is_empty()
    }
}

/// What the engine keeps of a translation
#[derive(Debug)]
pub struct TranslationInfo {
    pub name: String,
    /// Its own chapter and verse counts, by book, so passages are read the way it numbers them
    pub(crate) versification: BTreeMap<BookId, ChapterVerses>,
}

/// One verse of one translation
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub translation: u16,
    pub reference: VerseRef,
    pub text: u32,
    /// Its whitespace-separated words' token counts, in [`Engine::word_tokens`]
    pub words: (u32, u32),
}

/// A distinct verse text: translations that word a verse the same way, and verses worded the
/// same within one, share it, so it is only aligned once
#[derive(Clone, Copy, Debug)]
pub(crate) struct Text {
    pub tokens: (u32, u32),
    pub owners: (u32, u32),
}

/// Every token seen in the Bible text, by id
#[derive(Debug, Default)]
pub(crate) struct Vocab {
    ids: FxHashMap<Box<str>, u32>,
    words: Vec<Box<str>>,
    shapes: Vec<Shape>,
    /// How much a token says about which verse is read: log2(texts / texts with it)
    bits: Vec<f32>,
    near: NearIndex,
}

impl Vocab {
    fn intern(&mut self, token: &str) -> u32 {
        if let Some(&id) = self.ids.get(token) {
            return id;
        }
        let id = self.words.len() as u32;
        self.ids.insert(token.into(), id);
        self.words.push(token.into());
        self.shapes.push(Shape::of(token));
        id
    }

    pub fn get(&self, token: &str) -> Option<u32> {
        self.ids.get(token).copied()
    }

    pub fn bits(&self, id: u32) -> f32 {
        self.bits[id as usize]
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// A token's near misses in the vocabulary
    pub fn near(&self, id: u32) -> &[u32] {
        self.near.of(id)
    }

    /// The near misses of a token not in the vocabulary
    pub fn near_unknown(&self, token: &str, out: &mut Vec<u32>) {
        self.near.lookup(token, &self.words, &self.shapes, out);
    }
}

/**
Every verse of every translation it was given, ready to search transcripts for quotations.

Build it once (it normalizes and indexes the text) and search many transcripts with it, from as
many threads as you like.
*/
pub struct Engine {
    pub(crate) data: BibleData,
    pub(crate) translations: Vec<TranslationInfo>,
    pub(crate) vocab: Vocab,
    /// Sorted by translation, then reference
    pub(crate) slots: Vec<Slot>,
    pub(crate) texts: Vec<Text>,
    pub(crate) tokens: Vec<u32>,
    pub(crate) word_tokens: Vec<u8>,
    /// Slots by text: `texts[t].owners` is a range of this
    pub(crate) owners: Vec<u32>,
    everything: OnceLock<Index>,
    /// Near misses of words not in the vocabulary, remembered between transcripts
    unknown: RwLock<FxHashMap<Box<str>, Box<[u32]>>>,
}

/// Words not in the vocabulary whose near misses are remembered, at most
const REMEMBERED_UNKNOWN: usize = 200_000;

#[derive(Default)]
pub struct EngineBuilder {
    data: Option<BibleData>,
    translations: Vec<Translation>,
}

impl EngineBuilder {
    /// The book names and numbering references are parsed with (topos's defaults otherwise; use
    /// `BibleData::with_deuterocanon()` for the deuterocanonical books)
    pub fn data(mut self, data: BibleData) -> Self {
        self.data = Some(data);
        self
    }

    pub fn translation(mut self, translation: Translation) -> Self {
        self.translations.push(translation);
        self
    }

    pub fn add(&mut self, translation: Translation) -> &mut Self {
        self.translations.push(translation);
        self
    }

    pub fn build(self) -> Engine {
        Engine::build(self.data.unwrap_or_default(), self.translations)
    }
}

impl Engine {
    pub fn builder() -> EngineBuilder {
        EngineBuilder::default()
    }

    pub fn new(translations: impl IntoIterator<Item = Translation>) -> Self {
        Self::build(BibleData::default(), translations.into_iter().collect())
    }

    fn build(data: BibleData, translations: Vec<Translation>) -> Self {
        let mut vocab = Vocab::default();
        let mut infos = Vec::with_capacity(translations.len());
        let mut slots = vec![];
        let mut texts: Vec<Text> = vec![];
        let mut tokens: Vec<u32> = vec![];
        let mut word_tokens: Vec<u8> = vec![];
        let mut by_tokens: FxHashMap<Vec<u32>, u32> = FxHashMap::default();
        let mut scratch: Vec<String> = vec![];
        let mut ids: Vec<u32> = vec![];

        for (t, translation) in translations.into_iter().enumerate() {
            let mut counts: BTreeMap<BookId, Vec<u8>> = BTreeMap::new();
            for (reference, text) in &translation.verses {
                let chapters = counts.entry(reference.book).or_default();
                if chapters.len() < reference.chapter as usize {
                    chapters.resize(reference.chapter as usize, 0);
                }
                if reference.chapter > 0 {
                    let last = &mut chapters[reference.chapter as usize - 1];
                    *last = (*last).max(reference.verse);
                }

                ids.clear();
                let words_start = word_tokens.len() as u32;
                for word in text.split_whitespace() {
                    scratch.clear();
                    tokenize_into(word, &mut scratch);
                    word_tokens.push(scratch.len().min(255) as u8);
                    ids.extend(scratch.iter().map(|s| vocab.intern(s)));
                }
                if ids.is_empty() {
                    word_tokens.truncate(words_start as usize);
                    continue;
                }
                let text_id = match by_tokens.get(&ids) {
                    Some(&id) => id,
                    None => {
                        let id = texts.len() as u32;
                        let start = tokens.len() as u32;
                        tokens.extend_from_slice(&ids);
                        texts.push(Text {
                            tokens: (start, ids.len() as u32),
                            owners: (0, 0),
                        });
                        by_tokens.insert(ids.clone(), id);
                        id
                    }
                };
                slots.push(Slot {
                    translation: t as u16,
                    reference: *reference,
                    text: text_id,
                    words: (words_start, word_tokens.len() as u32 - words_start),
                });
            }
            infos.push(TranslationInfo {
                name: translation.name,
                versification: counts
                    .into_iter()
                    .map(|(b, c)| (b, ChapterVerses::new(c)))
                    .collect(),
            });
        }

        // Each token's information, from how many texts have it
        let mut df = vec![0u32; vocab.words.len()];
        let mut last = vec![u32::MAX; vocab.words.len()];
        for (t, text) in texts.iter().enumerate() {
            for &id in &tokens[text.tokens.0 as usize..(text.tokens.0 + text.tokens.1) as usize] {
                if last[id as usize] != t as u32 {
                    last[id as usize] = t as u32;
                    df[id as usize] += 1;
                }
            }
        }
        let total = texts.len().max(1) as f32;
        vocab.bits = df
            .iter()
            .map(|&d| (total / d.max(1) as f32).log2())
            .collect();
        vocab.near = NearIndex::new(&vocab.words, &vocab.shapes);

        // Owners, grouped by text
        for slot in &slots {
            texts[slot.text as usize].owners.1 += 1;
        }
        let mut at = 0;
        for text in &mut texts {
            text.owners.0 = at;
            at += text.owners.1;
            text.owners.1 = 0;
        }
        let mut owners = vec![0u32; slots.len()];
        for (i, slot) in slots.iter().enumerate() {
            let text = &mut texts[slot.text as usize];
            owners[(text.owners.0 + text.owners.1) as usize] = i as u32;
            text.owners.1 += 1;
        }

        Engine {
            data,
            translations: infos,
            vocab,
            slots,
            texts,
            tokens,
            word_tokens,
            owners,
            everything: OnceLock::new(),
            unknown: RwLock::default(),
        }
    }

    /// The topos data references are read with
    pub fn data(&self) -> &BibleData {
        &self.data
    }

    pub fn translations(&self) -> impl Iterator<Item = (TranslationId, &str)> {
        self.translations
            .iter()
            .enumerate()
            .map(|(i, t)| (TranslationId(i as u16), t.name.as_str()))
    }

    /// A translation by name, ignoring case
    pub fn translation(&self, name: &str) -> Option<TranslationId> {
        let name = name.trim();
        self.translations
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
            .map(|i| TranslationId(i as u16))
    }

    pub fn translation_name(&self, id: TranslationId) -> Option<&str> {
        self.translations
            .get(id.0 as usize)
            .map(|t| t.name.as_str())
    }

    /// A translation's chapter and verse counts for a book
    pub fn versification(
        &self,
        translation: TranslationId,
        book: BookId,
    ) -> Option<&ChapterVerses> {
        self.translations
            .get(translation.0 as usize)?
            .versification
            .get(&book)
    }

    /// Verses with text, over all translations
    pub fn verse_count(&self) -> usize {
        self.slots.len()
    }

    /// Distinct verse texts (verses worded alike are one)
    pub fn text_count(&self) -> usize {
        self.texts.len()
    }

    /// A reference like `John 3:16-18` or `Ps 23`, read with the engine's book names
    pub fn parse(&self, reference: &str) -> Option<Passage> {
        self.data.parse(reference)
    }

    /// A passage written out, like `John 3:16-18`
    pub fn format(&self, passage: &Passage) -> String {
        topos_bible::segments::formatter::FormatOptions::default()
            .passage(passage, &self.data)
            .unwrap_or_else(|| format!("{} {}", passage.book.0, passage.segments))
    }

    /// The near misses of words not in the vocabulary, in order
    pub(crate) fn near_unknown(&self, words: &[&str]) -> Vec<Box<[u32]>> {
        let mut found: Vec<Option<Box<[u32]>>> = match self.unknown.read() {
            Ok(cache) => words.iter().map(|w| cache.get(*w).cloned()).collect(),
            Err(_) => vec![None; words.len()],
        };
        let mut new = vec![];
        let mut scratch = vec![];
        for (word, slot) in words.iter().zip(&mut found) {
            if slot.is_none() {
                self.vocab.near_unknown(word, &mut scratch);
                let list: Box<[u32]> = scratch.as_slice().into();
                new.push((Box::<str>::from(*word), list.clone()));
                *slot = Some(list);
            }
        }
        if !new.is_empty()
            && let Ok(mut cache) = self.unknown.write()
            && cache.len() + new.len() <= REMEMBERED_UNKNOWN
        {
            cache.extend(new);
        }
        found.into_iter().map(Option::unwrap_or_default).collect()
    }

    /// Searches every verse of every translation
    pub fn searcher(&self) -> Searcher<'_> {
        let index = self.everything.get_or_init(|| Index::new(self, None));
        Searcher::new(self, Cow::Borrowed(index))
    }

    /// Searches only some translations or passages. Building one is quick (a book takes a few
    /// milliseconds); keep it to search many transcripts
    pub fn scoped(&self, scope: &Scope) -> Result<Searcher<'_>, ScopeError> {
        if scope.is_everything() {
            return Ok(self.searcher());
        }
        let in_scope = scope.resolve(self)?;
        Ok(Searcher::new(
            self,
            Cow::Owned(Index::new(self, Some(in_scope))),
        ))
    }

    pub(crate) fn text_tokens(&self, text: u32) -> &[u32] {
        let (start, len) = self.texts[text as usize].tokens;
        &self.tokens[start as usize..(start + len) as usize]
    }

    pub(crate) fn text_owners(&self, text: u32) -> &[u32] {
        let (start, len) = self.texts[text as usize].owners;
        &self.owners[start as usize..(start + len) as usize]
    }

    pub(crate) fn slot_words(&self, slot: u32) -> &[u8] {
        let (start, len) = self.slots[slot as usize].words;
        &self.word_tokens[start as usize..(start + len) as usize]
    }

    /// The slot read right after this one, in the same translation: its next verse with text,
    /// in the same chapter or the next (verses between with no text of their own, like the later
    /// verses of a bridge, `4-5`, are read with them)
    pub(crate) fn next_slot(&self, slot: u32) -> Option<u32> {
        let (a, b) = (
            &self.slots[slot as usize],
            self.slots.get(slot as usize + 1)?,
        );
        (a.translation == b.translation && in_reach(a.reference, b.reference)).then_some(slot + 1)
    }

    pub(crate) fn prev_slot(&self, slot: u32) -> Option<u32> {
        let (a, b) = (
            self.slots.get(slot.checked_sub(1)? as usize)?,
            &self.slots[slot as usize],
        );
        (a.translation == b.translation && in_reach(a.reference, b.reference)).then_some(slot - 1)
    }

    /// Whether `next` is read right after `verse` in a translation: no verse between them has
    /// text in it, and `next` is in the same chapter or the next. Neither need be one of its own
    pub(crate) fn follows(
        &self,
        translation: TranslationId,
        verse: VerseRef,
        next: VerseRef,
    ) -> bool {
        if verse >= next || !in_reach(verse, next) {
            return false;
        }
        let slots = &self.slots[self.translation_slots(translation.0)];
        slots.partition_point(|s| s.reference <= verse)
            == slots.partition_point(|s| s.reference < next)
    }

    /// A verse of a translation
    pub(crate) fn find_slot(&self, translation: u16, reference: VerseRef) -> Option<u32> {
        self.slots
            .binary_search_by(|s| (s.translation, s.reference).cmp(&(translation, reference)))
            .ok()
            .map(|i| i as u32)
    }

    /// The slots of a translation
    pub(crate) fn translation_slots(&self, translation: u16) -> std::ops::Range<usize> {
        let start = self.slots.partition_point(|s| s.translation < translation);
        let end = self.slots.partition_point(|s| s.translation <= translation);
        start..end
    }
}

/// Whether `next` can be read right after `verse` when nothing between them has text: in the same
/// book, and the same chapter or the next
fn in_reach(verse: VerseRef, next: VerseRef) -> bool {
    verse.book == next.book && next.chapter <= verse.chapter.saturating_add(1)
}
