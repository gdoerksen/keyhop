//! Alphabet preset templates and a builder that resolves a [`HintConfig`]
//! into the final character set used by [`crate::HintEngine`].
//!
//! The Settings dialog exposes a small dropdown of named presets
//! ([`AlphabetPreset`]) plus modifier checkboxes (numbers, extended,
//! exclude-ambiguous) and a free-form custom-additions field. This
//! module is the single source of truth for translating those choices
//! into a concrete alphabet string.
//!
//! Design goals:
//!
//! - **Deterministic**: same config in → same alphabet out, no
//!   surprises for users who script `config.toml` by hand.
//! - **Idempotent**: applying a preset is a pure function — no global
//!   state, easy to unit-test.
//! - **Comfort order**: [`crate::HintEngine`] treats alphabet order as
//!   preference — single-key hints come from the front, prefixes of
//!   longer hints from the back. Presets are therefore plain *sets* of
//!   keys (`asdfghjkl`, `a-z`), and every built-in alphabet is sorted by
//!   [`comfort_order`]: strongest finger first (index, middle, ring,
//!   pinky, then sideways stretches), home row before the top, bottom
//!   and number rows. On QWERTY the home row comes out `fjdkslagh`.
//!   The ordering lives in one place, so it follows the layout and the
//!   board geometry automatically.
//! - **No silent dedupe**: if the user types `aa` into "custom
//!   additions" they probably want one `a`, not a panic. We dedupe
//!   while preserving first-occurrence order.

use crate::config::{AlphabetPreset, HintConfig};
use crate::keyboard::{comfort_order, translate_charset, translate_positional, Geometry, Layout};

/// Characters we never want to ship in the same alphabet because they
/// look near-identical in the overlay font and the user can't tell
/// which one to type. Stripped when [`HintConfig::exclude_ambiguous`]
/// is on (the default).
///
/// Kept conservative on purpose: with the Consolas overlay font (added
/// in v0.4.0) `I` vs `l` is already crisp, so we only strip the truly
/// indistinguishable pair `O` / `0`. Users who want a wider strip can
/// add characters via `exclude_ambiguous` + `custom_additions` (clear
/// then re-add) — but the default needs to *not* nuke `l` from the
/// home-row preset, which would silently shrink the most common
/// alphabet from 9 chars to 8.
pub const AMBIGUOUS_CHARS: &[char] = &['O', '0'];

/// The nine home-row keys (QWERTY positions, written left to right).
/// Base of [`AlphabetPreset::HomeRow`]; the builder reorders it by
/// finger comfort (`fjdkslagh` on QWERTY).
pub const HOME_ROW: &str = "asdfghjkl";

/// Right-pinky extension keys appended for [`AlphabetPreset::HomeRowExtended`].
pub const HOME_ROW_EXT: &str = ";'";

/// Top-row digits.
pub const NUMBERS: &str = "0123456789";

/// Lowercase a-z.
pub const LOWERCASE_ALPHA: &str = "abcdefghijklmnopqrstuvwxyz";

/// Build the final alphabet string from a [`HintConfig`], assuming a
/// QWERTY layout on a standard staggered board. See [`build_alphabet_for`].
pub fn build_alphabet(cfg: &HintConfig) -> String {
    build_alphabet_for(cfg, &Layout::qwerty(), Geometry::Staggered)
}

