//! Generation of short keyboard labels (the strings shown over hints).
//!
//! Two strategies are supported:
//!
//! - [`HintStrategy::FixedLength`] (legacy): every label has the same length,
//!   chosen as the smallest L such that `alphabet.len()^L >= count`. Works
//!   well when the user is comfortable typing N keys for every selection.
//! - [`HintStrategy::ShortestFirst`] (default): allocate single-character
//!   labels first, then two-character, and so on. Mixed lengths feel
//!   natural in practice because the overlay updates in real-time as the
//!   user types — there is no ambiguity once a complete label has been
//!   matched. This is the lowest-keystroke strategy and matches what
//!   Vimium / surfingkeys do.
//!
//! The default alphabet is the home row (`asdfghjkl`) so the most common
//! single-character hints stay under the resting position of the typing
//! hand.

use serde::{Deserialize, Serialize};

use crate::keyboard::Finger;

/// Default home-row alphabet, ordered by typing comfort for QWERTY layouts.
pub const DEFAULT_ALPHABET: &str = "asdfghjkl";

/// Which hint-allocation strategy [`HintEngine::generate`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HintStrategy {
    /// Allocate the shortest possible label to each hint, in order. The
    /// first `alphabet.len()` hints get single-character labels, the next
    /// batch gets two-character labels, and so on. Single hints can
    /// appear alongside multi-character ones — the overlay's prefix-match
    /// loop disambiguates as the user types.
    #[default]
    ShortestFirst,
    /// Every label has the same length (the legacy v0.1 behaviour). The
    /// length is the smallest L such that `alphabet.len()^L >= count`.
    /// Predictable, but always pays the worst-case keystroke cost.
    FixedLength,
}

/// Which consecutive key pairs labels may contain. Typing the same key
/// twice in a row — or two keys with the same finger — means lifting and
/// re-striking with one finger, which is slower than alternating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConsecutiveKeys {
    /// Any pair is allowed (the original behaviour).
    Allow,
    /// Never the same key twice in a row (`ll`, `lla`).
    NoRepeat,
    /// Never the same key twice, nor two keys typed by the same finger
    /// (`fg`, `hj` on QWERTY). Fingers come from the keyboard layout +
    /// geometry (see [`crate::keyboard`]); characters with no known finger
    /// only get the same-key check.
    #[default]
    NoSameFinger,
}

/// Produces short label strings for a given number of targets.
#[derive(Debug, Clone)]
pub struct HintEngine {
    alphabet: Vec<char>,
    strategy: HintStrategy,
    /// Minimum length-1 hints to guarantee on every scene. See
    /// [`Self::with_min_singles`] for the trade-off.
    min_singles: usize,
    /// Consecutive-key rule. See [`Self::with_key_rule`].
    key_rule: ConsecutiveKeys,
    /// Fingers that may type each alphabet character (parallel to
    /// `alphabet`; empty = unknown).
    fingers: Vec<&'static [Finger]>,
    /// Successor tables derived from `key_rule` + `fingers`, strictest
    /// first (see [`Self::successor_tables`]). Built once in
    /// [`Self::with_key_rule`].
    succ_tables: Vec<Vec<Vec<usize>>>,
}

impl Default for HintEngine {
    fn default() -> Self {
        Self::new(DEFAULT_ALPHABET)
    }
}

impl HintEngine {
    /// Build an engine over the given alphabet using the default
    /// [`HintStrategy::ShortestFirst`] strategy. Duplicate characters in
    /// the alphabet are preserved as-is; callers are responsible for
    /// deduping if desired.
    ///
    /// # Panics
    ///
    /// Panics if `alphabet` is empty.
    pub fn new(alphabet: &str) -> Self {
        Self::with_strategy(alphabet, HintStrategy::default())
    }

    /// Build an engine with an explicit strategy. See [`HintStrategy`]
    /// for the trade-offs.
    ///
    /// # Panics
    ///
    /// Panics if `alphabet` is empty.
    pub fn with_strategy(alphabet: &str, strategy: HintStrategy) -> Self {
        assert!(!alphabet.is_empty(), "hint alphabet must not be empty");
        Self {
            alphabet: alphabet.chars().collect(),
            strategy,
            min_singles: 0,
            key_rule: ConsecutiveKeys::Allow,
            fingers: Vec::new(),
            succ_tables: Vec::new(),
        }
    }

    /// Forbid repeated (and optionally same-finger) consecutive keys in
    /// multi-character labels. `fingers` gives the fingers that may type
    /// each alphabet character, in alphabet order (usually one; two for
    /// keys typists split between hands, see
    /// [`crate::keyboard::Geometry::fingers`]). Missing or empty entries
    /// mean "unknown finger" (same-key check only). Two keys clash when
    /// their finger sets overlap.
    ///
    /// Defaults to [`ConsecutiveKeys::Allow`]. If the rule can't be met
    /// for some character — e.g. alphabet `"fg"` where both keys share a
    /// finger — generation relaxes to [`ConsecutiveKeys::NoRepeat`], then
    /// to [`ConsecutiveKeys::Allow`], rather than run out of labels.
    pub fn with_key_rule(mut self, rule: ConsecutiveKeys, fingers: Vec<&'static [Finger]>) -> Self {
        self.key_rule = rule;
        self.fingers = fingers;
        self.succ_tables = self.successor_tables();
        self
    }

    /// Return the configured consecutive-key rule.
    pub fn key_rule(&self) -> ConsecutiveKeys {
        self.key_rule
    }

