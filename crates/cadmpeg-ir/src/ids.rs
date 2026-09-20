// SPDX-License-Identifier: Apache-2.0
//! Typed string identifiers for the IR graph.
//!
//! Each identity kind wraps a string in a distinct newtype, preventing references
//! between incompatible entity arenas and state-local member sets. Entity IDs
//! must be stable and globally unique within a document. State-local IDs need
//! only be unique within their owning state.
//!
//! Entity IDs follow `<format>:<scope>:<kind>#<key>` (exactly three colon
//! components before `#`). Compose typed IDs from an [`IdentityNamespace`]
//! and an [`IdentityKey`]; validate existing strings with [`is_valid_identity`].

use serde::Deserialize;

fn deserialize_local_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        Err(serde::de::Error::custom(IdentityError::InvalidId { value }))
    } else {
        Ok(value)
    }
}
use std::fmt::{self, Display};

/// True when `id` matches `<format>:<scope>:<kind>#<key>`.
///
/// The key is non-empty, contains no `#`, and the whole id has no whitespace.
#[must_use]
pub fn is_valid_identity(id: &str) -> bool {
    let Some((namespace, key)) = id.split_once('#') else {
        return false;
    };
    if key.is_empty() || key.contains('#') || id.chars().any(char::is_whitespace) {
        return false;
    }
    let mut components = namespace.split(':');
    components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_none()
}

/// An entity identity with validated namespace and key grammar.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct Identity(String);

impl serde::Serialize for Identity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::schema::serialize_reference_id(self.as_str(), serializer)
    }
}

// Unicode White_Space matches char::is_whitespace; ECMAScript \s does not.
#[cfg(feature = "schema")]
const SCHEMA_WHITESPACE: &str =
    r"\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000";

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Identity {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Identity".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // A strict end assertion also refuses a trailing line break.
        let pattern = format!(
            r"^(?:[^:#{SCHEMA_WHITESPACE}]+:){{2}}[^:#{SCHEMA_WHITESPACE}]+#[^#{SCHEMA_WHITESPACE}]+(?![\s\S])"
        );
        schemars::json_schema!({"type": "string", "pattern": pattern})
    }
}

impl Identity {
    /// Admit a string matching the entity identity grammar.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        if is_valid_identity(&value) {
            Ok(Self(value))
        } else {
            Err(IdentityError::InvalidId { value })
        }
    }

    /// Borrow the identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Copy the key whose grammar was admitted with this identity.
    #[must_use]
    pub fn key(&self) -> IdentityKey {
        // Admission guarantees exactly one separator and a nonempty key.
        IdentityKey(std::borrow::Cow::Owned(self.0.split('#').skip(1).collect()))
    }

    /// Replace the kind while retaining the admitted format, scope, and key.
    #[must_use]
    pub fn with_kind(&self, kind: &IdentityComponent) -> Self {
        let mut value = String::with_capacity(self.0.len() + kind.as_str().len());
        // Admission guarantees three namespace components and one key separator.
        for component in self.0.split(':').take(2) {
            value.push_str(component);
            value.push(':');
        }
        value.push_str(kind.as_str());
        value.push('#');
        for key in self.0.split('#').skip(1) {
            value.push_str(key);
        }
        Self(value)
    }

    /// Append an admitted tail to this identity's key.
    #[must_use]
    pub fn with_key_tail(mut self, tail: &IdentityKeyTail) -> Self {
        self.0.push_str(tail.as_str());
        self
    }

    /// Consume the identity into its string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl TryFrom<String> for Identity {
    type Error = IdentityError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for Identity {
    type Error = IdentityError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Return the Unicode scalar at `index` and the number of bytes it occupies.
///
/// `str` guarantees valid UTF-8, so this decoder needs no error arm. Keeping
/// the decoder here makes the identity grammar available to const evaluation;
/// `str::chars` is not const on the supported toolchain.
const fn decode_scalar(bytes: &[u8], index: usize) -> (u32, usize) {
    let first = bytes[index];
    if first < 0x80 {
        (first as u32, 1)
    } else if first < 0xe0 {
        (
            (((first & 0x1f) as u32) << 6) | ((bytes[index + 1] & 0x3f) as u32),
            2,
        )
    } else if first < 0xf0 {
        (
            (((first & 0x0f) as u32) << 12)
                | (((bytes[index + 1] & 0x3f) as u32) << 6)
                | ((bytes[index + 2] & 0x3f) as u32),
            3,
        )
    } else {
        (
            (((first & 0x07) as u32) << 18)
                | (((bytes[index + 1] & 0x3f) as u32) << 12)
                | (((bytes[index + 2] & 0x3f) as u32) << 6)
                | ((bytes[index + 3] & 0x3f) as u32),
            4,
        )
    }
}

/// True for the Unicode `White_Space` property used by `char::is_whitespace`.
const fn is_unicode_whitespace(codepoint: u32) -> bool {
    matches!(
        codepoint,
        0x0009..=0x000d
            | 0x0020
            | 0x0085
            | 0x00a0
            | 0x1680
            | 0x2000..=0x200a
            | 0x2028
            | 0x2029
            | 0x202f
            | 0x205f
            | 0x3000
    )
}

/// True when `value` contains Unicode whitespace.
const fn contains_unicode_whitespace(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let (codepoint, width) = decode_scalar(bytes, index);
        if is_unicode_whitespace(codepoint) {
            return true;
        }
        index += width;
    }
    false
}