/// Build the final alphabet string from a [`HintConfig`] for `layout`
/// typed on a `geometry` board.
///
/// Resolution order:
/// 1. Pick the base set from `cfg.preset`. For [`AlphabetPreset::Custom`]
///    we use `cfg.alphabet` verbatim and skip every modifier — power
///    users opting into Custom get exactly what they typed.
/// 2. Apply the `include_numbers` / `include_extended` flags by
///    appending the corresponding constant.
/// 3. Translate through the layout. Positional sets (home row, the `; '`
///    extension, the number row) are QWERTY *key positions*, so they turn
///    into whatever the layout types there (`aoeuidhtn` on Dvorak).
///    Character sets (`a-z`) keep every character the layout can type and
///    replace the rest with the layout's character on the same key (see
///    [`translate_charset`]).
/// 4. Sort everything so far by [`comfort_order`] for `layout` and
///    `geometry`, so the easiest keys get the single-key hints.
/// 5. Append `cfg.custom_additions` verbatim, in the order typed, so users
///    can plug in extra keys (national characters, etc.) without giving
///    up the preset.
/// 6. If `cfg.exclude_ambiguous` is on, strip [`AMBIGUOUS_CHARS`].
/// 7. Always dedupe while preserving first-occurrence order so the
///    alphabet doesn't quietly waste hint slots on duplicates.
///
/// Returns at minimum the home-row default if every step somehow
/// produced an empty string — [`crate::HintEngine`] panics on an empty
/// alphabet and we'd rather fall back than crash.
pub fn build_alphabet_for(cfg: &HintConfig, layout: &Layout, geometry: Geometry) -> String {
    if matches!(cfg.preset, AlphabetPreset::Custom) {
        // Custom mode bypasses the modifier flags entirely.
        let result = dedupe_preserve_order(&cfg.alphabet);
        return if result.is_empty() {
            crate::hint::DEFAULT_ALPHABET.to_string()
        } else {
            result
        };
    }

    let mut buf = String::new();
    match cfg.preset {
        AlphabetPreset::LowercaseAlpha | AlphabetPreset::Alphanumeric => {
            buf.push_str(&translate_charset(base_for_preset(cfg.preset), layout));
        }
        _ => buf.push_str(&translate_positional(base_for_preset(cfg.preset), layout)),
    }

    if cfg.include_numbers && !cfg.preset_already_has_numbers() {
        buf.push_str(&translate_positional(NUMBERS, layout));
    }
    if cfg.include_extended && !matches!(cfg.preset, AlphabetPreset::HomeRowExtended) {
        buf.push_str(&translate_positional(HOME_ROW_EXT, layout));
    }
    let mut buf = comfort_order(&buf, layout, geometry);
    if !cfg.custom_additions.is_empty() {
        buf.push_str(&cfg.custom_additions);
    }

    if cfg.exclude_ambiguous {
        buf.retain(|c| !AMBIGUOUS_CHARS.contains(&c));
    }

    let result = dedupe_preserve_order(&buf);
    if result.is_empty() {
        // Defensive: every char was stripped. Prevent an empty alphabet
        // from reaching `HintEngine::new` (which would panic).
        crate::hint::DEFAULT_ALPHABET.to_string()
    } else {
        result
    }
}

/// Base character set for a built-in preset, written as QWERTY
/// characters. `Custom` is intentionally not handled here — call sites
/// short-circuit before reaching this function.
fn base_for_preset(p: AlphabetPreset) -> &'static str {
    match p {
        AlphabetPreset::HomeRow => HOME_ROW,
        AlphabetPreset::HomeRowExtended => "asdfghjkl;'",
        AlphabetPreset::LowercaseAlpha => LOWERCASE_ALPHA,
        AlphabetPreset::Alphanumeric => "abcdefghijklmnopqrstuvwxyz0123456789",
        AlphabetPreset::Numbers => NUMBERS,
        // Custom is handled before this fn is called.
        AlphabetPreset::Custom => HOME_ROW,
    }
}

impl HintConfig {
    /// True when the configured preset already includes the digits 0-9.
    /// Used by [`build_alphabet`] to avoid double-appending the numbers
    /// when the user toggles `include_numbers` on top of `Alphanumeric`.
    fn preset_already_has_numbers(&self) -> bool {
        matches!(
            self.preset,
            AlphabetPreset::Alphanumeric | AlphabetPreset::Numbers
        )
    }
}

/// Remove duplicate characters while preserving the first occurrence's
/// position. Important: HintEngine assigns single-letter hints in
/// alphabet order, so reordering here would undo [`comfort_order`].
fn dedupe_preserve_order(s: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if seen.insert(ch) {
            out.push(ch);
        }
    }
    out
}

