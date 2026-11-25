// SPDX-License-Identifier: Apache-2.0
//! Source-format namespaces retained outside the format-neutral model.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::text::{CodecId, NonBlankString};
use cadmpeg_core::CodecError;
#[cfg(feature = "schema")]
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

pub mod bytes;
mod canon;
pub mod catalogue;
mod compare;
mod copy;
/// Native enum classification for privacy projection.
pub mod privacy;
pub(crate) mod replay;
pub(crate) mod view;

#[cfg(test)]
pub(crate) fn test_ctx() -> DecodeContext<'static> {
    let arena = Box::leak(Box::new(cadmpeg_core::decode::DecodeArena::new()));
    DecodeContext::from_root_bytes(&[], arena, &cadmpeg_core::decode::DecodePolicy::service())
        .expect("empty test input fits the service policy")
        .0
}

/// Deepest container chain one native record field may hold.
///
/// Every descent of a stored field recurses: a typed or value read of the
/// stored text lifts the parser's limit, [`replay::emit`] starts one parse
/// per container, and `canon::CanonText` enters one frame per container.
/// The bound is therefore stated where a field enters the record, so every
/// reader of a constructed record is already inside it.
/// It bounds stored text in memory and is deeper than any field a CADIR
/// document holds, so every such field is read back. A document holds a
/// native record at its fifth level, and the decode wrapper gives up a record
/// that passes the document's 128 levels (§2.5).
const MAX_NATIVE_NESTING_DEPTH: usize = 256;

/// The text every refusal of [`MAX_NATIVE_NESTING_DEPTH`] carries.
///
/// The write path, the replay path and the constructor all answer to one
/// number, so they say the same thing when they refuse.
fn nests_too_deep_message() -> String {
    format!("native value nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers")
}

/// Measures the deepest container chain inside `value`, returning `None` if it
/// exceeds `limit`.
///
/// The pending containers live in this function's own vector, so measuring a
/// value cannot itself overflow the machine stack. Only containers enter it: a
/// scalar carries no chain, and a key is not a value.
fn max_container_depth(value: &Value, limit: usize) -> Option<usize> {
    let mut pending: Vec<(&Value, usize)> = Vec::new();
    let mut deepest = 0_usize;
    push_container(&mut pending, value, 1);
    while let Some((value, depth)) = pending.pop() {
        if depth > limit {
            return None;
        }
        deepest = deepest.max(depth);
        match value {
            Value::Array(items) => {
                for item in items {
                    push_container(&mut pending, item, depth + 1);
                }
            }
            Value::Object(entries) => {
                for entry in entries.values() {
                    push_container(&mut pending, entry, depth + 1);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Some(deepest)
}

/// Rewrites every negative zero inside `value` as zero: a document holds one
/// zero in every float, a native record's included (§2.5).
fn hold_one_zero(value: &mut Value) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Array(items) => pending.extend(items.iter_mut()),
            Value::Object(entries) => pending.extend(entries.values_mut()),
            Value::Number(number)
                if number
                    .as_f64()
                    .is_some_and(|number| number == 0.0 && number.is_sign_negative()) =>
            {
                *value = Value::from(0.0);
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
}

/// Records `value` at `depth` when it is a container.
fn push_container<'a>(pending: &mut Vec<(&'a Value, usize)>, value: &'a Value, depth: usize) {
    if matches!(value, Value::Array(_) | Value::Object(_)) {
        pending.push((value, depth));
    }
}

/// One non-empty native arena reported as an exporter loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LossCount {
    /// Source-format namespace this arena belongs to.
    pub format: String,
    /// Arena name within that namespace.
    pub kind: String,
    /// Number of records in the arena.
    pub count: NonZeroUsize,
}

/// Conversion failure between codec-owned typed records and generic records.
#[derive(Debug, thiserror::Error)]
pub enum NativeConvertError {
    /// The decode context refused a native record: a session limit, or a
    /// tree it could not parse.
    #[error(transparent)]
    Resource(#[from] cadmpeg_core::CodecError),
    /// A serialized typed record has no string `id` field.
    #[error("native record is missing a string id")]
    MissingId,
    /// A native identity does not follow the entity identity grammar.
    #[error("native record has an invalid identity: {0}")]
    InvalidIdentity(#[from] crate::ids::IdentityError),
    /// A typed record did not serialize as a JSON object.
    #[error("native record did not serialize as an object")]
    NonObject,
    /// A caller-owned field nests deeper than a native record may hold.
    #[error(
        "native record {id}: field {field} nests deeper than {} containers",
        MAX_NATIVE_NESTING_DEPTH
    )]
    FieldNestsTooDeep {
        /// Identity the refused field was offered under.
        id: crate::ids::Identity,
        /// Name of the refused field.
        field: String,
    },
    /// A typed record holds a NaN or infinite number, which a stored record
    /// cannot state.
    #[error("native record field {field} holds a non-finite number")]
    NonFiniteNumber {
        /// Member path of the refused number: keys joined by `.`, sequence
        /// elements written `[index]`.
        field: String,
    },
    /// JSON conversion failed.
    #[error("native record conversion failed: {0}")]
    Serde(#[from] serde_json::Error),
    /// Canonical conversion refused an input member without a JSON parse error.
    #[error("native record conversion failed: {0}")]
    ConversionMessage(String),
    /// A stored record does not satisfy its codec-owned reader.
    #[error("native record {id}: {source}")]
    ReadRecord {
        /// Identity of the refused stored record.
        id: crate::ids::Identity,
        /// Codec-owned field admission error.
        #[source]
        source: serde_json::Error,
    },
    /// A producer's record cannot enter a native arena.
    #[error("native input record at ordinal {ordinal}: {source}")]
    WriteRecord {
        /// Zero-based position in the producer's input iterator.
        ordinal: usize,
        /// Record conversion error before an identity is necessarily available.
        #[source]
        source: Box<NativeConvertError>,
    },
    /// An arena name holds no character that is not white space.
    #[error("native arena name {0:?} is blank")]
    BlankArena(String),
    /// A named arena contains a refused record.
    #[error("native arena {arena}: {source}")]
    Arena {
        /// Owning arena name.
        arena: String,
        /// Located record conversion error.
        #[source]
        source: Box<NativeConvertError>,
    },
    /// A typed child record references no record in its owning arena.
    #[error("native record has an invalid owner: {0}")]
    InvalidOwner(String),
    /// A native collection violates its codec-owned admission contract.
    #[error("native collection is invalid: {0}")]
    InvalidCollection(String),
    /// A source-independent unknown record has no retained source counterpart.
    #[error("native unknown record has no retained source record: {0}")]
    MissingRetainedSourceRecord(String),
    /// A native namespace is named by text that is not a codec id (§2.2).
    #[error("native namespace {0:?} is not a codec id")]
    NamespaceNotCodecId(String),
}

/// The codec id that names the native namespace `format`.
pub(crate) fn namespace_codec(format: &str) -> Result<CodecId, NativeConvertError> {
    CodecId::new(format).ok_or_else(|| NativeConvertError::NamespaceNotCodecId(format.to_owned()))
}

impl From<NativeConvertError> for cadmpeg_core::CodecError {
    fn from(error: NativeConvertError) -> Self {
        if let Some(limit) = error.resource_limit() {
            Self::ResourceLimit(limit)
        } else {
            Self::Malformed(error.to_string())
        }
    }
}

impl NativeConvertError {
    fn resource_limit(&self) -> Option<cadmpeg_core::decode::ResourceLimit> {
        match self {
            Self::Resource(cadmpeg_core::CodecError::ResourceLimit(limit)) => Some(*limit),
            Self::WriteRecord { source, .. } | Self::Arena { source, .. } => {
                source.resource_limit()
            }
            _ => None,
        }
    }
}

/// Schema descriptor for a native record's identity and open field map.
#[derive(Serialize)]
#[cfg(feature = "schema")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename = "NativeRecord")]
struct RecordShape<'a> {
    /// Globally unique record identity.
    id: crate::ids::Identity,
    /// Codec-owned record fields.
    #[serde(flatten)]
    fields: &'a Map<String, Value>,
}

/// One native record field a producer states without nesting it.
///
/// A record assembled from these cannot reach [`MAX_NATIVE_NESTING_DEPTH`], so
/// [`NativeRecord::from_identity`] admits them with nothing to measure and
/// nothing to refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeField {
    /// A JSON string.
    Text(String),
    /// A JSON array of strings.
    TextList(Vec<String>),
}

