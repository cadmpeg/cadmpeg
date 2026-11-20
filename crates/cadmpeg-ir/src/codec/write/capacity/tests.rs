// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{TextCapacity, TextUnit};

const UTF8: TextCapacity = TextCapacity::new("name", TextUnit::Utf8Byte, 4);
const UTF16: TextCapacity = TextCapacity::new("name", TextUnit::Utf16CodeUnit, 3);

#[test]
fn a_text_that_fits_is_written_unchanged() {
    let fitted = UTF16.fit("abc");
    assert_eq!(
        (fitted.text, fitted.units, fitted.stated_units),
        ("abc", 3, 3)
    );
    assert!(!fitted.was_cut());
    assert_eq!(fitted.unwritable_entry("r"), None);
}

#[test]
fn an_ascii_text_is_cut_to_the_width() {
    let fitted = UTF8.fit("abcdef");
    assert_eq!(
        (fitted.text, fitted.units, fitted.stated_units),
        ("abcd", 4, 6)
    );
    assert!(fitted.was_cut());
}

#[test]
fn a_multibyte_utf8_character_is_never_split() {
    // "ab" + U+00E9 (2 bytes) + U+20AC (3 bytes): the euro sign starts at byte 4.
    let fitted = UTF8.fit("ab\u{e9}\u{20ac}");
    assert_eq!(
        (fitted.text, fitted.units, fitted.stated_units),
        ("ab\u{e9}", 4, 7)
    );
    let fitted = TextCapacity::new("name", TextUnit::Utf8Byte, 3).fit("ab\u{e9}");
    assert_eq!((fitted.text, fitted.units), ("ab", 2));
}

#[test]
fn a_utf16_surrogate_pair_is_never_split() {
    // U+1F600 is two UTF-16 code units; at width 3 only "ab" fits before it.
    let fitted = UTF16.fit("ab\u{1f600}c");
    assert_eq!(
        (fitted.text, fitted.units, fitted.stated_units),
        ("ab", 2, 5)
    );
    let fitted = TextCapacity::new("name", TextUnit::Utf16CodeUnit, 4).fit("ab\u{1f600}c");
    assert_eq!((fitted.text, fitted.units), ("ab\u{1f600}", 4));
}

#[test]
fn a_zero_width_field_holds_the_empty_text() {
    let fitted = TextCapacity::new("name", TextUnit::Utf16CodeUnit, 0).fit("a");
    assert_eq!((fitted.text, fitted.units, fitted.stated_units), ("", 0, 1));
    let fitted = TextCapacity::new("name", TextUnit::Utf16CodeUnit, 0).fit("");
    assert!(!fitted.was_cut());
}

#[test]
fn the_entry_names_the_record_field_and_width() {
    assert_eq!(
        UTF16
            .fit("abcdef")
            .unwritable_entry("doc:material#1")
            .as_deref(),
        Some("doc:material#1 name: 6 UTF-16 code units cut to 3; the field holds 3")
    );
}

#[test]
fn the_unwritable_and_residual_codes_are_listed_warnings() {
    use crate::report::Severity;
    let note = crate::codec::write::value_unwritable("x", "cuts texts", ["b", "a", "b"]).unwrap();
    assert_eq!(note.code.local_code(), "value.unwritable");
    assert_eq!(note.severity, Severity::Warning);
    assert_eq!(note.message, "the x write cuts texts (2): a, b");
    let note =
        crate::codec::write::record_residual_unavailable("x", "states no tokens", ["f"]).unwrap();
    assert_eq!(note.code.local_code(), "record.residual-unavailable");
    assert_eq!(note.severity, Severity::Warning);
    assert!(crate::codec::write::value_unwritable("x", "r", Vec::<String>::new()).is_none());
}
