// SPDX-License-Identifier: Apache-2.0
//! Source-format namespaces retained outside the format-neutral model.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

#[cfg(feature = "schema")]
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

mod canon;
pub mod catalogue;
mod replay;

/// Deepest container chain one native record field may hold.
///
/// [`NativeRecord::new`] and [`NativeRecord::from_identity`] take a
/// caller-owned `Map`, so a field tree is not limited by the parse that would
/// otherwise have produced it. Reading one back descends it:
/// `serde_json::from_value` carries no recursion counter, and [`replay::emit`]
/// starts one parse per container. Both refuse a field nested past this bound.
/// It is twice the 128 containers a `serde_json` text parse admits, so every
/// field a CADIR document can state is read back.
const MAX_NATIVE_NESTING_DEPTH: usize = 256;

/// States whether `value` holds a container chain longer than `limit`.
///
/// The pending containers live in this function's own vector, so measuring a
/// value cannot itself overflow the machine stack. Only containers enter it: a
/// scalar carries no chain, and a key is not a value.
fn nests_past(value: &Value, limit: usize) -> bool {
    let mut pending: Vec<(&Value, usize)> = Vec::new();
    push_container(&mut pending, value, 1);
    while let Some((value, depth)) = pending.pop() {
        if depth > limit {
            return true;
        }
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
    false
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
    /// A serialized typed record has no string `id` field.
    #[error("native record is missing a string id")]
    MissingId,
    /// A native identity does not follow the entity identity grammar.
    #[error("native record has an invalid identity: {0}")]
    InvalidIdentity(#[from] crate::ids::IdentityError),
    /// A typed record did not serialize as a JSON object.
    #[error("native record did not serialize as an object")]
    NonObject,
    /// JSON conversion failed.
    #[error("native record conversion failed: {0}")]
    Serde(#[from] serde_json::Error),
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
}

impl From<NativeConvertError> for cadmpeg_core::CodecError {
    fn from(error: NativeConvertError) -> Self {
        Self::Malformed(error.to_string())
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

/// One source-native record with a stable identity and codec-owned fields.
///
/// The record holds the value it was constructed from. `serde_json::Map` is
/// key-sorted, so the stored map is already in canonical order and the record
/// renders its canonical text — `id` first, then the other keys sorted — on
/// demand through [`Serialize`]. Reading a field is a map lookup and has no
/// failing branch.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeRecord {
    /// Globally unique record identity, serialized as the leading `id` member.
    id: crate::ids::Identity,
    /// Codec-owned fields in canonical key order, never holding `id`.
    fields: Map<String, Value>,
}

impl NativeRecord {
    /// Build a record from a stable identity and its codec-owned fields.
    ///
    /// Any `id` member of `fields` is dropped in favour of `id`.
    /// Identity syntax is checked here; document validation checks uniqueness.
    pub fn new(
        id: impl Into<String>,
        fields: Map<String, Value>,
    ) -> Result<Self, NativeConvertError> {
        Ok(Self::from_identity(crate::ids::Identity::new(id)?, fields))
    }

    /// Build a record from an admitted identity and codec-owned fields.
    pub fn from_identity(
        id: impl Into<crate::ids::Identity>,
        mut fields: Map<String, Value>,
    ) -> Self {
        let id = id.into();
        fields.remove("id");
        Self { id, fields }
    }

    /// Build a record by serializing one codec-owned typed record.
    ///
    /// The canonical serializer admits what the plain value serializer does
    /// not: object keys must be distinct, and a `RawValue` payload is read
    /// through one-container replay rather than a recursion-limited parse.
    fn from_typed<T: Serialize>(record: &T) -> Result<Self, NativeConvertError> {
        let canon::Node::Object(mut fields) = record.serialize(canon::CanonValue)? else {
            return Err(NativeConvertError::NonObject);
        };
        let Some(Value::String(id)) = fields.remove("id") else {
            return Err(NativeConvertError::MissingId);
        };
        Ok(Self {
            id: crate::ids::Identity::new(id)?,
            fields,
        })
    }

    /// Globally unique record identity.
    #[must_use]
    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    /// The codec-owned fields, excluding `id`.
    ///
    /// This clones the stored map; read it once and reuse it when inspecting
    /// more than one field, and prefer [`field`](Self::field) when one field is
    /// all that is wanted.
    #[must_use]
    pub fn fields(&self) -> Map<String, Value> {
        self.fields.clone()
    }

    /// One codec-owned field.
    ///
    /// `id` is not a codec-owned field and is never answered here.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<Value> {
        self.fields.get(name).cloned()
    }

    /// Deserialize the record into a codec-owned typed record.
    ///
    /// `serde_json::from_value` descends the value with no recursion counter,
    /// so the fields are measured first: a field nested past
    /// [`MAX_NATIVE_NESTING_DEPTH`] is refused by name instead of driving the
    /// descent.
    fn to_typed<T: DeserializeOwned>(&self) -> Result<T, NativeConvertError> {
        for (name, value) in &self.fields {
            if nests_past(value, MAX_NATIVE_NESTING_DEPTH) {
                return Err(NativeConvertError::ReadRecord {
                    id: self.id.clone(),
                    source: <serde_json::Error as serde::de::Error>::custom(format!(
                        "field {name} nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers"
                    )),
                });
            }
        }
        let mut record = self.fields.clone();
        record.insert("id".to_owned(), Value::String(self.id.as_str().to_owned()));
        serde_json::from_value(Value::Object(record)).map_err(|source| {
            NativeConvertError::ReadRecord {
                id: self.id.clone(),
                source,
            }
        })
    }
}

impl Serialize for NativeRecord {
    /// Writes the record member for member through `serializer`, so it honours
    /// the caller's formatting: `to_string_pretty` must indent a native record
    /// the same way it indents every other document entity. It writes the
    /// constructors' order: `id` first, then the codec-owned fields by key.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.fields.len() + 1))?;
        serde::ser::SerializeMap::serialize_entry(&mut map, "id", self.id.as_str())?;
        for (key, value) in &self.fields {
            serde::ser::SerializeMap::serialize_entry(&mut map, key, value)?;
        }
        serde::ser::SerializeMap::end(map)
    }
}

impl<'de> Deserialize<'de> for NativeRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = cadmpeg_core::distinct_keys::json_object(deserializer)?;
        let Some(Value::String(id)) = fields.remove("id") else {
            return Err(<D::Error as serde::de::Error>::custom(
                NativeConvertError::MissingId,
            ));
        };
        Self::new(id, fields).map_err(serde::de::Error::custom)
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
/// not state stops the walk with that record's own error.
pub fn arena_from<T, E, I>(records: I) -> Result<Vec<NativeRecord>, E>
where
    T: Serialize,
    E: From<NativeConvertError>,
    I: IntoIterator<Item = Result<T, E>>,
{
    let mut converted = records
        .into_iter()
        .enumerate()
        .map(|(ordinal, record)| {
            let record = record?;
            NativeRecord::from_typed(&record).map_err(|source| {
                E::from(NativeConvertError::WriteRecord {
                    ordinal,
                    source: Box::new(source),
                })
            })
        })
        .collect::<Result<Vec<_>, E>>()?;
    converted.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(converted)
}

/// Source-format arena collection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NativeNamespace {
    /// Record arenas keyed by stable arena name.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    arenas: BTreeMap<String, Vec<NativeRecord>>,
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
    pub fn arenas(&self) -> &BTreeMap<String, Vec<NativeRecord>> {
        &self.arenas
    }

    /// Return the record arenas for modification.
    pub fn arenas_mut(&mut self) -> &mut BTreeMap<String, Vec<NativeRecord>> {
        &mut self.arenas
    }

    /// Replace an arena by serializing codec-owned typed records.
    pub fn set_arena<T: Serialize>(
        &mut self,
        name: impl Into<String>,
        records: &[T],
    ) -> Result<(), NativeConvertError> {
        self.set_arena_from(name, records.iter())
    }

    /// Replace an arena by serializing codec-owned typed records one at a time.
    ///
    /// Codecs whose typed records must be reshaped before storage should build
    /// the reshaped record inside the iterator rather than collecting a full
    /// second copy of the population first.
    pub fn set_arena_from<T: Serialize, I: IntoIterator<Item = T>>(
        &mut self,
        name: impl Into<String>,
        records: I,
    ) -> Result<(), NativeConvertError> {
        let name = name.into();
        let converted =
            arena_from(records.into_iter().map(Ok::<T, NativeConvertError>)).map_err(|source| {
                NativeConvertError::Arena {
                    arena: name.clone(),
                    source: Box::new(source),
                }
            })?;
        self.arenas.insert(name, converted);
        Ok(())
    }

    /// Admit an arena through a codec-owned collection constructor.
    pub fn arena_as_collection<T, C>(&self, name: &str) -> Result<C, NativeConvertError>
    where
        T: DeserializeOwned,
        C: TryFrom<Vec<T>, Error = NativeConvertError>,
    {
        C::try_from(self.arena_as(name)?)
    }

    /// Deserialize an arena into codec-owned typed records.
    pub fn arena_as<T: DeserializeOwned>(&self, name: &str) -> Result<Vec<T>, NativeConvertError> {
        self.arena_iter_as(name).collect()
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
            .flat_map(|(arena, records)| {
                records.iter().map(move |record| {
                    record
                        .to_typed()
                        .map_err(|source| NativeConvertError::Arena {
                            arena: arena.clone(),
                            source: Box::new(source),
                        })
                })
            })
    }
}

/// Native records grouped by source-format namespace id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Native(
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub  BTreeMap<String, NativeNamespace>,
);

impl Native {
    /// Return a source-format namespace.
    pub fn namespace(&self, format: &str) -> Option<&NativeNamespace> {
        self.0.get(format)
    }

    /// Return or create a source-format namespace.
    pub fn namespace_mut(&mut self, format: impl Into<String>) -> &mut NativeNamespace {
        self.0.entry(format.into()).or_default()
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
                        format: format.clone(),
                        kind: kind.clone(),
                        count,
                    })
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
