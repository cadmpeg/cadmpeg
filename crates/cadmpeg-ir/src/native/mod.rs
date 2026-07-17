// SPDX-License-Identifier: Apache-2.0
//! Source-format namespaces retained outside the format-neutral model.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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

/// One source-native record with a stable identity and codec-owned fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Codec-owned record fields.
    #[serde(flatten)]
    pub fields: Map<String, Value>,
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
        let mut converted = Vec::with_capacity(records.len());
        for record in records {
            let Value::Object(mut fields) = serde_json::to_value(record)? else {
                return Err(NativeConvertError::NonObject);
            };
            let Some(Value::String(id)) = fields.remove("id") else {
                return Err(NativeConvertError::MissingId);
            };
            converted.push(NativeRecord { id, fields });
        }
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
            .map(|record| {
                let mut fields = record.fields.clone();
                fields.insert("id".into(), Value::String(record.id.clone()));
                Ok(serde_json::from_value(Value::Object(fields))?)
            })
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
