//! Word normalization shared by Bible text and transcripts, so both sides compare the same way.

use unicode_normalization::UnicodeNormalization;

/// Lowercased, accent-free, punctuation-free tokens of a piece of text. Numerals are spelled
/// out ("70" → "seventy"), and hyphenated or apostrophe-joined words are split or joined the way
/// a transcriber would most likely write them.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || c == '-' || c == '—' || c == '–' || c == '/') {
        let folded: String = raw
            .nfkd()
            .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
            .flat_map(char::to_lowercase)
            .filter(|c| c.is_alphanumeric())
            .collect();
        if folded.is_empty() {
            continue;
        }
        if folded.chars().all(|c| c.is_ascii_digit()) {
            match folded.parse::<u64>() {
                Ok(n) if n < 1_000_000 => out.extend(number_words(n)),
                _ => out.push(folded),
            }
        } else if let Some(n) = ordinal(&folded) {
            out.extend(n);
        } else {
            out.push(folded);
        }
    }
    out
}

/// "1st", "2nd", "3rd", "20th" → spelled ordinals
fn ordinal(word: &str) -> Option<Vec<String>> {
    let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
    let suffix = &word[digits.len()..];
    if digits.is_empty() || !matches!(suffix, "st" | "nd" | "rd" | "th") {
        return None;
    }
    let mut words = number_words(digits.parse().ok()?);
    let last = words.pop()?;
    let ord = match last.as_str() {
        "one" => "first".into(),
        "two" => "second".into(),
        "three" => "third".into(),
        "five" => "fifth".into(),
        "eight" => "eighth".into(),
        "nine" => "ninth".into(),
        "twelve" => "twelfth".into(),
        w if w.ends_with('y') => format!("{}ieth", &w[..w.len() - 1]),
        w => format!("{w}th"),
    };
    words.push(ord);
    Some(words)
}

const ONES: [&str; 20] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
    "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// A number as English words, the way Bible text writes it ("one hundred and twenty")
pub fn number_words(n: u64) -> Vec<String> {
    fn under_thousand(n: u64, out: &mut Vec<String>) {
        let (h, rest) = (n / 100, n % 100);
        if h > 0 {
            out.push(ONES[h as usize].into());
            out.push("hundred".into());
            if rest > 0 {
                out.push("and".into());
            }
        }
        if rest >= 20 {
            out.push(TENS[(rest / 10) as usize].into());
            if rest % 10 > 0 {
                out.push(ONES[(rest % 10) as usize].into());
            }
        } else if rest > 0 || h == 0 {
            out.push(ONES[rest as usize].into());
        }
    }
    let mut out = Vec::new();
    let (thousands, rest) = (n / 1000, n % 1000);
    if thousands > 0 {
        under_thousand(thousands, &mut out);
        out.push("thousand".into());
        if rest > 0 && rest < 100 {
            out.push("and".into());
        }
    }
    if rest > 0 || thousands == 0 {
        under_thousand(rest, &mut out);
    }
    out
}

/// How alike two normalized tokens are: 2 for equal, 1 for a near miss (a transcription slip
/// like "thee"/"the", "saith"/"sayeth"), 0 otherwise
pub fn token_similarity(a: &str, b: &str) -> u8 {
    if a == b {
        return 2;
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    let shorter = la.min(lb);
    if shorter < 3 {
        return 0;
    }
    let allowed = if shorter <= 4 { 1 } else { 2 };
    if la.abs_diff(lb) > allowed {
        return 0;
    }
    if levenshtein(a, b) <= allowed { 1 } else { 0 }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes() {
        assert_eq!(tokenize("For God so loved—the world,"), ["for", "god", "so", "loved", "the", "world"]);
        assert_eq!(tokenize("Éphraïm's 70"), ["ephraims", "seventy"]);
        assert_eq!(tokenize("120"), ["one", "hundred", "and", "twenty"]);
        assert_eq!(tokenize("3rd 20th"), ["third", "twentieth"]);
    }

    #[test]
    fn similar() {
        assert_eq!(token_similarity("saith", "sayeth"), 1);
        assert_eq!(token_similarity("the", "thee"), 1);
        assert_eq!(token_similarity("god", "good"), 1);
        assert_eq!(token_similarity("loved", "world"), 0);
    }
}
