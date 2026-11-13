//! Physical keyboard model: key positions, keyboard layouts, and board
//! geometry (which finger types which key).
//!
//! Three concepts, kept strictly apart:
//!
//! ```text
//! PhysKey (row, col) ──Geometry──▶ Finger
//!       ▲
//!       │ Layout: PhysKey ⇄ char
//!       ▼
//!     char  (what hint labels and the alphabet contain)
//! ```
//!
//! - [`PhysKey`] names a *physical* key position on the main block. It
//!   doesn't change when the user switches layouts.
//! - [`Layout`] maps positions to the (unshifted) characters they produce.
//!   Built-in tables cover the common alternative Latin layouts; on
//!   Windows any installed layout can also be captured at runtime (see
//!   `crate::windows::keyboard_layout`).
//! - [`Geometry`] maps positions to fingers. It's independent of the
//!   layout, so finger-aware logic stays correct on Dvorak, Colemak, a
//!   Cyrillic layout, or a firmware-remapped board.
//!
//! This module is platform-neutral and pure data, so new layouts are a
//! table, not code.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// A row of the main key block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Row {
    /// Digits row (`` ` `` `1` … `=` on QWERTY).
    Number,
    /// Top letter row (`q` … `\` on QWERTY).
    Top,
    /// Home row (`a` … `'` on QWERTY).
    Home,
    /// Bottom letter row (`z` … `/` on QWERTY).
    Bottom,
}

/// A physical key position on the main block.
///
/// `col` is the touch-typing column: `0` is the left pinky's main column
/// (`1` / `q` / `a` / `z` on QWERTY) and columns line up vertically across
/// rows, so `col 3` is always the left index finger's home column. Keys
/// outside the 10 main columns use the neighbouring indices: `-1` is the
/// `` ` `` key on the number row and the ISO-only key left of `z` on the
/// bottom row; `10`+ are the right-pinky extension keys (`-` `=` `[` `]`
/// `\` `'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PhysKey {
    /// Which row.
    pub row: Row,
    /// Touch-typing column (see type docs).
    pub col: i8,
}

impl PhysKey {
    /// Construct a key position.
    pub const fn new(row: Row, col: i8) -> Self {
        Self { row, col }
    }
}

/// A finger. Thumbs are omitted — no main-block key is typed with one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Finger {
    LeftPinky,
    LeftRing,
    LeftMiddle,
    LeftIndex,
    RightIndex,
    RightMiddle,
    RightRing,
    RightPinky,
}

/// Physical board shape. Determines which finger types each key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Geometry {
    /// Ordinary row-staggered laptop/desktop keyboard with standard
    /// touch-typing finger assignments. The diagonal `b` and `6` keys sit
    /// between the two index fingers and typists split on which hand takes
    /// them, so [`Geometry::fingers`] reports both index fingers there.
    #[default]
    Staggered,
    /// Row-staggered keyboard typed with the "angle mod": the left hand's
    /// bottom row shifts one key toward the pinky, so `z` is the ring
    /// finger, `x` the middle, and `c` `v` `b` the index (the ISO key left
    /// of `z` takes the pinky).
    AngleMod,
    /// Ortholinear or column-staggered (split/ergo) board: every column
    /// belongs to exactly one finger, the inner two columns per hand to
    /// the index fingers — `b` is strictly left index, `6` right index.
    Ortho,
}

impl Geometry {
    /// Which finger types `key` on this geometry.
    pub fn finger(self, key: PhysKey) -> Finger {
        if self == Geometry::AngleMod && key.row == Row::Bottom {
            match key.col {
                i8::MIN..=-1 => return Finger::LeftPinky,
                0 => return Finger::LeftRing,
                1 => return Finger::LeftMiddle,
                2..=4 => return Finger::LeftIndex,
                _ => {}
            }
        }
        column_finger(key.col)
    }

