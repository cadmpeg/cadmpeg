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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Annotations {
    /// Source location for each annotated entity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, AnnotationProvenance>,
    /// Non-byte-exact entity or field annotations.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    exactness: BTreeMap<String, ExactnessNote>,
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
        if wire.fields.contains_key("") {
            return Err(D::Error::custom(
                "ExactnessNote.fields keys must not be empty",
            ));
        }
        if wire.entity == Exactness::ByteExact && wire.fields.is_empty() {
            return Err(D::Error::custom(
                "ExactnessNote cannot store the implicit byte-exact default",
            ));
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

    /// Explicit field exactness notes.
    pub fn fields(&self) -> &BTreeMap<String, Exactness> {
        &self.fields
    }
}

/// Opaque handle for an interned source stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamHandle(Arc<str>);

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

    /// Name a source stream and return its reusable handle.
    pub fn stream(&mut self, stream: impl Into<String>) -> StreamHandle {
        StreamHandle(Arc::<str>::from(stream.into()))
    }

    /// Record an entity's source location.
    ///
    /// The returned value supports the ergonomic
    /// `builder.note(&id, &stream, offset).tag("face")` form.
    pub fn note(
        &mut self,
        id: impl Display,
        stream: &StreamHandle,
        offset: u64,
    ) -> ProvenanceNote<'_> {
        let id = id.to_string();
        self.annotations.provenance.insert(
            id.clone(),
            AnnotationProvenance::annotation(stream.0.clone(), offset, None),
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
    pub fn derived(
        &mut self,
        id: impl Display,
        field: impl Into<String>,
    ) -> Result<&mut Self, &'static str> {
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
    ) -> Result<&mut Self, &'static str> {
        let id = id.to_string();
        let field = field.into();
        if field.is_empty() {
            return Err("ExactnessNote.fields keys must not be empty");
        }
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
                    note.fields.insert(field, exactness);
                }
            }
        }
        Ok(self)
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

    /// Return the number of distinct source streams named by provenance.
    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.provenance
            .values()
            .map(AnnotationProvenance::stream)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }

    /// Append another annotation set.
    ///
    /// Every provenance owns its stream name, so there is no catalog to rebase.
    pub fn append(&mut self, mut other: Self) {
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
    fn builder_names_streams_and_records_provenance() {
        let mut builder = AnnotationBuilder::new();
        let first = builder.stream("f3d:Breps.BlobParts/body.smbh");
        let second = builder.stream("f3d:Breps.BlobParts/body.smbh");

        assert_eq!(first, second);
        builder.note("f3d:body#0", &first, 42).tag("body");

        let annotations = builder.build();
        assert_eq!(annotations.stream_count(), 1);
        let provenance = &annotations.provenance["f3d:body#0"];
        assert_eq!(provenance.stream(), "f3d:Breps.BlobParts/body.smbh");
        assert_eq!(provenance.offset, 42);
        assert_eq!(provenance.tag.as_deref(), Some("body"));
    }

    #[test]
    fn annotation_provenance_names_its_stream_and_refuses_the_deleted_index_table() {
        let mut builder = AnnotationBuilder::new();
        let stream = builder.stream("f3d:Breps.BlobParts/body.smbh");
        builder.note("f3d:body#0", &stream, 42).tag("body");
        let annotations = builder.build();

        let value = serde_json::to_value(&annotations).unwrap();
        assert!(value.get("streams").is_none());
        assert_eq!(
            value["provenance"]["f3d:body#0"]["stream"],
            "f3d:Breps.BlobParts/body.smbh"
        );
        assert_eq!(
            serde_json::from_value::<Annotations>(value).unwrap(),
            annotations
        );

        let error = serde_json::from_value::<Annotations>(serde_json::json!({
            "streams": ["f3d:native"],
            "provenance": {
                "f3d:body#0": {"stream": "f3d:native", "offset": 42}
            }
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("unknown field `streams`"), "{error}");

        let error = serde_json::from_value::<Annotations>(serde_json::json!({
            "provenance": {
                "f3d:body#0": {"stream": "", "offset": 42}
            }
        }))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("a source stream name cannot be empty"),
            "{error}"
        );
    }

    #[test]
    fn a_stream_name_survives_foreign_builder_clone_and_resume() {
        let mut first = AnnotationBuilder::new();
        let handle = first.stream("first");
        let mut second = AnnotationBuilder::new();
        second.stream("second");
        second.note("foreign", &handle, 1);
        let mut cloned = first.clone();
        cloned.note("cloned", &handle, 2);
        let mut resumed = AnnotationBuilder::resume(first.build());
        resumed.note("resumed", &handle, 3);
        let mut empty = AnnotationBuilder::new();
        empty.note("empty", &handle, 4);
        let mut same_name = AnnotationBuilder::new();
        same_name.stream("first");
        same_name.note("same-name", &handle, 5);
        assert_eq!(same_name.annotations().stream_count(), 1);
        for (builder, id) in [
            (second, "foreign"),
            (cloned, "cloned"),
            (resumed, "resumed"),
            (empty, "empty"),
            (same_name, "same-name"),
        ] {
            let annotations = builder.build();
            assert_eq!(annotations.provenance[id].stream(), "first");
            let wire = serde_json::to_value(&annotations).unwrap();
            let restored: Annotations = serde_json::from_value(wire).unwrap();
            assert_eq!(restored, annotations);
            assert_eq!(restored.provenance[id].stream(), "first");
        }
    }

    #[test]
    fn exactness_table_stays_sparse() {
        let mut builder = AnnotationBuilder::new();

        builder
            .derived("f3d:edge#0", "param_range")
            .expect("nonempty exactness field")
            .exactness("f3d:edge#0", Exactness::Inferred);
        builder
            .field_exactness("f3d:edge#0", "param_range", Exactness::ByteExact)
            .expect("nonempty exactness field");

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
    fn explicit_field_exactness_survives_an_equal_entity_note() {
        let wire = serde_json::json!({
            "entity": "derived",
            "fields": {"geometry": "derived"}
        });
        let note: ExactnessNote = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(note).unwrap(), wire);

        for entity_first in [false, true] {
            let mut builder = AnnotationBuilder::new();
            if entity_first {
                builder.exactness("nx:model:surface#1", Exactness::Derived);
            }
            builder
                .derived("nx:model:surface#1", "geometry")
                .expect("nonempty exactness field");
            if !entity_first {
                builder.exactness("nx:model:surface#1", Exactness::Derived);
            }
            assert_eq!(
                serde_json::to_value(&builder.annotations().exactness["nx:model:surface#1"])
                    .unwrap(),
                if entity_first {
                    wire.clone()
                } else {
                    serde_json::json!({"entity": "derived"})
                }
            );
        }
    }

    #[test]
    fn removing_an_entity_removes_provenance_and_exactness() {
        let mut builder = AnnotationBuilder::new();
        let stream = builder.stream("catia:e5_0d_03");
        builder.note("catia:e5:curve#0", &stream, 42).tag("circle");
        builder
            .derived("catia:e5:curve#0", "geometry")
            .expect("nonempty exactness field");

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

    #[test]
    fn exactness_field_admission_rejects_empty_keys_without_mutation() {
        let id = "test:model:point#0";
        let mut builder = AnnotationBuilder::new();
        builder.exactness(id, Exactness::Inferred);
        builder.derived(id, "position.x").expect("nonempty path");
        let before = serde_json::to_value(builder.annotations()).expect("serialize annotations");
        for exactness in [
            Exactness::ByteExact,
            Exactness::Derived,
            Exactness::Inferred,
        ] {
            let error = builder
                .field_exactness(id, "", exactness)
                .expect_err("empty path");
            assert!(error.contains("fields"));
            assert_eq!(
                serde_json::to_value(builder.annotations()).expect("serialize annotations"),
                before
            );
        }
        assert!(builder.derived(id, "").is_err());
        assert_eq!(
            serde_json::to_value(builder.annotations()).expect("serialize annotations"),
            before
        );
        for entity in ["byte_exact", "derived"] {
            let error = serde_json::from_value::<ExactnessNote>(
                serde_json::json!({"entity": entity, "fields": {"": "derived"}}),
            )
            .expect_err("empty field key");
            assert!(error.to_string().contains("fields"));
        }
        let wire = serde_json::json!({"entity": "derived", "fields": {"position.x": "byte_exact"}});
        let admitted: ExactnessNote = serde_json::from_value(wire.clone()).expect("nonempty path");
        assert_eq!(
            serde_json::to_value(admitted).expect("serialize note"),
            wire
        );
    }
}