impl NativeField {
    /// This field as the value a record stores.
    fn into_value(self) -> Value {
        match self {
            Self::Text(text) => Value::String(text),
            Self::TextList(items) => Value::Array(items.into_iter().map(Value::String).collect()),
        }
    }
}

/// One source-native record with a stable identity and codec-owned fields.
///
/// Every field is at most [`MAX_NATIVE_NESTING_DEPTH`] containers deep: the
/// four ways to build one are [`NativeRecord::new`], which measures the
/// caller's map, `Deserialize`, which calls it, [`NativeNamespace::set_arena`],
/// whose serializer counts the containers it enters, and
/// [`NativeRecord::from_identity`], whose fields cannot nest. Readers of a
/// stored record therefore descend a bounded value and measure nothing.
///
/// The record holds its canonical compact JSON text — `id` first, then the
/// codec-owned fields by key — and not a parsed [`Value`] tree. A `Value`
/// tree spends a separately allocated map node, key string and enum cell on
/// every member at every depth, an order of magnitude more than the text.
/// Decoded files retain hundreds of thousands of native records with nested
/// arrays, so a reader parses the members it needs on demand.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeRecord {
    /// Globally unique record identity, serialized as the leading `id` member.
    id: crate::ids::Identity,
    /// Canonical compact JSON object text: `id` first, then the codec-owned
    /// fields in key order.
    json: Box<str>,
    /// Deepest container nesting in `json`, the outer record object included.
    /// Bounded by `MAX_NATIVE_NESTING_DEPTH + 1` (257).
    depth: usize,
}