/// True when `value` is a valid namespace component.
const fn valid_component_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || contains_unicode_whitespace(value) {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b':' | b'#') {
            return false;
        }
        index += 1;
    }
    true
}

/// True when `value` is a valid identity key.
const fn valid_key_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || contains_unicode_whitespace(value) {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' {
            return false;
        }
        index += 1;
    }
    true
}

/// A static component whose grammar was admitted during const evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StaticIdentityComponent {
    value: &'static str,
}

impl StaticIdentityComponent {
    /// Admit a static component, returning `None` for invalid grammar.
    #[must_use]
    pub const fn new(value: &'static str) -> Option<Self> {
        if valid_component_text(value) {
            Some(Self { value })
        } else {
            None
        }
    }
}

/// A `<format>`, `<scope>` or `<kind>` component of an entity identity.
///
/// The component grammar is: at least one character, and no `:`, `#` or
/// Unicode whitespace. A value of this type exists only because that check
/// passed, so typed composition has no failure route.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentityComponent(std::borrow::Cow<'static, str>);

impl IdentityComponent {
    /// Admit component text and retain a useful error for the source route.
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        if valid_component_text(&value) {
            Ok(Self(std::borrow::Cow::Owned(value)))
        } else {
            Err(IdentityError::InvalidComponent {
                label: "component",
                value,
            })
        }
    }

    /// Construct a component from a static proof.
    #[must_use]
    pub const fn from_static(proof: StaticIdentityComponent) -> Self {
        Self(std::borrow::Cow::Borrowed(proof.value))
    }

    /// This component, then `part`, with no separator.
    ///
    /// Both sides carry the component grammar, which no concatenation can
    /// break, so the result needs no second admission.
    #[must_use]
    pub fn then(self, part: impl Into<Self>) -> Self {
        let mut text = self.0.into_owned();
        text.push_str(part.into().as_str());
        Self(std::borrow::Cow::Owned(text))
    }

    /// The component text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! identity_component_from_number {
    ($($number:ty),* $(,)?) => {$(
        impl From<$number> for IdentityComponent {
            fn from(value: $number) -> Self {
                Self(std::borrow::Cow::Owned(value.to_string()))
            }
        }
    )*};
}

identity_component_from_number!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

impl From<&IdentityComponent> for IdentityComponent {
    fn from(value: &IdentityComponent) -> Self {
        value.clone()
    }
}

impl TryFrom<String> for IdentityComponent {
    type Error = IdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl TryFrom<&str> for IdentityComponent {
    type Error = IdentityError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_new(value.to_owned())
    }
}

/// A static key whose grammar was admitted during const evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StaticIdentityKey {
    value: &'static str,
}

