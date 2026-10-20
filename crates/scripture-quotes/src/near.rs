//! Which words are near misses of which, found once for the whole vocabulary.
//!
//! Two words within `k` edits of each other share a string made by deleting at most `k`
//! characters from each (symmetric deletion), so each word's deletion variants are indexed and
//! only words sharing one are compared.

use crate::text::{Shape, near};

/// The near misses of every vocabulary word, and the index to find a new word's
#[derive(Debug, Default)]
pub(crate) struct NearIndex {
    /// (hash of a deletion variant, word id), sorted
    variants: Vec<(u64, u32)>,
    /// Where each value of a hash's top 16 bits starts in `variants`
    buckets: Vec<u32>,
    /// Per word id, a range of `ids`
    lists: Vec<(u32, u32)>,
    ids: Vec<u32>,
}

/// Edits a word of this many characters may be from another and still be a near miss
/// (the shorter word decides; see [`crate::text::token_similarity`])
fn edits(len: usize) -> usize {
    match len {
        0..3 => 0,
        3..=4 => 1,
        _ => 2,
    }
}

const P: u64 = 0x100_0000_01B3;

/// A string's hash from its pieces: `hash(A + B) = hash(A) * P^len(B) + hash(B)`
struct Hasher {
    /// `pre[i]`: the hash of the first `i` characters
    pre: Vec<u64>,
    pow: Vec<u64>,
}

impl Hasher {
    fn new() -> Self {
        Hasher {
            pre: vec![],
            pow: vec![],
        }
    }

    fn load(&mut self, word: &str) -> usize {
        self.pre.clear();
        self.pre.push(0);
        for c in word.chars() {
            let last = *self.pre.last().unwrap_or(&0);
            self.pre
                .push(last.wrapping_mul(P).wrapping_add(c as u64 + 1));
        }
        let len = self.pre.len() - 1;
        while self.pow.len() <= len {
            let last = self.pow.last().map_or(1, |p| p.wrapping_mul(P));
            self.pow.push(last);
        }
        len
    }

    /// The hash of characters `x..y`
    fn range(&self, x: usize, y: usize) -> u64 {
        self.pre[y].wrapping_sub(self.pre[x].wrapping_mul(self.pow[y - x]))
    }

    /// The hash of the word with characters `skip` left out, mixed so its top bits are even
    fn without(&self, len: usize, skip: &[usize]) -> u64 {
        let mut h = 0u64;
        let mut at = 0;
        for &k in skip.iter().chain(std::iter::once(&len)) {
            h = h
                .wrapping_mul(self.pow[k - at])
                .wrapping_add(self.range(at, k));
            at = k + 1;
        }
        // splitmix64's finalizer
        let mut z = h ^ (len - skip.len()) as u64;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Hashes of a word and every string made by deleting up to `k` of its characters
fn variants(word: &str, hasher: &mut Hasher, out: &mut Vec<u64>) {
    out.clear();
    let len = hasher.load(word);
    let k = edits(len);
    if k == 0 {
        return;
    }
    out.push(hasher.without(len, &[]));
    for a in 0..len {
        out.push(hasher.without(len, &[a]));
        if k >= 2 {
            for b in a + 1..len {
                out.push(hasher.without(len, &[a, b]));
            }
        }
    }
    out.sort_unstable();
    out.dedup();
}

impl NearIndex {
    pub fn new(words: &[Box<str>], shapes: &[Shape]) -> Self {
        let mut all: Vec<(u64, u32)> = vec![];
        let mut scratch = vec![];
        let mut hasher = Hasher::new();
        for (id, word) in words.iter().enumerate() {
            variants(word, &mut hasher, &mut scratch);
            all.extend(scratch.iter().map(|&h| (h, id as u32)));
        }
        all.sort_unstable();

        // Words that share a variant, checked
        let mut pairs: Vec<(u32, u32)> = vec![];
        for group in all.chunk_by(|a, b| a.0 == b.0) {
            for (x, &(_, a)) in group.iter().enumerate() {
                for &(_, b) in &group[x + 1..] {
                    let (a, b) = (a as usize, b as usize);
                    if near(&words[a], shapes[a], &words[b], shapes[b]) {
                        pairs.push((a as u32, b as u32));
                        pairs.push((b as u32, a as u32));
                    }
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        let mut lists = vec![(0u32, 0u32); words.len()];
        for group in pairs.chunk_by(|a, b| a.0 == b.0) {
            let start = (group.as_ptr() as usize - pairs.as_ptr() as usize)
                / std::mem::size_of::<(u32, u32)>();
            lists[group[0].0 as usize] = (start as u32, group.len() as u32);
        }
        let mut buckets = vec![0u32; (1 << 16) + 1];
        for &(h, _) in &all {
            buckets[(h >> 48) as usize + 1] += 1;
        }
        for b in 1..buckets.len() {
            buckets[b] += buckets[b - 1];
        }
        NearIndex {
            variants: all,
            buckets,
            lists,
            ids: pairs.into_iter().map(|(_, b)| b).collect(),
        }
    }

    /// A vocabulary word's near misses
    pub fn of(&self, id: u32) -> &[u32] {
        let (start, len) = self.lists[id as usize];
        &self.ids[start as usize..(start + len) as usize]
    }

    /// The vocabulary words a word not in it is a near miss of
    pub fn lookup(&self, word: &str, words: &[Box<str>], shapes: &[Shape], out: &mut Vec<u32>) {
        out.clear();
        let mut scratch = vec![];
        variants(word, &mut Hasher::new(), &mut scratch);
        let shape = Shape::of(word);
        for h in scratch {
            let top = (h >> 48) as usize;
            let bucket = &self.variants[self.buckets[top] as usize..self.buckets[top + 1] as usize];
            for &(vh, id) in bucket {
                if vh == h
                    && !out.contains(&id)
                    && near(word, shape, &words[id as usize], shapes[id as usize])
                {
                    out.push(id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_near_misses() {
        let words: Vec<Box<str>> = ["the", "thee", "saith", "sayeth", "loved", "world", "he"]
            .map(Into::into)
            .into();
        let shapes: Vec<Shape> = words.iter().map(|w| Shape::of(w)).collect();
        let index = NearIndex::new(&words, &shapes);
        assert_eq!(index.of(0), [1]);
        assert_eq!(index.of(2), [3]);
        assert!(index.of(4).is_empty());
        let mut out = vec![];
        index.lookup("sayth", &words, &shapes, &mut out);
        out.sort();
        assert_eq!(out, [2, 3]);
    }
}