/// Human-readable label for use in the Settings dropdown. Kept in this
/// module so the UI doesn't have to duplicate the enum-to-string mapping.
pub fn preset_label(p: AlphabetPreset) -> &'static str {
    match p {
        AlphabetPreset::HomeRow => "Home row (asdfghjkl)",
        AlphabetPreset::HomeRowExtended => "Home row + ; '",
        AlphabetPreset::LowercaseAlpha => "Lowercase a-z",
        AlphabetPreset::Alphanumeric => "Alphanumeric a-z 0-9",
        AlphabetPreset::Numbers => "Numbers 0-9",
        AlphabetPreset::Custom => "Custom (use Alphabet field)",
    }
}

/// Like [`preset_label`], but shows the characters the positional
/// presets produce on `layout` (e.g. "Home row (aoeuidhtn)" on Dvorak).
pub fn preset_label_for(p: AlphabetPreset, layout: &Layout) -> String {
    match p {
        AlphabetPreset::HomeRow => {
            format!("Home row ({})", translate_positional(HOME_ROW, layout))
        }
        AlphabetPreset::HomeRowExtended => {
            let ext: Vec<String> = translate_positional(HOME_ROW_EXT, layout)
                .chars()
                .map(String::from)
                .collect();
            format!("Home row + {}", ext.join(" "))
        }
        AlphabetPreset::Numbers => {
            let row = translate_positional("1234567890", layout);
            if row == "1234567890" {
                "Numbers 0-9".to_string()
            } else {
                format!("Number row ({row})")
            }
        }
        _ => preset_label(p).to_string(),
    }
}