impl StaticIdentityKey {
    /// Admit a static key, returning `None` for invalid grammar.
    #[must_use]
    pub const fn new(value: &'static str) -> Option<Self> {
        if valid_key_text(value) {
            Some(Self { value })
        } else {
            None
        }
    }
}

/// The `<key>` component of an entity identity.
///
/// The key grammar is: at least one character, and no `#` or Unicode
/// whitespace. A `:` is part of the grammar because the key follows the `#`
/// that ends the namespace. A value of this type exists only because that
/// check passed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdentityKey(std::borrow::Cow<'static, str>);

impl IdentityKey {
    /// Encode one source component for use as an identity key.
    ///
    /// ASCII letters, digits, `.`, `_`, `-`, and `/` remain literal. Every other
    /// UTF-8 byte becomes `%HH` with uppercase hexadecimal digits. Empty input
    /// becomes `%EMPTY`; a literal percent sign is escaped, so that spelling
    /// cannot alias a nonempty source value.
    #[must_use]
    pub fn encode_segment(value: &str) -> Self {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        if value.is_empty() {
            return Self(std::borrow::Cow::Owned("%EMPTY".to_owned()));
        }
        let mut encoded = String::with_capacity(value.len());
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/') {
                encoded.push(char::from(byte));
            } else {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
        Self(std::borrow::Cow::Owned(encoded))
    }

    /// Encode arbitrary source text as key text, keeping every character the
    /// key grammar admits literal.
    ///
    /// `#` and every Unicode whitespace character become `%HH` per UTF-8 byte,
    /// `%` becomes `%25` so no encoded spelling aliases a literal one, and
    /// empty input becomes `%EMPTY`. Text that is already key text and carries
    /// no `%` keeps its spelling, so this encodes an id without renaming the
    /// ids that already have keys. Use [`encode_segment`](Self::encode_segment)
    /// instead for one component of a composed key, which must also escape the
    /// separators.
    #[must_use]
    pub fn encode_key_text(value: &str) -> Self {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        if value.is_empty() {
            return Self(std::borrow::Cow::Owned("%EMPTY".to_owned()));
        }
        let mut encoded = String::with_capacity(value.len());
        for character in value.chars() {
            if character == '%' || character == '#' || character.is_whitespace() {
                let mut buffer = [0u8; 4];
                for byte in character.encode_utf8(&mut buffer).as_bytes() {
                    encoded.push('%');
                    encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                    encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
                }
            } else {
                encoded.push(character);
            }
        }
        Self(std::borrow::Cow::Owned(encoded))
    }

    /// Copy this key with ASCII letters converted to lowercase.
    #[must_use]
    pub fn to_ascii_lowercase(&self) -> Self {
        Self(std::borrow::Cow::Owned(self.0.to_ascii_lowercase()))
    }

    /// Consume this key into its text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0.into_owned()
    }

    /// Construct an unsigned decimal key with a minimum zero-padded width.
    #[must_use]
    pub fn zero_padded(value: u64, width: usize) -> Self {
        Self(std::borrow::Cow::Owned(format!("{value:0width$}")))
    }

    /// Construct a key from the two lowercase hexadecimal digits of a byte.
    #[must_use]
    pub fn hex_byte(value: u8) -> Self {
        let mut text = String::with_capacity(2);
        append_hex_bytes(&mut text, &[value]);
        Self(std::borrow::Cow::Owned(text))
    }

    /// Append two lowercase hexadecimal digits for each byte.
    #[must_use]
    pub fn with_hex_bytes(self, bytes: &[u8]) -> Self {
        let mut text = self.0.into_owned();
        append_hex_bytes(&mut text, bytes);
        Self(std::borrow::Cow::Owned(text))
    }

    /// Admit key text and retain the rejected value for the source route.
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        if valid_key_text(&value) {
            Ok(Self(std::borrow::Cow::Owned(value)))
        } else {
            Err(IdentityError::InvalidKey { value })
        }
    }

    /// Construct a key from a static proof.
    #[must_use]
    pub const fn from_static(proof: StaticIdentityKey) -> Self {
        Self(std::borrow::Cow::Borrowed(proof.value))
    }

    /// The key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn append_hex_bytes(text: &mut String, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
}

