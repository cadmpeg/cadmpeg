// SPDX-License-Identifier: Apache-2.0
//! Source text that carries its own non-blank proof.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::decode::text::TextSource;
use crate::decode::DecodeContext;
use crate::CodecError;

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
        if byte.is_ascii() && !matches!(byte, b'\t'..=b'\r' | b' ') {
            // endian-exception: reconstructed-scalar
            match char::from_u32(u32::from_be_bytes([0, 0, 0, byte])) {
                Some(character) => Some(Self(character)),
                None => None,
            }
        } else {
            None
        }
    }

    /// The lowercase hexadecimal digit naming the low four bits of `nibble`.
    ///
    /// Every hexadecimal digit is non-whitespace, and the mask makes the four
    /// bits total over `u8`, so this constructor refuses nothing.
    #[must_use]
    pub fn hex_digit(nibble: u8) -> Self {
        const DIGITS: [char; 16] = [
            '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
        ];
        Self(DIGITS[usize::from(nibble & 0x0f)])
    }
}

impl std::fmt::Display for NonWhitespaceChar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
    }
}

/// Static text whose leading byte is a non-whitespace ASCII character.
/// The proof is Copy so constant initializers need no destructor evaluation.
#[derive(Debug, Clone, Copy)]
pub struct StaticNonBlankText(&'static str);

impl StaticNonBlankText {
    /// Validate at most one byte and retain the static borrow.
    pub const fn new(value: &'static str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        match NonWhitespaceChar::from_ascii(bytes[0]) {
            Some(_) => Some(Self(value)),
            None => None,
        }
    }
}

/// A source string that holds at least one non-whitespace character.
///
/// A selection id, an external document identity and a native name are read
/// back and compared as text. A run of spaces names nothing, so it is refused
/// here rather than by each reader.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NonBlankString(Cow<'static, str>);

#[cfg(feature = "schema")]
impl JsonSchema for NonBlankString {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "NonBlankString".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Unicode White_Space matches char::is_whitespace; ECMAScript \s does not.
        schemars::json_schema!({
            "type": "string",
            "pattern": r"[^\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]"
        })
    }
}

impl crate::decode::cost::DecodeCost for NonBlankString {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        crate::decode::cost::DecodeCost::decode_cost(self.as_str(), ctx, operation)
    }
}

/// Owned or borrowed text whose non-blank scan was admitted by the caller.
/// The token preserves blank spelling and cannot be cloned or mutated.
#[derive(Debug)]
pub struct NonBlankText<S> {
    pub(crate) source: S,
    pub(crate) nonblank: bool,
}

impl<S: TextSource> AsRef<str> for NonBlankText<S> {
    fn as_ref(&self) -> &str {
        self.source.as_text()
    }
}

impl TryFrom<NonBlankText<String>> for NonBlankString {
    type Error = &'static str;

    fn try_from(value: NonBlankText<String>) -> Result<Self, Self::Error> {
        if value.nonblank {
            Ok(Self(Cow::Owned(value.source)))
        } else {
            Err("source identity must not be blank")
        }
    }
}

impl TryFrom<String> for NonBlankString {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.chars().any(|character| !character.is_whitespace()) {
            Ok(Self(Cow::Owned(value)))
        } else {
            Err("source identity must not be blank")
        }
    }
}

impl TryFrom<&str> for NonBlankString {
    type Error = &'static str;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(String::from(value))
    }
}

impl NonBlankString {
    /// Validate UTF-8 text before transferring owned or copying borrowed storage.
    /// The caller admits existing owned storage; a blank borrow needs no copy.
    pub fn for_decode<S: TextSource>(
        ctx: &DecodeContext<'_>,
        value: S,
        operation: &'static str,
    ) -> Result<Option<Self>, crate::decode::ResourceLimit> {
        let value = ctx.validate_nonblank_text(value, operation)?;
        if !value.nonblank {
            return Ok(None);
        }
        Ok(Some(Self(Cow::Owned(
            value.source.into_retained_text(ctx, operation)?,
        ))))
    }

