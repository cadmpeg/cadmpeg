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
    /// Admit an ASCII byte that is not whitespace.
    #[must_use]
    pub const fn from_ascii(byte: u8) -> Option<Self> {
        if byte.is_ascii() && !byte.is_ascii_whitespace() {
            Some(Self(byte as char))
        } else {
            None
        }
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
///
/// ```compile_fail
/// let _ = cadmpeg_core::nonblank_literal!(" {}", "name");
/// ```
#[macro_export]
macro_rules! nonblank_literal {
    ($template:literal $(, $argument:expr)* $(,)?) => {{
        const NONBLANK_LITERAL_LEADING: $crate::text::NonWhitespaceChar = {
            let bytes = $template.as_bytes();
            assert!(
                !bytes.is_empty() && bytes[0] != b'{',
                "a nonblank literal must start with literal ASCII text",
            );
            match $crate::text::NonWhitespaceChar::from_ascii(bytes[0]) {
                Some(character) => character,
                None => panic!("a nonblank literal starts with a non-whitespace ASCII character"),
            }
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

/// A source property the reader could not key.
///
/// A blank key names nothing, so the entry it carries cannot be asked for. A
/// restated key names an entry that is already keyed, so the second value has
/// nowhere to go. Either way the value is still source content, so the reader
/// states the property it could not key instead of dropping it without a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedEntryError {
    /// The key holds no non-whitespace character.
    Blank {
        /// The record the reader named as the owner of the property set.
        record: String,
    },
    /// The record states this key a second time.
    Restated {
        /// The record the reader named as the owner of the property set.
        record: String,
        /// The key the record states twice.
        key: NonBlankString,
    },
}

impl NamedEntryError {
    /// The record the reader named as the owner of the property set.
    pub fn record(&self) -> &str {
        match self {
            Self::Blank { record } | Self::Restated { record, .. } => record,
        }
    }
}

impl std::fmt::Display for NamedEntryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blank { record } => {
                write!(formatter, "{record} states a property with a blank key")
            }
            Self::Restated { record, key } => write!(
                formatter,
                "{record} states the property {key} a second time"
            ),
        }
    }
}

impl std::error::Error for NamedEntryError {}

/// Keys a map by the entry names, and states every name it cannot key.
///
/// Returns the entries whose keys name something and are stated once, and one
/// [`NamedEntryError`] per entry whose key is blank or restated, in the order
/// the reader supplied. `record` names the owning record so the refused
/// property can be reported against the record that states it; it is rendered
/// only when a key is refused.
///
/// The first value a key states is the one that is kept, so a later restatement
/// never overwrites what is already keyed. The blank key names its record and
/// nothing else: the entries arrive as an iterator whose order this function
/// does not know, so a position counted here would name nothing in the source.
/// A reader that does know where in its source the property sits puts that in
/// `record`, which it builds. A restated key names itself as well as the
/// record, because the key is what the source states twice.
///
/// This is the route for a reader that keeps the properties it can key and
/// reports the rest. A reader that refuses the whole set uses
/// [`named_entries`].
pub fn named_entries_reporting<V>(
    record: impl std::fmt::Display,
    entries: impl IntoIterator<Item = (String, V)>,
) -> (BTreeMap<NonBlankString, V>, Vec<NamedEntryError>) {
    let mut kept = BTreeMap::new();
    let mut refused = Vec::new();
    for (name, value) in entries {
        match NonBlankString::new(name) {
            Some(key) => match kept.entry(key) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(value);
                }
                std::collections::btree_map::Entry::Occupied(slot) => {
                    refused.push(NamedEntryError::Restated {
                        record: record.to_string(),
                        key: slot.key().clone(),
                    });
                }
            },
            None => refused.push(NamedEntryError::Blank {
                record: record.to_string(),
            }),
        }
    }
    (kept, refused)
}

