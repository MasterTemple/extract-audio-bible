//! Aligning a verse's tokens with a stretch of the transcript, word by word (Smith-Waterman),
//! with near-miss words counted, and keeping the longest part that is accurate enough.

use crate::engine::Engine;

/// A transcript token not in any verse
pub(crate) const UNKNOWN: u32 = u32::MAX;

const MATCH: i32 = 3;
const NEAR: i32 = 2;
const MISMATCH: i32 = -2;
const SKIPPED: i32 = -2;
const ADDED: i32 = -1;

/// A near-miss word counts this much of a word said exactly
const NEAR_WEIGHT: f32 = 0.8;
/// A skipped or changed verse word counts as one word against accuracy, and each place the
/// speaker added words counts this much, however many: a comment mid-verse is one interruption
const INTERRUPTION: f32 = 0.5;

/// The transcript, as the aligner sees it
pub(crate) struct Prepared {
    /// Vocabulary ids, [`UNKNOWN`] for words not in any verse
    pub ids: Vec<u32>,
    /// Per token, its near misses in the vocabulary, as a range of `near_ids`
    pub near: Vec<(u32, u32)>,
    pub near_ids: Vec<u32>,
}

impl Prepared {
    fn near(&self, k: usize) -> &[u32] {
        let (start, len) = self.near[k];
        &self.near_ids[start as usize..(start + len) as usize]
    }
}

/// One step of an alignment
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Exact,
    Near,
    /// A verse word said as another word
    Changed,
    /// A verse word not said
    Skipped,
    /// A word said that isn't the verse's
    Added,
}

/// A reading of one verse text, by token positions in the transcript
#[derive(Clone, Debug)]
pub(crate) struct Reading {
    pub text: u32,
    pub tok_start: usize,
    pub tok_end: usize,
    pub score: f32,
    pub accuracy: f32,
    pub coverage: f32,
    pub precision: f32,
    /// Per token of the verse
    pub heard: Vec<bool>,
    pub heard_count: usize,
    /// The heard words' information, in bits (see [`crate::Quotation::information`])
    pub information: f32,
    /// Long enough to be a quotation on its own, not only alongside others of its chapter
    pub alone: bool,
    /// Token positions in the span not aligned to the verse
    pub extra: Vec<usize>,
}

#[derive(Default)]
pub(crate) struct Aligner {
    h: Vec<i32>,
    dir: Vec<u8>,
    sim: Vec<u8>,
    path: Vec<(Op, u32, u32)>,
    /// Per vocabulary id, the alignment it was last in the verse of, so near misses of the
    /// verse's words are found with one look each
    in_verse: Vec<u32>,
    round: u32,
}

