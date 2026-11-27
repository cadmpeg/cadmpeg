// SPDX-License-Identifier: Apache-2.0
//! The reader that `#[derive(cadmpeg_derive::DeserializeTagged)]` gives an
//! enum tagged by serde's `tag`, or by `tag` and `content`.
//!
//! serde's derived reader of such an enum keeps the value in its private
//! `Content` buffer and reads the payload from the buffer, so each type that
//! the payload reaches is compiled once more against the buffer's
//! deserializer. This reader writes the value as compact JSON text through
//! the deserializer that it gets, and reads the payload from that text with
//! `serde_json`. Each payload type is then compiled against one deserializer,
//! the one that reads CADIR text.
//!
//! The reader calls the deserializer as serde's reader does: the same method
//! for the value, for each key, and for the tag, in the same order. The
//! deserializer walks each value that the text holds, as it walks each value
//! that serde buffers, so its depth limit refuses at the same container. The
//! payload's text is read with no depth limit of its own. The reader thus
//! accepts the documents that serde's reader accepts and refuses the others.
//! A refusal inside the payload carries no position in the payload's text:
//! the parser that reads the enclosing value states its own position.

use std::fmt::{self, Write as _};

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, Error as _, IgnoredAny,
    MapAccess, SeqAccess, Unexpected, VariantAccess, Visitor,
};
use serde_json::de::StrRead;

/// A tag as the deserializer states it, before it names a variant.
#[derive(Debug, Clone, Copy)]
pub enum Tag<'a> {
    /// A string.
    Str(&'a str),
    /// A byte string.
    Bytes(&'a [u8]),
    /// A variant index.
    Index(u64),
}

impl Tag<'_> {
    /// Reads the variant that the tag names into `T`, an enum that derives
    /// serde's `Deserialize` with `#[serde(variant_identifier)]`.
    ///
    /// # Errors
    ///
    /// Returns serde's message for a tag that names no variant.
    pub fn read<T: DeserializeOwned>(self) -> Result<T, String> {
        let read = match self {
            Self::Str(name) => {
                T::deserialize(de::value::StrDeserializer::<serde_json::Error>::new(name))
            }
            Self::Bytes(name) => {
                T::deserialize(de::value::BytesDeserializer::<serde_json::Error>::new(name))
            }
            Self::Index(index) => {
                T::deserialize(de::value::U64Deserializer::<serde_json::Error>::new(index))
            }
        };
        read.map_err(|error| error.to_string())
    }
}

/// One step of reading an enum tagged by `tag` and `content`.
#[derive(Debug, Clone, Copy)]
pub enum Event<'a> {
    /// The tag, read where serde reads it.
    Tag(Tag<'a>),
    /// The text of the content, or `None` for an object that states the tag
    /// and no content. It follows the tag.
    Content(Option<&'a str>),
}

/// The form of an enum tagged by `tag` and `content`.
#[derive(Debug, Clone, Copy)]
pub struct Adjacent {
    /// The enum's Rust name, which the messages state.
    pub rust_name: &'static str,
    /// The name of the enum that the reader gives to `deserialize_struct`.
    pub name: &'static str,
    /// The key of the tag.
    pub tag: &'static str,
    /// The key of the content.
    pub content: &'static str,
    /// The tag's key and the content's key, in this order.
    pub fields: &'static [&'static str],
    /// Whether a key other than the tag and the content is refused. When it
    /// is not refused, its value is skipped.
    pub deny_unknown_fields: bool,
    /// What the reader expects, as the messages state it.
    pub expecting: &'static str,
}