/// Every preset, in dropdown order. Stable so the Settings dialog can
/// translate selected-index to enum without a separate mapping.
pub const ALL_PRESETS: &[AlphabetPreset] = &[
    AlphabetPreset::HomeRow,
    AlphabetPreset::HomeRowExtended,
    AlphabetPreset::LowercaseAlpha,
    AlphabetPreset::Alphanumeric,
    AlphabetPreset::Numbers,
    AlphabetPreset::Custom,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> HintConfig {
        HintConfig::default()
    }

    #[test]
    fn default_config_yields_home_row() {
        let alphabet = build_alphabet(&cfg());
        assert_eq!(alphabet, "fjdkslagh");
    }

    #[test]
    fn default_alphabet_constant_matches_default_config() {
        assert_eq!(build_alphabet(&cfg()), crate::hint::DEFAULT_ALPHABET);
    }

    #[test]
    fn lowercase_alpha_is_finger_ordered_on_qwerty() {
        let mut c = cfg();
        c.preset = AlphabetPreset::LowercaseAlpha;
        assert_eq!(build_alphabet(&c), "fjdkslaghrueiwoqptyvmcxzbn");
    }

    #[test]
    fn alphanumeric_puts_ordered_digits_after_letters() {
        let mut c = cfg();
        c.preset = AlphabetPreset::Alphanumeric;
        // '0' is stripped by the default ambiguous filter.
        assert_eq!(build_alphabet(&c), "fjdkslaghrueiwoqptyvmcxzbn473829156");
    }

    #[test]
    fn lowercase_alpha_on_dvorak_is_all_letters_in_dvorak_order() {
        let mut c = cfg();
        c.preset = AlphabetPreset::LowercaseAlpha;
        let alphabet = build_alphabet_for(&c, &Layout::dvorak(), Geometry::Staggered);
        assert!(alphabet.starts_with("uhetonasid"), "{alphabet}");
        let mut chars: Vec<char> = alphabet.chars().collect();
        chars.sort_unstable();
        assert_eq!(chars, ('a'..='z').collect::<Vec<_>>());
    }

    #[test]
    fn angle_mod_reorders_bottom_row_letters() {
        let mut c = cfg();
        c.preset = AlphabetPreset::LowercaseAlpha;
        let alphabet = build_alphabet_for(&c, &Layout::qwerty(), Geometry::AngleMod);
        assert!(alphabet.ends_with("cmxzvnb"), "{alphabet}");
    }

    #[test]
    fn include_extended_is_sorted_into_the_home_row() {
        let mut c = cfg();
        c.include_extended = true;
        assert_eq!(build_alphabet(&c), "fjdksla;gh'");
    }

    #[test]
    fn every_builtin_alphabet_is_in_comfort_order() {
        use crate::keyboard::{ALL_GEOMETRIES, ALL_LAYOUT_CHOICES};
        for choice in ALL_LAYOUT_CHOICES {
            let Some(layout) = Layout::builtin(*choice) else {
                continue;
            };
            for &geometry in ALL_GEOMETRIES {
                for &preset in ALL_PRESETS {
                    if preset == AlphabetPreset::Custom {
                        continue;
                    }
                    for (numbers, extended) in [(false, false), (true, true)] {
                        let mut c = cfg();
                        c.preset = preset;
                        c.include_numbers = numbers;
                        c.include_extended = extended;
                        let alphabet = build_alphabet_for(&c, &layout, geometry);
                        let ranks: Vec<_> = alphabet
                            .chars()
                            .map(|ch| geometry.comfort_rank(layout.typing_key(ch).unwrap()))
                            .collect();
                        assert!(
                            ranks.windows(2).all(|w| w[0] <= w[1]),
                            "{choice:?} {geometry:?} {preset:?}: {alphabet}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn lowercase_alpha_preset_excludes_ambiguous_by_default() {
        let mut c = cfg();
        c.preset = AlphabetPreset::LowercaseAlpha;
        let alphabet = build_alphabet(&c);
        // 'O' / '0' are stripped by the default ambiguous list. 'l' is
        // *not* stripped — Consolas keeps it visually distinct from
        // 'I' / '1', and dropping it would silently break the home-row
        // preset.
        assert!(alphabet.contains('a'));
        assert!(alphabet.contains('l'));
        assert!(alphabet.contains('z'));
    }

    #[test]
    fn lowercase_alpha_preset_keeps_o_when_disabled() {
        let mut c = cfg();
        c.preset = AlphabetPreset::LowercaseAlpha;
        c.exclude_ambiguous = false;
        let alphabet = build_alphabet(&c);
        // 'o' is uppercase 'O' here is the actual ambiguous one; lowercase
        // o is fine. Confirm that disabling the strip doesn't break the
        // base set either way.
        assert!(alphabet.contains('o'));
    }

    #[test]
    fn include_numbers_appends_digits() {
        let mut c = cfg();
        c.include_numbers = true;
        let alphabet = build_alphabet(&c);
        // Default exclude_ambiguous strips only '0'. '1' is intentionally
        // kept because the Consolas overlay font draws it distinctly
        // from lowercase 'l'.
        assert!(alphabet.contains('1'));
        assert!(alphabet.contains('2'));
        assert!(alphabet.contains('9'));
        assert!(!alphabet.contains('0'));
    }

    #[test]
    fn alphanumeric_preset_doesnt_double_append_numbers() {
        let mut c = cfg();
        c.preset = AlphabetPreset::Alphanumeric;
        c.include_numbers = true;
        let alphabet = build_alphabet(&c);
        // Each digit (that survives the ambiguous filter) appears once.
        for d in "123456789".chars() {
            assert_eq!(
                alphabet.matches(d).count(),
                1,
                "digit {d} should appear exactly once in {alphabet:?}"
            );
        }
    }

    #[test]
    fn custom_additions_are_appended() {
        let mut c = cfg();
        c.custom_additions = "zé".to_string();
        let alphabet = build_alphabet(&c);
        // Built-in keys are comfort-ordered; additions follow as typed.
        assert_eq!(alphabet, "fjdkslaghzé");
    }

    #[test]
    fn custom_preset_uses_alphabet_verbatim() {
        let mut c = cfg();
        c.preset = AlphabetPreset::Custom;
        c.alphabet = "qwerty".to_string();
        c.include_numbers = true; // ignored in custom mode
        let alphabet = build_alphabet(&c);
        assert_eq!(alphabet, "qwerty");
    }

    #[test]
    fn empty_result_falls_back_to_default() {
        // Alphabet entirely composed of ambiguous chars + exclude on
        // would otherwise produce empty string.
        let mut c = cfg();
        c.preset = AlphabetPreset::Custom;
        c.alphabet = "".to_string();
        let alphabet = build_alphabet(&c);
        assert_eq!(alphabet, crate::hint::DEFAULT_ALPHABET);
    }

    #[test]
    fn dedupe_preserves_first_occurrence_order() {
        assert_eq!(dedupe_preserve_order("aabbcc"), "abc");
        assert_eq!(dedupe_preserve_order("baca"), "bac");
    }

    #[test]
    fn home_row_extended_preset_includes_extras() {
        let mut c = cfg();
        c.preset = AlphabetPreset::HomeRowExtended;
        let alphabet = build_alphabet(&c);
        assert_eq!(alphabet, "fjdksla;gh'");
    }

    #[test]
    fn home_row_follows_layout() {
        let g = Geometry::Staggered;
        let alphabet = build_alphabet_for(&cfg(), &Layout::dvorak(), g);
        assert_eq!(alphabet, "uhetonaid");
        let alphabet = build_alphabet_for(&cfg(), &Layout::colemak(), g);
        assert_eq!(alphabet, "tnseriadh");
    }

    #[test]
    fn home_row_extended_follows_layout() {
        let mut c = cfg();
        c.preset = AlphabetPreset::HomeRowExtended;
        assert_eq!(
            build_alphabet_for(&c, &Layout::dvorak(), Geometry::Staggered),
            "uhetonasid-"
        );
    }

    #[test]
    fn lowercase_alpha_on_alt_latin_layout_keeps_the_same_letters() {
        let mut c = cfg();
        c.preset = AlphabetPreset::LowercaseAlpha;
        let sorted = |l: &Layout| {
            let mut v: Vec<char> = build_alphabet_for(&c, l, Geometry::Staggered)
                .chars()
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(sorted(&Layout::dvorak()), sorted(&Layout::qwerty()));
    }

    #[test]
    fn non_latin_layout_maps_letters_by_position() {
        use crate::keyboard::{PhysKey, Row};
        let cyr = Layout::from_pairs(
            "фывапролд"
                .chars()
                .enumerate()
                .map(|(i, ch)| (PhysKey::new(Row::Home, i as i8), ch)),
        );
        // QWERTY positions f j d k s l a g h.
        assert_eq!(
            build_alphabet_for(&cfg(), &cyr, Geometry::Staggered),
            "аовлыдфпр"
        );
    }

    #[test]
    fn custom_preset_ignores_layout() {
        let mut c = cfg();
        c.preset = AlphabetPreset::Custom;
        c.alphabet = "asdf".to_string();
        assert_eq!(
            build_alphabet_for(&c, &Layout::dvorak(), Geometry::Staggered),
            "asdf"
        );
    }

    #[test]
    fn preset_labels_show_resolved_keys() {
        assert_eq!(
            preset_label_for(AlphabetPreset::HomeRow, &Layout::dvorak()),
            "Home row (aoeuidhtn)"
        );
        assert_eq!(
            preset_label_for(AlphabetPreset::HomeRow, &Layout::qwerty()),
            "Home row (asdfghjkl)"
        );
        assert_eq!(
            preset_label_for(AlphabetPreset::Numbers, &Layout::qwerty()),
            "Numbers 0-9"
        );
    }

    #[test]
    fn numbers_preset_emits_only_digits_minus_ambiguous() {
        let mut c = cfg();
        c.preset = AlphabetPreset::Numbers;
        let alphabet = build_alphabet(&c);
        // Default exclude_ambiguous strips only '0' (kept '1' since
        // Consolas draws it clearly). Digits are finger-ordered too.
        assert_eq!(alphabet, "473829156");
    }
}