impl TryFrom<String> for IdentityKey {
    type Error = IdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl TryFrom<&str> for IdentityKey {
    type Error = IdentityError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_new(value.to_owned())
    }
}

/// A static namespace whose three components were admitted during const evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StaticIdentityNamespace {
    format: &'static str,
    scope: &'static str,
    kind: &'static str,
}

impl StaticIdentityNamespace {
    /// Admit a static namespace, returning `None` for invalid grammar.
    #[must_use]
    pub const fn new(
        format: &'static str,
        scope: &'static str,
        kind: &'static str,
    ) -> Option<Self> {
        if valid_component_text(format) && valid_component_text(scope) && valid_component_text(kind)
        {
            Some(Self {
                format,
                scope,
                kind,
            })
        } else {
            None
        }
    }
}

/// The three typed components before an identity's `#` key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentityNamespace {
    format: IdentityComponent,
    scope: IdentityComponent,
    kind: IdentityComponent,
}

impl IdentityNamespace {
    /// Admit a namespace assembled from runtime text.
    pub fn new(
        format: impl Into<String>,
        scope: impl Into<String>,
        kind: impl Into<String>,
    ) -> Result<Self, IdentityError> {
        let format = format.into();
        let scope = scope.into();
        let kind = kind.into();
        let format_component = Self::component("format", format)?;
        let scope_component = Self::component("scope", scope)?;
        let kind_component = Self::component("kind", kind)?;
        Ok(Self {
            format: format_component,
            scope: scope_component,
            kind: kind_component,
        })
    }

    /// Construct a namespace from a static proof.
    #[must_use]
    pub const fn from_static(proof: StaticIdentityNamespace) -> Self {
        Self {
            format: IdentityComponent(std::borrow::Cow::Borrowed(proof.format)),
            scope: IdentityComponent(std::borrow::Cow::Borrowed(proof.scope)),
            kind: IdentityComponent(std::borrow::Cow::Borrowed(proof.kind)),
        }
    }

    /// Combine three already-admitted runtime components.
    #[must_use]
    pub fn from_components(
        format: &IdentityComponent,
        scope: &IdentityComponent,
        kind: &IdentityComponent,
    ) -> Self {
        Self {
            format: format.clone(),
            scope: scope.clone(),
            kind: kind.clone(),
        }
    }

    fn component(label: &'static str, value: String) -> Result<IdentityComponent, IdentityError> {
        if valid_component_text(&value) {
            Ok(IdentityComponent(std::borrow::Cow::Owned(value)))
        } else {
            Err(IdentityError::InvalidComponent { label, value })
        }
    }

    /// The format component.
    #[must_use]
    pub fn format(&self) -> &str {
        self.format.as_str()
    }

    /// The scope component.
    #[must_use]
    pub fn scope(&self) -> &str {
        self.scope.as_str()
    }

    /// The kind component.
    #[must_use]
    pub fn kind(&self) -> &str {
        self.kind.as_str()
    }
}

/// Build a checked namespace from three literal components.
///
/// ```compile_fail
/// # fn main() {
/// let _ = cadmpeg_ir::identity_namespace!("bad#format", "scope", "kind");
/// # }
/// ```
#[macro_export]
macro_rules! identity_namespace {
    ($format:literal, $scope:literal, $kind:literal $(,)?) => {{
        const STATIC_IDENTITY_NAMESPACE: $crate::ids::StaticIdentityNamespace =
            match $crate::ids::StaticIdentityNamespace::new($format, $scope, $kind) {
                Some(namespace) => namespace,
                None => panic!("identity namespace literal has invalid grammar"),
            };
        $crate::ids::IdentityNamespace::from_static(STATIC_IDENTITY_NAMESPACE)
    }};
}

/// Build a checked namespace component from one literal.
#[macro_export]
macro_rules! identity_component {
    ($value:literal) => {{
        const STATIC_IDENTITY_COMPONENT: $crate::ids::StaticIdentityComponent =
            match $crate::ids::StaticIdentityComponent::new($value) {
                Some(component) => component,
                None => panic!("identity component literal has invalid grammar"),
            };
        $crate::ids::IdentityComponent::from_static(STATIC_IDENTITY_COMPONENT)
    }};
}