    /// Every finger that may type `key`. One finger for most keys; on
    /// row-staggered boards the diagonal `b` (bottom col 4) and `6` (number
    /// col 5) keys report both index fingers, so finger-aware rules treat
    /// them as clashing with either hand's index keys.
    pub fn fingers(self, key: PhysKey) -> &'static [Finger] {
        let diagonal = matches!((key.row, key.col), (Row::Bottom, 4) | (Row::Number, 5));
        if diagonal && self != Geometry::Ortho {
            return &[Finger::LeftIndex, Finger::RightIndex];
        }
        single(self.finger(key))
    }
}

fn single(f: Finger) -> &'static [Finger] {
    match f {
        Finger::LeftPinky => &[Finger::LeftPinky],
        Finger::LeftRing => &[Finger::LeftRing],
        Finger::LeftMiddle => &[Finger::LeftMiddle],
        Finger::LeftIndex => &[Finger::LeftIndex],
        Finger::RightIndex => &[Finger::RightIndex],
        Finger::RightMiddle => &[Finger::RightMiddle],
        Finger::RightRing => &[Finger::RightRing],
        Finger::RightPinky => &[Finger::RightPinky],
    }
}

fn column_finger(col: i8) -> Finger {
    match col {
        i8::MIN..=0 => Finger::LeftPinky,
        1 => Finger::LeftRing,
        2 => Finger::LeftMiddle,
        3 | 4 => Finger::LeftIndex,
        5 | 6 => Finger::RightIndex,
        7 => Finger::RightMiddle,
        8 => Finger::RightRing,
        _ => Finger::RightPinky,
    }
}

/// Which keyboard layout to assume. `Auto` follows the active OS layout;
/// the others force a built-in table (useful when the keyboard remaps in
/// firmware while the OS stays on QWERTY).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum LayoutChoice {
    #[default]
    Auto,
    Qwerty,
    Dvorak,
    Colemak,
    ColemakDh,
    Workman,
}

/// Every [`LayoutChoice`], in Settings-dropdown order.
pub const ALL_LAYOUT_CHOICES: &[LayoutChoice] = &[
    LayoutChoice::Auto,
    LayoutChoice::Qwerty,
    LayoutChoice::Dvorak,
    LayoutChoice::Colemak,
    LayoutChoice::ColemakDh,
    LayoutChoice::Workman,
];

impl LayoutChoice {
    /// Human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            LayoutChoice::Auto => "Auto (follow Windows)",
            LayoutChoice::Qwerty => "QWERTY",
            LayoutChoice::Dvorak => "Dvorak",
            LayoutChoice::Colemak => "Colemak",
            LayoutChoice::ColemakDh => "Colemak-DH (ANSI)",
            LayoutChoice::Workman => "Workman",
        }
    }
}

/// A keyboard layout: which (unshifted, lowercase) character each
/// physical key produces. Keys that produce nothing usable (dead keys,
/// unrenderable glyphs) are simply absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    keys: Vec<(PhysKey, char)>,
}

