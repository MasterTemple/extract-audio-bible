use scripture_quotes::{Engine, Kind, Options, Scope, Transcript, Translation, VerseRef};

const JOHN: u8 = 43;
const ROMANS: u8 = 45;

fn kjv() -> Translation {
    let mut t = Translation::new("KJV");
    t.verse(VerseRef::new(JOHN, 3, 1), "There was a man of the Pharisees, named Nicodemus, a ruler of the Jews:")
        .verse(VerseRef::new(JOHN, 3, 3), "Jesus answered and said unto him, Verily, verily, I say unto thee, Except a man be born again, he cannot see the kingdom of God.")
        .verse(VerseRef::new(JOHN, 3, 4), "Nicodemus saith unto him, How can a man be born when he is old? can he enter the second time into his mother's womb, and be born?")
        .verse(VerseRef::new(JOHN, 3, 16), "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.")
        .verse(VerseRef::new(JOHN, 11, 35), "Jesus wept.")
        .verse(VerseRef::new(JOHN, 11, 36), "Then said the Jews, Behold how he loved him!")
        .verse(VerseRef::new(ROMANS, 8, 28), "And we know that all things work together for good to them that love God, to them who are the called according to his purpose.");
    t
}

fn esv() -> Translation {
    let mut t = Translation::new("ESV");
    t.verse(VerseRef::new(JOHN, 3, 16), "For God so loved the world, that he gave his only Son, that whoever believes in him should not perish but have eternal life.")
        .verse(VerseRef::new(JOHN, 11, 35), "Jesus wept.")
        .verse(VerseRef::new(ROMANS, 8, 28), "And we know that for those who love God all things work together for good, for those who are called according to his purpose.");
    t
}

fn refs(engine: &Engine, found: &scripture_quotes::Found) -> Vec<(String, Kind)> {
    found
        .quotations
        .iter()
        .map(|q| (engine.format(&q.reference.passage()), q.kind))
        .collect()
}

#[test]
fn finds_exact_close_and_neighbors() {
    let engine = Engine::new([kjv()]);
    let t = Transcript::from_text(
        "so turn with me. Jesus answered and said unto him verily verily I say unto thee except a man be born again he cannot see the kingdom of God. \
         Nicodemus saith unto him how can a man be born when he is old can he enter the second time into his mother's womb and be born? amen. \
         later on we read jesus wept then said the jews behold how he loved him and that's the point. \
         and again except a man be born again he can't see the kingdom of God",
    );
    let found = engine.searcher().search(&t, &Options::default());
    let r = refs(&engine, &found);
    assert!(r.contains(&("John 3:3".into(), Kind::Exact)), "{r:?}");
    assert!(r.contains(&("John 3:4".into(), Kind::Exact)), "{r:?}");
    assert!(
        r.contains(&("John 11:35".into(), Kind::Exact)),
        "short verse by its neighbor: {r:?}"
    );
    assert!(r.contains(&("John 11:36".into(), Kind::Exact)), "{r:?}");
    assert!(
        r.iter().any(|q| q.0 == "John 3:3" && q.1 != Kind::Exact),
        "the partial re-reading: {r:?}"
    );
    assert_eq!(found.runs.len(), 2, "{:?}", found.runs);
    assert_eq!(engine.format(&found.runs[0].passage()), "John 3:3-4");

    let q = found
        .quotations
        .iter()
        .find(|q| q.kind == Kind::Exact)
        .unwrap();
    let text = "so turn with me. Jesus answered";
    assert_eq!(q.words.start, text.split_whitespace().count() - 2);
}

