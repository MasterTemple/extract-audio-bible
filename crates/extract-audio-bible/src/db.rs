/*!
A project's readings kept in SQLite (`readings.sqlite` in its folder), so what's on screen (a
chapter, a sermon, a page of the readings) is read without the rest, and anything that reads
SQLite (`sqlite3`, Python, DuckDB) can read them too.

```sql
-- Every reading, as the readings export has them
select sermon, speaker, reference, translation, said from readings_table where speaker = 'Zac Poonen';
```

The tables:

- `readings`: every reading found, by its `sermon`; times in seconds, `verse_key` the verse as one
  number (`book * 1000000 + chapter * 1000 + verse`), `verse_words_heard` a string of `1` and `0`
  (each word of the verse heard or not), `also_in`, `also_at`, and `extra_words` JSON arrays, and
  `active` whether its sermon is still in the project
- `runs`: readings of verse after verse, with their readings' ids (`reading_ids`, a JSON array)
- `sermons`: the project's sermons (kept as the project changes), in its order (`position`)
- `books` and `translations`: what readings' books and translations are called
- `verse_counts` and `chapter_counts`: how many readings each verse and chapter has, of sermons in
  the project (kept as they change); `sermon_counts` and `sermon_translations`: how many each
  sermon has, and of each translation
- `meta`: `schema` (its version) and `analyzed_at` (milliseconds since 1970)

A reading whose sermon is taken out of the project is only marked inactive (until the next search
rewrites them all), so putting the sermon back brings its readings back.
*/

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

use crate::{
    bible::Bible,
    error::{Error, Result},
    files::Files,
    matcher::{Match, MatchKind, Run},
    project::{MatchSet, Project},
    readings::{Names, ReadingRow},
    transcript::Transcript,
};

/// The database's name in a project's folder
pub const FILE: &str = "readings.sqlite";
/// Readings kept as JSON before they were kept in SQLite, read once to make the database
pub const LEGACY_FILE: &str = "matches.json";
/// The version of the tables written
const SCHEMA: i64 = 3;

const TABLES: &str = "
create table meta (key text primary key, value text not null);
create table translations (id text primary key, abbreviation text not null, name text not null, position integer not null);
create table books (id integer primary key, name text not null, osis text not null);
create table sermons (
    id text primary key,
    position integer not null,
    file_id text not null,
    title text not null,
    speaker text,
    series text,
    passage text,
    transcript_path text not null,
    audio_path text not null
);
create table readings (
    id text primary key,
    sermon text not null,
    book integer not null,
    chapter integer not null,
    verse integer not null,
    word_start integer not null,
    word_end integer not null,
    start_time real not null,
    end_time real not null,
    score real not null,
    accuracy real not null,
    coverage real not null,
    precision real not null,
    kind text not null,
    translation text not null,
    also_in text not null,
    also_at text not null,
    run_id text,
    verse_words_heard text not null,
    extra_words text not null,
    said text not null,
    verse_key integer not null,
    active integer not null default 1
);
create table runs (
    id text primary key,
    sermon text not null,
    book integer not null,
    start_chapter integer not null,
    start_verse integer not null,
    end_chapter integer not null,
    end_verse integer not null,
    start_time real not null,
    end_time real not null,
    reading_ids text not null,
    active integer not null default 1
);
create table verse_counts (verse_key integer primary key, readings integer not null);
create table sermon_counts (sermon text primary key, readings integer not null, verses integer not null);
create table chapter_counts (
    book integer not null,
    chapter integer not null,
    readings integer not null,
    verses integer not null,
    primary key (book, chapter)
);
create table sermon_translations (
    sermon text not null,
    translation text not null,
    readings integer not null,
    primary key (sermon, translation)
);
create view readings_table as
    select s.title as sermon, s.file_id as sermon_file_id, s.speaker, s.series,
        coalesce(b.name, r.book) || ' ' || r.chapter || ':' || r.verse as reference,
        r.book, r.chapter, r.verse, coalesce(t.abbreviation, r.translation) as translation,
        r.start_time, r.end_time, r.kind, r.accuracy, r.coverage, r.said
    from readings r
    join sermons s on s.id = r.sermon
    left join books b on b.id = r.book
    left join translations t on t.id = r.translation
    where r.active
    order by s.position, r.word_start;
";

/// Made after the readings are written, which is quicker than keeping them up as they're written
const INDEXES: &str = "
create index readings_by_verse on readings (verse_key);
create index readings_by_sermon on readings (sermon, word_start);
create index readings_by_run on readings (run_id) where run_id is not null;
create index runs_by_sermon on runs (sermon);
";

/// Each sermon's counts of readings, made once they're written (which sermons are in the project
/// is left to what reads them)
const SERMON_COUNTS: &str = "
insert into sermon_counts select sermon, count(*), count(distinct verse_key) from readings group by sermon;
insert into sermon_translations select sermon, translation, count(*) from readings group by sermon, translation;
";

/// Each verse's and chapter's counts of readings (of sermons in the project), made again
const VERSE_COUNTS: &str = "
delete from verse_counts;
delete from chapter_counts;
insert into verse_counts select verse_key, count(*) from readings where active group by verse_key;
insert into chapter_counts select book, chapter, count(*), count(distinct verse) from readings where active group by book, chapter;
";

/// Past this many sermons taken out or put back at once, every verse is counted again rather than
/// only theirs
const RECOUNT_ALL_PAST: usize = 2000;

const READING_COLUMNS: &str = "r.id, r.sermon, r.book, r.chapter, r.verse, r.word_start, r.word_end, r.start_time, \
     r.end_time, r.score, r.accuracy, r.coverage, r.precision, r.kind, r.translation, r.also_in, r.also_at, \
     r.run_id, r.verse_words_heard, r.extra_words, r.said";

const RUN_COLUMNS: &str = "u.id, u.sermon, u.book, u.start_chapter, u.start_verse, u.end_chapter, u.end_verse, \
     u.start_time, u.end_time, u.reading_ids";

/// Where a project's folder keeps its readings
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// A verse as one number: `book * 1000000 + chapter * 1000 + verse`
pub fn verse_key(book: u8, chapter: u16, verse: u16) -> i64 {
    book as i64 * 1_000_000 + chapter as i64 * 1000 + verse as i64
}