impl NativeRecord {
    /// Build a record from a stable identity and an arbitrary field map.
    ///
    /// Any `id` member of `fields` is dropped in favour of `id`.
    /// The identity is checked before this call; document validation checks uniqueness.
    /// A field nested past [`MAX_NATIVE_NESTING_DEPTH`] is refused by name:
    /// the map is the caller's own, so nothing about it is bounded until it
    /// is measured here.
    pub fn new(
        id: crate::ids::Identity,
        mut fields: Map<String, Value>,
    ) -> Result<Self, NativeConvertError> {
        fields.remove("id");
        let mut max_field_depth = 0_usize;
        for (field, value) in &fields {
            let Some(field_depth) = max_container_depth(value, MAX_NATIVE_NESTING_DEPTH) else {
                return Err(NativeConvertError::FieldNestsTooDeep {
                    id,
                    field: field.clone(),
                });
            };
            max_field_depth = max_field_depth.max(field_depth);
        }
        fields.values_mut().for_each(hold_one_zero);
        let json = render(&id, &fields);
        let depth = 1 + max_field_depth;
        Ok(Self { id, json, depth })
    }

    /// Build a record from an admitted identity and fields that do not nest.
    ///
    /// A `NativeField` states its own shape, so this path has nothing to
    /// measure and no failing branch. An `id` entry is dropped in favour of
    /// `id`, and a repeated name keeps the last entry.
    pub fn from_identity(
        id: impl Into<crate::ids::Identity>,
        fields: impl IntoIterator<Item = (String, NativeField)>,
    ) -> Self {
        let id = id.into();
        let mut stored = Map::new();
        let mut has_list = false;
        for (name, value) in fields {
            if name != "id" {
                has_list |= matches!(value, NativeField::TextList(_));
                stored.insert(name, value.into_value());
            }
        }
        let json = render(&id, &stored);
        let depth = if has_list { 2 } else { 1 };
        Self { id, json, depth }
    }

