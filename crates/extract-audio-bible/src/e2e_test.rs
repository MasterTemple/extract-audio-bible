//! Against a whole Bible: `KJV_USFM=/path/to/usfm cargo test --release e2e -- --ignored --nocapture`

use std::{path::PathBuf, time::Instant};

use crate::{
    bible::{self, Bible},
    matcher::{self, find_matches},
    transcript::Transcript,
};

#[test]
#[ignore]
fn e2e() {
    let dir = PathBuf::from(std::env::var("KJV_USFM").expect("KJV_USFM"));
    let t = Instant::now();
    let bible = Bible::new("kjv", "KJV", "KJV", bible::import_paths(&[dir]).unwrap());
    println!("imported {} verses in {:?}", bible.summary().verse_count, t.elapsed());
    let t = Instant::now();
    let engine = matcher::engine(&bible).unwrap();
    let searcher = engine.searcher();
    println!("indexed {} verses in {:?}", searcher.verse_count(), t.elapsed());

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
        .map(|(i, w)| {
            format!(
                r#"{{"word": {}, "start": {:.2}, "end": {:.2}, "score": 0.9}}"#,
                serde_json::to_string(w).unwrap(),
                i as f64 * 0.35,
                i as f64 * 0.35 + 0.28
            )
        })
        .collect();
    let json = format!(
        r#"{{"segments": [{{"start": 0, "end": 99999, "text": "", "words": [{}]}}]}}"#,
        words.join(",")
    );
    let transcript = Transcript::parse(&json).unwrap();
    let t = Instant::now();
    let (matches, runs) = find_matches(&searcher, &transcript, "s", &Default::default());
    println!("{} words matched in {:?}", transcript.words.len(), t.elapsed());
    for m in &matches {
        println!(
            "{} {}:{} {:?} score {:.2} cov {:.2} prec {:.2} run {:?}",
            m.book,
            m.chapter,
            m.verse,
            m.kind,
            m.score,
            m.coverage,
            m.precision,
            m.run_id.is_some()
        );
    }
    for r in &runs {
        println!(
            "run {}:{}–{}:{} ({} verses)",
            r.start_chapter,
            r.start_verse,
            r.end_chapter,
            r.end_verse,
            r.match_ids.len()
        );
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
    let bible = Bible::new("kjv", "King James Version", "KJV", bible::import_paths(&[dir]).unwrap());
    let engine = matcher::engine(&bible).unwrap();
    let transcript = Transcript::open(&crate::Disk, &format!("{sermon}.json")).unwrap();
    let (matches, runs) = find_matches(&engine.searcher(), &transcript, "s1", &Default::default());
    for m in &matches {
        println!(
            "{}:{} {:?} {:.2} {:.2}–{:.2}",
            m.chapter, m.verse, m.kind, m.score, m.start, m.end
        );
    }
    assert_eq!(runs.len(), 1);
    let out = std::env::temp_dir().join("cab-export-e2e");
    let _ = std::fs::remove_dir_all(&out);
    let mut project = Project {
        id: "p".into(),
        name: "Test".into(),
        kind: Default::default(),
        translations: vec![],
        scope: Default::default(),
        bible_id: "kjv".into(),
        book: Some(43),
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
            file_id: String::new(),
        }],
        selections: Default::default(),
        settings: Default::default(),
        metadata: Default::default(),
        files: Default::default(),
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
                gain_db: 0.0,
                needs_trim: false,
                note: String::new(),
                joined: vec![],
            },
        );
    }
    project.metadata.artist = "Test Church".into();
    project.metadata.year = "2026".into();
    let chapters = export::chapters_to_export(&project, &bible);
    assert_eq!(chapters.len(), 1);
    let t = Instant::now();
    let files = export::export_to_dir(
        &project,
        &bible,
        &[ChapterRef { book: 43, chapter: 3 }],
        &crate::Disk,
        &out,
        |p| println!("{p:?}"),
    )
    .unwrap();
    println!("exported in {:?}: {:?}", t.elapsed(), files);
    let tag = id3::Tag::read_from_path(&files[0].path).unwrap();
    use id3::TagLike;
    assert_eq!(tag.title(), Some("John 3"));
    assert_eq!(tag.album(), Some("King James Version – John"));
    assert_eq!(tag.artist(), Some("Test Church"));
    assert_eq!(tag.track(), Some(3));
    let spoken: f64 = matches
        .iter()
        .filter(|m| m.run_id.is_some())
        .map(|m| m.end - m.start)
        .sum();
    // Lead-in 0.5, tail 1.0, and 1.0 before verse 16
    assert!(
        (files[0].duration - (spoken + 2.5)).abs() < 0.1,
        "{} vs {}",
        files[0].duration,
        spoken + 2.5
    );
    assert_eq!(files[0].missing.len(), 31);

    // Verse files too, the whole export leveled as one, and verse 16 turned up 6 dB
    use crate::project::{ExportMode, Leveling};
    project.metadata.export_mode = ExportMode::Both;
    project.settings.leveling = Leveling::Export;
    project.selections.get_mut("43.3.16").unwrap().gain_db = 6.0;
    let files = export::export_to_dir(&project, &bible, &chapters, &crate::Disk, &out, |_| {}).unwrap();
    assert_eq!(files.len(), 6);
    let v16 = files.iter().find(|f| f.verse == Some(16)).unwrap();
    assert!(v16.path.ends_with("43 John/John 3/43_John_003_016.mp3"), "{}", v16.path);
    let tag = id3::Tag::read_from_path(&v16.path).unwrap();
    assert_eq!(tag.title(), Some("John 3:16"));
    assert_eq!(
        (tag.track(), tag.total_tracks(), tag.disc()),
        (Some(16), Some(36), Some(3))
    );
    let loudness = |path: &str| {
        let pcm = crate::audio::decode_range(&crate::Disk, path, 0.0, 1e4).unwrap();
        20.0 * crate::audio::speech_rms(crate::audio::levels(&pcm.samples, pcm.rate))
            .unwrap()
            .log10()
    };
    let v15 = files.iter().find(|f| f.verse == Some(15)).unwrap();
    let (l15, l16) = (loudness(&v15.path), loudness(&v16.path));
    println!("verse 15 {l15:.1} dB, verse 16 {l16:.1} dB");
    assert!((l16 - l15 - 6.0).abs() < 1.5, "{l15} {l16}");
    assert!((l15 + 20.0).abs() < 3.0, "{l15}");
}