/// The verse a [`verse_key`] is
fn verse_of(key: i64) -> (u8, u16, u16) {
    ((key / 1_000_000) as u8, (key / 1000 % 1000) as u16, (key % 1000) as u16)
}

/**
The first and last keys of the verses a reference is of: a book (`2 Corinthians`, `2 Cor`), a
chapter (`Romans 8`), chapters (`Psalm 23-24`), a verse (`John 3:16`), or verses (`2:14-17`,
`7:24-8:2`). `None` if it isn't one
*/
/// The books of one chapter, by number
const ONE_CHAPTER: [u8; 5] = [31, 57, 63, 64, 65];

fn reference_keys(text: &str) -> Option<(i64, i64)> {
    let text = text.trim().replace(['–', '—'], "-");
    let (name, verses) = match text.rsplit_once(' ') {
        Some((name, verses)) if verses.starts_with(|c: char| c.is_ascii_digit()) => (name, Some(verses)),
        _ => (text.as_str(), None),
    };
    // A number alone is a book's number to `find_book`, not what's being typed ("1" of "1 John")
    if !name.contains(|c: char| c.is_alphabetic()) {
        return None;
    }
    let book = crate::bible::find_book(name)?.0;
    let Some(verses) = verses else {
        return Some((verse_key(book, 0, 0), verse_key(book, 999, 999)));
    };
    let number = |s: &str| s.parse::<u16>().ok().filter(|&n| (1..1000).contains(&n));
    // A book of one chapter (Obadiah, Philemon, 2 and 3 John, Jude) is numbered by verse: `Jude 5`
    let verses = match (ONE_CHAPTER.contains(&book), verses.contains(':')) {
        (true, false) => format!("1:{}", verses.replace('-', "-1:")),
        _ => verses.to_string(),
    };
    let verses = verses.as_str();
    let (from, to) = verses.split_once('-').map_or((verses, None), |(a, b)| (a, Some(b)));
    let (start_chapter, start_verse) = match from.split_once(':') {
        Some((c, v)) => (number(c)?, Some(number(v)?)),
        None => (number(from)?, None),
    };
    let (end_chapter, end_verse) = match to {
        None => (start_chapter, start_verse),
        Some(to) => match to.split_once(':') {
            Some((c, v)) => (number(c)?, Some(number(v)?)),
            // `2:14-17` is verses of one chapter; `23-24` is chapters
            None if start_verse.is_some() => (start_chapter, Some(number(to)?)),
            None => (number(to)?, None),
        },
    };
    let (start, end) = (
        verse_key(book, start_chapter, start_verse.unwrap_or(0)),
        verse_key(book, end_chapter, end_verse.unwrap_or(999)),
    );
    (start <= end).then_some((start, end))
}

/// The first and last keys a chapter's verses could have
fn chapter_keys(book: u8, chapter: u16) -> (i64, i64) {
    (verse_key(book, chapter, 0), verse_key(book, chapter, 999))
}

/// A project's readings database, open
pub struct Readings {
    conn: Connection,
    names: Names,
}

/// How many readings there are, and of which verses
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// When the sermons were last searched (milliseconds since 1970; 0 if never)
    pub analyzed_at: u64,
    pub readings: usize,
    pub runs: usize,
    /// Each verse that has readings, as `[book, chapter, verse, readings]`
    pub verses: Vec<(u8, u16, u16, usize)>,
}

/// Which readings to list: each given part narrows them
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RowFilter {
    /// Only readings of these kinds (none: any)
    pub kinds: Vec<MatchKind>,
    /// The translation each fits best, by abbreviation (as rows give it)
    pub translation: Option<String>,
    /// Only sermons by these speakers, or in these series (none: any)
    pub speakers: Vec<String>,
    pub series: Vec<String>,
    /// Words its reference ("Romans 8:28") has, whatever their case
    pub reference: String,
}

impl RowFilter {
    /// Whether it keeps every reading
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
            && self.translation.is_none()
            && self.speakers.is_empty()
            && self.series.is_empty()
            && self.reference.trim().is_empty()
    }
}

/// Which readings of those filtered to list
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "by")]
pub enum Group {
    /// One sermon's, as said
    Sermon { id: String },
    /// One chapter's, by verse
    Chapter { book: u8, chapter: u16 },
    /// Every one, by sermon and as said
    All,
}