    /// Build a record by serializing one codec-owned typed record.
    ///
    /// The canonical serializer admits what the plain value serializer does
    /// not: a NaN or infinite number is refused by its member path, object
    /// keys must be distinct, and a `RawValue` payload is read through
    /// one-container replay rather than a recursion-limited parse.
    #[cfg(test)]
    pub(crate) fn from_typed<T: Serialize>(record: &T) -> Result<Self, NativeConvertError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        // Format-boundary fixtures admit the record root and every allowed field container.
        policy.limits.max_recursion_depth = u64_from_index(MAX_NATIVE_NESTING_DEPTH + 1);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        Self::from_typed_for_decode(&ctx, record)
    }

    /// Build a record the decoded document keeps, retaining its text.
    fn from_typed_for_decode<T: Serialize>(
        ctx: &DecodeContext<'_>,
        record: &T,
    ) -> Result<Self, NativeConvertError> {
        let record = Self::canonical(ctx, record)?;
        ctx.retain_bytes(u64_from_index(record.json.len()), "serialize native record")?;
        Ok(record)
    }

    /// Build a record under the caller's depth account without retaining it.
    fn canonical<T: Serialize>(
        ctx: &DecodeContext<'_>,
        record: &T,
    ) -> Result<Self, NativeConvertError> {
        use std::cell::RefCell;
        thread_local! {
            static CANON_SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        }
        let mut fallback = Vec::new();
        CANON_SCRATCH.with(|scratch| {
            let mut borrowed = scratch.try_borrow_mut();
            let out: &mut Vec<u8> = match &mut borrowed {
                Ok(guard) => {
                    guard.clear();
                    guard
                }
                Err(_) => &mut fallback,
            };
            let mut min_remaining = MAX_NATIVE_NESTING_DEPTH + 1;
            let serialized = record
                .serialize(canon::CanonText::for_record_tracking(
                    ctx,
                    out,
                    &mut min_remaining,
                ))
                .map_err(|error| error.into_native(ctx))?;
            let canon::Written::Object(members) = serialized else {
                return Err(NativeConvertError::NonObject);
            };
            let (id, json) = members.into_record(out)?;
            if out.capacity() > 64 * 1024 {
                *out = Vec::new();
            }
            let depth = (MAX_NATIVE_NESTING_DEPTH + 1) - min_remaining;
            Ok(Self { id, json, depth })
        })
    }

    pub(crate) fn digest_unknown(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let fields = self.fields()?;
        for key in fields.keys() {
            if key != "links" {
                return Err(CodecError::malformed(format_args!(
                    "native unknown {} has unexpected field {key}",
                    self.id
                )));
            }
        }

        let links = match fields.get("links") {
            None => &[][..],
            Some(Value::Array(links)) => links.as_slice(),
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "native unknown {} links is not a sequence",
                    self.id
                )))
            }
        };
        for value in links {
            let Value::String(link) = value else {
                return Err(CodecError::malformed(format_args!(
                    "native unknown {} link is not text",
                    self.id
                )));
            };

            if !crate::ids::is_valid_identity(link) {
                return Err(CodecError::malformed(format_args!(
                    "native unknown {} has invalid link {link}",
                    self.id
                )));
            }
        }
        Self::canonical(
            ctx,
            &DigestUnknown {
                id: self.id.as_str(),
                links,
            },
        )
        .map_err(CodecError::from)
    }

    /// Globally unique record identity.
    #[must_use]
    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    /// Length of the stored canonical record text.
    pub(crate) fn text_len(&self) -> usize {
        self.json.len()
    }

    /// Borrow the stored canonical compact JSON object text.
    pub(crate) fn json_text(&self) -> &str {
        &self.json
    }

    /// Parse the codec-owned fields, excluding `id`.
    ///
    /// Each call builds a new [`Value`] tree from the stored text. Read one
    /// member with [`field`](Self::field).
    /// Invalid stored text returns a conversion error naming this record.
    pub fn fields(&self) -> Result<Map<String, Value>, NativeConvertError> {
        let value = replay::value(&self.json).map_err(|source| NativeConvertError::ReadRecord {
            id: self.id.clone(),
            source,
        })?;
        let Value::Object(mut fields) = value else {
            return Err(NativeConvertError::NonObject);
        };
        fields.remove("id");
        Ok(fields)
    }

    /// Copy codec-owned fields under the caller's depth account.
    pub fn copy_fields(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Map<String, Value>, NativeConvertError> {
        Ok(copy::fields(ctx, &self.fields()?)?)
    }

    /// Rewrite native identity fields through their typed owner's field walk.
    pub fn rewrite_fields<T, F>(
        &self,
        ctx: &DecodeContext<'_>,
        map: F,
    ) -> Result<Map<String, Value>, NativeConvertError>
    where
        T: crate::schema::rewrite::typed::RewriteIdentities,
        F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
    {
        let mut fields = self.fields()?;
        fields.insert("id".to_owned(), Value::String(self.id.as_str().to_owned()));
        let mut value = Value::Object(fields);
        let mut identities = crate::schema::rewrite::typed::IdentityMap::new(map);
        T::rewrite_native_value(ctx, &mut value, &mut identities)?;
        identities.finish()?;
        let Value::Object(mut fields) = value else {
            return Err(
                cadmpeg_core::CodecError::malformed("native record must be an object").into(),
            );
        };
        fields.remove("id");
        Ok(fields)
    }

    /// Parse one codec-owned field.
    ///
    /// Only the named member is built; the other members are scanned past.
    /// `id` is not a codec-owned field and is never answered here.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<Value> {
        if name == "id" || !self.may_hold_field(name) {
            return None;
        }
        replay::value(replay::member(&self.json, name)?.get()).ok()
    }

    /// Cheap text check before parsing `self.json` for `name`: canonical
    /// compact JSON renders any unescaped ASCII key literally as `"name":`.
    fn may_hold_field(&self, name: &str) -> bool {
        let plain_ascii = name
            .bytes()
            .all(|byte| (0x20..=0x7e).contains(&byte) && byte != b'"' && byte != b'\\');
        if !plain_ascii {
            return true;
        }
        if name.len() <= 64 {
            let mut needle = [0_u8; 67];
            needle[0] = b'"';
            needle[1..=name.len()].copy_from_slice(name.as_bytes());
            needle[name.len() + 1] = b'"';
            needle[name.len() + 2] = b':';
            if let Ok(needle) = std::str::from_utf8(&needle[..name.len() + 3]) {
                return self.json.contains(needle);
            }
        }
        self.json.contains(name)
    }

    /// Build a canonical record directly from pre-formatted canonical compact
    /// JSON text (`{"id":...}` with `id` first, remaining keys sorted, and a
    /// float zero written `0.0`, never `-0.0`, per §2.5) of
    /// known container nesting `depth` (`>= 1`, `<= MAX_NATIVE_NESTING_DEPTH + 1`).
    ///
    /// Enforces identity validation, caller depth admission, and retained-byte
    /// admission identically to [`Self::from_typed_for_decode`].
    #[inline]
    pub fn from_canonical_flat(
        ctx: &DecodeContext<'_>,
        id: &str,
        json: String,
        depth: usize,
    ) -> Result<Self, NativeConvertError> {
        let id = crate::ids::Identity::new(id).map_err(NativeConvertError::InvalidIdentity)?;
        if depth == 0 || depth > MAX_NATIVE_NESTING_DEPTH + 1 {
            return Err(NativeConvertError::NonObject);
        }
        check_depth(ctx, depth)?;
        ctx.retain_bytes(u64_from_index(json.len()), "serialize native record")?;
        Ok(Self {
            id,
            json: json.into_boxed_str(),
            depth,
        })
    }

    /// Deserialize the codec-owned record straight from its stored text.
    ///
    /// The read holds one frame of the caller's depth account per container
    /// level of the stored text, the record's own object included. A refused
    /// field states the message without the text position the parser appends.
    fn to_typed<T: DeserializeOwned>(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<T, NativeConvertError> {
        check_depth(ctx, self.depth)?;
        // The writer bounded each field's nesting, so the parser's recursion
        // limit is lifted.
        let mut deserializer = serde_json::Deserializer::from_str(&self.json);
        deserializer.disable_recursion_limit();
        let value = T::deserialize(&mut deserializer)
            .and_then(|value| deserializer.end().map(|()| value))
            .map_err(|source| NativeConvertError::ReadRecord {
                id: self.id.clone(),
                source: without_position(source),
            })?;
        privacy::observe::<T>(self.id(), &self.json);
        Ok(value)
    }

    /// Try a caller-provided fast parser on the canonical JSON text, falling
    /// back to [`Self::to_typed`] when it returns `None`.
    fn to_typed_fast<T: DeserializeOwned>(
        &self,
        ctx: &DecodeContext<'_>,
        fast: impl FnOnce(&str) -> Option<T>,
    ) -> Result<T, NativeConvertError> {
        check_depth(ctx, self.depth)?;
        if let Some(value) = fast(&self.json) {
            privacy::observe::<T>(self.id(), &self.json);
            return Ok(value);
        }
        let mut deserializer = serde_json::Deserializer::from_str(&self.json);
        deserializer.disable_recursion_limit();
        let value = T::deserialize(&mut deserializer)
            .and_then(|value| deserializer.end().map(|()| value))
            .map_err(|source| NativeConvertError::ReadRecord {
                id: self.id.clone(),
                source: without_position(source),
            })?;
        privacy::observe::<T>(self.id(), &self.json);
        Ok(value)
    }

    /// Compare a typed serialization against this record's stored text
    /// without parsing its payload.
    /// Finite-number, distinct-key, depth and caller resource admission still apply.
    pub fn matches_typed_for_decode<T: Serialize + ?Sized>(
        &self,
        ctx: &DecodeContext<'_>,
        expected: &T,
    ) -> Result<bool, NativeConvertError> {
        compare::matches(ctx, self, expected)
    }
}

