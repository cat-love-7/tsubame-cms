//! Slugs: the URL-safe form of a value.
//!
//! A slug is a field type of its own rather than a text field with a stricter rule, because what
//! makes a slug a slug is **normalisation**: `Hello World`, `hello world` and `hello-world` are
//! one slug, so a URL that names an item by its slug names exactly one item. A rule that only
//! refused the first two would leave the editor to type the third by hand, and two spellings
//! would still be able to exist side by side.
//!
//! The canonical form is lower-case ASCII letters and digits, separated by single hyphens:
//! `[a-z0-9]` runs joined by `-`. Anything else is a separator - including letters that are not
//! ASCII, which are *dropped* rather than transliterated (`café` becomes `caf`): guessing at a
//! transliteration would put a value in a URL that the editor never wrote. A value that has
//! nothing usable left over is refused rather than silently stored as empty.
//!
//! The same rule exists in TypeScript (`frontend/sl_cms/src/app/models/schema/slug.ts`) for the
//! editor's live feedback and its "generate from another field" button; the two are kept in step
//! by the same table of cases, tested on both sides. The server's answer is the one that is
//! stored.

/// The longest slug a value may normalise to.
///
/// Not a setting: a URL segment that long is already unusable, and one number is easier to explain
/// than an option that has to be chosen. `field_too_long` names it when it is exceeded.
pub const SLUG_MAX_LENGTH: usize = 200;

/// The canonical form of `value`.
///
/// Idempotent: normalising a slug that is already canonical changes nothing.
pub fn normalise(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut separator_pending = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            // A separator is only written once there is something to separate: this is what trims
            // a leading hyphen and collapses runs of them.
            if separator_pending && !out.is_empty() {
                out.push('-');
            }
            separator_pending = false;
            out.push(character.to_ascii_lowercase());
        } else {
            separator_pending = true;
        }
    }
    out
}

/// Whether anything usable is left after normalisation.
pub fn is_usable(value: &str) -> bool {
    !normalise(value).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table the TypeScript implementation is tested against as well, so the two cannot drift
    /// into disagreeing about what a slug is.
    pub(crate) const CASES: &[(&str, &str)] = &[
        ("Hello World", "hello-world"),
        ("hello-world", "hello-world"),
        ("  Hello, World!  ", "hello-world"),
        ("One -- Two", "one-two"),
        ("Already-slugged_1", "already-slugged-1"),
        ("--leading and trailing--", "leading-and-trailing"),
        ("CamelCaseTitle", "camelcasetitle"),
        ("Ünïcödé", "n-c-d"),
        ("café au lait", "caf-au-lait"),
        ("日本語", ""),
        ("", ""),
        ("-", ""),
        ("2026-09-15 release", "2026-09-15-release"),
    ];

    #[test]
    fn normalises_to_letters_digits_and_single_hyphens() {
        for (input, expected) in CASES {
            assert_eq!(normalise(input), *expected, "normalising {input:?}");
        }
    }

    #[test]
    fn normalising_is_idempotent() {
        for (_, expected) in CASES {
            assert_eq!(normalise(expected), *expected, "re-normalising {expected:?}");
        }
    }

    #[test]
    fn says_when_nothing_usable_is_left() {
        assert!(!is_usable("日本語"));
        assert!(!is_usable("   "));
        assert!(is_usable("a"));
        assert!(is_usable("日本語 with a title"));
    }
}