impl Aligner {
    /**
    The best local alignment of a verse text with `transcript[from..to]`, cut down to its longest
    part at least `min_accuracy` accurate
    - `band`: the diagonals the alignment may use, as (transcript position − verse position)
      within the window, when the votes say where the verse is; the whole window otherwise
    */
    #[allow(clippy::too_many_arguments)]
    pub fn align(
        &mut self,
        engine: &Engine,
        t: &Prepared,
        text: u32,
        from: usize,
        to: usize,
        band: Option<(isize, isize)>,
        min_accuracy: f32,
        min_partial_accuracy: f32,
    ) -> Option<Reading> {
        let verse = engine.text_tokens(text);
        let (m, n) = (verse.len(), to.saturating_sub(from));
        if m == 0 || n == 0 {
            return None;
        }
        // Cells are stored by diagonal: row i (1-based), column k is window position
        // j = i + lo + k (1-based), so a band of b diagonals takes b cells a row
        let (lo, hi) = band.unwrap_or((1 - m as isize, n as isize - 1));
        let (lo, hi) = (lo.max(1 - m as isize), hi.min(n as isize - 1));
        if lo > hi {
            return None;
        }
        let b = (hi - lo + 1) as usize;
        // A column of padding each side, so neighbors out of the band read as 0
        let w = b + 2;
        let cell = |i: usize, k: isize| i * w + (k + 1) as usize;
        // The columns of row i inside the window
        let cols = |i: usize| {
            let first = (1 - i as isize - lo).max(0);
            let last = (n as isize - i as isize - lo).min(b as isize - 1);
            first..=last
        };
        self.h.clear();
        self.h.resize((m + 1) * w, 0);
        self.dir.clear();
        self.dir.resize((m + 1) * w, 0);
        self.sim.clear();
        self.sim.resize((m + 1) * w, 0);

        // How alike each verse token is to each transcript token: equal, then near misses
        let window = &t.ids[from..to];
        let mut any = false;
        for (i0, &v) in verse.iter().enumerate() {
            let i = i0 + 1;
            for k in cols(i) {
                let j0 = (i as isize + lo + k - 1) as usize;
                let same = window[j0] == v;
                self.sim[cell(i, k)] = u8::from(same) << 1;
                any |= same;
            }
        }
        if self.in_verse.len() != engine.vocab.len() {
            self.in_verse = vec![0; engine.vocab.len()];
        }
        self.round = self.round.wrapping_add(1);
        for &v in verse {
            self.in_verse[v as usize] = self.round;
        }
        for j0 in 0..n {
            for &u in t.near(from + j0) {
                if self.in_verse[u as usize] != self.round {
                    continue;
                }
                for (i0, &v) in verse.iter().enumerate() {
                    let k = j0 as isize - i0 as isize - lo;
                    if v == u && (0..b as isize).contains(&k) && self.sim[cell(i0 + 1, k)] == 0 {
                        self.sim[cell(i0 + 1, k)] = 1;
                        any = true;
                    }
                }
            }
        }
        if !any {
            return None;
        }

        // 0 stop, 1 diagonal, 2 up (skipped a verse token), 3 left (added a transcript token)
        let mut best = (0, 0, 0isize);
        for i in 1..=m {
            for k in cols(i) {
                let here = cell(i, k);
                let diag = self.h[here - w]
                    + match self.sim[here] {
                        2 => MATCH,
                        1 => NEAR,
                        _ => MISMATCH,
                    };
                let up = self.h[here - w + 1] + SKIPPED;
                let left = self.h[here - 1] + ADDED;
                let (mut val, mut d) = (0, 0);
                if diag > val || (diag == val && diag > 0) {
                    (val, d) = (diag, 1);
                }
                if up > val {
                    (val, d) = (up, 2);
                }
                if left > val {
                    (val, d) = (left, 3);
                }
                self.h[here] = val;
                self.dir[here] = d;
                if val > best.0 {
                    best = (val, i, k);
                }
            }
        }
        let (_, mut i, mut k) = best;
        if i == 0 {
            return None;
        }

        self.path.clear();
        while i > 0 && (0..b as isize).contains(&k) && self.h[cell(i, k)] > 0 {
            // Window position (0-based) of this cell
            let j0 = i as isize + lo + k - 1;
            if j0 < 0 {
                break;
            }
            let j0 = j0 as u32;
            match self.dir[cell(i, k)] {
                1 => {
                    let op = match self.sim[cell(i, k)] {
                        2 => Op::Exact,
                        1 => Op::Near,
                        _ => Op::Changed,
                    };
                    self.path.push((op, i as u32 - 1, j0));
                    i -= 1;
                }
                2 => {
                    self.path.push((Op::Skipped, i as u32 - 1, j0 + 1));
                    i -= 1;
                    k += 1;
                }
                3 => {
                    self.path.push((Op::Added, i as u32, j0));
                    k -= 1;
                }
                _ => break,
            }
        }
        self.path.reverse();

        let (mut a, mut b) = accurate_part(&self.path, min_accuracy)?;
        // Partial quotations must be more accurate: if this is one, its part that is
        let (matched, accuracy) = weigh(&self.path[a..=b]);
        if matched < crate::search::WHOLE * m as f32 && accuracy < min_partial_accuracy {
            (a, b) = accurate_part(&self.path, min_partial_accuracy)?;
        }
        let path = &self.path[a..=b];
        let mut heard = vec![false; m];
        let mut matched = 0.0f32;
        let mut verse_ops = 0usize;
        let mut interruptions = 0usize;
        let mut aligned = 0usize;
        let mut information = 0.0f32;
        let mut previous = Op::Exact;
        for &(op, vi, _) in path {
            match op {
                Op::Exact | Op::Near => {
                    heard[vi as usize] = true;
                    information += engine.vocab.bits(verse[vi as usize]);
                    matched += if op == Op::Exact { 1.0 } else { NEAR_WEIGHT };
                    aligned += 1;
                    verse_ops += 1;
                }
                Op::Changed | Op::Skipped => verse_ops += 1,
                Op::Added => interruptions += usize::from(previous != Op::Added),
            }
            previous = op;
        }
        let tok_start = from + path[0].2 as usize;
        let tok_end = from + path[path.len() - 1].2 as usize + 1;
        let span = tok_end - tok_start;
        let accuracy = matched / (verse_ops as f32 + INTERRUPTION * interruptions as f32);
        let coverage = matched / m as f32;
        let precision = aligned as f32 / span.max(1) as f32;
        let mut extra = Vec::with_capacity(span - aligned);
        let mut ops = path
            .iter()
            .filter(|(op, ..)| matches!(op, Op::Exact | Op::Near))
            .map(|p| from + p.2 as usize)
            .peekable();
        for k in tok_start..tok_end {
            if ops.peek() == Some(&k) {
                ops.next();
            } else {
                extra.push(k);
            }
        }
        Some(Reading {
            text,
            tok_start,
            tok_end,
            score: coverage * (0.75 + 0.25 * precision),
            accuracy,
            coverage,
            precision,
            heard_count: aligned,
            information,
            alone: true,
            heard,
            extra,
        })
    }
}