#[derive(Serialize)]
struct DigestUnknown<'a> {
    id: &'a str,
    links: &'a [Value],
}

/// The refusal without the text position a parse appends to its message.
fn without_position(error: serde_json::Error) -> serde_json::Error {
    if error.line() == 0 {
        return error;
    }
    let suffix = format!(" at line {} column {}", error.line(), error.column());
    let message = error.to_string();
    match message.strip_suffix(&suffix) {
        Some(message) => <serde_json::Error as serde::de::Error>::custom(message),
        None => error,
    }
}

/// Enter and release `depth` nesting frames on `ctx` without heap allocation
/// for common shallow records.
#[inline]
fn check_depth(ctx: &DecodeContext<'_>, depth: usize) -> Result<(), NativeConvertError> {
    match depth {
        0 => Ok(()),
        1 => {
            let _g1 = ctx.enter_nested(canon::WORK)?;
            Ok(())
        }
        2 => {
            let _g1 = ctx.enter_nested(canon::WORK)?;
            let _g2 = ctx.enter_nested(canon::WORK)?;
            Ok(())
        }
        3 => {
            let _g1 = ctx.enter_nested(canon::WORK)?;
            let _g2 = ctx.enter_nested(canon::WORK)?;
            let _g3 = ctx.enter_nested(canon::WORK)?;
            Ok(())
        }
        4 => {
            let _g1 = ctx.enter_nested(canon::WORK)?;
            let _g2 = ctx.enter_nested(canon::WORK)?;
            let _g3 = ctx.enter_nested(canon::WORK)?;
            let _g4 = ctx.enter_nested(canon::WORK)?;
            Ok(())
        }
        _ => {
            let mut frames = Vec::with_capacity(depth);
            for _ in 0..depth {
                frames.push(ctx.enter_nested(canon::WORK)?);
            }
            Ok(())
        }
    }
}

/// Append `text` as a quoted JSON string literal to `out`.
#[inline]
pub fn push_json_str(out: &mut String, text: &str) {
    let bytes = text.as_bytes();
    if !bytes.iter().any(|&b| b < 0x20 || b == b'"' || b == b'\\') {
        out.push('"');
        out.push_str(text);
        out.push('"');
        return;
    }
    push_json_str_escaped(out, text);
}

#[cold]
#[inline(never)]
fn push_json_str_escaped(out: &mut String, text: &str) {
    out.push('"');
    let bytes = text.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let esc = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            0x08 => "\\b",
            0x0c => "\\f",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0x00..=0x1f => {
                out.push_str(&text[start..i]);
                let hi = b >> 4;
                let lo = b & 0x0f;
                let hex = |n: u8| char::from(if n < 10 { b'0' + n } else { b'a' + (n - 10) });
                out.push_str("\\u00");
                out.push(hex(hi));
                out.push(hex(lo));
                start = i + 1;
                continue;
            }
            _ => continue,
        };
        out.push_str(&text[start..i]);
        out.push_str(esc);
        start = i + 1;
    }
    out.push_str(&text[start..]);
    out.push('"');
}

/// Adds an arena name to a semantic typed-record refusal.
fn read_record<T: DeserializeOwned>(
    ctx: &DecodeContext<'_>,
    arena: &str,
    record: &NativeRecord,
) -> Result<T, NativeConvertError> {
    match record.to_typed(ctx) {
        Ok(value) => Ok(value),
        Err(source) if source.resource_limit().is_some() => Err(source),
        Err(source) => {
            let arena = arena.to_owned();

            Err(NativeConvertError::Arena {
                arena,
                source: Box::new(source),
            })
        }
    }
}

/// Adds an arena name to a semantic typed-record refusal, using `fast` when it
/// matches the canonical JSON shape.
fn read_record_fast<T: DeserializeOwned>(
    ctx: &DecodeContext<'_>,
    arena: &str,
    record: &NativeRecord,
    fast: impl FnOnce(&str) -> Option<T>,
) -> Result<T, NativeConvertError> {
    match record.to_typed_fast(ctx, fast) {
        Ok(value) => Ok(value),
        Err(source) if source.resource_limit().is_some() => Err(source),
        Err(source) => {
            let arena = arena.to_owned();

            Err(NativeConvertError::Arena {
                arena,
                source: Box::new(source),
            })
        }
    }
}

impl Serialize for NativeRecord {
    /// Writes the record member for member through `serializer`, so it honours
    /// the caller's formatting: `to_string_pretty` must indent a native record
    /// the same way it indents every other document entity. It writes the
    /// constructors' order: `id` first, then the codec-owned fields by key.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        replay::transcode(&self.json, serializer)
    }
}