#[test]
fn accuracy_threshold() {
    let engine = Engine::new([kjv()]);
    // Six of the verse's twenty-six words changed
    let t = Transcript::from_text(
        "for god so loved the planet that he sent his only begotten child that anyone who believeth in him should never perish but have eternal life",
    );
    let strict = engine.searcher().search(&t, &Options::default());
    // Only the accurate parts are quotations at 90%
    for q in &strict.quotations {
        assert!(q.accuracy >= 0.9 && q.kind == Kind::Partial, "{q:?}");
    }
    let loose = engine.searcher().search(
        &t,
        &Options {
            min_accuracy: 0.7,
            ..Options::default()
        },
    );
    let q = &loose.quotations[0];
    assert!(q.coverage > 0.7 && q.accuracy < 0.9, "{q:?}");
    assert_eq!(q.words, 0..t.word_count());
}

#[test]
fn translations_are_told_apart() {
    let engine = Engine::new([kjv(), esv()]);
    let esv = engine.translation("esv").unwrap();
    let kjv = engine.translation("KJV").unwrap();
    let t = Transcript::from_text(
        "paul says and we know that for those who love god all things work together for good for those who are called according to his purpose. \
         and jesus says for god so loved the world that he gave his only begotten son that whosoever believeth in him should not perish but have everlasting life. \
         jesus wept.",
    );
    let found = engine.searcher().search(&t, &Options::default());
    let r = refs(&engine, &found);
    assert_eq!(
        r.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["Romans 8:28", "John 3:16", "John 11:35"]
    );
    assert_eq!(found.quotations[0].translation, esv);
    assert_eq!(found.quotations[1].translation, kjv);
    // Worded the same in both
    let wept = &found.quotations[2];
    assert_eq!(wept.also_in.len(), 1);
    assert_eq!(engine.text_count(), engine.verse_count() - 1);
}

