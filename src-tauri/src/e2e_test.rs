//! Against a whole Bible: `KJV_USFM=/path/to/usfm cargo test --release e2e -- --ignored --nocapture`

use std::{path::PathBuf, time::Instant};

use crate::{
    bible::{self, Bible},
    matcher::{MatchOptions, VerseIndex, find_matches},
    transcript::Transcript,
};

#[test]
#[ignore]
fn e2e() {
    let dir = PathBuf::from(std::env::var("KJV_USFM").expect("KJV_USFM"));
    let t = Instant::now();
    let (books, format) = bible::import(&[dir]).unwrap();
    let bible = Bible { id: "kjv".into(), name: "KJV".into(), abbreviation: "KJV".into(), language: None, format, books };
    println!("imported {} verses in {:?}", bible.summary().verse_count, t.elapsed());
    let t = Instant::now();
    let index = VerseIndex::new(&bible, None);
    println!("indexed in {:?}", t.elapsed());

    let john = bible.books.iter().find(|b| b.id == 43).unwrap();
    let ps = bible.books.iter().find(|b| b.id == 19).unwrap();
    let mut said = String::from("good morning church let's open our bibles to john chapter three verse fourteen. ");
    for v in 14..=18 {
        said += &john.chapters[2][v - 1];
        said += " ";
    }
    said += "now notice what he says there. whosoever believeth in him should not perish. isn't that amazing? \
             and the psalmist says ";
    said += &ps.chapters[22][0].replace("shepherd", "shepard");
    said += " and he goes on. ";
    said += &john.chapters[10][34];
    said += " let's pray.";
    // Filler, so the sermon is the length of a real one
    for _ in 0..300 {
        said += " and so we see that the lord is good and we must trust him every day in all that we do ";
    }
    let words: Vec<String> = said
        .split_whitespace()
        .enumerate()
        .map(|(i, w)| format!(r#"{{"word": {}, "start": {:.2}, "end": {:.2}, "score": 0.9}}"#, serde_json::to_string(w).unwrap(), i as f64 * 0.35, i as f64 * 0.35 + 0.28))
        .collect();
    let json = format!(r#"{{"segments": [{{"start": 0, "end": 99999, "text": "", "words": [{}]}}]}}"#, words.join(","));
    let path = std::env::temp_dir().join("cab-e2e.json");
    std::fs::write(&path, json).unwrap();
    let transcript = Transcript::open(&path).unwrap();
    let t = Instant::now();
    let (matches, runs) = find_matches(&index, &transcript, "s", &MatchOptions::default());
    println!("{} words matched in {:?}", transcript.words.len(), t.elapsed());
    for m in &matches {
        println!("{} {}:{} {:?} score {:.2} cov {:.2} prec {:.2} run {:?}", m.book, m.chapter, m.verse, m.kind, m.score, m.coverage, m.precision, m.run_id.is_some());
    }
    for r in &runs {
        println!("run {}:{}–{}:{} ({} verses)", r.start_chapter, r.start_verse, r.end_chapter, r.end_verse, r.match_ids.len());
    }
    let has = |b: u8, c: u16, v: u16| matches.iter().any(|m| m.book == b && m.chapter == c && m.verse == v);
    for v in 14..=18 {
        assert!(has(43, 3, v), "John 3:{v}");
    }
    assert!(has(19, 23, 1));
    assert!(has(43, 11, 35), "Jesus wept");
}

/// `KJV_USFM=… SERMON=/path/sunday (with .mp3 and .json) cargo test --release export_e2e -- --ignored --nocapture`
#[test]
#[ignore]
fn export_e2e() {
    use crate::{
        export::{self, ChapterRef},
        project::{Project, Selection, Source},
    };
    let dir = PathBuf::from(std::env::var("KJV_USFM").expect("KJV_USFM"));
    let sermon = std::env::var("SERMON").expect("SERMON");
    let (books, format) = bible::import(&[dir]).unwrap();
    let bible = Bible { id: "kjv".into(), name: "King James Version".into(), abbreviation: "KJV".into(), language: None, format, books };
    let index = VerseIndex::new(&bible, None);
    let transcript = Transcript::open(std::path::Path::new(&format!("{sermon}.json"))).unwrap();
    let (matches, runs) = find_matches(&index, &transcript, "s1", &MatchOptions::default());
    for m in &matches {
        println!("{}:{} {:?} {:.2} {:.2}–{:.2}", m.chapter, m.verse, m.kind, m.score, m.start, m.end);
    }
    assert_eq!(runs.len(), 1);
    let out = std::env::temp_dir().join("cab-export-e2e");
    let _ = std::fs::remove_dir_all(&out);
    let mut project = Project {
        id: "p".into(),
        name: "Test".into(),
        bible_id: "kjv".into(),
        created_at: 0,
        updated_at: 0,
        sources: vec![Source {
            id: "s1".into(),
            label: "Sunday".into(),
            audio_path: format!("{sermon}.mp3"),
            transcript_path: format!("{sermon}.json"),
            duration: None,
            word_count: 0,
            match_count: 0,
        }],
        selections: Default::default(),
        settings: Default::default(),
        metadata: Default::default(),
    };
    for m in matches.iter().filter(|m| m.run_id.is_some()) {
        project.selections.insert(
            export::verse_key(m.book, m.chapter, m.verse),
            Selection {
                match_id: m.id.clone(),
                source_id: m.source_id.clone(),
                start: m.start,
                end: m.end,
                trim_start: 0.0,
                trim_end: 0.0,
                pad_before: if m.verse == 16 { 1.0 } else { 0.0 },
                pad_after: 0.0,
                needs_trim: false,
                note: String::new(),
            },
        );
    }
    project.metadata.output_dir = Some(out.to_string_lossy().into_owned());
    project.metadata.artist = "Test Church".into();
    project.metadata.year = "2026".into();
    let chapters = export::chapters_to_export(&project, &bible);
    assert_eq!(chapters.len(), 1);
    let t = Instant::now();
    let files = export::export(&project, &bible, &[ChapterRef { book: 43, chapter: 3 }], |p| println!("{p:?}")).unwrap();
    println!("exported in {:?}: {:?}", t.elapsed(), files);
    let tag = id3::Tag::read_from_path(&files[0].path).unwrap();
    use id3::TagLike;
    assert_eq!(tag.title(), Some("John 3"));
    assert_eq!(tag.album(), Some("King James Version – John"));
    assert_eq!(tag.artist(), Some("Test Church"));
    assert_eq!(tag.track(), Some(3));
    let spoken: f64 = matches.iter().filter(|m| m.run_id.is_some()).map(|m| m.end - m.start).sum();
    // Lead-in 0.5, tail 1.0, and 1.0 before verse 16
    assert!((files[0].duration - (spoken + 2.5)).abs() < 0.1, "{} vs {}", files[0].duration, spoken + 2.5);
    assert_eq!(files[0].missing.len(), 31);
}
