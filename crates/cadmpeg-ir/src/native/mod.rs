// SPDX-License-Identifier: Apache-2.0
//! Source-format namespaces retained outside the format-neutral model.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// One non-empty native arena reported as an exporter loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    /// A typed record did not serialize as a JSON object.
    #[error("native record did not serialize as an object")]
    NonObject,
    /// JSON conversion failed.
    #[error("native record conversion failed: {0}")]
    Serde(#[from] serde_json::Error),
    /// A typed child record references no record in its owning arena.
    #[error("native record has an invalid owner: {0}")]
    InvalidOwner(String),
    /// A source-independent unknown record has no retained source counterpart.
    #[error("native unknown record has no retained source record: {0}")]
    MissingRetainedSourceRecord(String),
}

// The wire and schema shape of a native record: `id`, then the codec-owned
// fields in the order `Map` iterates. `NativeRecord` both stores and emits
// exactly what this produces, so one type fixes the document shape instead of
// every path that builds, serializes, or describes a record. Its doc comments
// are the ones that reach the generated JSON Schema.
/// One source-native record with a stable identity and codec-owned fields.
#[derive(Serialize, JsonSchema)]
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
    #[must_use]
    pub fn new(id: impl Into<String>, mut fields: Map<String, Value>) -> Self {
        let id = id.into();
        fields.remove("id");
        let json = Self::canonical_json(&id, &fields);
        Self { id, json }
    }

    /// Build a record by serializing one codec-owned typed record.
    fn from_typed<T: Serialize>(record: &T) -> Result<Self, NativeConvertError> {
        let Value::Object(mut fields) = serde_json::to_value(record)? else {
            return Err(NativeConvertError::NonObject);
        };
        let Some(Value::String(id)) = fields.remove("id") else {
            return Err(NativeConvertError::MissingId);
        };
        let json = Self::canonical_json(&id, &fields);
        Ok(Self { id, json })
    }

    /// Globally unique record identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Parse the codec-owned fields, excluding `id`.
    ///
    /// This allocates a fresh [`Value`] tree on every call; read it once and
    /// reuse the map when inspecting more than one field.
    #[must_use]
    pub fn fields(&self) -> Map<String, Value> {
        let mut fields = self.parsed();
        fields.remove("id");
        fields
    }

    /// Parse one codec-owned field.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<Value> {
        if name == "id" {
            return None;
        }
        self.parsed().remove(name)
    }

    /// Deserialize the record into a codec-owned typed record.
    fn to_typed<T: DeserializeOwned>(&self) -> Result<T, NativeConvertError> {
        Ok(serde_json::from_str(&self.json)?)
    }

    /// Parse the stored text into the whole field map, `id` included.
    fn parsed(&self) -> Map<String, Value> {
        serde_json::from_str(&self.json).expect("a native record always holds a JSON object")
    }

    /// Render `id` and `fields` as canonical record text.
    fn canonical_json(id: &str, fields: &Map<String, Value>) -> Box<str> {
        serde_json::to_string(&RecordShape { id, fields })
            .expect("a JSON object of JSON values always serializes")
            .into_boxed_str()
    }
}

/// Two records are equal when their canonical texts are equal. Canonicalizing
/// on every construction makes that the same relation as comparing the parsed
/// identity and field map.
impl PartialEq for NativeRecord {
    fn eq(&self, other: &Self) -> bool {
        self.json == other.json
    }
}

impl Serialize for NativeRecord {
    /// Emits through `serializer` rather than splicing the stored text, so the
    /// record honours the caller's formatting: `to_string_pretty` must indent a
    /// native record the same way it indents every other document entity.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = self.fields();
        RecordShape {
            id: &self.id,
            fields: &fields,
        }
        .serialize(serializer)
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
        let json = Self::canonical_json(&id, &fields);
        Ok(Self { id, json })
    }
}

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

/// Independently versioned source-format arena collection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeNamespace {
    /// Codec-owned namespace schema version.
    pub version: u32,
    /// Record arenas keyed by stable arena name.
    #[serde(default)]
    pub arenas: BTreeMap<String, Vec<NativeRecord>>,
}

impl NativeNamespace {
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
        let mut converted = records
            .into_iter()
            .map(|record| NativeRecord::from_typed(&record))
            .collect::<Result<Vec<_>, NativeConvertError>>()?;
        converted.sort_by(|left, right| left.id.cmp(&right.id));
        self.arenas.insert(name.into(), converted);
        Ok(())
    }

    /// Deserialize an arena into codec-owned typed records.
    pub fn arena_as<T: DeserializeOwned>(&self, name: &str) -> Result<Vec<T>, NativeConvertError> {
        self.arenas
            .get(name)
            .into_iter()
            .flatten()
            .map(NativeRecord::to_typed)
            .collect()
    }
}

/// Native records grouped by source-format namespace id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
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
                records.sort_by(|left, right| left.id.cmp(&right.id));
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
