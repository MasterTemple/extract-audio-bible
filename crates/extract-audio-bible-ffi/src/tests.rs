use crate::*;

const KJV: &str = "John 11:33 When Jesus therefore saw her weeping, and the Jews also weeping which came with her, he groaned in the spirit, and was troubled,
John 11:34 And said, Where have ye laid him? They said unto him, Lord, come and see.
John 11:35 Jesus wept.
John 11:36 Then said the Jews, Behold how he loved him!";

fn bible(eab: &ExtractAudioBible) -> Bible {
    eab.import_bible(
        vec![InputFile {
            path: "kjv.txt".into(),
            bytes: KJV.as_bytes().to_vec(),
        }],
        "King James Version".into(),
        "KJV".into(),
    )
    .unwrap()
}

/// A sermon reading John 11:34-36, as a transcript with word times and a tone as its audio
fn sermon(files: &Files) {
    let said = "good morning and said where have ye laid him they said unto him lord come and see jesus wept then said the jews behold how he loved him amen";
    let words: Vec<String> = (said.split(' ').enumerate())
        .map(|(i, w)| {
            format!(
                r#"{{"word": "{w}", "start": {:.2}, "end": {:.2}}}"#,
                i as f64 * 0.4,
                i as f64 * 0.4 + 0.3
            )
        })
        .collect();
    let json = format!(
        r#"{{"segments": [{{"start": 0, "end": 12, "text": "", "words": [{}]}}]}}"#,
        words.join(",")
    );
    files.add("sermons/sunday.json".into(), json.into_bytes());
    let rate = 44_100;
    let samples = (0..rate * 12)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / rate as f32).sin() * 0.3)
        .collect();
    files.add(
        "sermons/sunday.mp3".into(),
        eab::audio::mp3(&eab::audio::Pcm { rate, samples }, 64).unwrap(),
    );
}