    /// Transfer owned text whose first byte is a non-whitespace ASCII character.
    /// Validation reads at most one byte and does not copy the owned buffer.
    pub fn from_ascii_leading(value: String) -> Option<Self> {
        NonWhitespaceChar::from_ascii(*value.as_bytes().first()?)?;
        Some(Self(Cow::Owned(value)))
    }

    /// Borrow static text with an existing leading-byte proof.
    pub const fn from_static(value: StaticNonBlankText) -> Self {
        Self(Cow::Borrowed(value.0))
    }

    /// Format retained text and verify its leading non-whitespace character.
    /// The prefix check reads at most one Unicode scalar after charged formatting.
    pub fn formatted(
        ctx: &DecodeContext<'_>,
        leading: NonWhitespaceChar,
        arguments: std::fmt::Arguments<'_>,
    ) -> Result<Self, CodecError> {
        let value = ctx.format_retained(arguments, "format nonblank text")?;
        if !value.starts_with(leading.0) {
            return Err(CodecError::malformed(
                "formatted nonblank text has a different prefix",
            ));
        }
        Ok(Self(Cow::Owned(value)))
    }

    /// Format a non-whitespace prefix and a suffix through the caller budget.
    pub fn prefixed(
        ctx: &DecodeContext<'_>,
        prefix: NonWhitespaceChar,
        suffix: impl std::fmt::Display,
    ) -> Result<Self, CodecError> {
        Self::formatted(ctx, prefix, format_args!("{prefix}{suffix}"))
    }

