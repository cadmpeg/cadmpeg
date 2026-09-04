// SPDX-License-Identifier: Apache-2.0
//! Sparse document-wide provenance and exactness annotations.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::Arc;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::provenance::{AnnotationProvenance, Exactness};

/// Document-wide provenance and exactness tables keyed by globally unique
/// entity id.
///
/// An entity absent from `exactness` is byte-exact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Annotations {
    streams: Vec<Arc<str>>,
    /// Source location for each annotated entity.
    pub provenance: BTreeMap<String, AnnotationProvenance>,
    /// Non-byte-exact entity or field annotations.
    exactness: BTreeMap<String, ExactnessNote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct AnnotationProvenanceWire {
    stream: u32,
    offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct AnnotationsWire {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    streams: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    provenance: BTreeMap<String, AnnotationProvenanceWire>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    exactness: BTreeMap<String, ExactnessNote>,
}

impl Serialize for Annotations {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut streams = self.streams.clone();
        let mut provenance = BTreeMap::new();
        for (id, location) in &self.provenance {
            let index = streams
                .iter()
                .position(|stream| Arc::ptr_eq(stream, location.stream_ref()))
                .unwrap_or_else(|| {
                    streams.push(location.stream_ref().clone());
                    streams.len() - 1
                });
            let stream = u32::try_from(index).map_err(serde::ser::Error::custom)?;
            provenance.insert(
                id.clone(),
                AnnotationProvenanceWire {
                    stream,
                    offset: location.offset,
                    tag: location.tag.clone(),
                },
            );
        }
        AnnotationsWire {
            streams: streams
                .into_iter()
                .map(|stream| stream.to_string())
                .collect(),
            provenance,
            exactness: self.exactness.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Annotations {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = AnnotationsWire::deserialize(deserializer)?;
        let streams = wire
            .streams
            .into_iter()
            .map(Arc::<str>::from)
            .collect::<Vec<_>>();
        let provenance = wire
            .provenance
            .into_iter()
            .map(|(id, location)| {
                let stream = streams
                    .get(location.stream as usize)
                    .cloned()
                    .ok_or_else(|| {
                        D::Error::custom(format!(
                            "annotation provenance {id:?} references missing stream {}",
                            location.stream
                        ))
                    })?;
                Ok((
                    id,
                    AnnotationProvenance::annotation(stream, location.offset, location.tag),
                ))
            })
            .collect::<Result<_, D::Error>>()?;
        Ok(Self {
            streams,
            provenance,
            exactness: wire.exactness,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Annotations {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Annotations".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        AnnotationsWire::json_schema(generator)
    }
}

/// Exactness for an entity and sparse overrides for its serialized fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactnessNote {
    /// Exactness of the entity except where overridden by `fields`.
    entity: Exactness,
    /// Exactness overrides keyed by serde field path.
    fields: BTreeMap<String, Exactness>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ExactnessNoteWire {
    entity: Exactness,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fields: BTreeMap<String, Exactness>,
}

impl Serialize for ExactnessNote {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ExactnessNoteWire {
            entity: self.entity,
            fields: self.fields.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExactnessNote {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ExactnessNoteWire::deserialize(deserializer)?;
        if wire.entity == Exactness::ByteExact && wire.fields.is_empty() {
            return Err(D::Error::custom(
                "ExactnessNote cannot store the implicit byte-exact default",
            ));
        }
        if let Some(field) = wire
            .fields
            .iter()
            .find_map(|(field, exactness)| (*exactness == wire.entity).then_some(field))
        {
            return Err(D::Error::custom(format!(
                "ExactnessNote.fields[{field:?}] duplicates entity exactness"
            )));
        }
        Ok(Self {
            entity: wire.entity,
            fields: wire.fields,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for ExactnessNote {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ExactnessNote".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ExactnessNoteWire::json_schema(generator)
    }
}

impl ExactnessNote {
    /// Exactness of the entity except where a field override exists.
    pub const fn entity(&self) -> Exactness {
        self.entity
    }

    /// Sparse field overrides whose values differ from entity exactness.
    pub fn fields(&self) -> &BTreeMap<String, Exactness> {
        &self.fields
    }
}

/// Opaque handle for an interned source stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamHandle(u32);

/// Incrementally constructs document annotations while interning stream names.
#[derive(Debug, Default, Clone)]
pub struct AnnotationBuilder {
    annotations: Annotations,
}

impl AnnotationBuilder {
    /// Create an empty annotation builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Continue building an existing annotation set.
    pub fn resume(annotations: Annotations) -> Self {
        Self { annotations }
    }

    /// Intern a source stream name and return its reusable handle.
    pub fn stream(&mut self, stream: impl Into<String>) -> StreamHandle {
        let stream = stream.into();
        if let Some(index) = self
            .annotations
            .streams
            .iter()
            .position(|existing| existing.as_ref() == stream)
        {
            return StreamHandle(
                u32::try_from(index).expect("annotation stream count exceeds u32::MAX"),
            );
        }

        let stream = Arc::<str>::from(stream);
        self.annotations.streams.push(stream);
        StreamHandle(
            u32::try_from(self.annotations.streams.len() - 1)
                .expect("annotation stream count exceeds u32::MAX"),
        )
    }

    /// Record an entity's source location.
    ///
    /// The returned value supports the ergonomic
    /// `builder.note(&id, stream, offset).tag("face")` form.
    pub fn note(
        &mut self,
        id: impl Display,
        stream: StreamHandle,
        offset: u64,
    ) -> ProvenanceNote<'_> {
        let id = id.to_string();
        let stream = self
            .annotations
            .streams
            .get(stream.0 as usize)
            .cloned()
            .expect("stream handle was minted by this annotation builder");
        self.annotations.provenance.insert(
            id.clone(),
            AnnotationProvenance::annotation(stream, offset, None),
        );
        ProvenanceNote {
            provenance: self
                .annotations
                .provenance
                .get_mut(&id)
                .expect("provenance was just inserted"),
        }
    }

    /// Set entity-level exactness. Byte-exact entries are removed to preserve
    /// the table's sparse absent-means-byte-exact representation.
    pub fn exactness(&mut self, id: impl Display, exactness: Exactness) -> &mut Self {
        let id = id.to_string();
        if let Some(note) = self.annotations.exactness.get_mut(&id) {
            note.entity = exactness;
            note.fields.retain(|_, value| *value != exactness);
            if exactness == Exactness::ByteExact && note.fields.is_empty() {
                self.annotations.exactness.remove(&id);
            }
        } else if exactness != Exactness::ByteExact {
            self.annotations.exactness.insert(
                id,
                ExactnessNote {
                    entity: exactness,
                    fields: BTreeMap::new(),
                },
            );
        }
        self
    }

    /// Mark one serialized field as deterministically derived.
    pub fn derived(&mut self, id: impl Display, field: impl Into<String>) -> &mut Self {
        self.field_exactness(id, field, Exactness::Derived)
    }

    /// Set a serialized field's exactness.
    ///
    /// A byte-exact override is omitted because it is already the sparse
    /// default. Empty byte-exact notes are removed.
    pub fn field_exactness(
        &mut self,
        id: impl Display,
        field: impl Into<String>,
        exactness: Exactness,
    ) -> &mut Self {
        let id = id.to_string();
        let field = field.into();
        if exactness == Exactness::ByteExact {
            if let Some(note) = self.annotations.exactness.get_mut(&id) {
                if note.entity == Exactness::ByteExact {
                    note.fields.remove(&field);
                    if note.fields.is_empty() {
                        self.annotations.exactness.remove(&id);
                    }
                } else {
                    note.fields.insert(field, Exactness::ByteExact);
                }
            }
        } else {
            match self.annotations.exactness.entry(id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(ExactnessNote {
                        entity: Exactness::ByteExact,
                        fields: BTreeMap::from([(field, exactness)]),
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let note = entry.get_mut();
                    if exactness == note.entity {
                        note.fields.remove(&field);
                    } else {
                        note.fields.insert(field, exactness);
                    }
                }
            }
        }
        self
    }

    /// Remove every sparse exactness annotation.
    pub fn clear_exactness(&mut self) -> &mut Self {
        self.annotations.exactness.clear();
        self
    }

    /// Retain exactness annotations selected by identity.
    pub fn retain_exactness(&mut self, mut keep: impl FnMut(&str) -> bool) -> &mut Self {
        self.annotations.exactness.retain(|id, _| keep(id));
        self
    }

    /// Replace every exactness identity with a derived identity.
    pub fn map_exactness_ids(&mut self, mut map: impl FnMut(&str) -> String) -> &mut Self {
        self.annotations.exactness = std::mem::take(&mut self.annotations.exactness)
            .into_iter()
            .map(|(id, note)| (map(&id), note))
            .collect();
        self
    }

    /// Remove all annotations for an entity that was removed from the model.
    pub fn remove_entity(&mut self, id: impl Display) {
        let id = id.to_string();
        self.annotations.provenance.remove(&id);
        self.annotations.exactness.remove(&id);
    }

    /// Borrow the annotations built so far.
    pub fn annotations(&self) -> &Annotations {
        &self.annotations
    }

    /// Finish building and return the annotation tables.
    pub fn build(self) -> Annotations {
        self.annotations
    }
}

impl Annotations {
    /// Sparse non-byte-exact annotations keyed by entity identity.
    pub fn exactness(&self) -> &BTreeMap<String, ExactnessNote> {
        &self.exactness
    }

    /// Return the number of streams retained for wire serialization.
    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Append another annotation set without rebasing provenance indices.
    ///
    /// Provenance owns its stream reference. The wire adapter assigns the
    /// corresponding index after the two stream catalogs are combined.
    pub fn append(&mut self, mut other: Self) {
        self.streams.append(&mut other.streams);
        self.provenance.append(&mut other.provenance);
        self.exactness.append(&mut other.exactness);
    }

    /// Merge another annotation set while interning equal stream names.
    pub fn merge_interned(&mut self, mut other: Self) {
        let stream_map = other
            .streams
            .drain(..)
            .map(|source| {
                let target = self
                    .streams
                    .iter()
                    .find(|target| target.as_ref() == source.as_ref())
                    .cloned()
                    .unwrap_or_else(|| {
                        self.streams.push(source.clone());
                        source.clone()
                    });
                (source, target)
            })
            .collect::<Vec<_>>();
        for provenance in other.provenance.values_mut() {
            if let Some((_, target)) = stream_map
                .iter()
                .find(|(source, _)| Arc::ptr_eq(source, provenance.stream_ref()))
            {
                provenance.rebind_stream(target.clone());
            }
        }
        self.provenance.append(&mut other.provenance);
        self.exactness.append(&mut other.exactness);
    }
}

/// In-progress provenance annotation returned by [`AnnotationBuilder::note`].
pub struct ProvenanceNote<'a> {
    provenance: &'a mut AnnotationProvenance,
}

impl ProvenanceNote<'_> {
    /// Attach a source record or class name.
    pub fn tag(self, tag: impl Into<String>) {
        self.provenance.tag = Some(tag.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_interns_streams_and_records_provenance() {
        let mut builder = AnnotationBuilder::new();
        let first = builder.stream("f3d:Breps.BlobParts/body.smbh");
        let second = builder.stream("f3d:Breps.BlobParts/body.smbh");

        assert_eq!(first, second);
        builder.note("f3d:body#0", first, 42).tag("body");

        let annotations = builder.build();
        assert_eq!(annotations.stream_count(), 1);
        let provenance = &annotations.provenance["f3d:body#0"];
        assert_eq!(provenance.stream(), "f3d:Breps.BlobParts/body.smbh");
        assert_eq!(provenance.offset, 42);
        assert_eq!(provenance.tag.as_deref(), Some("body"));
    }

    #[test]
    fn annotation_wire_keeps_stream_indices_at_the_boundary() {
        let mut builder = AnnotationBuilder::new();
        let stream = builder.stream("f3d:Breps.BlobParts/body.smbh");
        builder.note("f3d:body#0", stream, 42).tag("body");
        let annotations = builder.build();

        let value = serde_json::to_value(&annotations).unwrap();
        assert_eq!(value["streams"][0], "f3d:Breps.BlobParts/body.smbh");
        assert_eq!(value["provenance"]["f3d:body#0"]["stream"], 0);
        assert_eq!(
            serde_json::from_value::<Annotations>(value).unwrap(),
            annotations
        );
    }

    #[test]
    fn annotation_wire_rejects_a_dangling_stream_index() {
        let error = serde_json::from_value::<Annotations>(serde_json::json!({
            "streams": ["f3d:native"],
            "provenance": {
                "f3d:body#0": {"stream": 1, "offset": 42}
            }
        }))
        .unwrap_err();

        assert!(error.to_string().contains("references missing stream 1"));
    }

    #[test]
    fn exactness_table_stays_sparse() {
        let mut builder = AnnotationBuilder::new();

        builder
            .derived("f3d:edge#0", "param_range")
            .exactness("f3d:edge#0", Exactness::Inferred);
        builder.field_exactness("f3d:edge#0", "param_range", Exactness::ByteExact);

        let expected_fields = BTreeMap::from([("param_range".to_string(), Exactness::ByteExact)]);
        assert_eq!(
            builder.annotations().exactness["f3d:edge#0"],
            ExactnessNote {
                entity: Exactness::Inferred,
                fields: expected_fields,
            }
        );

        builder.exactness("f3d:edge#0", Exactness::ByteExact);
        assert!(builder.annotations().exactness.is_empty());
    }

    #[test]
    fn exactness_wire_rejects_the_implicit_default_as_an_explicit_note() {
        let error = serde_json::from_value::<ExactnessNote>(serde_json::json!({
            "entity": "byte_exact"
        }))
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("cannot store the implicit byte-exact default"));
    }

    #[test]
    fn exactness_wire_rejects_a_redundant_field_override() {
        let error = serde_json::from_value::<ExactnessNote>(serde_json::json!({
            "entity": "derived",
            "fields": {"geometry": "derived"}
        }))
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("fields[\"geometry\"] duplicates entity exactness"));
    }

    #[test]
    fn removing_an_entity_removes_provenance_and_exactness() {
        let mut builder = AnnotationBuilder::new();
        let stream = builder.stream("catia:e5_0d_03");
        builder.note("catia:e5:curve#0", stream, 42).tag("circle");
        builder.derived("catia:e5:curve#0", "geometry");

        builder.remove_entity("catia:e5:curve#0");

        assert!(!builder
            .annotations()
            .provenance
            .contains_key("catia:e5:curve#0"));
        assert!(!builder
            .annotations()
            .exactness
            .contains_key("catia:e5:curve#0"));
    }
}