    /// Override the minimum-single-hints guarantee. Defaults to `0`
    /// (math-optimal Vimium allocation).
    ///
    /// When set to `m > 0` and `count > alphabet.len()`, the allocator
    /// forces `min(m, n − 1, count)` length-1 hints from the start of
    /// the alphabet, then runs the Vimium allocator over the remaining
    /// `n − m` prefix slots for the rest of the count. Trades a small
    /// average-typing penalty for guaranteed one-key reach on the
    /// first `m` elements (which are usually the most likely targets,
    /// since the element picker sorts elements nearest the configured
    /// [`crate::HintOrder`] anchor — window center by default — first).
    ///
    /// The `n − 1` cap is enforced at allocation time so we never
    /// starve multi-character hints of prefix slots.
    pub fn with_min_singles(mut self, min_singles: usize) -> Self {
        self.min_singles = min_singles;
        self
    }

    /// Return the configured alphabet.
    pub fn alphabet(&self) -> &[char] {
        &self.alphabet
    }

    /// Return the configured strategy.
    pub fn strategy(&self) -> HintStrategy {
        self.strategy
    }

    /// Return the configured `min_singles` floor.
    pub fn min_singles(&self) -> usize {
        self.min_singles
    }

    /// Generate `count` distinct labels.
    ///
    /// Behaviour depends on [`Self::strategy`]:
    ///
    /// - [`HintStrategy::ShortestFirst`]: shortest possible labels first.
    /// - [`HintStrategy::FixedLength`]: all labels share the same length,
    ///   chosen as the smallest L such that `alphabet.len()^L >= count`.
    pub fn generate(&self, count: usize) -> Vec<String> {
        if count == 0 {
            return Vec::new();
        }
        for succ in &self.succ_tables {
            let labels = match self.strategy {
                HintStrategy::FixedLength => self.generate_fixed_constrained(count, succ),
                HintStrategy::ShortestFirst => {
                    self.generate_shortest_first_constrained(count, succ)
                }
            };
            if let Some(labels) = labels {
                return labels;
            }
        }
        match self.strategy {
            HintStrategy::FixedLength => self.generate_fixed(count),
            HintStrategy::ShortestFirst => self.generate_shortest_first(count),
        }
    }

    fn generate_fixed(&self, count: usize) -> Vec<String> {
        let n = self.alphabet.len();
        let mut len = 1usize;
        let mut cap = n;
        while cap < count {
            len += 1;
            cap = cap.saturating_mul(n);
            if cap == usize::MAX {
                break;
            }
        }
        (0..count).map(|i| self.encode(i, len)).collect()
    }

    /// Vimium-style allocation: produce `count` labels with the smallest
    /// possible average length, using at most two distinct lengths.
    ///
    /// Algorithm (closed-form, runs in O(count) time):
    ///
    /// 1. Pick the smallest `L` such that `n^L ≥ count` — this is the
    ///    longest label any element will receive.
    /// 2. Compute `short_count = (n^L − count) / (n − 1)` length-(L−1)
    ///    labels. Each "short" label costs 1 slot but would have produced
    ///    `n` length-`L` labels if expanded, so promoting one short label
    ///    nets `n − 1` fewer slots than keeping it as a prefix.
    /// 3. The remaining `long_count = count − short_count` labels are
    ///    length `L`.
    ///
    /// No-prefix-collision is preserved because:
    ///
    /// - Short labels occupy positions `[0, short_count)` in length-`L−1`
    ///   base-`n` encoding.
    /// - Long labels occupy positions `[short_count·n, short_count·n +
    ///   long_count)` in length-`L` base-`n` encoding.
    /// - Each short label `i` "reserves" length-`L` positions
    ///   `[i·n, (i+1)·n)`, all of which are below the long-label range,
    ///   so no long label starts with a short label. ✓
    ///
    /// Worked examples with alphabet `"asdfghjkl"` (n = 9):
    /// - count = 5  → `["a", "s", "d", "f", "g"]` (L=1, all length-1).
    /// - count = 10 → 8 length-1 + 2 length-2: `["a", "s", "d", "f",
    ///   "g", "h", "j", "k", "la", "ls"]`.
    /// - count = 100 → 78 length-2 + 22 length-3 (avg 2.22 keystrokes).
    ///   The previous "reserve one slot per tier" algorithm needed 13
    ///   tiers (avg 6.76) for the same input — a 3× regression in
    ///   typing cost.
    fn generate_shortest_first(&self, count: usize) -> Vec<String> {
        let n = self.alphabet.len();
        debug_assert!(n > 0);

        // For n == 1 we can never fit more than one single-character hint
        // without prefix collision, and the math below would divide by
        // zero (n - 1 == 0). Emit labels of strictly-increasing length so
        // they remain unique: "a", "aa", "aaa", … This is a degenerate
        // path — a 1-char alphabet is effectively unusable in practice —
        // but at least it never panics and never returns duplicates.
        if n == 1 {
            let ch = self.alphabet[0];
            return (1..=count)
                .map(|len| std::iter::repeat(ch).take(len).collect())
                .collect();
        }

        // When count <= n, every label is length 1 regardless of
        // min_singles — no need for the reservation logic.
        if count <= n {
            return (0..count).map(|i| self.encode(i, 1)).collect();
        }

        // Pick the *larger* of `min_singles` and what pure Vimium would
        // naturally give us. This way:
        //   - Big alphabets where Vimium already produces many singles
        //     (e.g. 13 for n=18, count=100) are not penalised by a low
        //     min_singles floor.
        //   - Small alphabets (n=9) where Vimium would give 0 singles
        //     get exactly `min_singles` forced ones.
        // Always capped at `n - 1` so we keep at least one prefix slot
        // for multi-char hints, and at `count` for trivial scenes.
        let vimium_natural_singles = vimium_natural_short_count_at_length_one(count, n);
        let forced = self
            .min_singles
            .max(vimium_natural_singles)
            .min(n - 1)
            .min(count);

        if forced == 0 {
            // Pure Vimium path: no forced singles, find minimum L and
            // split into short (length L-1) + long (length L).
            return self.vimium_subrange(count, n, 0);
        }

        // Forced-singles path: emit `forced` length-1 hints, then run
        // the Vimium allocator over a sub-alphabet whose first character
        // must come from `alphabet[forced..n]`. Conceptually the
        // remaining `count - forced` hints occupy the *upper* slice of
        // the same encoding space the no-forced path uses, so we reuse
        // [`Self::vimium_subrange`] with a `prefix_offset = forced`.
        let mut out: Vec<String> = Vec::with_capacity(count);
        for i in 0..forced {
            out.push(self.encode(i, 1));
        }
        out.extend(self.vimium_subrange(count - forced, n, forced));
        out
    }