/// Words matched (near misses counting less) and accuracy of part of an alignment
fn weigh(path: &[(Op, u32, u32)]) -> (f32, f32) {
    let (mut matched, mut verse_ops, mut interruptions) = (0.0f32, 0usize, 0usize);
    let mut previous = Op::Exact;
    for &(op, ..) in path {
        match op {
            Op::Exact => matched += 1.0,
            Op::Near => matched += NEAR_WEIGHT,
            _ => {}
        }
        match op {
            Op::Exact | Op::Near | Op::Changed | Op::Skipped => verse_ops += 1,
            Op::Added => interruptions += usize::from(previous != Op::Added),
        }
        previous = op;
    }
    (
        matched,
        matched / (verse_ops as f32 + INTERRUPTION * interruptions as f32).max(1.0),
    )
}

/// The longest part of an alignment (by words matched) that is at least `min` accurate, from a
/// matched word to a matched word: a verse read with a few words left out in the middle is the
/// longer of its accurate halves, unless the whole of it is accurate enough
fn accurate_part(path: &[(Op, u32, u32)], min: f32) -> Option<(usize, usize)> {
    let len = path.len();
    // Prefix sums: matched weight, verse words, interruptions
    let mut sums = Vec::with_capacity(len + 1);
    sums.push((0.0f32, 0u32, 0u32));
    for (k, &(op, ..)) in path.iter().enumerate() {
        let &(w, v, a) = sums.last()?;
        sums.push(match op {
            Op::Exact => (w + 1.0, v + 1, a),
            Op::Near => (w + NEAR_WEIGHT, v + 1, a),
            Op::Changed | Op::Skipped => (w, v + 1, a),
            Op::Added => (w, v, a + u32::from(k == 0 || path[k - 1].0 != Op::Added)),
        });
    }
    let accuracy = |a: usize, b: usize| {
        let (w, v, x) = (
            sums[b + 1].0 - sums[a].0,
            sums[b + 1].1 - sums[a].1,
            sums[b + 1].2 - sums[a].2,
        );
        (w, w / (v as f32 + INTERRUPTION * x as f32).max(1.0))
    };
    let is_match = |k: usize| matches!(path[k].0, Op::Exact | Op::Near);
    let first = (0..len).find(|&k| is_match(k))?;
    let last = (0..len).rev().find(|&k| is_match(k))?;
    if accuracy(first, last).1 >= min {
        return Some((first, last));
    }
    let mut best: Option<(f32, usize, usize)> = None;
    for a in (first..=last).filter(|&k| is_match(k)) {
        for b in (a..=last).rev().filter(|&k| is_match(k)) {
            let (w, acc) = accuracy(a, b);
            if best.is_some_and(|(bw, ..)| w <= bw) {
                break;
            }
            if acc >= min {
                best = Some((w, a, b));
                break;
            }
        }
    }
    best.map(|(_, a, b)| (a, b))
}