impl Layout {
    /// Build from `(position, char)` pairs. Characters are lowercased.
    /// If a position appears twice the first wins; if a character appears
    /// on two positions (e.g. `\` on both the ANSI and ISO keys) both are
    /// kept but [`Self::key_of`] returns the first.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (PhysKey, char)>) -> Self {
        let mut keys: Vec<(PhysKey, char)> = Vec::new();
        for (k, c) in pairs {
            if keys.iter().any(|(existing, _)| *existing == k) {
                continue;
            }
            keys.push((k, lower(c)));
        }
        Self { keys }
    }

    /// Build from the four rows of an ANSI board. `number` starts at the
    /// `` ` `` key (col -1); the letter rows start at col 0.
    fn from_rows(number: &str, top: &str, home: &str, bottom: &str) -> Self {
        let mut pairs = Vec::new();
        for (row, start, chars) in [
            (Row::Number, -1i8, number),
            (Row::Top, 0, top),
            (Row::Home, 0, home),
            (Row::Bottom, 0, bottom),
        ] {
            for (i, c) in chars.chars().enumerate() {
                pairs.push((PhysKey::new(row, start + i as i8), c));
            }
        }
        Self::from_pairs(pairs)
    }

    /// Shared US QWERTY table (built once). Prefer this over
    /// [`Self::qwerty`] on hot paths.
    pub fn qwerty_ref() -> &'static Layout {
        static QWERTY: OnceLock<Layout> = OnceLock::new();
        QWERTY.get_or_init(Self::qwerty)
    }

    /// US QWERTY.
    pub fn qwerty() -> Self {
        Self::from_rows(
            "`1234567890-=",
            "qwertyuiop[]\\",
            "asdfghjkl;'",
            "zxcvbnm,./",
        )
    }

    /// US Dvorak.
    pub fn dvorak() -> Self {
        Self::from_rows(
            "`1234567890[]",
            "',.pyfgcrl/=\\",
            "aoeuidhtns-",
            ";qjkxbmwvz",
        )
    }

    /// Colemak.
    pub fn colemak() -> Self {
        Self::from_rows(
            "`1234567890-=",
            "qwfpgjluy;[]\\",
            "arstdhneio'",
            "zxcvbkm,./",
        )
    }

    /// Colemak-DH, ANSI variant.
    pub fn colemak_dh() -> Self {
        Self::from_rows(
            "`1234567890-=",
            "qwfpbjluy;[]\\",
            "arstgmneio'",
            "zxcdvkh,./",
        )
    }

    /// Workman.
    pub fn workman() -> Self {
        Self::from_rows(
            "`1234567890-=",
            "qdrwbjfup;[]\\",
            "ashtgyneoi'",
            "zxmcvkl,./",
        )
    }

    /// The built-in table for `choice`, or `None` for [`LayoutChoice::Auto`].
    pub fn builtin(choice: LayoutChoice) -> Option<Self> {
        Some(match choice {
            LayoutChoice::Auto => return None,
            LayoutChoice::Qwerty => Self::qwerty(),
            LayoutChoice::Dvorak => Self::dvorak(),
            LayoutChoice::Colemak => Self::colemak(),
            LayoutChoice::ColemakDh => Self::colemak_dh(),
            LayoutChoice::Workman => Self::workman(),
        })
    }

    /// Pick the layout to use. `Auto` takes `detected` (the OS layout) and
    /// falls back to QWERTY when detection wasn't possible.
    pub fn resolve(choice: LayoutChoice, detected: Option<Layout>) -> Self {
        Self::builtin(choice)
            .or(detected)
            .unwrap_or_else(Self::qwerty)
    }

    /// Character produced by `key`, if any.
    pub fn char_at(&self, key: PhysKey) -> Option<char> {
        self.keys.iter().find(|(k, _)| *k == key).map(|(_, c)| *c)
    }

    /// Position of the key that produces `c` (case-insensitive).
    pub fn key_of(&self, c: char) -> Option<PhysKey> {
        let c = lower(c);
        self.keys.iter().find(|(_, ch)| *ch == c).map(|(k, _)| *k)
    }

    /// The characters of the 9 home-row keys `a`…`l` (QWERTY positions),
    /// for display in Settings.
    pub fn home_row(&self) -> String {
        translate_positional("asdfghjkl", self)
    }
}

/// Reinterpret `qwerty` as a list of physical positions (the keys that
/// type those characters on QWERTY) and return what `layout` types on
/// the same positions. Characters that aren't on the QWERTY table pass
/// through unchanged; positions the layout has nothing usable for keep
/// the QWERTY character unless the layout types it elsewhere, in which
/// case the position is dropped.
///
/// This is how positional presets ("home row") follow the layout: the
/// home row is `asdfghjkl` on QWERTY and `aoeuidhtn` on Dvorak.
pub fn translate_positional(qwerty: &str, layout: &Layout) -> String {
    let base = Layout::qwerty_ref();
    qwerty
        .chars()
        .filter_map(|c| match base.key_of(c) {
            None => Some(c),
            Some(k) => match layout.char_at(k) {
                Some(mapped) => Some(mapped),
                // The layout has nothing usable on this key (dead key,
                // missing glyph): fall back to the QWERTY character, which
                // the overlay accepts for this physical key. If the layout
                // already types that character on a *different* key, the
                // fallback would be ambiguous (two keys, wrong finger), so
                // drop the position instead.
                None if layout.key_of(c).is_none() => Some(c),
                None => None,
            },
        })
        .collect()
}