/// The readings filtered, counted, and grouped by sermon and by book and chapter (to list each
/// group's when it's opened)
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// All of them, unfiltered
    pub readings: usize,
    pub verses: usize,
    pub books: usize,
    pub sermons: usize,
    /// How many each translation fits best, most first (unfiltered)
    pub translations: Vec<(String, usize)>,
    /// The speakers and series of the sermons with readings, sorted
    pub speakers: Vec<String>,
    pub series: Vec<String>,
    /// How many are left by the filter
    pub shown: usize,
    pub by_sermon: Vec<SermonGroup>,
    pub by_book: Vec<BookGroup>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonGroup {
    pub id: String,
    pub title: String,
    pub speaker: Option<String>,
    pub series: Option<String>,
    pub readings: usize,
    pub verses: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookGroup {
    pub book: u8,
    pub name: String,
    pub readings: usize,
    pub verses: usize,
    pub chapters: Vec<ChapterGroup>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterGroup {
    pub chapter: u16,
    pub readings: usize,
    pub verses: usize,
}

impl Readings {
    /**
    Writes a project's readings as its database at `path`, replacing what's there all at once (it's
    written beside it, then moved into place, so what reads it never sees half of it). `bibles` name
    the translations and books. Readings without the words said are written without them
    ([`fill_said`] gives them)
    */
    pub fn write(path: &Path, project: &Project, bibles: &[&Bible], set: &MatchSet) -> Result<()> {
        let tmp = path.with_extension("sqlite.tmp");
        let _ = std::fs::remove_file(&tmp);
        {
            let mut conn = Connection::open(&tmp)?;
            conn.execute_batch("pragma journal_mode = off; pragma synchronous = off;")?;
            conn.execute_batch(TABLES)?;
            let tx = conn.transaction()?;
            {
                let mut meta = tx.prepare("insert into meta (key, value) values (?1, ?2)")?;
                meta.execute(params!["schema", SCHEMA.to_string()])?;
                meta.execute(params!["analyzed_at", set.analyzed_at.to_string()])?;
                let mut translation =
                    tx.prepare("insert into translations (id, abbreviation, name, position) values (?1, ?2, ?3, ?4)")?;
                for (i, b) in bibles.iter().enumerate() {
                    let abbreviation = if b.abbreviation.is_empty() {
                        &b.name
                    } else {
                        &b.abbreviation
                    };
                    translation.execute(params![b.id, abbreviation, b.name, i as i64])?;
                }
                let named = bibles.iter().find(|b| b.id == project.bible_id).or(bibles.first());
                let mut book = tx.prepare("insert into books (id, name, osis) values (?1, ?2, ?3)")?;
                for b in named.map(|b| b.books.as_slice()).unwrap_or_default() {
                    book.execute(params![b.id, b.name, b.osis])?;
                }
                let mut reading = tx.prepare(
                    "insert or replace into readings values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, \
                     ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, 1)",
                )?;
                for m in &set.matches {
                    reading.execute(params![
                        m.id,
                        m.source_id,
                        m.book,
                        m.chapter,
                        m.verse,
                        m.word_start,
                        m.word_end,
                        m.start,
                        m.end,
                        m.score,
                        m.accuracy,
                        m.coverage,
                        m.precision,
                        kind_name(m.kind),
                        m.translation,
                        serde_json::to_string(&m.also_in)?,
                        serde_json::to_string(&m.also_at)?,
                        m.run_id,
                        m.verse_words_heard
                            .iter()
                            .map(|&h| if h { '1' } else { '0' })
                            .collect::<String>(),
                        serde_json::to_string(&m.extra_words)?,
                        m.said,
                        verse_key(m.book, m.chapter, m.verse),
                    ])?;
                }
                let mut run =
                    tx.prepare("insert or replace into runs values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1)")?;
                for r in &set.runs {
                    run.execute(params![
                        r.id,
                        r.source_id,
                        r.book,
                        r.start_chapter,
                        r.start_verse,
                        r.end_chapter,
                        r.end_verse,
                        r.start,
                        r.end,
                        serde_json::to_string(&r.match_ids)?,
                    ])?;
                }
            }
            tx.commit()?;
            conn.execute_batch(INDEXES)?;
            conn.execute_batch(SERMON_COUNTS)?;
            let mut readings = Readings {
                names: Names::from_parts(Default::default(), Default::default()),
                conn,
            };
            readings.sync_sermons(project)?;
            readings.conn.execute_batch("pragma journal_mode = delete; analyze;")?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// A project's readings database, if it has one ([`open_in`] makes it from the readings kept
    /// as JSON before)
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::not_found(format!("{}: no readings yet", path.display())));
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        let schema: Option<String> = conn
            .query_row("select value from meta where key = 'schema'", [], |r| r.get(0))
            .optional()
            .unwrap_or(None);
        match schema.and_then(|s| s.parse::<i64>().ok()) {
            Some(SCHEMA) => {}
            Some(newer) if newer > SCHEMA => {
                return Err(Error::invalid(format!(
                    "{}: made by a newer version of Extract Audio Bible",
                    path.display()
                )));
            }
            Some(_) => {
                return Err(Error::invalid(format!(
                    "{}: made by an older version of Extract Audio Bible: search the sermons again",
                    path.display()
                )));
            }
            None => return Err(Error::invalid(format!("{}: not a readings database", path.display()))),
        }
        let translations = {
            let mut q = conn.prepare("select id, abbreviation from translations")?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<HashMap<String, String>>>()?
        };
        let books = {
            let mut q = conn.prepare("select id, name from books")?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<HashMap<u8, String>>>()?
        };
        Ok(Readings {
            conn,
            names: Names::from_parts(translations, books),
        })
    }

    /**
    Keeps the `sermons` table as the project has them (their titles, speakers, and which are in it),
    if it's changed since it was last kept. When sermons were taken out or put back, their readings
    are marked so, and the counts made again (a few seconds for a million readings). Returns whether
    anything changed
    */
    pub fn sync_sermons(&mut self, project: &Project) -> Result<bool> {
        let rows: Vec<_> = (project.sources.iter().enumerate())
            .map(|(i, s)| {
                let meta = project.file_metadata(s);
                (
                    s.id.clone(),
                    i as i64,
                    s.file_id(),
                    project.source_title(s),
                    meta.speaker,
                    meta.series,
                    meta.passage,
                    s.transcript_path.clone(),
                    project.audio_path(s),
                )
            })
            .collect();
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            rows.hash(&mut h);
            h.finish().to_string()
        };
        let meta = |key: &str| -> Result<Option<String>> {
            Ok(self
                .conn
                .query_row("select value from meta where key = ?1", [key], |r| r.get(0))
                .optional()?)
        };
        let counted = meta("counts")?.is_some();
        if counted && meta("sermons")?.as_deref() == Some(hash.as_str()) {
            return Ok(false);
        }
        let before: HashSet<String> = {
            let mut q = self.conn.prepare("select id from sermons")?;
            q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        let now: HashSet<String> = rows.iter().map(|r| r.0.clone()).collect();
        let tx = self.conn.transaction()?;
        tx.execute("delete from sermons", [])?;
        {
            let mut insert =
                tx.prepare("insert or replace into sermons values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")?;
            for row in &rows {
                insert.execute(params![row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8])?;
            }
        }
        if !counted {
            // Every reading marked by whether its sermon is in the project
            for table in ["readings", "runs"] {
                tx.execute(
                    &format!("update {table} set active = sermon in (select id from sermons)"),
                    [],
                )?;
            }
        } else {
            // Only the readings of sermons taken out or put back
            for (ids, active) in [(before.difference(&now), 0), (now.difference(&before), 1)] {
                let ids: Vec<&String> = ids.collect();
                for chunk in ids.chunks(500) {
                    let marks = vec!["?"; chunk.len()].join(", ");
                    for table in ["readings", "runs"] {
                        tx.execute(
                            &format!("update {table} set active = {active} where sermon in ({marks})"),
                            params_from_iter(chunk),
                        )?;
                    }
                }
            }
        }
        let changed: Vec<&String> = before.symmetric_difference(&now).collect();
        if !counted || changed.len() > RECOUNT_ALL_PAST {
            tx.execute_batch(VERSE_COUNTS)?;
            tx.execute("insert or replace into meta (key, value) values ('counts', '1')", [])?;
        } else if !changed.is_empty() {
            recount(&tx, &changed)?;
        }
        tx.execute(
            "insert or replace into meta (key, value) values ('sermons', ?1)",
            params![hash],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Calls a translation by another name and abbreviation (as it's been renamed)
    pub fn rename_translation(&mut self, id: &str, name: &str, abbreviation: &str) -> Result<()> {
        let abbreviation = if abbreviation.is_empty() { name } else { abbreviation };
        self.conn.execute(
            "update translations set name = ?2, abbreviation = ?3 where id = ?1",
            params![id, name, abbreviation],
        )?;
        let translations = {
            let mut q = self.conn.prepare("select id, abbreviation from translations")?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<HashMap<String, String>>>()?
        };
        let names = std::mem::replace(
            &mut self.names,
            Names::from_parts(Default::default(), Default::default()),
        );
        self.names = names.with_translations(translations);
        Ok(())
    }

    /// How many readings there are, and of which verses
    pub fn summary(&self) -> Result<Summary> {
        let mut q = self
            .conn
            .prepare("select verse_key, readings from verse_counts order by verse_key")?;
        let verses = q
            .query_map([], |r| {
                let (book, chapter, verse) = verse_of(r.get(0)?);
                Ok((book, chapter, verse, r.get::<_, i64>(1)? as usize))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let runs: i64 = self
            .conn
            .query_row("select count(*) from runs where active", [], |r| r.get(0))?;
        Ok(Summary {
            analyzed_at: self.analyzed_at()?,
            readings: verses.iter().map(|v| v.3).sum(),
            runs: runs as usize,
            verses,
        })
    }

    /// A chapter's readings, best first within each verse, with the runs they're in and those
    /// runs' other readings (which may be of other chapters)
    pub fn chapter(&self, book: u8, chapter: u16) -> Result<MatchSet> {
        let (lo, hi) = chapter_keys(book, chapter);
        let mut matches = self.matches_query("where r.active and r.verse_key between ?1 and ?2", params![lo, hi])?;
        let run_ids: Vec<String> = {
            let ids: HashSet<&String> = matches.iter().filter_map(|m| m.run_id.as_ref()).collect();
            ids.into_iter().cloned().collect()
        };
        let mut runs = vec![];
        for chunk in run_ids.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(", ");
            runs.extend(self.runs_query(
                &format!("where u.active and u.id in ({marks})"),
                params_from_iter(chunk),
            )?);
            // The runs' readings of other chapters
            let mut args: Vec<Value> = chunk.iter().map(|id| Value::Text(id.clone())).collect();
            args.extend([Value::Integer(lo), Value::Integer(hi)]);
            let n = chunk.len();
            matches.extend(self.matches_query(
                &format!(
                    "where r.active and r.run_id in ({marks}) and r.verse_key not between ?{} and ?{}",
                    n + 1,
                    n + 2
                ),
                params_from_iter(args),
            )?);
        }
        matches.sort_by(|a, b| {
            (a.book, a.chapter, a.verse)
                .cmp(&(b.book, b.chapter, b.verse))
                .then(b.score.total_cmp(&a.score))
        });
        runs.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(MatchSet {
            analyzed_at: self.analyzed_at()?,
            matches,
            runs,
        })
    }

    /// A sermon's readings as said, with its runs (none, if it's no longer in the project)
    pub fn sermon(&self, id: &str) -> Result<MatchSet> {
        Ok(MatchSet {
            analyzed_at: self.analyzed_at()?,
            matches: self.matches_query("where r.active and r.sermon = ?1 order by r.word_start", params![id])?,
            runs: self.runs_query("where u.active and u.sermon = ?1 order by u.start_time", params![id])?,
        })
    }

    /// Readings by their ids (those of sermons still in the project)
    pub fn by_ids(&self, ids: &[String]) -> Result<Vec<Match>> {
        let mut out = vec![];
        for chunk in ids.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(", ");
            out.extend(self.matches_query(
                &format!("where r.active and r.id in ({marks})"),
                params_from_iter(chunk),
            )?);
        }
        Ok(out)
    }

    /// Every reading, with every run (in the sermons' order, then as said)
    pub fn all(&self) -> Result<MatchSet> {
        Ok(MatchSet {
            analyzed_at: self.analyzed_at()?,
            matches: self.matches_query(
                "join sermons s on s.id = r.sermon where r.active order by s.position, r.word_start",
                [],
            )?,
            runs: self.runs_query(
                "join sermons s on s.id = u.sermon where u.active order by s.position, u.start_time",
                [],
            )?,
        })
    }

    /// The readings filtered, counted, and grouped (see [`Overview`])
    pub fn overview(&self, filter: &RowFilter) -> Result<Overview> {
        let count = |sql: &str| -> Result<usize> { Ok(self.conn.query_row(sql, [], |r| r.get::<_, i64>(0))? as usize) };
        let mut overview = Overview {
            readings: count("select coalesce(sum(readings), 0) from verse_counts")?,
            verses: count("select count(*) from verse_counts")?,
            books: count("select count(distinct book) from chapter_counts")?,
            sermons: count("select count(*) from sermon_counts c join sermons s on s.id = c.sermon")?,
            ..Default::default()
        };
        {
            let mut q = self.conn.prepare(
                "select t.translation, sum(t.readings) from sermon_translations t join sermons s on s.id = t.sermon \
                 group by t.translation order by 2 desc",
            )?;
            overview.translations = q
                .query_map([], |r| {
                    Ok((
                        self.names.translation(&r.get::<_, String>(0)?),
                        r.get::<_, i64>(1)? as usize,
                    ))
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
        let distinct = |column: &str| -> Result<Vec<String>> {
            let mut q = self.conn.prepare(&format!(
                "select distinct trim(s.{column}) from sermons s join sermon_counts c on c.sermon = s.id \
                 where trim(coalesce(s.{column}, '')) != '' order by 1 collate nocase"
            ))?;
            Ok(q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
        };
        overview.speakers = distinct("speaker")?;
        overview.series = distinct("series")?;

        // Unfiltered, they're counted already; filtered, they're counted from the readings kept
        let (sermon_counts, chapter_counts, args) = if filter.is_empty() {
            (
                "select sermon, readings, verses from sermon_counts".to_string(),
                "select book, chapter, readings, verses from chapter_counts".to_string(),
                vec![],
            )
        } else {
            let (kept, args) = self.filtered(filter)?;
            (
                format!(
                    "select r.sermon as sermon, count(*) as readings, count(distinct r.verse_key) as verses \
                     from readings r \
                     where r.active and {kept} group by r.sermon"
                ),
                format!(
                    "select r.book, r.chapter, count(*), count(distinct r.verse) from readings r \
                     where r.active and {kept} group by r.book, r.chapter"
                ),
                args,
            )
        };
        {
            let mut q = self.conn.prepare(&format!(
                "select g.sermon, s.title, s.speaker, s.series, g.readings, g.verses \
                 from ({sermon_counts}) g join sermons s on s.id = g.sermon \
                 order by s.position"
            ))?;
            overview.by_sermon = q
                .query_map(params_from_iter(&args), |r| {
                    Ok(SermonGroup {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        speaker: r.get(2)?,
                        series: r.get(3)?,
                        readings: r.get::<_, i64>(4)? as usize,
                        verses: r.get::<_, i64>(5)? as usize,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
        overview.shown = overview.by_sermon.iter().map(|g| g.readings).sum();
        let mut q = self
            .conn
            .prepare(&format!("select * from ({chapter_counts}) order by 1, 2"))?;
        let chapters = q
            .query_map(params_from_iter(&args), |r| {
                Ok((
                    r.get::<_, u8>(0)?,
                    ChapterGroup {
                        chapter: r.get(1)?,
                        readings: r.get::<_, i64>(2)? as usize,
                        verses: r.get::<_, i64>(3)? as usize,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (book, chapter) in chapters {
            if overview.by_book.last().is_none_or(|b| b.book != book) {
                overview.by_book.push(BookGroup {
                    book,
                    name: self.names.book(book),
                    ..Default::default()
                });
            }
            let b = overview.by_book.last_mut().expect("just pushed");
            b.readings += chapter.readings;
            b.verses += chapter.verses;
            b.chapters.push(chapter);
        }
        Ok(overview)
    }

    /// The readings filtered, of a group, as rows: each given to `each` as it's read
    pub fn rows_each(&self, filter: &RowFilter, group: &Group, mut each: impl FnMut(ReadingRow)) -> Result<()> {
        let (kept, mut args) = self.filtered(filter)?;
        let (within, order) = match group {
            Group::Sermon { id } => {
                args.push(Value::Text(id.clone()));
                (format!("r.sermon = ?{}", args.len()), "r.word_start")
            }
            Group::Chapter { book, chapter } => {
                let (lo, hi) = chapter_keys(*book, *chapter);
                args.extend([Value::Integer(lo), Value::Integer(hi)]);
                (
                    format!("r.verse_key between ?{} and ?{}", args.len() - 1, args.len()),
                    "r.verse, s.position, r.word_start",
                )
            }
            Group::All => ("1".to_string(), "s.position, r.word_start"),
        };
        let mut q = self.conn.prepare(&format!(
            "select {READING_COLUMNS}, s.title, s.file_id, s.speaker, s.series from readings r \
             join sermons s on s.id = r.sermon where r.active and ({kept}) and {within} order by {order}"
        ))?;
        let mut rows = q.query(params_from_iter(&args))?;
        while let Some(row) = rows.next()? {
            let mut m = read_match(row)?;
            let meta = crate::FileMetadata {
                speaker: row.get(23)?,
                series: row.get(24)?,
                ..Default::default()
            };
            let said = std::mem::take(&mut m.said);
            each(
                self.names
                    .row(&m, &row.get::<_, String>(21)?, &row.get::<_, String>(22)?, &meta, said),
            );
        }
        Ok(())
    }

    /// The readings filtered, of a group, as rows
    pub fn rows(&self, filter: &RowFilter, group: &Group) -> Result<Vec<ReadingRow>> {
        let mut rows = vec![];
        self.rows_each(filter, group, |r| rows.push(r))?;
        Ok(rows)
    }

    /**
    Where clause (and its arguments) for what `filter` keeps, over `r` (readings). A reference
    keeps its book's, chapters', or verses' readings; text that isn't one has its words matched
    against the references of the verses read, here, and the verses they match kept in a
    temporary table
    */
    fn filtered(&self, filter: &RowFilter) -> Result<(String, Vec<Value>)> {
        let mut parts = vec!["1".to_string()];
        let mut args: Vec<Value> = vec![];
        let arg = |args: &mut Vec<Value>, value: Value| {
            args.push(value);
            format!("?{}", args.len())
        };
        if !filter.kinds.is_empty() {
            let kinds: Vec<String> = (filter.kinds.iter())
                .map(|&kind| arg(&mut args, Value::Text(kind_name(kind).into())))
                .collect();
            parts.push(format!("r.kind in ({})", kinds.join(", ")));
        }
        if let Some(abbreviation) = &filter.translation {
            let ids: Vec<String> = (self.names.translation_ids(abbreviation).into_iter())
                .map(|id| arg(&mut args, Value::Text(id)))
                .collect();
            parts.push(format!("r.translation in ({})", ids.join(", ")));
        }
        for (column, wanted) in [("speaker", &filter.speakers), ("series", &filter.series)] {
            if !wanted.is_empty() {
                let marks: Vec<String> = (wanted.iter())
                    .map(|w| arg(&mut args, Value::Text(w.trim().to_string())))
                    .collect();
                parts.push(format!(
                    "r.sermon in (select id from sermons where trim({column}) in ({}))",
                    marks.join(", ")
                ));
            }
        }
        let words: Vec<String> = filter.reference.split_whitespace().map(str::to_lowercase).collect();
        if let Some((start, end)) = reference_keys(&filter.reference) {
            let (start, end) = (
                arg(&mut args, Value::Integer(start)),
                arg(&mut args, Value::Integer(end)),
            );
            parts.push(format!("r.verse_key between {start} and {end}"));
        } else if !words.is_empty() {
            self.conn.execute_batch(
                "create temp table if not exists wanted_verses (verse_key integer primary key); \
                 delete from wanted_verses;",
            )?;
            let mut keys = self.conn.prepare("select verse_key from verse_counts")?;
            let keys = keys
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut insert = self.conn.prepare("insert into wanted_verses values (?1)")?;
            for key in keys {
                let (book, chapter, verse) = verse_of(key);
                let reference = self.names.reference(book, chapter, verse).to_lowercase();
                if words.iter().all(|w| reference.contains(w.as_str())) {
                    insert.execute([key])?;
                }
            }
            parts.push("r.verse_key in (select verse_key from wanted_verses)".into());
        }
        Ok((parts.join(" and "), args))
    }

    fn analyzed_at(&self) -> Result<u64> {
        let at: Option<String> = self
            .conn
            .query_row("select value from meta where key = 'analyzed_at'", [], |r| r.get(0))
            .optional()?;
        Ok(at.and_then(|a| a.parse().ok()).unwrap_or(0))
    }

    /// Readings, `tail` saying which (and in what order) of `readings r`
    fn matches_query(&self, tail: &str, args: impl rusqlite::Params) -> Result<Vec<Match>> {
        let mut q = self
            .conn
            .prepare(&format!("select {READING_COLUMNS} from readings r {tail}"))?;
        let mut rows = q.query(args)?;
        let mut out = vec![];
        while let Some(row) = rows.next()? {
            out.push(read_match(row)?);
        }
        Ok(out)
    }

    /// Runs, `tail` saying which (and in what order) of `runs u`
    fn runs_query(&self, tail: &str, args: impl rusqlite::Params) -> Result<Vec<Run>> {
        let mut q = self.conn.prepare(&format!("select {RUN_COLUMNS} from runs u {tail}"))?;
        let mut rows = q.query(args)?;
        let mut out = vec![];
        while let Some(row) = rows.next()? {
            out.push(Run {
                id: row.get(0)?,
                source_id: row.get(1)?,
                book: row.get(2)?,
                start_chapter: row.get(3)?,
                start_verse: row.get(4)?,
                end_chapter: row.get(5)?,
                end_verse: row.get(6)?,
                start: row.get(7)?,
                end: row.get(8)?,
                match_ids: serde_json::from_str(&row.get::<_, String>(9)?)?,
            });
        }
        Ok(out)
    }
}

/**
A project's readings database in its folder `dir`, its list of sermons kept as the project has
them: made first from the readings kept as JSON before (`matches.json`, then renamed
`matches.json.migrated`), with the words said read from the transcripts through `files` and the
translations from `bibles` (asked for only then). `None` when it has neither (it hasn't been
searched)
*/
pub fn open_in(
    dir: &Path,
    project: &Project,
    files: &dyn Files,
    bibles: impl FnOnce() -> Result<Vec<Bible>>,
) -> Result<Option<Readings>> {
    let db = path(dir);
    let legacy = dir.join(LEGACY_FILE);
    if !db.exists() && legacy.exists() {
        let mut set: MatchSet = serde_json::from_slice(&std::fs::read(&legacy)?)?;
        fill_said(project, &mut set, files);
        let bibles = bibles()?;
        Readings::write(&db, project, &bibles.iter().collect::<Vec<_>>(), &set)?;
        std::fs::rename(&legacy, dir.join(format!("{LEGACY_FILE}.migrated")))?;
    }
    if !db.exists() {
        return Ok(None);
    }
    let mut readings = Readings::open(&db)?;
    readings.sync_sermons(project)?;
    Ok(Some(readings))
}

/**
Gives readings found before the words said were kept their words, from their transcripts (read
through `files`); a sermon whose transcript can't be read keeps them empty. Sermons are read
several at a time, each thread holding one transcript at a time (holding them all could take more
memory than there is)
*/
pub fn fill_said(project: &Project, set: &mut MatchSet, files: &dyn Files) {
    use rayon::prelude::*;
    // Each sermon's readings without their words, by index
    let mut wanted: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, m) in set.matches.iter().enumerate() {
        if m.said.is_empty() {
            wanted.entry(m.source_id.as_str()).or_default().push(i);
        }
    }
    let matches = &set.matches;
    let said: Vec<(usize, String)> = (project.sources.par_iter())
        .filter_map(|s| Some((s, wanted.get(s.id.as_str())?)))
        .flat_map_iter(|(s, indexes)| {
            let transcript = Transcript::open(files, &s.transcript_path).ok();
            let words: Vec<(usize, String)> = (indexes.iter())
                .filter_map(|&i| {
                    let m = &matches[i];
                    let words = transcript
                        .as_ref()?
                        .words
                        .get(m.word_start as usize..=m.word_end as usize)?;
                    Some((i, words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")))
                })
                .collect();
            words
        })
        .collect();
    for (i, words) in said {
        set.matches[i].said = words;
    }
}

/// Counts again the verses and chapters read in some sermons, taken out or put back
fn recount(conn: &Connection, sermons: &[&String]) -> Result<()> {
    conn.execute_batch(
        "create temp table if not exists changed_sermons (id text primary key); delete from changed_sermons; \
         create temp table if not exists touched_verses (verse_key integer primary key); delete from touched_verses;",
    )?;
    {
        let mut insert = conn.prepare("insert or ignore into changed_sermons values (?1)")?;
        for id in sermons {
            insert.execute([id])?;
        }
    }
    conn.execute_batch(
        "insert or ignore into touched_verses \
             select distinct verse_key from readings where sermon in (select id from changed_sermons); \
         delete from verse_counts where verse_key in (select verse_key from touched_verses); \
         insert into verse_counts select verse_key, count(*) from readings \
             where active and verse_key in (select verse_key from touched_verses) group by verse_key;",
    )?;
    let chapters: Vec<i64> = {
        let mut q = conn.prepare("select distinct verse_key / 1000 from touched_verses")?;
        q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let mut delete = conn.prepare("delete from chapter_counts where book = ?1 and chapter = ?2")?;
    let mut insert = conn.prepare(
        "insert into chapter_counts select ?1, ?2, count(*), count(distinct verse) from readings \
         where active and verse_key between ?3 and ?4 having count(*) > 0",
    )?;
    for chapter in chapters {
        let (book, chapter) = ((chapter / 1000) as u8, (chapter % 1000) as u16);
        let (lo, hi) = chapter_keys(book, chapter);
        delete.execute(params![book, chapter])?;
        insert.execute(params![book, chapter, lo, hi])?;
    }
    Ok(())
}

fn kind_name(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::Exact => "exact",
        MatchKind::Close => "close",
        MatchKind::Partial => "partial",
    }
}

fn read_match(row: &Row) -> Result<Match> {
    let kind = match row.get::<_, String>(13)?.as_str() {
        "exact" => MatchKind::Exact,
        "close" => MatchKind::Close,
        _ => MatchKind::Partial,
    };
    Ok(Match {
        id: row.get(0)?,
        source_id: row.get(1)?,
        book: row.get(2)?,
        chapter: row.get(3)?,
        verse: row.get(4)?,
        word_start: row.get(5)?,
        word_end: row.get(6)?,
        start: row.get(7)?,
        end: row.get(8)?,
        score: row.get(9)?,
        accuracy: row.get(10)?,
        coverage: row.get(11)?,
        precision: row.get(12)?,
        kind,
        translation: row.get(14)?,
        also_in: serde_json::from_str(&row.get::<_, String>(15)?)?,
        also_at: serde_json::from_str(&row.get::<_, String>(16)?)?,
        run_id: row.get(17)?,
        verse_words_heard: row.get::<_, String>(18)?.chars().map(|c| c == '1').collect(),
        extra_words: serde_json::from_str(&row.get::<_, String>(19)?)?,
        said: row.get(20)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BookScope, Memory, Source, analyze_with, import, matcher, reading_rows};

    fn bible(id: &str, lines: &str) -> Bible {
        Bible::new(
            id,
            id,
            id.to_uppercase(),
            import(vec![(format!("{id}.txt"), lines.as_bytes().to_vec())]).unwrap(),
        )
    }

    fn source(id: &str) -> Source {
        Source {
            id: id.into(),
            label: id.into(),
            audio_path: String::new(),
            transcript_path: format!("{id}.json"),
            duration: None,
            word_count: 0,
            match_count: 0,
            file_id: id.into(),
        }
    }

    /// Two sermons searched in two translations, and a folder to keep them in
    fn searched() -> (Project, Vec<Bible>, MatchSet, Memory, PathBuf) {
        let kjv = bible(
            "kjv",
            "John 11:35 Jesus wept.\nJohn 11:36 Then said the Jews, Behold how he loved him!\nGenesis 1:1 In the beginning God created the heaven and the earth.",
        );
        let web = bible(
            "web",
            "John 11:35 Jesus wept.\nJohn 11:36 The Jews therefore said, See how much affection he had for him!\nGenesis 1:1 In the beginning, God created the heavens and the earth.",
        );
        let mut files = Memory::new();
        files.insert(
            "a.json",
            br#"{"segments": [{"start": 0, "end": 30, "text": "In the beginning, God created the heavens and the earth. Then Jesus wept. The Jews therefore said, See how much affection he had for him!"}]}"#.to_vec(),
        );
        files.insert(
            "b.json",
            br#"{"segments": [{"start": 0, "end": 20, "text": "Jesus wept. Then said the Jews, Behold how he loved him!"}]}"#.to_vec(),
        );
        let mut project = Project::readings("p", "", &[&kjv, &web], BookScope::default()).unwrap();
        project.sources = vec![source("a"), source("b")];
        project.apply_file_metadata(
            [(
                "b".into(),
                crate::FileMetadata {
                    speaker: Some("Paul".into()),
                    ..Default::default()
                },
            )]
            .into(),
        );
        let engine = matcher::engine_of([&kjv, &web]).unwrap();
        let found = analyze_with(&mut project, &engine, &files, &|_, _, _| {}).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "eab-db-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (project, vec![kjv, web], found.matches, files, dir)
    }

    /// Whether the counts kept are those counted again from every reading
    fn counts_hold(db: &Readings) -> bool {
        let rows = |sql: &str| -> Vec<(i64, i64, i64, i64)> {
            let mut q = db.conn.prepare(sql).unwrap();
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        rows("select verse_key, readings, 0, 0 from verse_counts order by 1")
            == rows("select verse_key, count(*), 0, 0 from readings where active group by 1 order by 1")
            && rows("select * from chapter_counts order by 1, 2")
                == rows(
                    "select book, chapter, count(*), count(distinct verse) from readings where active \
                     group by 1, 2 order by 1, 2",
                )
    }

    #[test]
    fn references_filtered_by() {
        let keys = |text| reference_keys(text).map(|(a, b)| (verse_of(a), verse_of(b)));
        // 2 Corinthians 2, not 1 Corinthians (nor 2 Corinthians 12:2)
        assert_eq!(keys("2 Corinthians 2"), Some(((47, 2, 0), (47, 2, 999))));
        assert_eq!(keys("2 cor 2"), keys("2 Corinthians 2"));
        assert_eq!(keys("John"), Some(((43, 0, 0), (43, 999, 999))));
        assert_eq!(keys("John 3:16"), Some(((43, 3, 16), (43, 3, 16))));
        assert_eq!(keys("Romans 7:24–8:2"), Some(((45, 7, 24), (45, 8, 2))));
        assert_eq!(keys("Psalm 23-24"), Some(((19, 23, 0), (19, 24, 999))));
        assert_eq!(keys("2 Corinthians 2:14-17"), Some(((47, 2, 14), (47, 2, 17))));
        // A book of one chapter is numbered by verse
        assert_eq!(keys("Jude 5"), Some(((65, 1, 5), (65, 1, 5))));
        assert_eq!(keys("Jude 3-4"), Some(((65, 1, 3), (65, 1, 4))));
        assert_eq!(keys("Jude 1:5"), keys("Jude 5"));
        // Not references: matched word by word
        assert_eq!(keys("1"), None);
        assert_eq!(keys("John 3:"), None);
        assert_eq!(keys("John 3:17-16"), None);
    }

    #[test]
    fn readings_kept_and_read_back() {
        let (mut project, bibles, set, files, dir) = searched();
        let bibles: Vec<&Bible> = bibles.iter().collect();
        Readings::write(&path(&dir), &project, &bibles, &set).unwrap();
        let mut db = Readings::open(&path(&dir)).unwrap();

        let summary = db.summary().unwrap();
        assert_eq!(summary.readings, set.matches.len());
        assert_eq!(summary.runs, set.runs.len());
        assert!(summary.verses.contains(&(43, 11, 35, 2)), "{:?}", summary.verses);

        // Read back as they were written
        let all = db.all().unwrap();
        assert_eq!(all.matches.len(), set.matches.len());
        let written = |id: &str| set.matches.iter().find(|m| m.id == id).unwrap();
        for m in &all.matches {
            let w = written(&m.id);
            assert_eq!(
                (&m.said, &m.verse_words_heard, &m.also_in, m.kind),
                (&w.said, &w.verse_words_heard, &w.also_in, w.kind)
            );
        }
        assert!(all.matches.iter().all(|m| !m.said.is_empty()));

        // A chapter's, best first in each verse, with its runs and their readings
        let john = db.chapter(43, 11).unwrap();
        assert!(john.matches.iter().any(|m| m.verse == 35) && john.matches.iter().any(|m| m.verse == 36));
        for run in &john.runs {
            assert!(run.match_ids.iter().all(|id| john.matches.iter().any(|m| &m.id == id)));
        }
        let b = db.sermon("b").unwrap();
        assert!(b.matches.iter().all(|m| m.source_id == "b") && !b.matches.is_empty());
        assert_eq!(db.by_ids(&[b.matches[0].id.clone()]).unwrap().len(), 1);

        // The rows the readings export gives, the same as from the readings in memory
        let rows = db.rows(&RowFilter::default(), &Group::All).unwrap();
        assert_eq!(rows, reading_rows(&project, &set, &bibles, &files));

        // Filtered and grouped
        let overview = db.overview(&RowFilter::default()).unwrap();
        assert_eq!(
            (overview.readings, overview.sermons, overview.shown),
            (rows.len(), 2, rows.len())
        );
        assert_eq!(overview.speakers, ["Paul"]);
        assert_eq!(
            overview.by_sermon.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(overview.by_book.iter().map(|b| b.book).collect::<Vec<_>>(), [1, 43]);
        // Kinds together keep each one's readings
        let of_kinds = |kinds: &[MatchKind]| {
            let only = RowFilter {
                kinds: kinds.to_vec(),
                ..Default::default()
            };
            db.rows(&only, &Group::All).unwrap().len()
        };
        let (exact, close) = (of_kinds(&[MatchKind::Exact]), of_kinds(&[MatchKind::Close]));
        assert!(exact > 0, "{rows:?}");
        assert_eq!(of_kinds(&[MatchKind::Exact, MatchKind::Close]), exact + close);
        assert_eq!(
            of_kinds(&[MatchKind::Exact, MatchKind::Close, MatchKind::Partial]),
            rows.len()
        );
        let pauls = RowFilter {
            speakers: vec!["Paul".into()],
            ..Default::default()
        };
        assert_eq!(db.overview(&pauls).unwrap().by_sermon.len(), 1);
        let web = RowFilter {
            translation: Some("WEB".into()),
            reference: "john 11:36".into(),
            ..Default::default()
        };
        let found = db.rows(&web, &Group::Chapter { book: 43, chapter: 11 }).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (found[0].sermon_id.as_str(), found[0].reference.as_str()),
            ("a", "John 11:36")
        );
        assert_eq!(db.rows(&pauls, &Group::Sermon { id: "a".into() }).unwrap().len(), 0);

        // A sermon taken out leaves its readings out, until it's put back
        let removed = project.sources.remove(0);
        assert!(db.sync_sermons(&project).unwrap());
        assert!(!db.sync_sermons(&project).unwrap());
        assert!(db.all().unwrap().matches.iter().all(|m| m.source_id == "b"));
        assert!(counts_hold(&db));
        assert!(db.summary().unwrap().readings < set.matches.len());
        assert_eq!(db.overview(&RowFilter::default()).unwrap().sermons, 1);
        project.sources.insert(0, removed);
        db.sync_sermons(&project).unwrap();
        assert_eq!(db.summary().unwrap().readings, set.matches.len());
        assert!(counts_hold(&db));
        assert_eq!(
            db.overview(&RowFilter::default())
                .unwrap()
                .translations
                .iter()
                .map(|t| t.1)
                .sum::<usize>(),
            rows.len()
        );

        // A translation renamed is called so, and filtered by, at once and when opened again
        let of_web = |db: &Readings, abbreviation: &str| {
            let only = RowFilter {
                translation: Some(abbreviation.into()),
                ..Default::default()
            };
            db.rows(&only, &Group::All).unwrap().len()
        };
        let web = of_web(&db, "WEB");
        assert!(web > 0);
        db.rename_translation("web", "World English Bible", "WEBUS").unwrap();
        assert_eq!((of_web(&db, "WEBUS"), of_web(&db, "WEB")), (web, 0));
        let again = Readings::open(&path(&dir)).unwrap();
        assert_eq!(of_web(&again, "WEBUS"), web);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn readings_kept_as_json_are_moved_in() {
        let (project, bibles, mut set, files, dir) = searched();
        let bibles: Vec<&Bible> = bibles.iter().collect();
        let owned = || Ok(bibles.iter().map(|b| (*b).clone()).collect());
        assert!(open_in(&dir, &project, &files, owned).unwrap().is_none());
        // As kept before the words said were
        for m in &mut set.matches {
            m.said.clear();
        }
        std::fs::write(dir.join(LEGACY_FILE), serde_json::to_vec(&set).unwrap()).unwrap();
        let db = open_in(&dir, &project, &files, owned).unwrap().unwrap();
        assert!(!dir.join(LEGACY_FILE).exists() && dir.join("matches.json.migrated").exists());
        let all = db.all().unwrap();
        assert_eq!(all.matches.len(), set.matches.len());
        assert!(all.matches.iter().all(|m| !m.said.is_empty()));
        assert!(Readings::open(&dir.join("nothing.sqlite")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