/// Canonical compact text of one record, with `id` first and fields by key.
/// JSON values render into strings without a fallible serializer or writer.
fn render(id: &crate::ids::Identity, fields: &Map<String, Value>) -> Box<str> {
    let mut json = String::from("{\"id\":");
    json.push_str(&Value::String(id.as_str().to_owned()).to_string());
    for (key, value) in fields {
        json.push(',');
        json.push_str(&Value::String(key.clone()).to_string());
        json.push(':');
        json.push_str(&value.to_string());
    }
    json.push('}');
    json.into_boxed_str()
}

impl<'de> Deserialize<'de> for NativeRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = cadmpeg_core::distinct_keys::json_object(deserializer)?;
        let Some(Value::String(id)) = fields.remove("id") else {
            return Err(<D::Error as serde::de::Error>::custom(
                NativeConvertError::MissingId,
            ));
        };
        Self::new(
            crate::ids::Identity::new(id).map_err(serde::de::Error::custom)?,
            fields,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for NativeRecord {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "NativeRecord".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::NativeRecord").into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        RecordShape::json_schema(generator)
    }
}

/// Serialize codec-owned typed records into one arena in canonical record order.
///
/// The arena is returned rather than stored, so a caller that only needs the
/// canonical records never has to read them back out of a namespace. Each
/// record is converted before the next is read, and a record the source could
/// not state stops the walk with that record's own error. Equal identities can
/// occur before document validation, so ordering preserves their input order.
pub fn arena_from<T, E, I>(ctx: &DecodeContext<'_>, records: I) -> Result<Vec<NativeRecord>, E>
where
    T: Serialize,
    E: From<NativeConvertError>,
    I: IntoIterator<Item = Result<T, E>>,
{
    let mut converted = Vec::new();
    for (ordinal, record) in records.into_iter().enumerate() {
        let record = record?;
        let record = match NativeRecord::from_typed_for_decode(ctx, &record) {
            Ok(record) => record,
            Err(source) if source.resource_limit().is_some() => return Err(E::from(source)),
            Err(source) => {
                return Err(E::from(NativeConvertError::WriteRecord {
                    ordinal,
                    source: Box::new(source),
                }));
            }
        };
        converted.push(record);
    }
    // A stable sort keeps equal identities in input order.
    converted.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(converted)
}

/// The arena key `name` spells, refused when it is blank.
fn arena_name(name: &str) -> Result<NonBlankString, NativeConvertError> {
    NonBlankString::new(name).ok_or_else(|| NativeConvertError::BlankArena(name.to_owned()))
}

/// Source-format arena collection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NativeNamespace {
    /// Record arenas keyed by stable arena name.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    arenas: BTreeMap<NonBlankString, Vec<NativeRecord>>,
}