#[test]
fn scopes() {
    let engine = Engine::new([kjv(), esv()]);
    let t = Transcript::from_text(
        "for god so loved the world that he gave his only begotten son that whosoever believeth in him should not perish but have everlasting life. \
         and we know that all things work together for good to them that love god to them who are the called according to his purpose",
    );
    let only = |scope: Scope| {
        let found = engine
            .scoped(&scope)
            .unwrap()
            .search(&t, &Options::default());
        refs(&engine, &found)
            .into_iter()
            .map(|r| r.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(only(Scope::default().passages("Rom 8")), ["Romans 8:28"]);
    assert_eq!(only(Scope::default().passages("Romans")), ["Romans 8:28"]);
    assert_eq!(
        only(Scope::default().passages("John 3:16-18")),
        ["John 3:16"]
    );
    // Read from the KJV, it's still John 3:16 in the ESV, less accurately
    assert_eq!(
        only(Scope::default().book(JOHN).translation("esv")),
        ["John 3:16"]
    );
    let esv = engine
        .scoped(&Scope::default().book(JOHN).translation("esv"))
        .unwrap()
        .search(&t, &Options::default());
    let kjv = engine
        .scoped(&Scope::default().book(JOHN).translation("kjv"))
        .unwrap()
        .search(&t, &Options::default());
    assert!(esv.quotations[0].accuracy < kjv.quotations[0].accuracy);
    assert_eq!(kjv.quotations[0].accuracy, 1.0);
    assert_eq!(
        only(Scope::default().translation("kjv")),
        ["John 3:16", "Romans 8:28"]
    );
    assert!(engine.scoped(&Scope::default().translation("NIV")).is_err());
    assert!(
        engine
            .scoped(&Scope::default().passages("nothing here"))
            .is_err()
    );
}

#[test]
fn segments_without_word_times() {
    let engine = Engine::new([kjv()]);
    let t = Transcript::from_segments([
        "Turn with me to John 11.",
        "It says Jesus wept.",
        "Then said the Jews, behold how he loved him.",
        "Amen.",
    ]);
    let found = engine.searcher().search(&t, &Options::default());
    assert_eq!(found.runs.len(), 1);
    assert_eq!(t.segment_range(&found.runs[0].words), 1..3);
}

#[test]
fn short_quotations_need_their_chapter() {
    let mut t = Translation::new("ESV");
    t.verse(VerseRef::new(62, 1, 8), "If we say we have no sin, we deceive ourselves, and the truth is not in us.")
        .verse(VerseRef::new(62, 1, 9), "If we confess our sins, he is faithful and just to forgive us our sins and to cleanse us from all unrighteousness.");
    // How rare words are is measured against the engine's text, so give it some, with common
    // words as common as in a Bible
    for i in 0..200u16 {
        let mut words = vec!["and", "the"];
        for (every, word) in [(8, "we"), (6, "have"), (3, "is"), (20, "no")] {
            if i % every == 0 {
                words.push(word);
            }
        }
        let filler = format!("{} filler{i} words{i}", words.join(" "));
        t.verse(
            VerseRef::new(1, (i / 50 + 1) as u8, (i % 50 + 1) as u8),
            filler,
        );
    }
    let engine = Engine::new([t]);
    let alone = Transcript::from_text(
        "some people would tell you we have no sin at all but the scripture disagrees",
    );
    assert!(
        engine
            .searcher()
            .search(&alone, &Options::default())
            .quotations
            .is_empty()
    );

    let with_chapter = Transcript::from_text(
        "if we confess our sins he is faithful and just to forgive us our sins and to cleanse us from all unrighteousness. \
         some people would tell you we have no sin at all but the scripture disagrees",
    );
    let found = engine.searcher().search(&with_chapter, &Options::default());
    let r = refs(&engine, &found);
    assert_eq!(
        r,
        [
            ("1 John 1:9".to_string(), Kind::Exact),
            ("1 John 1:8".to_string(), Kind::Partial)
        ]
    );
    // Function words alone are no one's, even in context
    let common = Transcript::from_text(
        "if we confess our sins he is faithful and just to forgive us our sins and to cleanse us from all unrighteousness. and the truth is",
    );
    let found = engine.searcher().search(&common, &Options::default());
    assert_eq!(found.quotations.len(), 1, "{:?}", refs(&engine, &found));
}

#[test]
fn runs_across_chapters() {
    let mut t = Translation::new("KJV");
    t.verse(VerseRef::new(JOHN, 3, 36), "He that believeth on the Son hath everlasting life: and he that believeth not the Son shall not see life; but the wrath of God abideth on him.")
        .verse(VerseRef::new(JOHN, 4, 1), "When therefore the Lord knew how the Pharisees had heard that Jesus made and baptized more disciples than John,");
    let engine = Engine::new([t]);
    let found = engine.searcher().search(
        &Transcript::from_text(
            "he that believeth on the son hath everlasting life and he that believeth not the son shall not see life but the wrath of god abideth on him. \
             when therefore the lord knew how the pharisees had heard that jesus made and baptized more disciples than john",
        ),
        &Options::default(),
    );
    assert_eq!(found.runs.len(), 1);
    assert_eq!(engine.format(&found.runs[0].passage()), "John 3:36-4:1");
}

#[test]
fn a_comment_mid_verse_is_one_interruption() {
    let mut t = Translation::new("NASB");
    t.verse(VerseRef::new(52, 2, 7), "But we proved to be gentle among you, as a nursing mother tenderly cares for her own children.");
    let engine = Engine::new([t]);
    let said = Transcript::from_text(
        "but we prove to be gentle among you. same phrase in the greek. as a nursing mother tenderly cares for her own children",
    );
    let found = engine.searcher().search(&said, &Options::default());
    let q = &found.quotations[0];
    assert_eq!(q.words, 0..said.word_count());
    assert!(q.accuracy > 0.9 && q.kind == Kind::Close, "{q:?}");
    assert_eq!(q.extra_words, [8, 9, 10, 11, 12]);
    // Words scattered through it are each an interruption
    let scattered = Transcript::from_text(
        "but we um proved uh to be like gentle you know among you as a uh nursing mother",
    );
    let found = engine.searcher().search(&scattered, &Options::default());
    assert!(
        found.quotations.iter().all(|q| q.words.len() < 8),
        "{:?}",
        found.quotations
    );
}