    /// Vimium-style allocation for `remaining` hints using a sub-alphabet
    /// whose first character is constrained to
    /// `alphabet[prefix_offset..n]`.
    ///
    /// When `prefix_offset == 0` this collapses to the standard
    /// allocator. When it's non-zero, all emitted labels have length
    /// ≥ 2 (since the length-1 slots `alphabet[0..prefix_offset]` are
    /// reserved by the caller as forced singles).
    ///
    /// Returns labels in the same shortest-first order — short hints
    /// at length L-1 first, then long hints at length L.
    fn vimium_subrange(&self, remaining: usize, n: usize, prefix_offset: usize) -> Vec<String> {
        if remaining == 0 {
            return Vec::new();
        }
        debug_assert!(prefix_offset < n);
        let usable_prefixes = n - prefix_offset;

        // Find smallest L such that the sub-capacity covers `remaining`.
        // sub-capacity at length L = usable_prefixes * n^(L-1).
        // When prefix_offset == 0 this equals n^L, matching the
        // unconstrained case.
        let min_len = if prefix_offset == 0 { 1 } else { 2 };
        let mut l = min_len;
        let mut cap = if l == 1 {
            usable_prefixes
        } else {
            usable_prefixes * n
        };
        while cap < remaining {
            l += 1;
            cap = match cap.checked_mul(n) {
                Some(c) => c,
                None => break,
            };
        }

        // Demote to length L-1 only if L-1 is still ≥ min_len — we must
        // not synthesize length-1 sub-shorts when the caller has
        // reserved that slot for forced singles.
        let short_count = if l > min_len {
            (cap - remaining) / (n - 1)
        } else {
            0
        };
        let long_count = remaining - short_count;

        let mut out: Vec<String> = Vec::with_capacity(remaining);

        // Short hints at length L-1, starting at the first encoding
        // whose top digit is ≥ prefix_offset. That's
        // `prefix_offset * n^(L-2)` for L ≥ 2.
        if short_count > 0 {
            let short_offset = prefix_offset.saturating_mul(pow_usize(n, l - 2));
            for i in 0..short_count {
                out.push(self.encode(short_offset + i, l - 1));
            }
        }

        // Long hints at length L, starting at
        // `prefix_offset * n^(L-1) + short_count * n` — the +short*n
        // skips length-L positions whose length-(L-1) prefix is one of
        // the short_count labels just emitted.
        let long_offset =
            prefix_offset.saturating_mul(pow_usize(n, l - 1)) + short_count.saturating_mul(n);
        for i in 0..long_count {
            out.push(self.encode(long_offset + i, l));
        }

        debug_assert_eq!(
            out.len(),
            remaining,
            "vimium_subrange must produce exactly `remaining` labels"
        );
        out
    }

    /// Successor tables to try, strictest first: for each alphabet index,
    /// the indices that may follow it under [`Self::key_rule`] and then
    /// each weaker rule. Tables where some character has no successor are
    /// skipped. Empty means no restriction applies (use the unconstrained
    /// allocators); [`Self::generate`] also falls back to those when no
    /// table can produce enough labels.
    fn successor_tables(&self) -> Vec<Vec<Vec<usize>>> {
        let n = self.alphabet.len();
        let ladder: &[ConsecutiveKeys] = match self.key_rule {
            ConsecutiveKeys::Allow => return Vec::new(),
            ConsecutiveKeys::NoRepeat => &[ConsecutiveKeys::NoRepeat],
            ConsecutiveKeys::NoSameFinger => {
                &[ConsecutiveKeys::NoSameFinger, ConsecutiveKeys::NoRepeat]
            }
        };
        ladder
            .iter()
            .map(|&rule| {
                (0..n)
                    .map(|i| (0..n).filter(|&j| self.can_follow(rule, i, j)).collect())
                    .collect::<Vec<Vec<usize>>>()
            })
            .filter(|succ| succ.iter().all(|s| !s.is_empty()))
            .collect()
    }

    fn can_follow(&self, rule: ConsecutiveKeys, i: usize, j: usize) -> bool {
        if self.alphabet[i] == self.alphabet[j] {
            return false;
        }
        if rule == ConsecutiveKeys::NoSameFinger {
            let fingers = |k: usize| self.fingers.get(k).copied().unwrap_or(&[]);
            let (a, b) = (fingers(i), fingers(j));
            return !a.iter().any(|f| b.contains(f));
        }
        true
    }