impl NativeNamespace {
    /// Admit codec-owned aggregate state from this raw namespace.
    pub fn admit<'a, T>(&'a self) -> Result<T, NativeConvertError>
    where
        T: TryFrom<&'a Self, Error = NativeConvertError>,
    {
        T::try_from(self)
    }

    /// Return the record arenas keyed by stable arena name.
    #[must_use]
    pub fn arenas(&self) -> &BTreeMap<NonBlankString, Vec<NativeRecord>> {
        &self.arenas
    }

    /// Return the record arenas for modification.
    pub fn arenas_mut(&mut self) -> &mut BTreeMap<NonBlankString, Vec<NativeRecord>> {
        &mut self.arenas
    }

    /// Replace an arena by serializing codec-owned typed records.
    pub fn set_arena<T: Serialize>(
        &mut self,
        ctx: &DecodeContext<'_>,
        name: impl AsRef<str>,
        records: &[T],
    ) -> Result<(), NativeConvertError> {
        self.set_arena_from(ctx, name, records.iter())
    }

    /// Replace an arena by serializing codec-owned typed records one at a time.
    ///
    /// Codecs whose typed records must be reshaped before storage should build
    /// the reshaped record inside the iterator rather than collecting a full
    /// second copy of the population first.
    pub fn set_arena_from<T: Serialize, I: IntoIterator<Item = T>>(
        &mut self,
        ctx: &DecodeContext<'_>,
        name: impl AsRef<str>,
        records: I,
    ) -> Result<(), NativeConvertError> {
        let name = arena_name(name.as_ref())?;

        let converted = match arena_from(ctx, records.into_iter().map(Ok::<T, NativeConvertError>))
        {
            Ok(converted) => converted,
            Err(source) if source.resource_limit().is_some() => return Err(source),
            Err(source) => {
                return Err(NativeConvertError::Arena {
                    arena: name.into_string(),
                    source: Box::new(source),
                });
            }
        };

        self.arenas.insert(name, converted);
        Ok(())
    }

    /// Replace an arena with pre-built canonical [`NativeRecord`]s, sorted in
    /// canonical identity order (preserving input order for equal identities).
    pub fn set_arena_records(
        &mut self,
        name: impl AsRef<str>,
        mut records: Vec<NativeRecord>,
    ) -> Result<(), NativeConvertError> {
        let name = arena_name(name.as_ref())?;
        records.sort_by(|left, right| left.id().cmp(right.id()));
        self.arenas.insert(name, records);
        Ok(())
    }

    /// Admit an arena through a codec-owned collection constructor.
    pub fn arena_as_collection<T, C>(&self, name: &str) -> Result<C, NativeConvertError>
    where
        T: DeserializeOwned,
        C: TryFrom<Vec<T>, Error = NativeConvertError>,
    {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        self.arena_as_collection_for_decode(&ctx, name)
    }

    /// Admits a typed collection under the caller's decode session limits.
    pub fn arena_as_collection_for_decode<T, C>(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
    ) -> Result<C, NativeConvertError>
    where
        T: DeserializeOwned,
        C: TryFrom<Vec<T>, Error = NativeConvertError>,
    {
        C::try_from(self.arena_as_for_decode(ctx, name)?)
    }

    /// Deserialize an arena into codec-owned typed records.
    pub fn arena_as<T: DeserializeOwned>(&self, name: &str) -> Result<Vec<T>, NativeConvertError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        self.arena_as_for_decode(&ctx, name)
    }

    /// Deserialize one arena with admission before each retained typed copy.
    pub fn arena_as_for_decode<T: DeserializeOwned>(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
    ) -> Result<Vec<T>, NativeConvertError> {
        let mut typed = Vec::new();
        if let Some((arena, records)) = self.arenas.get_key_value(name) {
            for record in records {
                typed.push(read_record(ctx, arena.as_str(), record)?);
            }
        }
        Ok(typed)
    }

    /// Deserialize an arena into codec-owned typed records one at a time.
    ///
    /// A record is parsed only when the iterator is advanced onto it, so a
    /// caller that consumes and releases each one — reshaping an arena, or
    /// reducing it for hashing — never holds the typed population alongside
    /// whatever it produces from it.
    pub fn arena_iter_as<'a, T: DeserializeOwned + 'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = Result<T, NativeConvertError>> + 'a {
        self.arenas
            .get_key_value(name)
            .into_iter()
            .flat_map(|(name, records)| {
                records.iter().map(move |record| {
                    let arena = cadmpeg_core::decode::DecodeArena::new();
                    let policy = cadmpeg_core::decode::DecodePolicy::default();
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    read_record(&ctx, name.as_str(), record)
                })
            })
    }

    /// Reads typed records lazily under the caller's decode session limits.
    pub fn arena_iter_as_for_decode<'a, T: DeserializeOwned + 'a>(
        &'a self,
        ctx: &'a DecodeContext<'_>,
        name: &str,
    ) -> impl Iterator<Item = Result<T, NativeConvertError>> + 'a {
        self.arenas
            .get_key_value(name)
            .into_iter()
            .flat_map(move |(arena, records)| {
                records
                    .iter()
                    .map(move |record| read_record(ctx, arena.as_str(), record))
            })
    }

    /// Reads typed records lazily under the caller's decode session limits,
    /// trying `fast` on each record's canonical JSON text before falling back
    /// to `serde_json`.
    pub fn arena_iter_as_for_decode_fast<'a, T, F>(
        &'a self,
        ctx: &'a DecodeContext<'_>,
        name: &str,
        fast: F,
    ) -> impl Iterator<Item = Result<T, NativeConvertError>> + 'a
    where
        T: DeserializeOwned + 'a,
        F: Fn(&str) -> Option<T> + Copy + 'a,
    {
        self.arenas
            .get_key_value(name)
            .into_iter()
            .flat_map(move |(arena, records)| {
                records
                    .iter()
                    .map(move |record| read_record_fast(ctx, arena.as_str(), record, fast))
            })
    }
}

/// Native records grouped by the codec id of their namespace.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Native(
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub  BTreeMap<CodecId, NativeNamespace>,
);

impl Native {
    /// Return a source-format namespace.
    pub fn namespace(&self, format: &str) -> Option<&NativeNamespace> {
        self.0.get(format)
    }

    /// Return or create a source-format namespace.
    pub fn namespace_mut(&mut self, format: CodecId) -> &mut NativeNamespace {
        self.0.entry(format).or_default()
    }

    /// Whether every namespace arena is in increasing order of id.
    pub(crate) fn is_finalized(&self) -> bool {
        self.0.values().all(|namespace| {
            namespace
                .arenas
                .values()
                .all(|records| records.windows(2).all(|pair| pair[0].id() <= pair[1].id()))
        })
    }

    /// The first arena, as `<codec>.<arena>`, whose records do not state
    /// their ids in strictly increasing order.
    pub(crate) fn unordered_arena(&self) -> Option<String> {
        self.0.iter().find_map(|(format, namespace)| {
            namespace.arenas.iter().find_map(|(arena, records)| {
                (!records.windows(2).all(|pair| pair[0].id() < pair[1].id()))
                    .then(|| format!("{format}.{arena}"))
            })
        })
    }

    /// States one record of each id in each sorted namespace arena whose
    /// copies hold one value, and returns each id whose copies differ, all of
    /// whose copies it keeps.
    pub(crate) fn repeated_ids(&mut self) -> Vec<crate::document::RepeatedId> {
        let mut repeated = Vec::new();
        for (format, namespace) in &mut self.0 {
            for (arena, records) in &mut namespace.arenas {
                crate::document::drop_repeated_copies(
                    records,
                    NativeRecord::id,
                    &crate::give_up::Arena::Native {
                        codec: format.as_str().to_owned(),
                        arena: arena.as_str().to_owned(),
                    },
                    &mut repeated,
                );
            }
        }
        repeated
    }