    /// Returns the source string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Copy a previously admitted non-blank value into decoder-retained storage.
    pub fn try_clone_for_decode(
        &self,
        ctx: &crate::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, crate::CodecError> {
        let text = ctx.copy_retained_text(self.as_str(), operation)?;
        Ok(Self(Cow::Owned(text)))
    }

    /// Consumes the value and returns the source string.
    pub fn into_string(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        ctx.charge_work(0, operation)?;
        match self.0 {
            Cow::Owned(value) => Ok(value),
            Cow::Borrowed(value) => ctx.copy_retained_text(value, operation),
        }
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
/// literal prefix and cannot make the result blank. Static templates borrow
/// their rendered literal. Templates with arguments take the caller context
/// as their first argument and return `Result<NonBlankString, CodecError>`.
///
/// ```compile_fail
/// let _ = cadmpeg_core::nonblank_literal!(ctx, " {}", "name");
/// ```
///
/// ```compile_fail
/// let _ = cadmpeg_core::nonblank_literal!("\x0b");
/// ```
#[macro_export]
macro_rules! nonblank_literal {
    ($template:literal $(,)?) => {{
        const NONBLANK_LITERAL_TEXT: &str = {
            let bytes = $template.as_bytes();
            assert!(!bytes.is_empty() && bytes[0] != b'{',
                "a nonblank literal must start with literal ASCII text");
            match format_args!($template).as_str() {
                Some(text) => text,
                None => panic!("a formatted nonblank literal requires a decode context"),
            }
        };
        const NONBLANK_LITERAL_VALUE: $crate::text::StaticNonBlankText =
            match $crate::text::StaticNonBlankText::new(NONBLANK_LITERAL_TEXT) {
                Some(value) => value,
                None => panic!("a nonblank literal starts with a non-whitespace ASCII character"),
            };
        $crate::text::NonBlankString::from_static(NONBLANK_LITERAL_VALUE)
    }};
    ($ctx:expr, $template:literal $(, $argument:expr)* $(,)?) => {{
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
        $crate::text::NonBlankString::formatted($ctx, NONBLANK_LITERAL_LEADING,
            format_args!($template $(, $argument)*))
    }};
}

/// Lookup by the plain string the key spells.
///
/// `Ord` and `Hash` are derived over the same text, so a map keyed by this
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
    /// The resource policy or allocator refused the operation.
    ResourceRefusal(crate::decode::ResourceLimit),
    /// The owning record could not be formatted.
    FormattingRefusal,
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

impl std::fmt::Display for NamedEntryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResourceRefusal(limit) => write!(
                formatter,
                "resource refusal during {}: {:?}",
                limit.operation, limit.dimension
            ),
            Self::FormattingRefusal => formatter.write_str("cannot format named entry record"),
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

impl NamedEntryError {
    fn from_refusal(error: &CodecError) -> Self {
        match error {
            CodecError::ResourceLimit(limit) => Self::ResourceRefusal(*limit),
            _ => Self::FormattingRefusal,
        }
    }
}

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
/// [`named_entries_for_decode`].
///
/// Keys named entries after the caller admits each new map node and refusal.
///
/// The caller context charges collection slots and retained storage before
/// each vacant map node and refused-entry vector append. Refusal text and
/// restated keys are copied after retained-byte
/// admission. Input names and values must be admitted by their caller before
/// they are supplied to this function.
pub fn named_entries_reporting<V>(
    ctx: &DecodeContext<'_>,
    record: impl std::fmt::Display,
    entries: impl IntoIterator<Item = (String, V)>,
) -> Result<(BTreeMap<NonBlankString, V>, Vec<NamedEntryError>), CodecError> {
    let mut kept = BTreeMap::new();
    let mut refused = Vec::new();
    let mut entries = entries.into_iter();
    loop {
        let Some((name, value)) = ctx.next_charged(&mut entries, "named entry key scan")? else {
            break;
        };
        match NonBlankString::for_decode(ctx, name, "named entry key scan")? {
            Some(key) => {
                if ctx.contains_key_btree_map(&kept, &key, "named entry key comparisons")? {
                    let record = ctx
                        .format_retained(format_args!("{record}"), "named entry refused record")?;
                    let key = key.try_clone_for_decode(ctx, "named entry refused key")?;
                    ctx.reserve_vec(&mut refused, 1, "named entry refusals")?;
                    refused.push(NamedEntryError::Restated { record, key });
                } else {
                    ctx.insert_btree_map(&mut kept, key, value, "named entry map nodes")?;
                }
            }
            None => {
                let record =
                    ctx.format_retained(format_args!("{record}"), "named entry refused record")?;
                ctx.reserve_vec(&mut refused, 1, "named entry refusals")?;
                refused.push(NamedEntryError::Blank { record });
            }
        }
    }
    Ok((kept, refused))
}

/// Keys a named set through the caller's decode budget.
///
/// Returns the first blank or restated key, or a resource refusal.
pub fn named_entries_for_decode<V>(
    ctx: &DecodeContext<'_>,
    record: impl std::fmt::Display,
    entries: impl IntoIterator<Item = (String, V)>,
) -> Result<BTreeMap<NonBlankString, V>, NamedEntryError> {
    let (kept, refused) = named_entries_reporting(ctx, record, entries)
        .map_err(|error| NamedEntryError::from_refusal(&error))?;
    match refused.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(kept),
    }
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
    let arena = crate::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &crate::decode::DecodePolicy::default())
            .map_err(|error| NamedEntryError::from_refusal(&error))?;
    named_entries_for_decode(&ctx, record, entries)
}

/// Builds a [`NonBlankString`] from a `&'static str` constant.
///
/// [`nonblank_literal!`](crate::nonblank_literal) needs a literal template,
/// which a named constant is not. The proof is the same one: the leading byte
/// is read in the initializer of a `const` *item*, so a constant that is empty
/// or starts with whitespace or a non-ASCII byte fails `cargo check` with
/// E0080. Use it where the key is a pinned constant, so the spelling stays in
/// one place instead of being repeated as a literal beside the constant.
///
/// ```compile_fail
/// const BLANK: &str = "\x0b";
/// let _ = cadmpeg_core::nonblank_const!(BLANK);
/// ```
#[macro_export]
macro_rules! nonblank_const {
    ($constant:expr) => {{
        const NONBLANK_CONST_VALUE: $crate::text::StaticNonBlankText =
            match $crate::text::StaticNonBlankText::new($constant) {
                Some(value) => value,
                None => panic!("a nonblank constant starts with a non-whitespace ASCII character"),
            };
        $crate::text::NonBlankString::from_static(NONBLANK_CONST_VALUE)
    }};
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
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use crate::CodecError;

    #[cfg(feature = "schema")]
    use std::collections::BTreeMap;

    use super::{
        named_entries, named_entries_for_decode, named_entries_reporting, NamedEntryError,
        NonBlankString, NonWhitespaceChar,
    };
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn checked_reporting(
        entries: Vec<(String, i32)>,
        collection_limit: u64,
        retained_limit: u64,
    ) -> Result<
        (
            std::collections::BTreeMap<NonBlankString, i32>,
            Vec<NamedEntryError>,
        ),
        crate::CodecError,
    > {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_retained_bytes = retained_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        named_entries_reporting(&ctx, "f", entries)
    }

    #[test]
    fn named_empty_entry_refuses_iteration_before_validation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        assert!(
            matches!(named_entries_reporting(&ctx, "record", [(String::new(), 1)]),
            Err(crate::CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "named entry key scan")
        );
    }

    #[test]
    fn nonblank_copy_refuses_retained_bytes_before_duplication() {
        let value = NonBlankString::try_from("abc").expect("nonblank fixture");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = value
            .try_clone_for_decode(&ctx, "nonblank copy")
            .expect_err("three bytes exceed two");
        assert!(matches!(error, crate::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "nonblank copy"));
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        assert_eq!(
            value
                .try_clone_for_decode(&ctx, "nonblank copy")
                .expect("service copy"),
            value
        );
    }

    #[test]
    fn prefixes_preserve_nonblank_strings_and_wire_values() {
        const HASH: NonWhitespaceChar = match NonWhitespaceChar::from_ascii(b'#') {
            Some(character) => character,
            None => panic!("the literal is an ASCII non-whitespace character"),
        };
        const PINNED: &str = "pinned";
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        for (prefix, suffix, expected) in [
            (HASH, "42", "#42"),
            (NonWhitespaceChar::hex_digit(0x0a), "", "a"),
            (NonWhitespaceChar::hex_digit(0xf0), "", "0"),
        ] {
            let value = NonBlankString::prefixed(&ctx, prefix, suffix).unwrap();
            assert_eq!(value.as_str(), expected);
            assert_eq!(serde_json::to_value(&value).unwrap(), expected);
        }
        assert_eq!(
            crate::nonblank_literal!(&ctx, "#{}", 42).unwrap().as_str(),
            "#42"
        );
        assert_eq!(crate::nonblank_const!(PINNED).as_str(), "pinned");
    }

    #[test]
    fn static_nonblank_text_preserves_wire_equality_and_ownership() {
        let borrowed = crate::nonblank_literal!("pinned");
        let owned = NonBlankString::try_from("pinned".to_owned()).unwrap();
        assert!(matches!(borrowed.0, std::borrow::Cow::Borrowed("pinned")));
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed.cmp(&owned), std::cmp::Ordering::Equal);
        let mut map = std::collections::HashMap::new();
        map.insert(borrowed.clone(), 7);
        assert_eq!(map.get(&owned), Some(&7));
        assert_eq!(map.get("pinned"), Some(&7));
        assert_eq!(
            serde_json::to_string(&borrowed).unwrap(),
            serde_json::to_string(&owned).unwrap()
        );
        assert_eq!(
            crate::nonblank_literal!("key {{value}}").as_str(),
            "key {value}"
        );
        for value in ["", " ", "\t", "é"] {
            assert!(super::StaticNonBlankText::new(value).is_none());
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            owned.into_string(&ctx, "move owned nonblank").unwrap(),
            "pinned"
        );
        let CodecError::ResourceLimit(refusal) = borrowed
            .into_string(&ctx, "copy static nonblank")
            .unwrap_err()
        else {
            panic!("resource refusal")
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.additional, 6);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    }