    /// Shortest-first allocation where only paths through `succ` are
    /// valid labels. Same contract as [`Self::generate_shortest_first`]:
    /// prefix-free, shortest labels first, forced singles honoured, and
    /// at most two lengths among the multi-character labels.
    ///
    /// Because branching is uneven (each character has its own number of
    /// allowed successors), the closed-form Vimium split doesn't apply.
    /// Instead we grow the label tree level by level until the deepest
    /// level `L` holds enough labels, then "demote" length-`L−1` nodes to
    /// labels themselves: each demotion costs its `kids − 1` surplus
    /// slots. Nodes with the fewest kids are demoted first, which
    /// maximises the number of shorter labels for the available surplus.
    ///
    /// Returns `None` when the table can't produce `count` labels at all
    /// (e.g. alphabet `"fg"` under no-repeat only ever yields `fgfg…`).
    ///
    /// The single-key count is searched rather than taken from the
    /// unconstrained Vimium formula: that formula assumes `n` successors
    /// per prefix, so under the rule it can reserve one single too many
    /// and push a few labels a key longer. We try every count between the
    /// `min_singles` floor and the formula's value and keep the allocation
    /// with the fewest total keystrokes (ties favour more singles).
    fn generate_shortest_first_constrained(
        &self,
        count: usize,
        succ: &[Vec<usize>],
    ) -> Option<Vec<String>> {
        let n = self.alphabet.len();
        if count <= n {
            return Some((0..count).map(|i| self.encode(i, 1)).collect());
        }
        let cap = |f: usize| f.min(n - 1).min(count);
        let floor = cap(self.min_singles);
        let ceiling = cap(self
            .min_singles
            .max(vimium_natural_short_count_at_length_one(count, n)));
        let total_len = |labels: &[String]| labels.iter().map(|l| l.chars().count()).sum::<usize>();
        let mut best: Option<(usize, Vec<String>)> = None;
        for forced in floor..=ceiling {
            let Some(labels) = self.constrained_with_singles(count, succ, forced) else {
                continue;
            };
            let cost = total_len(&labels);
            if best.as_ref().map_or(true, |(c, _)| cost <= *c) {
                best = Some((cost, labels));
            }
        }
        best.map(|(_, labels)| labels)
    }

    /// One constrained allocation with exactly `forced` single-key labels.
    fn constrained_with_singles(
        &self,
        count: usize,
        succ: &[Vec<usize>],
        forced: usize,
    ) -> Option<Vec<String>> {
        let n = self.alphabet.len();
        let remaining = count - forced;

        let mut out: Vec<String> = (0..forced).map(|i| self.encode(i, 1)).collect();

        // `prev` = level L-1, `cur` = level L. With forced singles the
        // length-1 prefixes aren't labels themselves, so we start at L=2
        // and never demote back to length 1.
        let roots: Vec<Vec<usize>> = (forced..n).map(|i| vec![i]).collect();
        let (mut prev, mut cur) = if forced == 0 {
            (Vec::new(), roots)
        } else {
            let next = expand(&roots, succ);
            (roots, next)
        };
        let mut demotable = false;
        let mut stalled = 0;
        while cur.len() < remaining {
            let next = grow(&cur, succ, &mut stalled, n)?;
            prev = std::mem::replace(&mut cur, next);
            demotable = true;
        }

        if !demotable {
            out.extend(cur.iter().take(remaining).map(|p| self.path_to_string(p)));
            return Some(out);
        }

        let mut surplus = cur.len() - remaining;
        let mut order: Vec<usize> = (0..prev.len()).collect();
        order.sort_by_key(|&i| succ[*prev[i].last().unwrap()].len());
        let mut demoted = vec![false; prev.len()];
        for i in order {
            let cost = succ[*prev[i].last().unwrap()].len() - 1;
            if cost <= surplus {
                demoted[i] = true;
                surplus -= cost;
            }
        }

        let shorts = prev
            .iter()
            .zip(&demoted)
            .filter(|(_, d)| **d)
            .map(|(p, _)| p.clone());
        let longs = prev
            .iter()
            .zip(&demoted)
            .filter(|(_, d)| !**d)
            .flat_map(|(p, _)| children(p, succ));
        out.extend(
            shorts
                .chain(longs)
                .take(remaining)
                .map(|p| self.path_to_string(&p)),
        );
        Some(out)
    }

    /// Fixed-length allocation over `succ`: the smallest `L` whose valid
    /// labels cover `count`, first `count` of them in alphabet order.
    fn generate_fixed_constrained(&self, count: usize, succ: &[Vec<usize>]) -> Option<Vec<String>> {
        let n = self.alphabet.len();
        let mut cur: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
        let mut stalled = 0;
        while cur.len() < count {
            cur = grow(&cur, succ, &mut stalled, n)?;
        }
        Some(
            cur.iter()
                .take(count)
                .map(|p| self.path_to_string(p))
                .collect(),
        )
    }

    fn path_to_string(&self, path: &[usize]) -> String {
        path.iter().map(|&i| self.alphabet[i]).collect()
    }

    fn encode(&self, mut idx: usize, len: usize) -> String {
        let n = self.alphabet.len();
        let mut chars = vec![self.alphabet[0]; len];
        for pos in (0..len).rev() {
            chars[pos] = self.alphabet[idx % n];
            idx /= n;
        }
        chars.into_iter().collect()
    }
}

