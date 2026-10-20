//! `cargo run --release --example bench -- SERMON_DIR BIBLE.json [BIBLE.json…]`
//!
//! Bibles are JSON like `{"abbreviation": "KJV", "books": [{"id": 43, "chapters": [["verse 1", …], …]}]}`;
//! sermons are WhisperX JSON.

use std::time::Instant;

use scripture_quotes::{Engine, Options, Transcript, Translation, VerseRef};
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let limit: usize = std::env::var("LIMIT")
        .ok()
        .and_then(|l| l.parse().ok())
        .unwrap_or(40);
    let t = Instant::now();
    let mut translations = vec![];
    for path in &args[1..] {
        let bible: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut tr = Translation::new(bible["abbreviation"].as_str().unwrap_or(path));
        for book in bible["books"].as_array().unwrap() {
            let id = book["id"].as_u64().unwrap() as u8;
            for (c, chapter) in book["chapters"].as_array().unwrap().iter().enumerate() {
                for (v, text) in chapter.as_array().unwrap().iter().enumerate() {
                    tr.verse(
                        VerseRef::new(id, c as u8 + 1, v as u8 + 1),
                        text.as_str().unwrap(),
                    );
                }
            }
        }
        translations.push(tr);
    }
    println!("read in {:?}", t.elapsed());
    let t = Instant::now();
    let engine = Engine::new(translations);
    println!(
        "{} verses ({} texts) normalized in {:?}",
        engine.verse_count(),
        engine.text_count(),
        t.elapsed()
    );
    let t = Instant::now();
    let searcher = engine.searcher();
    println!("indexed in {:?}", t.elapsed());

    let mut files: Vec<_> = std::fs::read_dir(&args[0])
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    files.truncate(limit);
    let (transcripts, texts): (Vec<Transcript>, Vec<Vec<String>>) = files
        .iter()
        .map(|p| {
            let json: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
            let mut t = Transcript::default();
            let mut all = vec![];
            for seg in json["segments"].as_array().unwrap() {
                let words: Vec<String> = match seg["words"].as_array() {
                    Some(words) if !words.is_empty() => words
                        .iter()
                        .map(|w| w["word"].as_str().unwrap_or("").to_string())
                        .collect(),
                    _ => seg["text"]
                        .as_str()
                        .unwrap_or("")
                        .split_whitespace()
                        .map(str::to_string)
                        .collect(),
                };
                for w in &words {
                    t.push_word(w);
                }
                all.extend(words);
                t.end_segment();
            }
            (t, all)
        })
        .unzip();
    let words: usize = transcripts.iter().map(|t| t.word_count()).sum();
    let mut options = Options::default();
    if let Some(a) = std::env::var("ACCURACY").ok().and_then(|a| a.parse().ok()) {
        options.min_accuracy = a;
    }
    let t = Instant::now();
    let found: Vec<_> = transcripts
        .iter()
        .map(|tr| searcher.search(tr, &options))
        .collect();
    let elapsed = t.elapsed();
    let quotes: usize = found.iter().map(|f| f.quotations.len()).sum();
    let runs: usize = found.iter().map(|f| f.runs.len()).sum();
    println!(
        "{} transcripts, {words} words: {quotes} quotations, {runs} runs in {elapsed:?} ({:?} each)",
        transcripts.len(),
        elapsed / transcripts.len() as u32
    );
    if let Ok(path) = std::env::var("DUMP") {
        let mut out = String::new();
        for (f, file) in found.iter().zip(&files) {
            for q in &f.quotations {
                out += &format!(
                    "{}\t{}\t{:?}\t{:?}\t{:.3}\t{}.{}.{}\t{:.1}\t{}\n",
                    file.file_name().unwrap().to_string_lossy(),
                    engine.format(&q.reference.passage()),
                    q.words,
                    q.kind,
                    q.accuracy,
                    q.reference.book.0,
                    q.reference.chapter,
                    q.reference.verse,
                    q.information,
                    q.also_at
                        .iter()
                        .map(|r| engine.format(&r.passage()))
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
        }
        std::fs::write(path, out).unwrap();
    }
    if std::env::var("SHOW").is_ok() {
        for ((f, tr), words) in found.iter().zip(&files).zip(&texts) {
            println!("\n{}", tr.display());
            for q in &f.quotations {
                println!(
                    "  {:<22} {:<8} {:?} acc {:.2} cov {:.2} bits {:>5.1} words {:?}{}",
                    engine.format(&q.reference.passage()),
                    engine.translation_name(q.translation).unwrap(),
                    q.kind,
                    q.accuracy,
                    q.coverage,
                    q.information,
                    q.words,
                    if q.also_in.is_empty() {
                        String::new()
                    } else {
                        format!(" also {}", q.also_in.len())
                    }
                );
                if std::env::var("SHOW").as_deref() == Ok("text") {
                    println!("      {}", words[q.words.clone()].join(" "));
                }
            }
        }
    }
}