/// Keys a map by the entry names, refusing a name that is blank or restated.
///
/// Codecs that read an open set of native names route the whole set through
/// here, so the rule is stated once instead of at each reader.
///
/// # Errors
///
/// Names the record that states the first key this function cannot key, and
/// for a restated key the key itself.
pub fn named_entries<V>(
    record: impl std::fmt::Display,
    entries: impl IntoIterator<Item = (String, V)>,
) -> Result<BTreeMap<NonBlankString, V>, NamedEntryError> {
    let (kept, refused) = named_entries_reporting(record, entries);
    match refused.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(kept),
    }
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
            match $crate::text::NonWhitespaceChar::from_ascii(bytes[0]) {
                Some(character) => character,
                None => panic!("a nonblank literal starts with a non-whitespace ASCII character"),
            }
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
        const HASH: NonWhitespaceChar = match NonWhitespaceChar::from_ascii(b'#') {
            Some(character) => character,
            None => panic!("the literal is an ASCII non-whitespace character"),
        };
        const PINNED: &str = "pinned";
        for (prefix, suffix, expected) in [
            (HASH, "42", "#42"),
            (NonWhitespaceChar::hex_digit(0x0a), "", "a"),
            (NonWhitespaceChar::hex_digit(0xf0), "", "0"),
        ] {
            let value = NonBlankString::prefixed(prefix, suffix);
            assert_eq!(value.as_str(), expected);
            assert_eq!(serde_json::to_value(&value).unwrap(), expected);
        }
        assert_eq!(crate::nonblank_literal!("#{}", 42).as_str(), "#42");
        assert_eq!(crate::nonblank_const!(PINNED).as_str(), "pinned");
    }

    #[test]
    fn ascii_character_admission_is_total_over_every_byte() {
        for byte in u8::MIN..=u8::MAX {
            let admitted = NonWhitespaceChar::from_ascii(byte);
            assert_eq!(
                admitted.is_some(),
                byte.is_ascii() && !byte.is_ascii_whitespace()
            );
            if let Some(character) = admitted {
                assert_eq!(character.to_string(), char::from(byte).to_string());
            }
        }
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

    #[test]
    fn a_blank_key_is_named_and_the_other_properties_survive() {
        let entries = [
            ("width".to_owned(), "10"),
            ("   ".to_owned(), "dropped"),
            ("depth".to_owned(), "4"),
        ];

        let (kept, refused) = named_entries_reporting("feature 7", entries.clone());
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].record(), "feature 7");
        assert_eq!(
            refused[0].to_string(),
            "feature 7 states a property with a blank key"
        );
        assert_eq!(
            kept.keys().map(NonBlankString::as_str).collect::<Vec<_>>(),
            ["depth", "width"]
        );

        let error = named_entries("feature 7", entries).unwrap_err();
        assert_eq!(error, refused[0]);
    }

    #[test]
    fn a_restated_key_is_named_and_its_second_value_is_not_kept() {
        let entries = [
            ("k".to_owned(), "first"),
            ("k".to_owned(), "second"),
            ("depth".to_owned(), "4"),
        ];

        let (kept, refused) = named_entries_reporting("feature 7", entries.clone());
        assert_eq!(kept.len(), 2);
        assert_eq!(kept.get("k"), Some(&"first"));
        assert_eq!(refused.len(), 1);
        assert_eq!(
            refused[0],
            NamedEntryError::Restated {
                record: "feature 7".to_owned(),
                key: NonBlankString::new("k").unwrap(),
            }
        );
        assert_eq!(
            refused[0].to_string(),
            "feature 7 states the property k a second time"
        );

        let error = named_entries("feature 7", entries).unwrap_err();
        assert_eq!(error, refused[0]);
    }

    #[test]
    fn keys_that_all_name_something_are_kept_whole() {
        let entries = [("width".to_owned(), "10"), ("depth".to_owned(), "4")];

        let kept = named_entries("feature 7", entries.clone()).unwrap();
        assert_eq!(kept.len(), 2);
        assert!(named_entries_reporting("feature 7", entries).1.is_empty());
    }
}