/// All one-character extensions of `path` allowed by `succ`.
fn children<'a>(
    path: &'a [usize],
    succ: &'a [Vec<usize>],
) -> impl Iterator<Item = Vec<usize>> + 'a {
    let last = *path.last().expect("paths are non-empty");
    succ[last].iter().map(move |&j| {
        let mut p = path.to_vec();
        p.push(j);
        p
    })
}

/// The next tree level: every child of every node, in order.
fn expand(level: &[Vec<usize>], succ: &[Vec<usize>]) -> Vec<Vec<usize>> {
    level.iter().flat_map(|p| children(p, succ)).collect()
}

/// Expand one level, giving up (`None`) once the level has failed to grow
/// for `n` consecutive steps — every path then has a single successor
/// and longer labels can never add capacity.
fn grow(
    level: &[Vec<usize>],
    succ: &[Vec<usize>],
    stalled: &mut usize,
    n: usize,
) -> Option<Vec<Vec<usize>>> {
    let next = expand(level, succ);
    if next.len() > level.len() {
        *stalled = 0;
    } else {
        *stalled += 1;
        if *stalled > n {
            return None;
        }
    }
    Some(next)
}

/// Saturating integer power. `n.pow(0) == 1`, `n.pow(k) == n*n*...` k
/// times, capped at `usize::MAX` instead of panicking on overflow.
/// Used by [`HintEngine::vimium_subrange`] to compute prefix offsets in
/// length-`L` encoding space. Saturating semantics are safe because the
/// call sites only need a finite ceiling — any overflow means the
/// requested `count` already exceeds what the alphabet can encode in a
/// reasonable label length, and the surrounding logic short-circuits.
fn pow_usize(n: usize, k: usize) -> usize {
    let mut acc: usize = 1;
    for _ in 0..k {
        acc = acc.saturating_mul(n);
    }
    acc
}