/// Keep each character `layout` can type; replace the rest with what the
/// layout types on the character's QWERTY position. Used for character-set
/// presets (`a-z`): on Dvorak/Colemak all letters stay, on a Cyrillic
/// layout they become the Cyrillic letters on the same keys, and on AZERTY
/// the unshifted number-row characters replace the (shifted) digits.
pub fn translate_charset(chars: &str, layout: &Layout) -> String {
    let base = Layout::qwerty_ref();
    chars
        .chars()
        .map(|c| {
            if layout.key_of(c).is_some() {
                c
            } else {
                base.key_of(c).and_then(|k| layout.char_at(k)).unwrap_or(c)
            }
        })
        .collect()
}

fn lower(c: char) -> char {
    // Single-char lowercase; multi-char expansions (rare, e.g. 'İ') keep
    // the original so a key never maps to a string.
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_builtins() -> Vec<(LayoutChoice, Layout)> {
        ALL_LAYOUT_CHOICES
            .iter()
            .filter_map(|c| Layout::builtin(*c).map(|l| (*c, l)))
            .collect()
    }

    #[test]
    fn builtins_are_bijections_over_the_same_keys() {
        let qwerty_keys: Vec<PhysKey> = Layout::qwerty().keys.iter().map(|(k, _)| *k).collect();
        for (choice, layout) in all_builtins() {
            let keys: Vec<PhysKey> = layout.keys.iter().map(|(k, _)| *k).collect();
            assert_eq!(keys, qwerty_keys, "{choice:?} covers different keys");
            let mut chars: Vec<char> = layout.keys.iter().map(|(_, c)| *c).collect();
            chars.sort_unstable();
            let n = chars.len();
            chars.dedup();
            assert_eq!(chars.len(), n, "{choice:?} has a duplicated character");
        }
    }

    #[test]
    fn home_rows_match_published_layouts() {
        assert_eq!(Layout::qwerty().home_row(), "asdfghjkl");
        assert_eq!(Layout::dvorak().home_row(), "aoeuidhtn");
        assert_eq!(Layout::colemak().home_row(), "arstdhnei");
        assert_eq!(Layout::colemak_dh().home_row(), "arstgmnei");
        assert_eq!(Layout::workman().home_row(), "ashtgyneo");
    }

    #[test]
    fn every_layout_has_all_26_letters() {
        for (choice, layout) in all_builtins() {
            for c in 'a'..='z' {
                assert!(layout.key_of(c).is_some(), "{choice:?} is missing {c}");
            }
        }
    }

    #[test]
    fn key_of_is_case_insensitive() {
        let q = Layout::qwerty();
        assert_eq!(q.key_of('A'), q.key_of('a'));
        assert_eq!(q.key_of('a'), Some(PhysKey::new(Row::Home, 0)));
    }

    #[test]
    fn duplicate_char_key_of_returns_first_position() {
        let l = Layout::from_pairs([
            (PhysKey::new(Row::Top, 12), '\\'),
            (PhysKey::new(Row::Bottom, -1), '\\'),
        ]);
        assert_eq!(l.key_of('\\'), Some(PhysKey::new(Row::Top, 12)));
        assert_eq!(l.char_at(PhysKey::new(Row::Bottom, -1)), Some('\\'));
    }

    #[test]
    fn resolve_prefers_override_then_detected_then_qwerty() {
        let detected = Layout::dvorak();
        assert_eq!(
            Layout::resolve(LayoutChoice::Colemak, Some(detected.clone())),
            Layout::colemak()
        );
        assert_eq!(
            Layout::resolve(LayoutChoice::Auto, Some(detected.clone())),
            detected
        );
        assert_eq!(Layout::resolve(LayoutChoice::Auto, None), Layout::qwerty());
    }

    #[test]
    fn translate_positional_follows_layout_and_passes_unknowns() {
        assert_eq!(translate_positional("asdf;'", &Layout::dvorak()), "aoeus-");
        // A layout missing a position (e.g. unrenderable glyph) keeps the
        // QWERTY char there.
        let sparse = Layout::from_pairs([(PhysKey::new(Row::Home, 0), 'ф')]);
        assert_eq!(translate_positional("as", &sparse), "фs");
        // Not a QWERTY character at all: passes through.
        assert_eq!(translate_positional("é", &Layout::dvorak()), "é");
    }

    #[test]
    fn translate_positional_drops_ambiguous_fallback() {
        // Greek-like: the ';' key is a dead key (absent) but ';' is typed
        // on the Q key. Falling back to ';' would make two keys type it.
        let l = Layout::from_pairs([
            (PhysKey::new(Row::Top, 0), ';'),
            (PhysKey::new(Row::Home, 0), 'α'),
        ]);
        assert_eq!(translate_positional("a;'", &l), "α'");
    }

    #[test]
    fn staggered_diagonal_keys_report_both_index_fingers() {
        let q = Layout::qwerty();
        let both = [Finger::LeftIndex, Finger::RightIndex];
        for g in [Geometry::Staggered, Geometry::AngleMod] {
            assert_eq!(g.fingers(q.key_of('b').unwrap()), &both);
            assert_eq!(g.fingers(q.key_of('6').unwrap()), &both);
            assert_eq!(g.fingers(q.key_of('v').unwrap()), &[Finger::LeftIndex]);
        }
        assert_eq!(
            Geometry::Ortho.fingers(q.key_of('b').unwrap()),
            &[Finger::LeftIndex]
        );
        assert_eq!(
            Geometry::Ortho.fingers(q.key_of('6').unwrap()),
            &[Finger::RightIndex]
        );
    }

    #[test]
    fn translate_charset_keeps_typeable_letters() {
        assert_eq!(translate_charset("abc", &Layout::dvorak()), "abc");
        let cyr = Layout::from_pairs([
            (PhysKey::new(Row::Home, 0), 'ф'),
            (PhysKey::new(Row::Home, 1), 'ы'),
        ]);
        assert_eq!(translate_charset("as", &cyr), "фы");
    }

    #[test]
    fn staggered_finger_spot_checks() {
        let g = Geometry::Staggered;
        let q = Layout::qwerty();
        let f = |c| g.finger(q.key_of(c).unwrap());
        assert_eq!(f('a'), Finger::LeftPinky);
        assert_eq!(f('z'), Finger::LeftPinky);
        assert_eq!(f('f'), Finger::LeftIndex);
        assert_eq!(f('g'), Finger::LeftIndex);
        assert_eq!(f('b'), Finger::LeftIndex);
        assert_eq!(f('h'), Finger::RightIndex);
        assert_eq!(f('k'), Finger::RightMiddle);
        assert_eq!(f('l'), Finger::RightRing);
        assert_eq!(f(';'), Finger::RightPinky);
        assert_eq!(f('\''), Finger::RightPinky);
        assert_eq!(f('1'), Finger::LeftPinky);
        assert_eq!(f('`'), Finger::LeftPinky);
    }

    #[test]
    fn angle_mod_shifts_left_bottom_row_only() {
        let g = Geometry::AngleMod;
        let q = Layout::qwerty();
        let f = |c| g.finger(q.key_of(c).unwrap());
        assert_eq!(f('z'), Finger::LeftRing);
        assert_eq!(f('x'), Finger::LeftMiddle);
        assert_eq!(f('c'), Finger::LeftIndex);
        assert_eq!(f('b'), Finger::LeftIndex);
        assert_eq!(f('a'), Finger::LeftPinky);
        assert_eq!(f('n'), Finger::RightIndex);
        assert_eq!(g.finger(PhysKey::new(Row::Bottom, -1)), Finger::LeftPinky);
    }

    #[test]
    fn finger_follows_position_not_character() {
        // On Dvorak, 'u' sits on the left-index home key.
        let d = Layout::dvorak();
        assert_eq!(
            Geometry::Staggered.finger(d.key_of('u').unwrap()),
            Finger::LeftIndex
        );
    }
}
