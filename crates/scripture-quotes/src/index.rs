//! Word trigrams of the verses being looked for, which a transcript's trigrams vote with.

use rustc_hash::FxHashMap;

use crate::engine::{Engine, VerseRef};

/// Trigrams in more verses than this say little about which verse is read (counted by
/// reference, so translations that share a phrase count it once)
pub(crate) const MAX_POSTINGS: usize = 48;

#[derive(Clone, Debug)]
pub(crate) struct Index {
    /// Per slot, whether it's looked for
    pub in_scope: Vec<bool>,
    /// Trigram → range of `postings`
    pub trigrams: FxHashMap<u64, (u32, u32)>,
    /// (text, position of the trigram in it)
    pub postings: Vec<(u32, u16)>,
    /// Texts of two tokens (no trigram), by their tokens
    pub pairs: FxHashMap<u64, Vec<u32>>,
}

/// Three token ids as one key: packed while ids fit in 21 bits (two million distinct words),
/// hashed beyond that
#[inline]
pub(crate) fn trigram(a: u32, b: u32, c: u32) -> u64 {
    const LIMIT: u32 = 1 << 21;
    if a < LIMIT && b < LIMIT && c < LIMIT {
        (a as u64) << 42 | (b as u64) << 21 | c as u64
    } else {
        let h = (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        (h ^ (c as u64).wrapping_mul(0x1656_67B1_9E37_79F9)) | 1 << 63
    }
}

#[inline]
pub(crate) fn pair(a: u32, b: u32) -> u64 {
    (a as u64) << 32 | b as u64
}

impl Index {
    pub fn new(engine: &Engine, in_scope: Option<Vec<bool>>) -> Self {
        let in_scope = in_scope.unwrap_or_else(|| vec![true; engine.slots.len()]);
        let wanted = |text: u32| {
            engine
                .text_owners(text)
                .iter()
                .any(|&s| in_scope[s as usize])
        };

        let mut all: Vec<(u64, u32, u16)> = vec![];
        let mut pairs: FxHashMap<u64, Vec<u32>> = FxHashMap::default();
        for text in 0..engine.texts.len() as u32 {
            if !wanted(text) {
                continue;
            }
            let tokens = engine.text_tokens(text);
            if let [a, b] = tokens {
                pairs.entry(pair(*a, *b)).or_default().push(text);
            }
            for (p, w) in tokens.windows(3).enumerate() {
                all.push((
                    trigram(w[0], w[1], w[2]),
                    text,
                    p.min(u16::MAX as usize) as u16,
                ));
            }
        }
        all.sort_unstable();

        let mut trigrams = FxHashMap::default();
        trigrams.reserve(all.len() / 2);
        let mut postings = Vec::with_capacity(all.len());
        let mut refs: Vec<VerseRef> = vec![];
        for group in all.chunk_by(|a, b| a.0 == b.0) {
            if group.len() > MAX_POSTINGS && !few_references(engine, &in_scope, group, &mut refs) {
                continue;
            }
            let start = postings.len() as u32;
            postings.extend(group.iter().map(|&(_, text, p)| (text, p)));
            trigrams.insert(group[0].0, (start, group.len() as u32));
        }
        Index {
            in_scope,
            trigrams,
            postings,
            pairs,
        }
    }
}

/// Whether a trigram's texts belong to few enough verses (by reference) to be worth voting with:
/// a phrase the same in many translations is still one verse's
fn few_references(
    engine: &Engine,
    in_scope: &[bool],
    group: &[(u64, u32, u16)],
    refs: &mut Vec<VerseRef>,
) -> bool {
    refs.clear();
    for &(_, text, _) in group {
        for &slot in engine.text_owners(text) {
            if in_scope[slot as usize] {
                refs.push(engine.slots[slot as usize].reference);
            }
        }
        if refs.len() > MAX_POSTINGS * 8 {
            refs.sort_unstable();
            refs.dedup();
            if refs.len() > MAX_POSTINGS {
                return false;
            }
        }
    }
    refs.sort_unstable();
    refs.dedup();
    refs.len() <= MAX_POSTINGS
}