/// How many length-1 hints pure Vimium would naturally allocate for a
/// given count + alphabet size. Returns 0 when Vimium would skip
/// length-1 entirely (i.e. the math-optimal allocation puts everything
/// at length ≥ 2). Used by `generate_shortest_first` to avoid
/// regressing big-alphabet scenes when the caller specifies a small
/// `min_singles` floor.
fn vimium_natural_short_count_at_length_one(count: usize, n: usize) -> usize {
    debug_assert!(n > 1);
    if count <= n {
        return count;
    }
    // Find smallest L such that n^L >= count.
    let mut l = 1usize;
    let mut cap = n;
    while cap < count {
        l += 1;
        cap = match cap.checked_mul(n) {
            Some(c) => c,
            None => return 0,
        };
    }
    // Length-1 hints only appear in Vimium's "short tier" when L == 2.
    // For deeper L, the short tier is at length L-1 ≥ 2, never length 1.
    if l == 2 {
        (cap - count) / (n - 1)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::{Geometry, Layout};
    use std::collections::HashSet;

    fn fingers_for(alphabet: &str, layout: &Layout, geometry: Geometry) -> Vec<&'static [Finger]> {
        alphabet
            .chars()
            .map(|c| layout.key_of(c).map_or(&[][..], |k| geometry.fingers(k)))
            .collect()
    }

    fn default_app_engine() -> HintEngine {
        HintEngine::new(DEFAULT_ALPHABET)
            .with_min_singles(8)
            .with_key_rule(
                ConsecutiveKeys::NoSameFinger,
                fingers_for(DEFAULT_ALPHABET, &Layout::qwerty(), Geometry::Staggered),
            )
    }

    fn assert_valid(labels: &[String], count: usize, ok_pair: impl Fn(char, char) -> bool) {
        assert_eq!(labels.len(), count);
        let set: HashSet<_> = labels.iter().collect();
        assert_eq!(set.len(), count, "duplicate labels at count={count}");
        for a in labels {
            for b in labels {
                if a != b {
                    assert!(!b.starts_with(a.as_str()), "{a} is a prefix of {b}");
                }
            }
            let chars: Vec<char> = a.chars().collect();
            for w in chars.windows(2) {
                assert!(ok_pair(w[0], w[1]), "bad pair in {a}");
            }
        }
    }

    #[test]
    fn no_same_finger_default_has_no_violations() {
        let e = default_app_engine();
        let q = Layout::qwerty();
        let finger = |c: char| Geometry::Staggered.finger(q.key_of(c).unwrap());
        for count in [10, 17, 18, 50, 100, 300, 500] {
            let labels = e.generate(count);
            assert_valid(&labels, count, |a, b| a != b && finger(a) != finger(b));
        }
    }

    #[test]
    fn no_same_finger_keeps_two_lengths_and_single_keys() {
        let e = default_app_engine();
        for count in [10, 17, 50, 100, 300] {
            let labels = e.generate(count);
            let singles = labels.iter().filter(|l| l.len() == 1).count();
            assert_eq!(singles, 8, "count={count}");
            let mut lens: Vec<usize> = labels
                .iter()
                .filter(|l| l.len() > 1)
                .map(|l| l.len())
                .collect();
            lens.sort_unstable();
            lens.dedup();
            assert!(lens.len() <= 2, "count={count} lengths={lens:?}");
        }
    }

    #[test]
    fn no_same_finger_shortest_labels_come_first() {
        let labels = default_app_engine().generate(100);
        let lens: Vec<usize> = labels.iter().map(|l| l.len()).collect();
        let mut sorted = lens.clone();
        sorted.sort_unstable();
        assert_eq!(lens, sorted);
    }

    #[test]
    fn no_same_finger_default_examples() {
        // 8 singles a..k; 'l' (right ring) can't be followed by 'l', and
        // nothing else on the home row shares its finger.
        let labels = default_app_engine().generate(16);
        assert_eq!(
            labels,
            vec![
                "a", "s", "d", "f", "g", "h", "j", "k", "la", "ls", "ld", "lf", "lg", "lh", "lj",
                "lk"
            ]
        );
        assert!(!default_app_engine()
            .generate(300)
            .iter()
            .any(|l| l.contains("fg")
                || l.contains("gf")
                || l.contains("hj")
                || l.contains("jh")
                || l.contains("ll")));
    }

    #[test]
    fn no_repeat_allows_same_finger_but_not_doubles() {
        let e = HintEngine::new(DEFAULT_ALPHABET).with_key_rule(ConsecutiveKeys::NoRepeat, vec![]);
        let labels = e.generate(80);
        assert_valid(&labels, 80, |a, b| a != b);
        assert!(labels.iter().any(|l| l.contains("fg")));
    }

    #[test]
    fn allow_rule_matches_legacy_output() {
        let legacy = HintEngine::new(DEFAULT_ALPHABET).with_min_singles(8);
        let allow = HintEngine::new(DEFAULT_ALPHABET)
            .with_min_singles(8)
            .with_key_rule(ConsecutiveKeys::Allow, vec![]);
        for count in [5, 10, 17, 50, 300] {
            assert_eq!(legacy.generate(count), allow.generate(count));
        }
        assert!(legacy.generate(17).contains(&"ll".to_string()));
    }

    #[test]
    fn unsatisfiable_finger_rule_relaxes_instead_of_failing() {
        // 'f', 'g', 'r' are all left-index keys: under NoSameFinger none
        // has a successor, so we relax to NoRepeat.
        let q = Layout::qwerty();
        let e = HintEngine::new("fgr").with_key_rule(
            ConsecutiveKeys::NoSameFinger,
            fingers_for("fgr", &q, Geometry::Staggered),
        );
        let labels = e.generate(7);
        assert_valid(&labels, 7, |a, b| a != b);
        // "fg" under NoRepeat only ever yields fgfg…, which can't hold 5
        // prefix-free labels: relax all the way to Allow.
        let e = HintEngine::new("fg").with_key_rule(ConsecutiveKeys::NoRepeat, vec![]);
        assert_valid(&e.generate(5), 5, |_, _| true);
        // n == 1 can't avoid repeats at all: falls back to Allow.
        let e = HintEngine::new("a").with_key_rule(ConsecutiveKeys::NoRepeat, vec![]);
        assert_eq!(e.generate(3), vec!["a", "aa", "aaa"]);
    }

    #[test]
    fn fixed_length_constrained_is_uniform_and_valid() {
        let e = HintEngine::with_strategy(DEFAULT_ALPHABET, HintStrategy::FixedLength)
            .with_key_rule(ConsecutiveKeys::NoRepeat, vec![]);
        let labels = e.generate(100);
        assert!(labels.iter().all(|l| l.len() == 3));
        assert_valid(&labels, 100, |a, b| a != b);
    }

    #[test]
    fn no_forced_singles_path_is_valid() {
        let e = HintEngine::new(DEFAULT_ALPHABET).with_key_rule(ConsecutiveKeys::NoRepeat, vec![]);
        for count in [10, 40, 72, 73, 200] {
            assert_valid(&e.generate(count), count, |a, b| a != b);
        }
    }

    #[test]
    fn singles_count_avoids_needless_three_key_labels() {
        // 18 keys, min_singles 4, 100 targets, no-repeat: the unconstrained
        // formula would force 13 singles, leaving 5 x 17 = 85 two-key slots
        // for 87 targets (3 labels of length 3). 12 singles fit all at 2.
        let e = HintEngine::new("asdfghjkl;'qwertyu")
            .with_min_singles(4)
            .with_key_rule(ConsecutiveKeys::NoRepeat, vec![]);
        let labels = e.generate(100);
        assert_valid(&labels, 100, |a, b| a != b);
        assert_eq!(labels.iter().map(|l| l.len()).max(), Some(2));
    }

    #[test]
    fn diagonal_b_clashes_with_both_index_fingers_on_staggered() {
        let alphabet = "bfj";
        let q = Layout::qwerty();
        let staggered = HintEngine::new(alphabet).with_key_rule(
            ConsecutiveKeys::NoSameFinger,
            fingers_for(alphabet, &q, Geometry::Staggered),
        );
        // Every pair clashes, so it relaxes to no-repeat (bf, bj, …).
        assert!(staggered
            .generate(5)
            .iter()
            .any(|l| ["bf", "fb", "bj", "jb"].iter().any(|p| l.contains(p))));
        let ortho = HintEngine::new(alphabet).with_key_rule(
            ConsecutiveKeys::NoSameFinger,
            fingers_for(alphabet, &q, Geometry::Ortho),
        );
        let labels = ortho.generate(5);
        assert!(!labels.iter().any(|l| l.contains("bf") || l.contains("fb")));
        assert!(labels.iter().any(|l| l.contains("bj") || l.contains("jb")));
    }

    #[test]
    fn finger_rule_follows_layout() {
        // On Dvorak the home row is aoeuidhtn; 'u' and 'i' share the left
        // index finger, so "ui" must never appear.
        let alphabet = "aoeuidhtn";
        let e = HintEngine::new(alphabet).with_min_singles(8).with_key_rule(
            ConsecutiveKeys::NoSameFinger,
            fingers_for(alphabet, &Layout::dvorak(), Geometry::Staggered),
        );
        let labels = e.generate(200);
        assert!(!labels.iter().any(|l| l.contains("ui") || l.contains("iu")));
        assert!(!labels.iter().any(|l| l.contains("dh") || l.contains("hd")));
    }

    #[test]
    fn empty_count_returns_empty() {
        let e = HintEngine::default();
        assert!(e.generate(0).is_empty());
    }

    #[test]
    fn fixed_length_single_char_labels_when_count_fits_alphabet() {
        let e = HintEngine::with_strategy("abc", HintStrategy::FixedLength);
        let labels = e.generate(3);
        assert_eq!(labels, vec!["a", "b", "c"]);
    }

    #[test]
    fn fixed_length_promotes_to_two_chars_when_count_exceeds_alphabet() {
        let e = HintEngine::with_strategy("ab", HintStrategy::FixedLength);
        let labels = e.generate(4);
        assert_eq!(labels, vec!["aa", "ab", "ba", "bb"]);
    }

    #[test]
    fn fixed_length_all_labels_unique_and_same_length() {
        let e = HintEngine::with_strategy(DEFAULT_ALPHABET, HintStrategy::FixedLength);
        let labels = e.generate(120);
        let len = labels[0].len();
        assert!(labels.iter().all(|l| l.len() == len));
        let set: HashSet<_> = labels.iter().collect();
        assert_eq!(set.len(), labels.len());
    }

    #[test]
    #[should_panic]
    fn empty_alphabet_panics() {
        let _ = HintEngine::new("");
    }

    #[test]
    fn shortest_first_uses_single_chars_when_possible() {
        let e = HintEngine::new(DEFAULT_ALPHABET);
        let labels = e.generate(5);
        assert_eq!(labels, vec!["a", "s", "d", "f", "g"]);
        assert!(labels.iter().all(|l| l.len() == 1));
    }

    #[test]
    fn shortest_first_default_strategy_is_shortest_first() {
        let e = HintEngine::default();
        assert_eq!(e.strategy(), HintStrategy::ShortestFirst);
    }

    #[test]
    fn shortest_first_uses_full_alphabet_at_count_equal_n() {
        let e = HintEngine::new("abc");
        let labels = e.generate(3);
        assert_eq!(labels, vec!["a", "b", "c"]);
    }

    #[test]
    fn shortest_first_mixes_lengths_when_overflowing_alphabet() {
        let e = HintEngine::new("abc");
        let labels = e.generate(5);
        // n=3, count=5, L=2 (n^2=9 >= 5).
        // short_count = (9 - 5) / (3 - 1) = 2 length-1 hints.
        // long_count = 3 length-2 hints starting at offset 2*3=6.
        // → ["a", "b", "ca", "cb", "cc"].
        assert_eq!(labels.len(), 5);
        assert_eq!(labels, vec!["a", "b", "ca", "cb", "cc"]);
        // Every label is unique.
        let set: HashSet<_> = labels.iter().collect();
        assert_eq!(set.len(), 5);
        // No prefix collisions: no single-character label is also the
        // prefix of any longer label.
        for (i, a) in labels.iter().enumerate() {
            for (j, b) in labels.iter().enumerate() {
                if i != j && b.len() > a.len() {
                    assert!(
                        !b.starts_with(a.as_str()),
                        "label {a:?} is a prefix of {b:?} — would create input ambiguity"
                    );
                }
            }
        }
    }

    #[test]
    fn shortest_first_no_prefix_collisions_at_scale() {
        let e = HintEngine::new(DEFAULT_ALPHABET);
        for count in [1, 5, 9, 10, 50, 81, 100, 200, 500] {
            let labels = e.generate(count);
            assert_eq!(
                labels.len(),
                count,
                "count {count} produced wrong number of labels"
            );
            let set: HashSet<_> = labels.iter().collect();
            assert_eq!(set.len(), labels.len(), "duplicate labels at count {count}");
            for (i, a) in labels.iter().enumerate() {
                for (j, b) in labels.iter().enumerate() {
                    if i != j && b.len() > a.len() {
                        assert!(
                            !b.starts_with(a.as_str()),
                            "prefix collision at count {count}: {a:?} ⊂ {b:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shortest_first_minimizes_keystrokes() {
        // For 9 elements with a 9-char alphabet, every label should be a
        // single character — any longer scheme is wasted typing.
        let e = HintEngine::new(DEFAULT_ALPHABET);
        let labels = e.generate(9);
        assert!(labels.iter().all(|l| l.len() == 1));
    }

    #[test]
    fn shortest_first_grows_to_two_chars_at_overflow() {
        // 10 elements, 9-char alphabet: must use at least one
        // length-2 label, and the average length should be < 2.
        let e = HintEngine::new(DEFAULT_ALPHABET);
        let labels = e.generate(10);
        let avg_len: f32 = labels.iter().map(|l| l.len() as f32).sum::<f32>() / labels.len() as f32;
        assert!(
            avg_len < 2.0,
            "shortest_first should keep average length below 2 for count=10, got {avg_len}"
        );
    }

    #[test]
    fn shortest_first_caps_label_length_at_log_n_count() {
        // Regression test for the v0.4.0 "one prefix per tier" allocator
        // that produced 11-character labels for 82 elements with the
        // 9-char home-row alphabet. Vimium-style allocation keeps the
        // longest label at ceil(log_n(count)) characters.
        let e = HintEngine::new(DEFAULT_ALPHABET);
        for (count, expected_max_len) in [
            (1, 1),
            (9, 1),
            (10, 2),
            (50, 2),
            (81, 2),
            (82, 3),
            (100, 3),
            (500, 3),
            (729, 3),
            (730, 4),
        ] {
            let labels = e.generate(count);
            let max_len = labels.iter().map(|l| l.len()).max().unwrap_or(0);
            assert_eq!(
                max_len, expected_max_len,
                "count={count}: expected max label length {expected_max_len}, got {max_len}"
            );
        }
    }

    #[test]
    fn shortest_first_uses_two_distinct_lengths_at_most() {
        let e = HintEngine::new(DEFAULT_ALPHABET);
        for count in [1, 5, 9, 10, 50, 81, 82, 100, 500] {
            let labels = e.generate(count);
            let mut lengths: Vec<usize> = labels.iter().map(|l| l.len()).collect();
            lengths.sort_unstable();
            lengths.dedup();
            assert!(
                lengths.len() <= 2,
                "count={count}: expected at most 2 distinct label lengths, got {lengths:?}"
            );
        }
    }

    #[test]
    fn min_singles_default_zero_preserves_optimal_allocation() {
        // Without min_singles, count=80 yields 0 length-1 + 80 length-2.
        let e = HintEngine::new(DEFAULT_ALPHABET);
        assert_eq!(e.min_singles(), 0);
        let labels = e.generate(80);
        let singles = labels.iter().filter(|l| l.len() == 1).count();
        assert_eq!(
            singles, 0,
            "optimal allocation should produce no singles at count=80"
        );
    }

    #[test]
    fn min_singles_forces_length_one_hints_when_optimal_skips_them() {
        // count=80, n=9, min_singles=4: should reserve 4 length-1
        // hints from the start of the alphabet, allocate the rest at
        // length 2-3.
        let e = HintEngine::new(DEFAULT_ALPHABET).with_min_singles(4);
        let labels = e.generate(80);
        assert_eq!(labels.len(), 80);
        // Exactly 4 length-1 labels, taken from the start of alphabet.
        let singles: Vec<&String> = labels.iter().filter(|l| l.len() == 1).collect();
        assert_eq!(singles.len(), 4);
        assert_eq!(singles[0], "a");
        assert_eq!(singles[1], "s");
        assert_eq!(singles[2], "d");
        assert_eq!(singles[3], "f");
    }

    #[test]
    fn min_singles_preserves_no_prefix_collision() {
        let e = HintEngine::new(DEFAULT_ALPHABET).with_min_singles(4);
        for count in [5, 9, 10, 50, 80, 81, 100, 300, 500] {
            let labels = e.generate(count);
            assert_eq!(labels.len(), count);
            let set: HashSet<_> = labels.iter().collect();
            assert_eq!(set.len(), labels.len(), "duplicate at count={count}");
            for (i, a) in labels.iter().enumerate() {
                for (j, b) in labels.iter().enumerate() {
                    if i != j && b.len() > a.len() {
                        assert!(
                            !b.starts_with(a.as_str()),
                            "prefix collision at count={count}: {a:?} ⊂ {b:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn min_singles_capped_at_n_minus_one() {
        // Asking for 9 forced singles with a 9-char alphabet would
        // leave zero prefix slots for multi-char hints, which is
        // unsatisfiable when count > 9. The cap silently demotes to
        // n - 1 = 8 forced singles so the rest can still be allocated.
        let e = HintEngine::new(DEFAULT_ALPHABET).with_min_singles(99);
        let labels = e.generate(50);
        let singles = labels.iter().filter(|l| l.len() == 1).count();
        assert_eq!(singles, 8, "min_singles must be capped at n - 1 = 8");
        assert_eq!(labels.len(), 50);
    }

    #[test]
    fn min_singles_inactive_when_count_fits_alphabet() {
        // count <= n: every label is length 1 anyway, min_singles is
        // a no-op (and we don't emit *more* singles than `count`).
        let e = HintEngine::new(DEFAULT_ALPHABET).with_min_singles(7);
        let labels = e.generate(3);
        assert_eq!(labels, vec!["a", "s", "d"]);
    }

    #[test]
    fn min_singles_floor_yields_to_richer_vimium_allocation() {
        // Big alphabet (n=18) with a moderate count: pure Vimium
        // already produces *many* length-1 hints — far more than a
        // small `min_singles` floor. The engine must pick the larger
        // of the two so we never *reduce* the natural single count
        // by setting a low floor.
        //
        // n=18, count=100 → L=2 (18² = 324), short = (324-100)/17 = 13
        // length-1 hints. Setting `min_singles = 4` must NOT collapse
        // that to 4; the answer must stay at 13.
        let e = HintEngine::new("asdfghjkl;'qwertyui").with_min_singles(4);
        let labels = e.generate(100);
        let singles = labels.iter().filter(|l| l.len() == 1).count();
        assert!(
            singles >= 13,
            "expected ≥ 13 singles from natural Vimium allocation, got {}",
            singles
        );
        // No-prefix invariant must still hold across the full set.
        for (i, a) in labels.iter().enumerate() {
            for (j, b) in labels.iter().enumerate() {
                if i == j {
                    continue;
                }
                assert!(
                    !b.starts_with(a.as_str()),
                    "prefix collision: {a:?} is a prefix of {b:?}"
                );
            }
        }
    }

    #[test]
    fn shortest_first_handles_single_char_alphabet() {
        // Edge case: n=1 cannot expand without prefix collisions. We
        // emit increasing-length labels so they at least stay unique.
        let e = HintEngine::new("a");
        let labels = e.generate(3);
        assert_eq!(labels, vec!["a", "aa", "aaa"]);
    }
}