/// Reads an enum tagged by `tag` alone, as serde does: an object whose key
/// `tag` names the variant, or an array whose first element names it.
/// `read_tag` reads the tag where serde reads it. Returns the text of the
/// other members, as an object, or of the other elements, as an array.
///
/// # Errors
///
/// Returns the deserializer's refusal, and the refusal of `read_tag`.
pub fn internally<'de, D: Deserializer<'de>>(
    deserializer: D,
    tag: &'static str,
    expecting: &'static str,
    read_tag: &mut dyn FnMut(Tag<'_>) -> Result<(), String>,
) -> Result<String, D::Error> {
    deserializer.deserialize_any(Internal {
        tag,
        expecting,
        read_tag,
    })
}

/// Reads an enum tagged by `tag` and `content`, as serde does. `read` gets
/// the tag where serde reads it and then the content, and reads the payload
/// where serde reads it.
///
/// # Errors
///
/// Returns the deserializer's refusal, and the refusal of `read`.
pub fn adjacently<'de, D: Deserializer<'de>>(
    deserializer: D,
    form: &Adjacent,
    read: &mut dyn FnMut(Event<'_>) -> Result<(), String>,
) -> Result<(), D::Error> {
    deserializer.deserialize_struct(form.name, form.fields, AdjacentVisitor { form, read })
}

/// The deserializer of a payload's text. The deserializer that wrote the
/// text walked every value in it under its own depth limit, so this one has
/// none.
pub fn payload(text: &str) -> serde_json::Deserializer<StrRead<'_>> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.disable_recursion_limit();
    deserializer
}

/// Completes the read of a payload: refuses text after the value, and
/// states a refusal without its position in the payload's text.
///
/// # Errors
///
/// Returns the refusal of `read`, or of text after the value.
pub fn end<T>(
    read: Result<T, serde_json::Error>,
    mut deserializer: serde_json::Deserializer<StrRead<'_>>,
) -> Result<T, String> {
    read.and_then(|value| deserializer.end().map(|()| value))
        .map_err(|error| crate::serde_erase::without_position(error).to_string())
}

/// Reads `T` from the text of a payload.
///
/// # Errors
///
/// Returns the refusal without its position in `text`.
pub fn read<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    crate::serde_erase::from_json_str(text).map_err(|error| error.to_string())
}

/// Reads the struct of a struct variant of an enum tagged by `tag` and
/// `content`. serde reads that struct from an object alone.
///
/// # Errors
///
/// Refuses an array, as serde does, and returns the struct's refusal without
/// its position in `text`.
pub fn read_object<T: DeserializeOwned>(text: &str, expecting: &str) -> Result<T, String> {
    if text.starts_with('[') {
        return Err(serde_json::Error::invalid_type(Unexpected::Seq, &expecting).to_string());
    }
    read(text)
}

/// Reads the content of a unit variant of an enum tagged by `tag` and
/// `content`: `null`, as serde reads it.
///
/// # Errors
///
/// Refuses any other value.
pub fn read_unit(text: &str, expecting: &str) -> Result<(), String> {
    let mut deserializer = payload(text);
    end(
        Deserializer::deserialize_any(&mut deserializer, UnitContent { expecting }),
        deserializer,
    )
}

/// Reads `T` for a newtype variant whose content the object does not state,
/// as serde does: an `Option` reads as `None`, and any other type refuses
/// the missing `field`.
///
/// # Errors
///
/// Returns the refusal of the missing field.
pub fn missing<T: DeserializeOwned>(field: &'static str) -> Result<T, String> {
    T::deserialize(Missing(field)).map_err(|error| error.to_string())
}

/// serde's message for the missing `field`.
pub fn missing_field(field: &'static str) -> String {
    serde_json::Error::missing_field(field).to_string()
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Writes `text` as a JSON string.
fn quote(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0}'..='\u{1f}' => {
                let mut bytes = [0; 4];
                let code = character.encode_utf8(&mut bytes).as_bytes()[0];
                out.push_str("\\u00");
                out.push(char::from(HEX[usize::from(code >> 4)]));
                out.push(char::from(HEX[usize::from(code & 0x0f)]));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Writes one value as JSON text, after `separator`.
struct Capture<'o> {
    out: &'o mut String,
    separator: Option<char>,
}

