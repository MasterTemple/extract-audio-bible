//! Word normalization shared by Bible text and transcripts, so both sides compare the same way.

use unicode_normalization::UnicodeNormalization;

/// Lowercased, accent-free, punctuation-free tokens of a piece of text. Numerals are spelled
/// out ("70" → "seventy"), and hyphenated or apostrophe-joined words are split or joined the way
/// a transcriber would most likely write them.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    tokenize_into(text, &mut out);
    out
}

pub(crate) fn tokenize_into(text: &str, out: &mut Vec<String>) {
    for raw in
        text.split(|c: char| c.is_whitespace() || c == '-' || c == '—' || c == '–' || c == '/')
    {
        let folded: String = if raw.is_ascii() {
            raw.chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_lowercase())
                .collect()
        } else {
            raw.nfkd()
                .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
                .flat_map(char::to_lowercase)
                .filter(|c| c.is_alphanumeric())
                .collect()
        };
        if folded.is_empty() {
            continue;
        }
        if folded.bytes().all(|c| c.is_ascii_digit()) {
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
}

/// "1st", "2nd", "3rd", "20th" → spelled ordinals
fn ordinal(word: &str) -> Option<Vec<String>> {
    let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
    let suffix = &word[digits.len()..];
    if digits.is_empty() || !matches!(suffix, "st" | "nd" | "rd" | "th") {
        return None;
    }
    // Spelled out below a million, as numerals are (`tokenize_into`)
    let mut words = number_words(digits.parse().ok().filter(|&n| n < 1_000_000)?);
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
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
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
    const SCALES: [&str; 7] = [
        "",
        "thousand",
        "million",
        "billion",
        "trillion",
        "quadrillion",
        "quintillion",
    ];
    // Groups of three digits, the last first
    let mut groups = vec![];
    let mut left = n;
    loop {
        groups.push(left % 1000);
        left /= 1000;
        if left == 0 {
            break;
        }
    }
    let mut out = Vec::new();
    for (scale, &group) in groups.iter().enumerate().skip(1).rev() {
        if group > 0 {
            under_thousand(group, &mut out);
            out.push(SCALES[scale].into());
        }
    }
    let rest = groups[0];
    if rest > 0 && rest < 100 && !out.is_empty() {
        out.push("and".into());
    }
    if rest > 0 || out.is_empty() {
        under_thousand(rest, &mut out);
    }
    out
}

/// What a near-miss check needs to know about a token, worked out once
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Shape {
    /// Characters, capped at 255
    pub len: u8,
    /// A bit per character (hashed), so two tokens' differing characters are counted in one step
    pub chars: u64,
}

impl Shape {
    pub fn of(token: &str) -> Self {
        let mut chars = 0u64;
        let mut len = 0usize;
        for c in token.chars() {
            chars |= 1 << ((c as u32).wrapping_mul(0x9E37_79B1) >> 26);
            len += 1;
        }
        Shape {
            len: len.min(255) as u8,
            chars,
        }
    }
}

/// How alike two normalized tokens are: 2 for equal, 1 for a near miss (a transcription slip
/// like "thee"/"the", "saith"/"sayeth"), 0 otherwise
pub fn token_similarity(a: &str, b: &str) -> u8 {
    if a == b {
        return 2;
    }
    u8::from(near(a, Shape::of(a), b, Shape::of(b)))
}

/// Whether two different tokens are a near miss: one edit apart for short words, two for longer
#[inline]
pub(crate) fn near(a: &str, sa: Shape, b: &str, sb: Shape) -> bool {
    let shorter = sa.len.min(sb.len);
    if shorter < 3 {
        return false;
    }
    let allowed = if shorter <= 4 { 1 } else { 2 };
    if sa.len.abs_diff(sb.len) > allowed {
        return false;
    }
    // Each edit changes at most two characters of the sets, so more differences can't be near
    if (sa.chars ^ sb.chars).count_ones() > 2 * allowed as u32 {
        return false;
    }
    levenshtein_within(a, b, allowed as usize)
}

/// Whether the edit distance is at most `limit`
fn levenshtein_within(a: &str, b: &str, limit: usize) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return within(a.as_bytes(), b.as_bytes(), limit);
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    within(&a, &b, limit)
}

fn within<T: PartialEq>(a: &[T], b: &[T], limit: usize) -> bool {
    const MAX: usize = 64;
    if b.len() >= MAX {
        return a == b;
    }
    let mut prev = [0usize; MAX];
    let mut cur = [0usize; MAX];
    for (j, p) in prev.iter_mut().enumerate().take(b.len() + 1) {
        *p = j;
    }
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut row_min = cur[0];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
            row_min = row_min.min(cur[j + 1]);
        }
        if row_min > limit {
            return false;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] <= limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes() {
        assert_eq!(
            tokenize("For God so loved—the world,"),
            ["for", "god", "so", "loved", "the", "world"]
        );
        assert_eq!(tokenize("Éphraïm's 70"), ["ephraims", "seventy"]);
        assert_eq!(tokenize("120"), ["one", "hundred", "and", "twenty"]);
        assert_eq!(tokenize("3rd 20th"), ["third", "twentieth"]);
        // Ordinals a million and past are left as written, like numerals
        assert_eq!(tokenize("5,100,000th 5100000"), ["5100000th", "5100000"]);
    }

    #[test]
    fn numbers_of_any_size() {
        let words = |n| number_words(n).join(" ");
        assert_eq!(words(0), "zero");
        assert_eq!(words(144_000), "one hundred and forty four thousand");
        assert_eq!(words(1_005), "one thousand and five");
        assert_eq!(words(5_100_000), "five million one hundred thousand");
        assert_eq!(words(2_000_000_050), "two billion and fifty");
        assert!(!number_words(u64::MAX).is_empty());
    }

    #[test]
    fn similar() {
        assert_eq!(token_similarity("saith", "sayeth"), 1);
        assert_eq!(token_similarity("the", "thee"), 1);
        assert_eq!(token_similarity("god", "good"), 1);
        assert_eq!(token_similarity("loved", "world"), 0);
        assert_eq!(token_similarity("righteousness", "righteousnesses"), 1);
        assert_eq!(token_similarity("to", "go"), 0);
    }
}