    #[test]
    fn formatted_nonblank_text_pins_work_and_retained_bytes() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let value = crate::nonblank_literal!(&ctx, "#{}", 42).unwrap();
        assert_eq!(value.as_str(), "#42");
        let CodecError::ResourceLimit(work) = ctx.charge_work(u64::MAX, "probe work").unwrap_err()
        else {
            panic!("work refusal")
        };
        // The length-counting pass and the formatting pass each visit three output bytes.
        assert_eq!(work.used, 6);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        assert_eq!(
            crate::nonblank_literal!(&ctx, "#{}", 42).unwrap().as_str(),
            "#42"
        );
        let CodecError::ResourceLimit(storage) =
            ctx.charge_retained(u64::MAX, "probe storage").unwrap_err()
        else {
            panic!("storage refusal")
        };
        // One three-byte output buffer; formatting creates no intermediate string.
        assert_eq!(storage.used, 3);
    }

    #[test]
    fn formatted_nonblank_text_refuses_before_suffix_and_preserves_refusal() {
        struct Suffix<'a>(&'a std::cell::Cell<bool>);
        impl std::fmt::Display for Suffix<'_> {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.set(true);
                formatter.write_str("suffix")
            }
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let called = std::cell::Cell::new(false);
        let CodecError::ResourceLimit(first) =
            crate::nonblank_literal!(&ctx, "prefix{}", Suffix(&called)).unwrap_err()
        else {
            panic!("resource refusal")
        };
        assert!(!called.get());
        assert_eq!(first.operation, "format nonblank text");
        assert_eq!(ctx.resource_refusal(), Some(first));
        let CodecError::ResourceLimit(second) =
            crate::nonblank_literal!(&ctx, "other{}", 1).unwrap_err()
        else {
            panic!("resource refusal")
        };
        assert_eq!(second, first);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let leading = NonWhitespaceChar::from_ascii(b'#').unwrap();
        assert!(matches!(
            NonBlankString::formatted(&ctx, leading, format_args!(" ")),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn ascii_character_admission_is_total_over_every_byte() {
        for byte in u8::MIN..=u8::MAX {
            let admitted = NonWhitespaceChar::from_ascii(byte);
            assert_eq!(
                admitted.is_some(),
                byte.is_ascii() && !char::from(byte).is_whitespace()
            );
            if let Some(character) = admitted {
                assert_eq!(character.to_string(), char::from(byte).to_string());
            }
        }
    }

    #[test]
    fn every_admitted_prefix_produces_text_accepted_by_the_public_reader() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        for byte in u8::MIN..=u8::MAX {
            let Some(prefix) = NonWhitespaceChar::from_ascii(byte) else {
                continue;
            };
            let value = NonBlankString::prefixed(&ctx, prefix, "\t\n\u{85}\u{3000}").unwrap();
            assert!(
                NonBlankString::try_from(value.as_str()).is_ok(),
                "byte {byte}"
            );
            let wire = serde_json::to_string(&value).unwrap();
            assert_eq!(
                serde_json::from_str::<NonBlankString>(&wire).unwrap(),
                value
            );
        }
        assert!(NonWhitespaceChar::from_ascii(0x0b).is_none());
    }

    #[cfg(feature = "schema")]
    #[test]
    fn nonblank_schema_constrains_property_names_without_trimming_source_text() {
        let schema = serde_json::to_value(schemars::schema_for!(NonBlankString)).unwrap();
        assert_eq!(schema["type"], "string");
        let pattern = schema["pattern"].as_str().unwrap();
        let map =
            serde_json::to_value(schemars::schema_for!(BTreeMap<NonBlankString, String>)).unwrap();
        assert_eq!(map["additionalProperties"], false);
        assert!(map["patternProperties"].get(pattern).is_some());
        for text in [" \tname\n", "\u{feff}", "\0"] {
            let value = NonBlankString::try_from(text).unwrap();
            assert_eq!(serde_json::to_value(value).unwrap(), text);
        }
    }

    #[test]
    fn a_source_string_of_whitespace_alone_is_blank() {
        assert!(NonBlankString::try_from("   ").is_err());
        assert!(NonBlankString::try_from("").is_err());
        assert!(NonBlankString::try_from("\t\n").is_err());
        assert_eq!(
            NonBlankString::try_from(" a ")
                .ok()
                .map(|value| value.as_str().to_owned()),
            Some(" a ".to_owned())
        );
    }

    #[test]
    fn a_blank_key_is_named_and_the_other_properties_survive() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let entries = [
            ("width".to_owned(), "10"),
            ("   ".to_owned(), "dropped"),
            ("depth".to_owned(), "4"),
        ];

        let (kept, refused) = named_entries_reporting(&ctx, "feature 7", entries.clone()).unwrap();
        assert_eq!(refused.len(), 1);
        assert!(
            matches!(&refused[0], super::NamedEntryError::Blank { record } if record == "feature 7")
        );
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
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let entries = [
            ("k".to_owned(), "first"),
            ("k".to_owned(), "second"),
            ("depth".to_owned(), "4"),
        ];

        let (kept, refused) = named_entries_reporting(&ctx, "feature 7", entries.clone()).unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept.get("k"), Some(&"first"));
        assert_eq!(refused.len(), 1);
        assert_eq!(
            refused[0],
            NamedEntryError::Restated {
                record: "feature 7".to_owned(),
                key: NonBlankString::try_from("k").unwrap(),
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
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let entries = [("width".to_owned(), "10"), ("depth".to_owned(), "4")];

        let kept = named_entries("feature 7", entries.clone()).unwrap();
        assert_eq!(kept.len(), 2);
        assert!(named_entries_reporting(&ctx, "feature 7", entries)
            .unwrap()
            .1
            .is_empty());
    }

    #[test]
    fn named_entry_map_storage_refuses_before_insertion() {
        let node_bytes = 11 * (std::mem::size_of::<NonBlankString>() + std::mem::size_of::<i32>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<NonBlankString>().max(std::mem::align_of::<usize>());
        // The first key allocates one root node in the empty tree.
        let bytes = node_bytes;
        let error = checked_reporting(
            vec![("k".into(), 1)],
            1,
            crate::decode::u64_from_index(bytes) - 1,
        )
        .unwrap_err();
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "named entry map nodes"
                && limit.additional == crate::decode::u64_from_index(bytes)));
    }

    #[test]
    fn named_entry_refusal_storage_refuses_before_vector_growth() {
        let bytes = std::mem::size_of::<NamedEntryError>();
        let error = checked_reporting(
            vec![(" ".into(), 1)],
            1,
            crate::decode::u64_from_index(bytes),
        )
        .unwrap_err();
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "named entry refusals"
                && limit.additional == crate::decode::u64_from_index(4 * bytes)));
    }

    #[test]
    fn named_entry_scan_refuses_work_before_map_insertion() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = named_entries_reporting(&ctx, "f", [("width".into(), 1)]).unwrap_err();
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "named entry key scan"));
    }

    #[test]
    fn named_entry_scan_refuses_before_advancing_source() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let advanced = std::cell::Cell::new(false);
        let entries = std::iter::from_fn(|| {
            advanced.set(true);
            Some((String::from("key"), 1))
        });
        let CodecError::ResourceLimit(first) =
            named_entries_reporting(&ctx, "f", entries).unwrap_err()
        else {
            panic!("refusal")
        };
        assert!(!advanced.get());
        let CodecError::ResourceLimit(second) = ctx.charge_work(0, "later").unwrap_err() else {
            panic!("refusal")
        };
        assert_eq!(first, second);
    }

    #[test]
    fn nonblank_decode_validation_admits_unicode_bytes_and_keeps_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One whitespace character and one non-whitespace character.
        policy.limits.max_work_units = 2;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let value = String::from("\u{2003}a");
        let pointer = value.as_ptr();
        let value = NonBlankString::for_decode(&ctx, value, "validate")
            .unwrap()
            .unwrap();
        assert_eq!(value.as_str(), "\u{2003}a");
        assert_eq!(value.as_str().as_ptr(), pointer);
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, "probe").unwrap_err() else {
            panic!("refusal")
        };
        assert_eq!(limit.used, 2);
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first =
            NonBlankString::for_decode(&ctx, String::from("\u{2003}a"), "validate").unwrap_err();
        let CodecError::ResourceLimit(second) = ctx.charge_work(0, "later").unwrap_err() else {
            panic!("refusal")
        };
        assert_eq!(first, second);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        for value in ["", "\u{2003}\n", " a "] {
            assert_eq!(
                NonBlankString::for_decode(&ctx, String::from(value), "validate")
                    .unwrap()
                    .is_some(),
                NonBlankString::try_from(value).is_ok()
            );
        }
    }

    #[test]
    fn nonblank_borrowed_construction_charges_validation_and_one_copy() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two validated characters and four copied bytes; one four-byte buffer.
        policy.limits.max_work_units = 6;
        policy.limits.max_retained_bytes = 4;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let source = String::from("\u{2003}a");
        let result = NonBlankString::for_decode(&ctx, &source, "borrowed nonblank")
            .unwrap()
            .unwrap();
        assert_eq!(result.as_str(), source);
        assert_ne!(result.as_str().as_ptr(), source.as_ptr());
        let CodecError::ResourceLimit(work) = ctx.charge_work(1, "probe").unwrap_err() else {
            panic!("work refusal")
        };
        assert_eq!(work.used, 6);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            NonBlankString::for_decode(&ctx, source.as_str(), "borrowed nonblank")
                .unwrap()
                .unwrap()
                .as_str(),
            source
        );
        let CodecError::ResourceLimit(storage) = ctx.charge_retained(1, "probe").unwrap_err()
        else {
            panic!("storage refusal")
        };
        assert_eq!(storage.used, 4);
    }

    #[test]
    fn nonblank_borrowed_refusal_precedes_copy_and_blank_needs_no_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        // One whitespace character is validated without retaining its text.
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(NonBlankString::for_decode(&ctx, "\u{2003}", "blank text")
            .unwrap()
            .is_none());
        let CodecError::ResourceLimit(work) = ctx.charge_work(1, "probe").unwrap_err() else {
            panic!("work refusal")
        };
        assert_eq!(work.used, 1);
        policy.limits.max_work_units = 8;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = NonBlankString::for_decode(&ctx, "\u{2003}a", "borrowed text").unwrap_err();
        assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(first.additional, 4);
        assert_eq!(ctx.resource_refusal(), Some(first));
        let CodecError::ResourceLimit(second) = NonBlankString::try_from("owned")
            .unwrap()
            .into_string(&ctx, "later owned move")
            .unwrap_err()
        else {
            panic!("fused refusal")
        };
        assert_eq!(second, first);
    }

    #[test]
    fn retained_owned_text_preserves_storage_and_original_refusal() {
        use crate::decode::text::TextSource;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let source = String::from("owned");
        let pointer = source.as_ptr();
        let result = source.into_retained_text(&ctx, "owned transfer").unwrap();
        assert_eq!(result.as_ptr(), pointer);
        let CodecError::ResourceLimit(first) = ctx.charge_work(1, "refuse").unwrap_err() else {
            panic!("work refusal")
        };
        let second = result
            .into_retained_text(&ctx, "later transfer")
            .unwrap_err();
        assert_eq!(second, first);
    }

    #[test]
    fn checked_nonblank_token_keeps_spelling_and_transfers_one_owned_buffer() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two nonblank-token characters and one blank-token character; both buffers already exist.
        policy.limits.max_work_units = 3;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let source = String::from("\u{2003}a");
        let pointer = source.as_ptr();
        let token = ctx
            .validate_nonblank_text(source, "validate owned token")
            .unwrap();
        assert_eq!(token.as_ref(), "\u{2003}a");
        let value = NonBlankString::try_from(token).unwrap();
        assert_eq!(value.as_str().as_ptr(), pointer);
        let blank = ctx
            .validate_nonblank_text(String::from("\u{2003}"), "validate blank token")
            .unwrap();
        assert_eq!(blank.as_ref(), "\u{2003}");
        assert!(NonBlankString::try_from(blank).is_err());
        let first = ctx
            .validate_nonblank_text(String::from("x"), "token refusal")
            .unwrap_err();
        assert_eq!(first.used, 3);
        assert_eq!(ctx.resource_refusal(), Some(first));
        assert_eq!(
            ctx.validate_nonblank_text(String::from("later"), "later token")
                .unwrap_err(),
            first
        );
    }

    #[test]
    fn ascii_leading_text_checks_one_byte_and_preserves_owned_storage() {
        for source in ["", " leading", "\u{2003}a", "é"] {
            assert!(NonBlankString::from_ascii_leading(String::from(source)).is_none());
        }
        let source = String::from("f001");
        let pointer = source.as_ptr();
        let value = NonBlankString::from_ascii_leading(source).unwrap();
        assert_eq!(value.as_str(), "f001");
        assert_eq!(value.as_str().as_ptr(), pointer);
        assert_eq!(serde_json::to_string(&value).unwrap(), "\"f001\"");
    }

    #[test]
    fn named_entry_strict_resource_refusal_preserves_dimension() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = named_entries_for_decode(&ctx, "f", [("width".into(), 1)]).unwrap_err();
        assert!(
            matches!(crate::CodecError::from(error), crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "named entry map nodes")
        );
    }

    #[test]
    fn checked_named_entry_map_refuses_before_btree_insertion() {
        let entries = vec![("width".to_owned(), 1)];
        assert_eq!(
            checked_reporting(entries.clone(), 1, 1000).unwrap().0.len(),
            1
        );
        assert!(matches!(
            checked_reporting(entries, 0, 1000),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "named entry map nodes"
        ));
    }

    #[test]
    fn checked_named_entry_refusals_refuse_before_vec_growth() {
        let entries = vec![("width".to_owned(), 1), ("width".to_owned(), 2)];
        assert_eq!(
            checked_reporting(entries.clone(), 2, 10_000)
                .unwrap()
                .1
                .len(),
            1
        );
        assert!(matches!(
            checked_reporting(entries, 1, 10_000),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "named entry refusals"
        ));
    }

    #[test]
    fn checked_named_entry_record_refuses_before_text_growth() {
        let entries = vec![(" ".to_owned(), 1)];
        assert!(matches!(
            checked_reporting(entries, 10, 0),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "named entry refused record"
        ));
    }

    #[test]
    fn checked_named_entry_restated_key_refuses_before_text_growth() {
        let entries = vec![("width".to_owned(), 1), ("width".to_owned(), 2)];
        // One root node and the one-byte record consume the retained ceiling before the duplicate key copy.
        let bytes = 11 * (std::mem::size_of::<NonBlankString>() + std::mem::size_of::<i32>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<NonBlankString>()
            + 1;
        assert!(matches!(
            checked_reporting(entries, 10, crate::decode::u64_from_index(bytes)),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "named entry refused key"
        ));
    }

    #[test]
    fn checked_named_entries_keep_order_and_first_value() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let entries = vec![
            ("width".to_owned(), 1),
            ("width".to_owned(), 2),
            ("depth".to_owned(), 3),
        ];
        let (kept, refused) = checked_reporting(entries.clone(), 10, 10_000).unwrap();
        assert_eq!(kept.get("width"), Some(&1));
        assert_eq!(
            kept.keys().map(NonBlankString::as_str).collect::<Vec<_>>(),
            ["depth", "width"]
        );
        assert_eq!(
            refused,
            named_entries_reporting(&ctx, "f", entries).unwrap().1
        );

        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            named_entries_for_decode(&ctx, "f", [(" ".to_owned(), 1)])
                .map_err(crate::CodecError::from),
            Err(crate::CodecError::Malformed(_))
        ));
    }
}