/// Build a checked identity key from one literal.
#[macro_export]
macro_rules! identity_key {
    ($value:literal) => {{
        const STATIC_IDENTITY_KEY: $crate::ids::StaticIdentityKey =
            match $crate::ids::StaticIdentityKey::new($value) {
                Some(key) => key,
                None => panic!("identity key literal has invalid grammar"),
            };
        $crate::ids::IdentityKey::from_static(STATIC_IDENTITY_KEY)
    }};
}

/// A tail appended to an identity key.
///
/// A tail may be empty, and it holds no `#` and no whitespace. Appending one
/// to an [`IdentityKey`] therefore always yields an [`IdentityKey`], which is
/// how an optional scope suffix reaches a key without a second grammar check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct IdentityKeyTail(String);

impl IdentityKeyTail {
    /// Admit text with no key separator or Unicode whitespace; empty text is valid.
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        if value.is_empty() || valid_key_text(&value) {
            Ok(Self(value))
        } else {
            Err(IdentityError::InvalidKey { value })
        }
    }

    /// Escape identity separators, the escape byte, and Unicode whitespace
    /// in one source component.
    #[must_use]
    pub fn percent_encode(value: &str) -> Self {
        let mut encoded = String::with_capacity(value.len());
        for character in value.chars() {
            if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                let mut bytes = [0; 4];
                for byte in character.encode_utf8(&mut bytes).as_bytes() {
                    const HEX: &[u8; 16] = b"0123456789ABCDEF";
                    encoded.push('%');
                    encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                    encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
                }
            } else {
                encoded.push(character);
            }
        }
        Self(encoded)
    }

    /// The tail that appends nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self(String::new())
    }

    /// This tail, then `-`, then `part`.
    #[must_use]
    pub fn dash(mut self, part: impl Into<IdentityKey>) -> Self {
        self.0.push('-');
        self.0.push_str(part.into().as_str());
        self
    }

    /// This tail, then `part`, with no separator.
    #[must_use]
    pub fn then(mut self, part: impl Into<IdentityKey>) -> Self {
        self.0.push_str(part.into().as_str());
        self
    }

    /// The tail text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl IdentityKey {
    /// This key, then `-`, then `part`.
    #[must_use]
    pub fn dash(self, part: impl Into<IdentityKey>) -> Self {
        let mut text = self.0.into_owned();
        text.push('-');
        text.push_str(part.into().as_str());
        Self(std::borrow::Cow::Owned(text))
    }

    /// This key, then `:`, then `part`.
    ///
    /// A `:` follows the `#` that ends the namespace, so it is part of the key
    /// grammar and never splits the identity.
    #[must_use]
    pub fn colon(self, part: impl Into<IdentityKey>) -> Self {
        let mut text = self.0.into_owned();
        text.push(':');
        text.push_str(part.into().as_str());
        Self(std::borrow::Cow::Owned(text))
    }

    /// This key, then `part`, with no separator.
    #[must_use]
    pub fn then(self, part: impl Into<IdentityKey>) -> Self {
        let mut text = self.0.into_owned();
        text.push_str(part.into().as_str());
        Self(std::borrow::Cow::Owned(text))
    }

    /// This key, then `tail`.
    #[must_use]
    pub fn with_tail(self, tail: &IdentityKeyTail) -> Self {
        let mut text = self.0.into_owned();
        text.push_str(tail.as_str());
        Self(std::borrow::Cow::Owned(text))
    }

    /// Prepend admitted text, which may be empty, to this nonempty key.
    #[must_use]
    pub fn with_prefix(self, prefix: &IdentityKeyTail) -> Self {
        let mut text = String::with_capacity(prefix.as_str().len() + self.0.len());
        text.push_str(prefix.as_str());
        text.push_str(self.as_str());
        Self(std::borrow::Cow::Owned(text))
    }
}