    /// The id of the record at `index` of `codec`'s arena `arena`.
    pub(crate) fn id_at(&self, codec: &str, arena: &str, index: usize) -> Option<String> {
        let records = self.0.get(codec)?.arenas.get(arena)?;
        records.get(index).map(|record| record.id().to_owned())
    }

    /// The canonical JSON text of the records of `id` in `codec`'s arena
    /// `arena`, or `None` where the arena holds none.
    pub(crate) fn record_text(&self, codec: &str, arena: &str, id: &str) -> Option<String> {
        let copies: Vec<&NativeRecord> = self
            .0
            .get(codec)?
            .arenas
            .get(arena)?
            .iter()
            .filter(|record| record.id() == id)
            .collect();
        (!copies.is_empty()).then(|| crate::give_up::record_text(&copies))
    }

    /// Removes every record of `id` from `codec`'s arena `arena`, returning
    /// the number removed.
    pub(crate) fn remove_records(&mut self, codec: &str, arena: &str, id: &str) -> usize {
        let Some(records) = self
            .0
            .get_mut(codec)
            .and_then(|namespace| namespace.arenas.get_mut(arena))
        else {
            return 0;
        };
        let before = records.len();
        records.retain(|record| record.id() != id);
        before - records.len()
    }

    /// Counts each namespace record in `references`.
    pub(crate) fn hold_ids(&self, references: &mut crate::give_up::References) {
        for record in self
            .0
            .values()
            .flat_map(|namespace| namespace.arenas.values().flatten())
        {
            references.hold(record.id());
        }
    }

    /// Adds the names each namespace record holds to `references`, other
    /// than the `unknowns` stubs, whose links are a set (§3.3).
    pub(crate) fn add_holders(&self, references: &mut crate::give_up::References) {
        for (codec, namespace) in &self.0 {
            for (arena, records) in &namespace.arenas {
                if arena.as_str() == "unknowns" {
                    continue;
                }
                let arena = crate::give_up::Arena::Native {
                    codec: codec.as_str().to_owned(),
                    arena: arena.as_str().to_owned(),
                };
                for record in records {
                    references.add(&arena, record.id(), record);
                }
            }
        }
    }

    /// The records of `id` in the native arena `arena`, which is in identity
    /// order.
    fn records(&self, arena: &crate::give_up::Arena, id: &str) -> &[NativeRecord] {
        let crate::give_up::Arena::Native { codec, arena } = arena else {
            return &[];
        };
        let Some(records) = self
            .0
            .get(codec.as_str())
            .and_then(|namespace| namespace.arenas.get(arena.as_str()))
        else {
            return &[];
        };
        let start = records.partition_point(|record| record.id() < id);
        let length = records[start..]
            .iter()
            .take_while(|record| record.id() == id)
            .count();
        &records[start..start + length]
    }

    /// The holder the records of `id` in the native arena `arena` are, where
    /// they name an id of `gone`.
    pub(crate) fn holder(
        &self,
        arena: &crate::give_up::Arena,
        id: &str,
        gone: &std::collections::BTreeSet<String>,
    ) -> Option<crate::give_up::Holder> {
        let names: Vec<(String, String)> = self
            .records(arena, id)
            .iter()
            .filter_map(|record| {
                crate::give_up::Holder::of(arena, id, record, |text| {
                    gone.get(text).map(String::as_str)
                })
            })
            .flat_map(|holder| holder.names)
            .collect();
        (!names.is_empty()).then(|| crate::give_up::Holder {
            arena: arena.clone(),
            id: id.to_owned(),
            names,
        })
    }

    /// Removes every record of the native arena `arena` whose id is in
    /// `ids`, returning the number removed of each id.
    pub(crate) fn remove_ids(
        &mut self,
        arena: &crate::give_up::Arena,
        ids: &std::collections::BTreeSet<String>,
    ) -> BTreeMap<String, usize> {
        let mut removed = BTreeMap::new();
        let crate::give_up::Arena::Native { codec, arena } = arena else {
            return removed;
        };
        let Some(records) = self
            .0
            .get_mut(codec.as_str())
            .and_then(|namespace| namespace.arenas.get_mut(arena.as_str()))
        else {
            return removed;
        };
        records.retain(|record| {
            if ids.contains(record.id()) {
                *removed.entry(record.id().to_owned()).or_insert(0) += 1;
                false
            } else {
                true
            }
        });
        removed
    }

    /// Sort every arena into canonical identity order.
    pub(crate) fn finalize(&mut self) {
        for namespace in self.0.values_mut() {
            for records in namespace.arenas.values_mut() {
                records.sort_by(|left, right| left.id().cmp(right.id()));
            }
        }
    }

    /// Return one count for each non-empty native arena.
    pub fn loss_counts(&self) -> Vec<LossCount> {
        self.0
            .iter()
            .flat_map(|(format, namespace)| {
                namespace.arenas.iter().filter_map(move |(kind, records)| {
                    let count = NonZeroUsize::new(records.len())?;
                    Some(LossCount {
                        format: format.to_string(),
                        kind: kind.as_str().to_owned(),
                        count,
                    })
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
