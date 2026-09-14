// SPDX-License-Identifier: Apache-2.0
//! Source text that carries its own non-blank proof.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A character that is not whitespace.
///
/// The type exists so [`NonBlankString::prefixed`] is total: a prefix of this
/// type makes the built string hold at least one non-whitespace character
/// whatever the suffix renders to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NonWhitespaceChar(char);

impl NonWhitespaceChar {
    /// The character an ASCII literal byte names.
    ///
    /// Macro-only: [`nonblank_literal!`](crate::nonblank_literal) is the one
    /// caller, and it evaluates this constructor in the initializer of a
    /// `const` item. The assertion is therefore an E0080 at every use, under
    /// `cargo check` as well as a build, and there is no run-time path that
    /// can reach it. Call it nowhere else.
    ///
    /// [`NonWhitespaceChar::hex_digit`] is the total constructor for a value
    /// computed at run time.
    #[doc(hidden)]
    #[must_use]
    pub const fn from_ascii_literal(byte: u8) -> Self {
        assert!(
            byte.is_ascii() && !byte.is_ascii_whitespace(),
            "a nonblank literal starts with a non-whitespace ASCII character",
        );
        Self(byte as char)
    }

    /// The lowercase hexadecimal digit naming the low four bits of `nibble`.
    ///
    /// Every hexadecimal digit is non-whitespace, and the mask makes the four
    /// bits total over `u8`, so this constructor refuses nothing.
    #[must_use]
    pub const fn hex_digit(nibble: u8) -> Self {
        const DIGITS: [char; 16] = [
            '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
        ];
        Self(DIGITS[(nibble & 0x0f) as usize])
    }
}

impl std::fmt::Display for NonWhitespaceChar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
    }
}

/// A source string that holds at least one non-whitespace character.
///
/// A selection id, an external document identity and a native name are read
/// back and compared as text. A run of spaces names nothing, so it is refused
/// here rather than by each reader.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NonBlankString(String);

impl NonBlankString {
    /// Constructs a source string that is not blank.
    ///
    /// Absent when the value holds no non-whitespace character, the empty
    /// string included.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        value
            .chars()
            .any(|character| !character.is_whitespace())
            .then_some(Self(value))
    }

    /// Constructs a non-blank string from a leading character and a suffix.
    ///
    /// Total: the prefix is non-whitespace, so the result holds it whatever
    /// the suffix renders to.
    pub fn prefixed(prefix: NonWhitespaceChar, suffix: impl std::fmt::Display) -> Self {
        Self(format!("{prefix}{suffix}"))
    }

    /// Returns the source string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the value and returns the source string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Builds a [`NonBlankString`] from a format literal whose first character is
/// literal, non-whitespace text.
///
/// The template is always a literal, so the leading byte is the initializer of
/// a `const` *item*: a template that starts with whitespace, with a non-ASCII
/// byte or with a `{` placeholder fails `cargo check` with E0080, not only a
/// codegen build. (An inline `const { … }` block is evaluated at codegen, so
/// `check` would pass a whitespace literal.) Formatted arguments follow that
/// literal prefix and cannot make the result blank.
#[macro_export]
macro_rules! nonblank_literal {
    ($template:literal $(, $argument:expr)* $(,)?) => {{
        const NONBLANK_LITERAL_LEADING: $crate::text::NonWhitespaceChar = {
            let bytes = $template.as_bytes();
            assert!(
                !bytes.is_empty() && bytes[0] != b'{',
                "a nonblank literal must start with literal ASCII text",
            );
            $crate::text::NonWhitespaceChar::from_ascii_literal(bytes[0])
        };
        let rendered = format!($template $(, $argument)*);
        $crate::text::NonBlankString::prefixed(NONBLANK_LITERAL_LEADING, &rendered[1..])
    }};
}

/// Lookup by the plain string the key spells.
///
/// `Ord` and `Hash` are derived over the same `String`, so a map keyed by this
/// type answers `get`, `contains_key` and indexing with a `&str` and orders its
/// keys exactly as a `String` key would.
impl std::borrow::Borrow<str> for NonBlankString {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

/// Keys a map by the entry names that are not blank.
///
/// A key that holds no non-whitespace character names nothing, so nothing can
/// ask for the entry it carries and the entry is dropped. Codecs that read an
/// open set of native names route the whole set through here, so the rule is
/// stated once instead of at each reader.
pub fn named_entries<V>(
    entries: impl IntoIterator<Item = (String, V)>,
) -> BTreeMap<NonBlankString, V> {
    entries
        .into_iter()
        .filter_map(|(name, value)| Some((NonBlankString::new(name)?, value)))
        .collect()
}

/// Builds a [`NonBlankString`] from a `&'static str` constant.
///
/// [`nonblank_literal!`](crate::nonblank_literal) needs a literal template,
/// which a named constant is not. The proof is the same one: the leading byte
/// is read in the initializer of a `const` *item*, so a constant that is empty
/// or starts with whitespace or a non-ASCII byte fails `cargo check` with
/// E0080. Use it where the key is a pinned constant, so the spelling stays in
/// one place instead of being repeated as a literal beside the constant.
#[macro_export]
macro_rules! nonblank_const {
    ($constant:expr) => {{
        const NONBLANK_CONST_LEADING: $crate::text::NonWhitespaceChar = {
            let bytes = $constant.as_bytes();
            assert!(
                !bytes.is_empty(),
                "a nonblank constant must hold at least one character",
            );
            $crate::text::NonWhitespaceChar::from_ascii_literal(bytes[0])
        };
        $crate::text::NonBlankString::prefixed(NONBLANK_CONST_LEADING, &$constant[1..])
    }};
}

impl PartialEq<str> for NonBlankString {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for NonBlankString {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl std::fmt::Display for NonBlankString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NonBlankString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("source identity must not be blank"))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn prefixes_preserve_nonblank_strings_and_wire_values() {
        for (prefix, suffix, expected) in [
            (NonWhitespaceChar::from_ascii_literal(b'#'), "42", "#42"),
            (NonWhitespaceChar::hex_digit(0x0a), "", "a"),
            (NonWhitespaceChar::hex_digit(0xf0), "", "0"),
        ] {
            let value = NonBlankString::prefixed(prefix, suffix);
            assert_eq!(value.as_str(), expected);
            assert_eq!(serde_json::to_value(&value).unwrap(), expected);
        }
        assert_eq!(crate::nonblank_literal!("#{}", 42).as_str(), "#42");
    }

    #[test]
    fn a_source_string_of_whitespace_alone_is_blank() {
        assert!(NonBlankString::new("   ").is_none());
        assert!(NonBlankString::new("").is_none());
        assert!(NonBlankString::new("\t\n").is_none());
        assert_eq!(
            NonBlankString::new(" a ").map(|value| value.as_str().to_owned()),
            Some(" a ".to_owned())
        );
    }
}