macro_rules! identity_key_from_number {
    ($($number:ty),* $(,)?) => {$(
        impl From<$number> for IdentityKey {
            fn from(value: $number) -> Self {
                Self(std::borrow::Cow::Owned(value.to_string()))
            }
        }

        impl From<&$number> for IdentityKey {
            fn from(value: &$number) -> Self {
                Self(std::borrow::Cow::Owned(value.to_string()))
            }
        }

        impl From<&&$number> for IdentityKey {
            fn from(value: &&$number) -> Self {
                Self(std::borrow::Cow::Owned(value.to_string()))
            }
        }
    )*};
}

identity_key_from_number!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

impl From<&IdentityKey> for IdentityKey {
    fn from(value: &IdentityKey) -> Self {
        value.clone()
    }
}

impl Display for IdentityKeyTail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Display for IdentityComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Display for IdentityKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Identity {
    /// Compose an identity from an admitted namespace and key.
    ///
    /// Both arguments carry their own grammar, so this operation cannot
    /// refuse and cannot produce a malformed identity.
    #[must_use]
    pub fn compose(namespace: &IdentityNamespace, key: impl Into<IdentityKey>) -> Self {
        let key = key.into();
        let mut value = String::with_capacity(
            namespace.format().len()
                + namespace.scope().len()
                + namespace.kind().len()
                + key.as_str().len()
                + 4,
        );
        value.push_str(namespace.format());
        value.push(':');
        value.push_str(namespace.scope());
        value.push(':');
        value.push_str(namespace.kind());
        value.push('#');
        value.push_str(key.as_str());
        Self(value)
    }
}

/// Failure to mint an entity identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// A colon-separated namespace component is empty or contains separators.
    InvalidComponent {
        /// Component name (`format`, `scope`, or `kind`).
        label: &'static str,
        /// Rejected value.
        value: String,
    },
    /// The `#` key is empty or contains `#` / whitespace.
    InvalidKey {
        /// Rejected key.
        value: String,
    },
    /// Composed id failed [`is_valid_identity`].
    InvalidId {
        /// Rejected id.
        value: String,
    },
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidComponent { label, value } => {
                write!(f, "identity {label} component is invalid: {value:?}")
            }
            Self::InvalidKey { value } => write!(f, "identity key is invalid: {value:?}"),
            Self::InvalidId { value } => write!(f, "identity is invalid: {value:?}"),
        }
    }
}

impl std::error::Error for IdentityError {}

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Deserialize)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[serde(transparent)]
        pub struct $name($crate::ids::Identity);

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                $crate::schema::serialize_reference_id(self.0.as_str(), serializer)
            }
        }

        impl $name {
            /// Mint an identity that matches `<format>:<scope>:<kind>#<key>`.
            pub fn mint(value: impl Into<String>) -> Result<Self, $crate::ids::IdentityError> {
                $crate::ids::Identity::new(value).map(Self::from)
            }

            /// Compose an identity from an admitted namespace and key.
            #[must_use]
            pub fn compose(
                namespace: &$crate::ids::IdentityNamespace,
                key: impl Into<$crate::ids::IdentityKey>,
            ) -> Self {
                Self($crate::ids::Identity::compose(namespace, key))
            }

            /// Return the underlying id string.
            #[must_use]
            pub fn into_string(self) -> String {
                self.0.into_string()
            }

            /// Borrow the underlying id string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }

            /// Copy the admitted key.
            #[must_use]
            pub fn key(&self) -> $crate::ids::IdentityKey {
                self.0.key()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0.as_str())
            }
        }

        impl From<$name> for $crate::ids::Identity {
            fn from(value: $name) -> Self { value.0 }
        }

        impl From<$crate::ids::Identity> for $name {
            fn from(value: $crate::ids::Identity) -> Self { Self(value) }
        }

        impl TryFrom<String> for $name {
            type Error = $crate::ids::IdentityError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::mint(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = $crate::ids::IdentityError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::mint(value)
            }
        }
    };
}

pub(crate) use id_type;