impl<'de> DeserializeSeed<'de> for Capture<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        if let Some(separator) = self.separator {
            self.out.push(separator);
        }
        deserializer.deserialize_any(Text(self.out))
    }
}

/// Writes one value as JSON text and returns the text.
struct Captured;

impl<'de> DeserializeSeed<'de> for Captured {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        let mut out = String::new();
        Capture {
            out: &mut out,
            separator: None,
        }
        .deserialize(deserializer)?;
        Ok(out)
    }
}

struct Text<'o>(&'o mut String);

impl<'de> Visitor<'de> for Text<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<(), E> {
        self.0.push_str(if value { "true" } else { "false" });
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<(), E> {
        write!(self.0, "{value}").map_err(E::custom)
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<(), E> {
        write!(self.0, "{value}").map_err(E::custom)
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<(), E> {
        // `serde_json` prints a float with a point or an exponent, so the
        // text reads back as the same float.
        match serde_json::Number::from_f64(value) {
            Some(number) => write!(self.0, "{number}").map_err(E::custom),
            None => Err(E::invalid_value(Unexpected::Float(value), &self)),
        }
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        quote(self.0, value);
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        self.0.push_str("null");
        Ok(())
    }

    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        self.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let out = self.0;
        out.push('[');
        let mut separator = None;
        while seq
            .next_element_seed(Capture {
                out: &mut *out,
                separator,
            })?
            .is_some()
        {
            separator = Some(',');
        }
        out.push(']');
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let out = self.0;
        out.push('{');
        let mut separator = None;
        while map
            .next_key_seed(Key {
                out: &mut *out,
                separator,
            })?
            .is_some()
        {
            map.next_value_seed(Capture {
                out: &mut *out,
                separator: Some(':'),
            })?;
            separator = Some(',');
        }
        out.push('}');
        Ok(())
    }
}

/// Writes one object key as a JSON string, after `separator`.
struct Key<'o> {
    out: &'o mut String,
    separator: Option<char>,
}

impl<'de> DeserializeSeed<'de> for Key<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        if let Some(separator) = self.separator {
            self.out.push(separator);
        }
        deserializer.deserialize_any(KeyText(self.out))
    }
}

struct KeyText<'o>(&'o mut String);

impl Visitor<'_> for KeyText<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string key")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        quote(self.0, value);
        Ok(())
    }
}

/// Reads a tag with `deserialize_identifier`, as serde reads a variant's
/// identifier, and hands it to the reader of the tag.
struct Identifier<'r>(&'r mut dyn FnMut(Tag<'_>) -> Result<(), String>);

impl<'de> DeserializeSeed<'de> for Identifier<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_identifier(self)
    }
}

impl Visitor<'_> for Identifier<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("variant identifier")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<(), E> {
        (self.0)(Tag::Index(value)).map_err(E::custom)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        (self.0)(Tag::Str(value)).map_err(E::custom)
    }

    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<(), E> {
        (self.0)(Tag::Bytes(value)).map_err(E::custom)
    }
}

/// Reads the tag of an enum tagged by `tag` and `content` with
/// `deserialize_enum`, as serde reads it: a variant name with no payload.
struct EnumTag<'r> {
    rust_name: &'static str,
    read_tag: &'r mut dyn FnMut(Tag<'_>) -> Result<(), String>,
}

impl<'de> DeserializeSeed<'de> for EnumTag<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_enum(self.rust_name, &[], self)
    }
}

impl<'de> Visitor<'de> for EnumTag<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "variant of enum {}", self.rust_name)
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<(), A::Error> {
        let ((), variant) = data.variant_seed(Identifier(self.read_tag))?;
        variant.unit_variant()
    }
}

/// A key of an object read as an enum tagged by `tag` alone.
enum Member {
    Tag,
    Other,
}

