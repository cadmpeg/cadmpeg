// SPDX-License-Identifier: Apache-2.0
//! Sparse document-wide provenance and exactness annotations.

use std::collections::BTreeMap;
use std::fmt::Display;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provenance::Exactness;

/// Document-wide provenance and exactness tables keyed by globally unique
/// entity id.
///
/// An entity absent from `exactness` is byte-exact.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Annotations {
    /// Interned source stream names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub streams: Vec<String>,
    /// Source location for each annotated entity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, Provenance>,
    /// Non-byte-exact entity or field annotations.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exactness: BTreeMap<String, ExactnessNote>,
}

/// Source location using an index into [`Annotations::streams`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    /// Index of the source stream in [`Annotations::streams`].
    pub stream: u32,
    /// Byte offset of the source record within the stream.
    pub offset: u64,
    /// Source record or class name, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// Exactness for an entity and sparse overrides for its serialized fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExactnessNote {
    /// Exactness of the entity except where overridden by `fields`.
    pub entity: Exactness,
    /// Exactness overrides keyed by serde field path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Exactness>,
}

impl Default for ExactnessNote {
    fn default() -> Self {
        Self {
            entity: Exactness::ByteExact,
            fields: BTreeMap::new(),
        }
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

    /// Intern a source stream name and return its reusable handle.
    pub fn stream(&mut self, stream: impl Into<String>) -> StreamHandle {
        let stream = stream.into();
        if let Some(index) = self
            .annotations
            .streams
            .iter()
            .position(|existing| existing == &stream)
        {
            return StreamHandle(index as u32);
        }

        let index = u32::try_from(self.annotations.streams.len())
            .expect("annotation stream count exceeds u32::MAX");
        self.annotations.streams.push(stream);
        StreamHandle(index)
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
        self.annotations.provenance.insert(
            id.clone(),
            Provenance {
                stream: stream.0,
                offset,
                tag: None,
            },
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
        let note = self.annotations.exactness.entry(id.clone()).or_default();
        note.entity = exactness;
        note.fields.retain(|_, value| *value != exactness);
        if exactness == Exactness::ByteExact && note.fields.is_empty() {
            self.annotations.exactness.remove(&id);
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
            self.annotations
                .exactness
                .entry(id)
                .or_default()
                .fields
                .insert(field, exactness);
        }
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

/// In-progress provenance annotation returned by [`AnnotationBuilder::note`].
pub struct ProvenanceNote<'a> {
    provenance: &'a mut Provenance,
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
        assert_eq!(annotations.streams.len(), 1);
        assert_eq!(
            annotations.provenance["f3d:body#0"],
            Provenance {
                stream: 0,
                offset: 42,
                tag: Some("body".to_string()),
            }
        );
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
