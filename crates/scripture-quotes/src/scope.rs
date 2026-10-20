//! What to search for: some translations, some passages, or everything.

use topos_bible::{data::books::BookId, matcher::BibleMatcher, segments::Passage};

use crate::engine::Engine;

/**
Which verses to look for. Empty lists mean all of them:

```
# use scripture_quotes::Scope;
let scope = Scope::default().translation("KJV").passages("John 3; Rom 8:28-39");
```
*/
#[derive(Clone, Debug, Default)]
pub struct Scope {
    translations: Vec<String>,
    passages: Vec<String>,
    parsed: Vec<Passage>,
    books: Vec<BookId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeError {
    /// No translation of this name was given to the engine
    UnknownTranslation(String),
    /// No reference could be read in this text
    NoPassage(String),
}

impl std::fmt::Display for ScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopeError::UnknownTranslation(name) => {
                write!(f, "there is no translation named {name:?}")
            }
            ScopeError::NoPassage(text) => write!(f, "no Bible reference was found in {text:?}"),
        }
    }
}

impl std::error::Error for ScopeError {}

impl Scope {
    /// Only this translation (by name, ignoring case); call again to add more
    pub fn translation(mut self, name: impl Into<String>) -> Self {
        self.translations.push(name.into());
        self
    }

    /// Only these passages: every reference topos finds in the text (`John 3:16-18; 5`,
    /// `Ps 23, Rom 8`), or a book by name (`Jude`, `1 Cor`); call again to add more
    pub fn passages(mut self, references: impl Into<String>) -> Self {
        self.passages.push(references.into());
        self
    }

    /// Only this passage, already parsed
    pub fn passage(mut self, passage: Passage) -> Self {
        self.parsed.push(passage);
        self
    }

    /// Only this book
    pub fn book(mut self, book: impl Into<BookId>) -> Self {
        self.books.push(book.into());
        self
    }

    pub fn is_everything(&self) -> bool {
        self.translations.is_empty()
            && self.passages.is_empty()
            && self.parsed.is_empty()
            && self.books.is_empty()
    }

    /// Per slot of the engine, whether it's searched for
    pub(crate) fn resolve(&self, engine: &Engine) -> Result<Vec<bool>, ScopeError> {
        let translations: Vec<u16> = if self.translations.is_empty() {
            (0..engine.translations.len() as u16).collect()
        } else {
            (self.translations.iter())
                .map(|name| {
                    engine
                        .translation(name)
                        .map(|t| t.0)
                        .ok_or_else(|| ScopeError::UnknownTranslation(name.clone()))
                })
                .collect::<Result<_, _>>()?
        };

        let mut passages = self.parsed.clone();
        let mut books = self.books.clone();
        if !self.passages.is_empty() {
            let matcher = BibleMatcher::new(engine.data.clone(), Default::default());
            for text in &self.passages {
                let found = matcher.search(text);
                if !found.is_empty() {
                    passages.extend(found.into_iter().map(|m| m.psg));
                } else if let Some(book) = engine.data.books().search(text.trim()) {
                    books.push(book);
                } else {
                    return Err(ScopeError::NoPassage(text.clone()));
                }
            }
        }
        let whole_books = passages.is_empty() && books.is_empty();

        let mut in_scope = vec![false; engine.slots.len()];
        for &t in &translations {
            let slots = engine.translation_slots(t);
            if whole_books {
                in_scope[slots].fill(true);
                continue;
            }
            for i in slots {
                if books.contains(&engine.slots[i].reference.book) {
                    in_scope[i] = true;
                }
            }
            for passage in &passages {
                // Whole chapters end where this translation ends them
                let versification = engine.translations[t as usize]
                    .versification
                    .get(&passage.book);
                for cv in passage.verses(versification) {
                    let reference = crate::VerseRef::new(passage.book, cv.chapter, cv.verse);
                    if let Some(slot) = engine.find_slot(t, reference) {
                        in_scope[slot as usize] = true;
                    }
                }
            }
        }
        Ok(in_scope)
    }
}