macro_rules! local_id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(#[serde(deserialize_with = "crate::ids::deserialize_local_id")] String);

        #[cfg(feature = "schema")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                let pattern = format!(r"^[^{}]+(?![\s\S])", SCHEMA_WHITESPACE);
                schemars::json_schema!({"type": "string", "pattern": pattern})
            }
        }

        impl $name {
            /// Mint a non-empty identity that has no whitespace.
            pub fn mint(value: impl Into<String>) -> Result<Self, $crate::ids::IdentityError> {
                let value = value.into();
                if value.is_empty() || value.chars().any(char::is_whitespace) {
                    return Err($crate::ids::IdentityError::InvalidId { value });
                }
                Ok(Self(value))
            }

            /// Compose a local identity from an admitted namespace and key.
            #[must_use]
            pub fn compose(
                namespace: &$crate::ids::IdentityNamespace,
                key: impl Into<$crate::ids::IdentityKey>,
            ) -> Self {
                Self($crate::ids::Identity::compose(namespace, key).into_string())
            }

            /// Return the underlying id string.
            #[must_use]
            pub fn into_string(self) -> String {
                self.0
            }

            /// Borrow the underlying id string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = $crate::ids::IdentityError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::mint(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = $crate::ids::IdentityError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::mint(value)
            }
        }
    };
}

id_type!(
    /// Identifies a [`crate::topology::Body`].
    BodyId
);
id_type!(
    /// Identifies one feature-input topology state.
    FeatureInputTopologyId
);
id_type!(
    /// Identifies one feature-result topology state.
    FeatureResultTopologyId
);
local_id_type!(
    /// Identifies a body within one feature-input topology state.
    HistoricalBodyId
);
local_id_type!(
    /// Identifies a face within one feature-input topology state.
    HistoricalFaceId
);
local_id_type!(
    /// Identifies an edge within one feature-input topology state.
    HistoricalEdgeId
);
local_id_type!(
    /// Identifies a vertex within one feature-input topology state.
    HistoricalVertexId
);
id_type!(
    /// Identifies a [`crate::topology::Region`].
    RegionId
);
id_type!(
    /// Identifies a [`crate::topology::Shell`].
    ShellId
);
id_type!(
    /// Identifies a [`crate::topology::Face`].
    FaceId
);
id_type!(
    /// Identifies a [`crate::topology::Loop`].
    LoopId
);
id_type!(
    /// Identifies a [`crate::topology::Coedge`].
    CoedgeId
);
id_type!(
    /// Identifies a [`crate::topology::Edge`].
    EdgeId
);
id_type!(
    /// Identifies a [`crate::topology::Vertex`].
    VertexId
);
id_type!(
    /// Identifies a [`crate::geometry::Surface`] carrier.
    SurfaceId
);
id_type!(
    /// Identifies a [`crate::geometry::Curve`] carrier.
    CurveId
);
id_type!(
    /// Identifies a [`crate::geometry::pcurve::Pcurve`] carrier.
    PcurveId
);
id_type!(
    /// Identifies a [`crate::geometry::ProceduralSurface`] construction.
    ProceduralSurfaceId
);
id_type!(
    /// Identifies a [`crate::geometry::ProceduralCurve`] construction.
    ProceduralCurveId
);
id_type!(
    /// Identifies a [`crate::subd::SubdSurface`] carrier.
    SubdId
);
id_type!(
    /// Identifies a [`crate::topology::Point`] carrier (a vertex position).
    PointId
);
id_type!(
    /// Identifies a passthrough [`crate::unknown::UnknownRecord`].
    UnknownId
);

impl std::borrow::Borrow<str> for UnknownId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

id_type!(
    /// Identifies a decoded [`crate::appearance::Appearance`] asset.
    AppearanceId
);
id_type!(
    /// Identifies an [`crate::appearance::AppearanceBinding`] assignment.
    AppearanceBindingId
);
id_type!(
    /// Identifies a linked [`crate::attributes::SourceAttribute`] record.
    AttributeId
);
id_type!(
    /// Identifies a canonical [`crate::products::ProductDefinition`].
    ProductDefinitionId
);
id_type!(
    /// Identifies a placed [`crate::products::Occurrence`].
    OccurrenceId
);
id_type!(
    /// Identifies a document-level [`crate::pmi::PmiAnnotation`].
    PmiId
);
id_type!(
    /// Identifies a [`crate::presentation::PresentationLayer`].
    LayerId
);

#[cfg(test)]
mod tests;