#[test]
fn bible_and_search_without_a_project() {
    let eab = ExtractAudioBible::new();
    let bible = bible(&eab);
    assert_eq!(bible.abbreviation(), "KJV");
    assert_eq!(bible.verse(43, 11, 35).as_deref(), Some("Jesus wept."));
    assert_eq!(bible.books()[0].verse_counts[10], 36);
    let again = eab.read_bible(bible.to_json()).unwrap();
    assert_eq!(again.chapter(43, 11).len(), 36);

    let t = eab.read_transcript(r#"{"segments": [{"start": 0, "end": 6, "text": "Jesus wept. Then said the Jews, behold how he loved him."}]}"#.into()).unwrap();
    assert_eq!(t.word_count(), 11);
    let found = eab
        .searcher(&bible, Some("Jn".into()), 0.85)
        .unwrap()
        .find(&t, "s".into());
    let verses: Vec<u16> = found.readings.iter().map(|r| r.verse).collect();
    assert_eq!(verses, [35, 36]);
    assert_eq!(found.runs.len(), 1);
    assert_eq!(found.readings[0].kind, ReadingKind::Exact);
    assert_eq!(eab.book_id("1 Thess".into()), Some(52));
}

#[test]
fn a_project_from_sermon_to_mp3() {
    let eab = ExtractAudioBible::new();
    let bible = bible(&eab);
    let files = Files::memory();
    sermon(&files);
    let project = eab.new_project(&bible, String::new(), Some("John".into())).unwrap();
    assert_eq!(project.name(), "John (KJV)");
    let added = project.add_sources(files.paths());
    assert_eq!((added.added, added.unpaired.len()), (1, 0));

    let summary = project.search(&bible, &files).unwrap();
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert!(summary.readings >= 3, "{summary:?}");

    // What's known about the sermon, by its file id
    assert_eq!(project.sources()[0].file_id, "sunday");
    project
        .apply_file_metadata_json(
            r#"{"sunday": {"passage": "John 11:34-36"}, "later": {"title": "Not added yet"}}"#.into(),
        )
        .unwrap();
    project
        .apply_file_metadata_json(r#"{"sunday": {"title": "Jesus Wept"}}"#.into())
        .unwrap();
    let info = &project.sources()[0];
    assert_eq!(
        (info.title.as_deref(), info.passage.as_deref()),
        (Some("Jesus Wept"), Some("John 11:34-36"))
    );
    assert_eq!(
        project.file_metadata("later".into()).title.as_deref(),
        Some("Not added yet")
    );
    project.set_file_metadata("later".into(), FileMetadata::default());
    assert_eq!(project.file_metadata("later".into()), FileMetadata::default());
    assert!(matches!(
        project.apply_file_metadata_json(r#"{"sunday": {"speaker": "x"}}"#.into()),
        Err(ExtractAudioBibleError::Invalid { .. })
    ));
    let chosen = project.choose_best(&bible, 0.75).unwrap();
    assert_eq!(chosen[0].verses, [34, 35, 36]);
    let status = project.status(&bible);
    assert_eq!((status[10].chosen, status[10].with_readings), (3, 3));

    // Changing a choice
    let mut v35 = project.selection(43, 11, 35).unwrap();
    v35.pad_before = 1.0;
    project.update_selection(v35);
    let timeline = project.chapter_timeline(&bible, 43, 11).unwrap();
    assert_eq!(timeline.len(), 3);
    assert!((timeline[1].start - timeline[0].end - 1.0).abs() < 1e-6, "{timeline:?}");

    let wav = project.render_chapter(&bible, &files, 43, 11).unwrap();
    assert_eq!(&wav[..4], b"RIFF");

    let mut metadata = project.metadata();
    metadata.artist = "Grace Church".into();
    metadata.export_mode = ExportMode::Both;
    project.set_metadata(metadata);
    let exported = project.export(&bible, &files, None).unwrap();
    assert_eq!(exported.len(), 4, "a chapter and three verses");
    assert_eq!(exported[0].path, "43 John/43_John_011.mp3");
    assert_eq!(&exported[0].bytes[..3], b"ID3");
    assert_eq!(exported[0].missing, [33], "verses 1-32 have no text here");

    // Saved and read again, with its readings
    let reopened = eab
        .read_project(project.to_json(), Some(project.readings_json()))
        .unwrap();
    assert_eq!(reopened.selections().len(), 3);
    assert_eq!(reopened.metadata().artist, "Grace Church");
    assert_eq!(reopened.verse_readings(43, 11, 35)[0].verse, 35);
}

#[test]
fn a_verse_read_in_two_pieces() {
    let eab = ExtractAudioBible::new();
    let bible = bible(&eab);
    let files = Files::memory();
    // The first half, a few minutes of preaching, then the second half
    let preaching = "and so we see the love of our lord for his friends ".repeat(30);
    let said = format!(
        "when jesus therefore saw her weeping and the jews also weeping which came with her {preaching}he groaned in the spirit and was troubled"
    );
    let words: Vec<String> = (said.split(' ').enumerate())
        .map(|(i, w)| {
            format!(
                r#"{{"word": "{w}", "start": {:.2}, "end": {:.2}}}"#,
                i as f64 * 0.4,
                i as f64 * 0.4 + 0.3
            )
        })
        .collect();
    let seconds = (words.len() as f64 * 0.4).ceil() as usize;
    let json = format!(
        r#"{{"segments": [{{"start": 0, "end": {seconds}, "text": "", "words": [{}]}}]}}"#,
        words.join(",")
    );
    files.add("sunday.json".into(), json.into_bytes());
    let samples = (0..44_100 * seconds).map(|i| (i as f32 * 0.03).sin() * 0.3).collect();
    files.add(
        "sunday.mp3".into(),
        eab::audio::mp3(&eab::audio::Pcm { rate: 44_100, samples }, 64).unwrap(),
    );
    let project = eab.new_project(&bible, String::new(), Some("John".into())).unwrap();
    project.add_sources(files.paths());
    project.search(&bible, &files).unwrap();
    let mut halves = project.verse_readings(43, 11, 33);
    assert_eq!(halves.len(), 2, "{halves:?}");
    halves.sort_by(|a, b| a.start.total_cmp(&b.start));

    // Joined in the verse's order, though the second half was chosen first
    project.select(halves[1].id.clone()).unwrap();
    let sel = project.join(halves[0].id.clone()).unwrap();
    assert_eq!(sel.reading_id, halves[0].id);
    assert_eq!(sel.joined.len(), 1);
    assert_eq!(sel.joined[0].reading_id, halves[1].id);
    let timeline = project.chapter_timeline(&bible, 43, 11).unwrap();
    let spoken = (halves[0].end - halves[0].start) + (halves[1].end - halves[1].start);
    assert!(
        (timeline[0].end - timeline[0].start - spoken).abs() < 1e-6,
        "{timeline:?}"
    );
    let wav = project.render_verse(&files, 43, 11, 33).unwrap().unwrap();
    assert_eq!(&wav[..4], b"RIFF");

    // Rearranged, then taken apart
    let sel = project
        .arrange(43, 11, 33, vec![halves[1].id.clone(), halves[0].id.clone()])
        .unwrap();
    assert_eq!(sel.reading_id, halves[1].id);
    assert!(matches!(
        project.arrange(43, 11, 33, vec![halves[1].id.clone(), halves[1].id.clone()]),
        Err(ExtractAudioBibleError::Invalid { .. })
    ));
    let left = project.unjoin(halves[1].id.clone()).unwrap().unwrap();
    assert_eq!(
        (left.reading_id.as_str(), left.joined.len()),
        (halves[0].id.as_str(), 0)
    );
    assert_eq!(project.unjoin(halves[0].id.clone()).unwrap(), None);
    assert_eq!(project.render_verse(&files, 43, 11, 33).unwrap(), None);
}

#[test]
fn settings_from_json() {
    let eab = ExtractAudioBible::new();
    let bible = bible(&eab);
    let project = eab.new_project(&bible, "Test".into(), None).unwrap();
    project
        .apply_json(r#"{"settings": {"gapBetweenVerses": 0.3}, "metadata": {"bitrate": 128}}"#.into())
        .unwrap();
    assert_eq!(project.settings().gap_between_verses, 0.3);
    assert_eq!(project.metadata().bitrate, 128);
    assert!(matches!(
        project.apply_json(r#"{"settings": {"nope": 1}}"#.into()),
        Err(ExtractAudioBibleError::Invalid { .. })
    ));
    assert!(matches!(
        project.apply_json(r#"{"settings": {"gapBetweenVerses": "x"}}"#.into()),
        Err(ExtractAudioBibleError::Invalid { .. })
    ));
    let mut settings = project.settings();
    settings.leveling = Leveling::WholeExport;
    project.set_settings(settings);
    assert_eq!(project.settings().leveling, Leveling::WholeExport);
}

#[test]
fn every_kind_of_error() {
    let eab = ExtractAudioBible::new();
    let bible = bible(&eab);
    let files = Files::memory();
    assert!(matches!(
        eab.read_transcript("nope".into()),
        Err(ExtractAudioBibleError::Invalid { .. })
    ));
    assert!(matches!(
        eab.new_project(&bible, String::new(), Some("Hezekiah".into())),
        Err(ExtractAudioBibleError::Invalid { .. })
    ));
    assert!(matches!(
        eab.new_project(&bible, String::new(), Some("Romans".into())),
        Err(ExtractAudioBibleError::NotFound { .. })
    ));
    assert!(matches!(
        eab.open_transcript(&files, "missing.json".into()),
        Err(ExtractAudioBibleError::NotFound { .. })
    ));
    files.add("noise.mp3".into(), vec![1, 2, 3, 4]);
    let clip = eab.render_clip(
        &files,
        "noise.mp3".into(),
        0.0,
        1.0,
        ClipOptions {
            pad_before: 0.0,
            pad_after: 0.0,
            processed: false,
            target_db: None,
            gain_db: 0.0,
        },
    );
    assert!(matches!(clip, Err(ExtractAudioBibleError::Audio { .. })), "{clip:?}");
    let dir = std::env::temp_dir();
    let read_dir = eab.open_transcript(&Files::disk(), dir.to_string_lossy().into_owned());
    assert!(
        matches!(read_dir, Err(ExtractAudioBibleError::Io { .. })),
        "{:?}",
        read_dir.err()
    );
    let project = eab.new_project(&bible, String::new(), None).unwrap();
    assert!(matches!(
        project.export(&bible, &files, None),
        Err(ExtractAudioBibleError::Invalid { .. })
    ));
    assert!(matches!(
        project.select("nope".into()),
        Err(ExtractAudioBibleError::NotFound { .. })
    ));
}
