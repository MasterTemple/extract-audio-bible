//! Which books to look for verses in, by testament, genre (topos's book groups: Pentateuch,
//! Gospels, Pauline Epistles…), and book.

use std::collections::BTreeSet;

use scripture_quotes::topos_bible::{
    data::bible_data::BibleData,
    filter::{
        BibleFilter, Operation,
        filters::{book::BookFilter, genre::GenreFilter, testament::TestamentFilter},
    },
};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Testament {
    Old,
    New,
}

/**
Which books to search, as topos filters them:

- Included testaments limit the search to them
- Included genres and books add up, within those testaments
- Excluded testaments, genres, and books are always left out

So the New Testament with `Pauline Epistles` is Paul's letters, `Pentateuch` with `Revelation` is
six books, and nothing at all is every book. Genres and books go by any of their names or
abbreviations (`Gospels`, `pauline`, `1 Cor`)
*/
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BookScope {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub testaments: Vec<Testament>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub books: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_testaments: Vec<Testament>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_genres: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_books: Vec<String>,
}

/// A genre: a group of books
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Genre {
    pub name: String,
    /// Its books, by topos's numbers (1 = Genesis)
    pub books: Vec<u8>,
}

impl BookScope {
    /// Every book
    pub fn is_everything(&self) -> bool {
        *self == Self::default()
    }

    /// The books searched, by number, or none for every book. An unknown genre or book is an
    /// error, and so are filters that leave nothing (`Old` with `Pauline Epistles`)
    pub fn books(&self) -> Result<Option<BTreeSet<u8>>> {
        if self.is_everything() {
            return Ok(None);
        }
        let mut filter = BibleFilter::new(BibleData::default());
        let testament = |t: &Testament| match t {
            Testament::Old => TestamentFilter::Old,
            Testament::New => TestamentFilter::New,
        };
        let failed = |e: scripture_quotes::topos_bible::error::ToposError| Error::invalid(e.to_string());
        for t in &self.testaments {
            filter.push(Operation::Include(testament(t))).map_err(failed)?;
        }
        for g in &self.genres {
            filter.push(Operation::Include(GenreFilter::new(g))).map_err(failed)?;
        }
        for b in &self.books {
            filter.push(Operation::Include(BookFilter::new(b))).map_err(failed)?;
        }
        for t in &self.exclude_testaments {
            filter.push(Operation::Exclude(testament(t))).map_err(failed)?;
        }
        for g in &self.exclude_genres {
            filter.push(Operation::Exclude(GenreFilter::new(g))).map_err(failed)?;
        }
        for b in &self.exclude_books {
            filter.push(Operation::Exclude(BookFilter::new(b))).map_err(failed)?;
        }
        if let Some(why) = filter.contradiction() {
            return Err(Error::invalid(format!("no books are searched: {why}")));
        }
        Ok(Some(filter.ids().iter().map(|b| b.0).collect()))
    }

    /// Every genre topos knows, in its order
    pub fn genres() -> Vec<Genre> {
        (BibleData::default().genres().iter())
            .map(|g| Genre {
                name: g.name().to_string(),
                books: g.books().iter().map(|b| b.0).collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(f: impl FnOnce(&mut BookScope)) -> BookScope {
        let mut s = BookScope::default();
        f(&mut s);
        s
    }

    #[test]
    fn books_by_testament_genre_and_book() {
        assert_eq!(BookScope::default().books().unwrap(), None);
        let nt = scope(|s| s.testaments = vec![Testament::New]).books().unwrap().unwrap();
        assert_eq!((nt.len(), nt.first(), nt.last()), (27, Some(&40), Some(&66)));
        let paul = scope(|s| {
            s.testaments = vec![Testament::New];
            s.genres = vec!["pauline".into()];
        });
        assert_eq!(paul.books().unwrap().unwrap().len(), 13);
        let six = scope(|s| s.genres = vec!["Pentateuch".into(), "Revelation".into()]);
        assert!(six.books().is_err(), "Revelation is a book, not a genre");
        let six = scope(|s| {
            s.genres = vec!["Pentateuch".into()];
            s.books = vec!["Rev".into()];
        });
        assert_eq!(
            six.books().unwrap().unwrap().into_iter().collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 66]
        );
        let gospels_but_john = scope(|s| {
            s.genres = vec!["Gospels".into()];
            s.exclude_books = vec!["John".into()];
        });
        assert_eq!(
            gospels_but_john
                .books()
                .unwrap()
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            [40, 41, 42]
        );
        let nothing = scope(|s| {
            s.testaments = vec![Testament::Old];
            s.genres = vec!["Pauline Epistles".into()];
        });
        assert!(nothing.books().is_err());
        assert!(
            BookScope::genres()
                .iter()
                .any(|g| g.name == "Gospels" && g.books == [40, 41, 42, 43])
        );
    }
}
