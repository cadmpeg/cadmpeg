// SPDX-License-Identifier: Apache-2.0
//! Source-format namespaces retained outside the format-neutral model.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

mod canon;
pub mod catalogue;
mod replay;

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
    pub count: usize,
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

/// One source-native record with a stable identity and codec-owned fields.
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename = "NativeRecord")]
struct RecordShape<'a> {
    /// Globally unique record identity.
    id: &'a str,
    /// Codec-owned record fields.
    #[serde(flatten)]
    fields: &'a Map<String, Value>,
}

/// One source-native record with a stable identity and codec-owned fields.
///
/// The codec-owned fields are held as the record's canonical JSON text rather
/// than as a parsed [`Value`] tree. A `Value` tree spends a separately
/// allocated map node, key string, and enum cell on every field at every depth,
/// which costs roughly an order of magnitude more memory than the equivalent
/// JSON and scatters it across the heap. Retained source populations reach
/// hundreds of thousands of records carrying deeply nested arrays, so the
/// parsed form is materialized per record on demand and never kept resident.
#[derive(Debug, Clone)]
pub struct NativeRecord {
    /// Globally unique record identity, also the leading `id` member of `json`.
    id: String,
    /// Canonical JSON object text, as produced by [`RecordShape`].
    json: Box<str>,
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
        let id = id.into().into_string();
        fields.remove("id");
        let json = Self::canonical_json(&id, &fields);
        Self { id, json }
    }

    fn require_identity(id: &str) -> Result<(), NativeConvertError> {
        if !crate::ids::is_valid_identity(id) {
            return Err(crate::ids::IdentityError::InvalidId {
                value: id.to_owned(),
            }
            .into());
        }
        Ok(())
    }

    /// Build a record by serializing one codec-owned typed record.
    ///
    /// Streams the record into canonical text — the bytes
    /// [`canonical_json`](Self::canonical_json) renders for the record's
    /// [`Value`] tree — without materializing that tree.
    fn from_typed<T: Serialize>(record: &T) -> Result<Self, NativeConvertError> {
        let canon::Node::Object(mut fields) = record.serialize(canon::CanonValue)? else {
            return Err(NativeConvertError::NonObject);
        };
        let Some(id_json) = fields.remove("id") else {
            return Err(NativeConvertError::MissingId);
        };
        if !id_json.starts_with('"') {
            return Err(NativeConvertError::MissingId);
        }
        let id: String = serde_json::from_str(&id_json)?;
        Self::require_identity(&id)?;
        let mut json = String::with_capacity(
            8 + id_json.len()
                + fields
                    .iter()
                    .map(|(key, value)| key.len() + value.len() + 4)
                    .sum::<usize>(),
        );
        json.push_str("{\"id\":");
        json.push_str(&id_json);
        for (key, value) in &fields {
            json.push(',');
            json.push_str(&serde_json::to_string(key)?);
            json.push(':');
            json.push_str(value);
        }
        json.push('}');
        Ok(Self {
            id,
            json: json.into_boxed_str(),
        })
    }

    /// Globally unique record identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Parse the codec-owned fields, excluding `id`.
    ///
    /// This allocates a fresh [`Value`] tree on every call; read it once and
    /// reuse the map when inspecting more than one field, and prefer
    /// [`field`](Self::field) when one field is all that is wanted.
    #[must_use]
    pub fn fields(&self) -> Map<String, Value> {
        let mut fields: Map<String, Value> =
            serde_json::from_str(&self.json).expect("a native record always holds a JSON object");
        fields.remove("id");
        fields
    }

    /// Parse one codec-owned field.
    ///
    /// Only the named field is materialized; the rest of the record is scanned
    /// past without being built.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<Value> {
        if name == "id" {
            return None;
        }
        replay::field(&self.json, name)
    }

    /// Deserialize the record into a codec-owned typed record.
    fn to_typed<T: DeserializeOwned>(&self) -> Result<T, NativeConvertError> {
        Ok(serde_json::from_str(&self.json)?)
    }

    /// Render `id` and `fields` as canonical record text.
    fn canonical_json(id: &str, fields: &Map<String, Value>) -> Box<str> {
        serde_json::to_string(&RecordShape { id, fields })
            .expect("a JSON object of JSON values always serializes")
            .into_boxed_str()
    }
}

impl PartialEq for NativeRecord {
    fn eq(&self, other: &Self) -> bool {
        self.json == other.json
    }
}

impl Serialize for NativeRecord {
    /// Replays the stored text member for member through `serializer` rather
    /// than splicing it, so the record honours the caller's formatting:
    /// `to_string_pretty` must indent a native record the same way it indents
    /// every other document entity. The text was written by [`RecordShape`] and
    /// is replayed in the order it holds, which is the order that shape emits:
    /// `id` first, then the codec-owned fields by key.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        replay::emit(&self.json, serializer)
    }
}

impl<'de> Deserialize<'de> for NativeRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = Map::<String, Value>::deserialize(deserializer)?;
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
        .map(|record| Ok(NativeRecord::from_typed(&record?)?))
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
        self.arenas.insert(
            name.into(),
            arena_from(records.into_iter().map(Ok::<T, NativeConvertError>))?,
        );
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
            .get(name)
            .into_iter()
            .flatten()
            .map(NativeRecord::to_typed)
    }
}

/// Native records grouped by source-format namespace id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Native(pub BTreeMap<String, NativeNamespace>);

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
                namespace
                    .arenas
                    .iter()
                    .filter(|(_, records)| !records.is_empty())
                    .map(move |(kind, records)| LossCount {
                        format: format.clone(),
                        kind: kind.clone(),
                        count: records.len(),
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