/// Reads a key as serde reads it with `deserialize_any`. It writes a key
/// other than the tag, after `separator`.
struct MemberKey<'o> {
    tag: &'static str,
    out: &'o mut String,
    separator: Option<char>,
}

impl<'de> DeserializeSeed<'de> for MemberKey<'_> {
    type Value = Member;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Member, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl Visitor<'_> for MemberKey<'_> {
    type Value = Member;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a type tag `{}` or any other value", self.tag)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Member, E> {
        if value == self.tag {
            return Ok(Member::Tag);
        }
        if let Some(separator) = self.separator {
            self.out.push(separator);
        }
        quote(self.out, value);
        Ok(Member::Other)
    }

    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<Member, E> {
        if value == self.tag.as_bytes() {
            return Ok(Member::Tag);
        }
        Err(E::invalid_type(Unexpected::Bytes(value), &self))
    }
}

struct Internal<'r> {
    tag: &'static str,
    expecting: &'static str,
    read_tag: &'r mut dyn FnMut(Tag<'_>) -> Result<(), String>,
}

impl<'de> Visitor<'de> for Internal<'_> {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<String, A::Error> {
        let Self { tag, read_tag, .. } = self;
        let mut out = String::from("{");
        let mut tagged = false;
        let mut separator = None;
        while let Some(member) = map.next_key_seed(MemberKey {
            tag,
            out: &mut out,
            separator,
        })? {
            match member {
                Member::Tag => {
                    if tagged {
                        return Err(A::Error::duplicate_field(tag));
                    }
                    map.next_value_seed(Identifier(&mut *read_tag))?;
                    tagged = true;
                }
                Member::Other => {
                    map.next_value_seed(Capture {
                        out: &mut out,
                        separator: Some(':'),
                    })?;
                    separator = Some(',');
                }
            }
        }
        if !tagged {
            return Err(A::Error::missing_field(tag));
        }
        out.push('}');
        Ok(out)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<String, A::Error> {
        let Self { tag, read_tag, .. } = self;
        if seq.next_element_seed(Identifier(read_tag))?.is_none() {
            return Err(A::Error::missing_field(tag));
        }
        let mut out = String::from("[");
        let mut separator = None;
        while seq
            .next_element_seed(Capture {
                out: &mut out,
                separator,
            })?
            .is_some()
        {
            separator = Some(',');
        }
        out.push(']');
        Ok(out)
    }
}

/// A key of an object read as an enum tagged by `tag` and `content` that
/// the reader reads.
enum Field {
    Tag,
    Content,
}

/// Reads a key with `deserialize_identifier`, as serde does. `None` is a key
/// that the reader skips.
struct FieldKey<'f>(&'f Adjacent);

impl<'de> DeserializeSeed<'de> for FieldKey<'_> {
    type Value = Option<Field>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Option<Field>, D::Error> {
        deserializer.deserialize_identifier(self)
    }
}

impl FieldKey<'_> {
    fn field(&self, value: &[u8]) -> Option<Field> {
        if value == self.0.tag.as_bytes() {
            Some(Field::Tag)
        } else if value == self.0.content.as_bytes() {
            Some(Field::Content)
        } else {
            None
        }
    }
}

impl Visitor<'_> for FieldKey<'_> {
    type Value = Option<Field>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Adjacent { tag, content, .. } = self.0;
        if self.0.deny_unknown_fields {
            write!(formatter, "{tag:?} or {content:?}")
        } else {
            write!(formatter, "{tag:?}, {content:?}, or other ignored fields")
        }
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Option<Field>, E> {
        match value {
            0 => Ok(Some(Field::Tag)),
            1 => Ok(Some(Field::Content)),
            _ if self.0.deny_unknown_fields => {
                Err(E::invalid_value(Unexpected::Unsigned(value), &self))
            }
            _ => Ok(None),
        }
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Option<Field>, E> {
        match self.field(value.as_bytes()) {
            None if self.0.deny_unknown_fields => {
                Err(E::invalid_value(Unexpected::Str(value), &self))
            }
            field => Ok(field),
        }
    }

    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<Option<Field>, E> {
        match self.field(value) {
            None if self.0.deny_unknown_fields => {
                Err(E::invalid_value(Unexpected::Bytes(value), &self))
            }
            field => Ok(field),
        }
    }
}

/// The next key that the reader reads. A key that it skips has its value
/// skipped.
fn next_field<'de, A: MapAccess<'de>>(
    map: &mut A,
    form: &Adjacent,
) -> Result<Option<Field>, A::Error> {
    while let Some(key) = map.next_key_seed(FieldKey(form))? {
        match key {
            Some(field) => return Ok(Some(field)),
            None => {
                map.next_value::<IgnoredAny>()?;
            }
        }
    }
    Ok(None)
}

/// Refuses a key that restates the tag or the content after both are read.
fn rest<'de, A: MapAccess<'de>>(map: &mut A, form: &Adjacent) -> Result<(), A::Error> {
    match next_field(map, form)? {
        Some(Field::Tag) => Err(A::Error::duplicate_field(form.tag)),
        Some(Field::Content) => Err(A::Error::duplicate_field(form.content)),
        None => Ok(()),
    }
}

struct AdjacentVisitor<'f, 'r> {
    form: &'f Adjacent,
    read: &'r mut dyn FnMut(Event<'_>) -> Result<(), String>,
}

impl<'de> Visitor<'de> for AdjacentVisitor<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.form.expecting)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let Self { form, read } = self;
        match next_field(&mut map, form)? {
            None => Err(A::Error::missing_field(form.tag)),
            Some(Field::Tag) => {
                map.next_value_seed(EnumTag {
                    rust_name: form.rust_name,
                    read_tag: &mut |name: Tag<'_>| read(Event::Tag(name)),
                })?;
                match next_field(&mut map, form)? {
                    Some(Field::Tag) => Err(A::Error::duplicate_field(form.tag)),
                    Some(Field::Content) => {
                        let text = map.next_value_seed(Captured)?;
                        read(Event::Content(Some(&text))).map_err(A::Error::custom)?;
                        rest(&mut map, form)
                    }
                    None => read(Event::Content(None)).map_err(A::Error::custom),
                }
            }
            Some(Field::Content) => {
                let text = map.next_value_seed(Captured)?;
                match next_field(&mut map, form)? {
                    Some(Field::Tag) => {
                        map.next_value_seed(EnumTag {
                            rust_name: form.rust_name,
                            read_tag: &mut |name: Tag<'_>| read(Event::Tag(name)),
                        })?;
                        read(Event::Content(Some(&text))).map_err(A::Error::custom)?;
                        rest(&mut map, form)
                    }
                    Some(Field::Content) => Err(A::Error::duplicate_field(form.content)),
                    None => Err(A::Error::missing_field(form.tag)),
                }
            }
        }
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let Self { form, read } = self;
        let tagged =
            seq.next_element_seed(Identifier(&mut |name: Tag<'_>| read(Event::Tag(name))))?;
        if tagged.is_none() {
            return Err(A::Error::invalid_length(0, &form.expecting));
        }
        match seq.next_element_seed(Captured)? {
            Some(text) => read(Event::Content(Some(&text))).map_err(A::Error::custom),
            None => Err(A::Error::invalid_length(1, &form.expecting)),
        }
    }
}

/// Reads the content of a unit variant: `null` alone.
struct UnitContent<'e> {
    expecting: &'e str,
}

impl Visitor<'_> for UnitContent<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
}

/// The deserializer of a field that the object does not state, as serde's.
struct Missing(&'static str);

impl<'de> Deserializer<'de> for Missing {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(serde_json::Error::missing_field(self.0))
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_none()
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_any(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_any(visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map identifier ignored_any
    }
}

#[cfg(test)]
mod tests;
