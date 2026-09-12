// SPDX-License-Identifier: Apache-2.0
//! Sparse document-wide provenance and exactness annotations.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::Arc;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
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

/// Non-empty serde field path keying an exactness override.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "String", into = "String")]
pub struct FieldName(String);

/// The empty string does not name a serialized field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an exactness field name cannot be empty")]
pub struct EmptyFieldName;

impl FieldName {
    /// Returns the field path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for FieldName {
    type Error = EmptyFieldName;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.is_empty() {
            return Err(EmptyFieldName);
        }
        Ok(Self(name))
    }
}

impl From<FieldName> for String {
    fn from(name: FieldName) -> Self {
        name.0
    }
}

impl std::borrow::Borrow<str> for FieldName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Display for FieldName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Exactness of an entity that is not byte-exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Inexactness {
    /// Computed deterministically from byte-exact inputs.
    Derived,
    /// Filled in from context or convention rather than an explicit source field.
    Inferred,
    /// Origin or trustworthiness could not be established.
    Unknown,
}

impl From<Inexactness> for Exactness {
    fn from(exactness: Inexactness) -> Self {
        match exactness {
            Inexactness::Derived => Self::Derived,
            Inexactness::Inferred => Self::Inferred,
            Inexactness::Unknown => Self::Unknown,
        }
    }
}

/// Byte-exact is the absent entity exactness, not an entity annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("byte-exact is the implicit entity exactness")]
pub struct ByteExactEntity;

impl TryFrom<Exactness> for Inexactness {
    type Error = ByteExactEntity;

    fn try_from(exactness: Exactness) -> Result<Self, Self::Error> {
        match exactness {
            Exactness::ByteExact => Err(ByteExactEntity),
            Exactness::Derived => Ok(Self::Derived),
            Exactness::Inferred => Ok(Self::Inferred),
            Exactness::Unknown => Ok(Self::Unknown),
        }
    }
}

/// Field exactness overrides, at least one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "BTreeMap<FieldName, Exactness>",
    into = "BTreeMap<FieldName, Exactness>"
)]
pub struct NonEmptyMap(BTreeMap<FieldName, Exactness>);

/// A field override table carries at least one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an exactness note without an entity exactness needs a field override")]
pub struct EmptyFieldMap;

impl NonEmptyMap {
    /// Returns the field overrides.
    #[must_use]
    pub const fn get(&self) -> &BTreeMap<FieldName, Exactness> {
        &self.0
    }
}

impl TryFrom<BTreeMap<FieldName, Exactness>> for NonEmptyMap {
    type Error = EmptyFieldMap;

    fn try_from(fields: BTreeMap<FieldName, Exactness>) -> Result<Self, Self::Error> {
        if fields.is_empty() {
            return Err(EmptyFieldMap);
        }
        Ok(Self(fields))
    }
}

impl From<NonEmptyMap> for BTreeMap<FieldName, Exactness> {
    fn from(fields: NonEmptyMap) -> Self {
        fields.0
    }
}

/// Exactness for an entity and sparse overrides for its serialized fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExactnessNote {
    /// The entity itself is not byte-exact; listed fields override it.
    Entity {
        /// Exactness of the entity except where overridden by `fields`.
        entity: Inexactness,
        /// Exactness overrides keyed by serde field path.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        fields: BTreeMap<FieldName, Exactness>,
    },
    /// The entity is byte-exact apart from the listed fields.
    Fields {
        /// Exactness overrides keyed by serde field path.
        fields: NonEmptyMap,
    },
}

impl ExactnessNote {
    /// Exactness of the entity except where a field override exists.
    #[must_use]
    pub fn entity(&self) -> Exactness {
        match self {
            Self::Entity { entity, .. } => (*entity).into(),
            Self::Fields { .. } => Exactness::ByteExact,
        }
    }

    /// Explicit field exactness notes.
    #[must_use]
    pub const fn fields(&self) -> &BTreeMap<FieldName, Exactness> {
        match self {
            Self::Entity { fields, .. } => fields,
            Self::Fields { fields } => fields.get(),
        }
    }

    fn fields_mut(&mut self) -> &mut BTreeMap<FieldName, Exactness> {
        match self {
            Self::Entity { fields, .. } => fields,
            Self::Fields { fields } => &mut fields.0,
        }
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
        let fields = match self.annotations.exactness.remove(&id) {
            Some(note) => {
                let mut fields = note.fields().clone();
                fields.retain(|_, value| *value != exactness);
                fields
            }
            None => BTreeMap::new(),
        };
        match Inexactness::try_from(exactness) {
            Ok(entity) => {
                self.annotations
                    .exactness
                    .insert(id, ExactnessNote::Entity { entity, fields });
            }
            Err(_) => {
                if let Ok(fields) = NonEmptyMap::try_from(fields) {
                    self.annotations
                        .exactness
                        .insert(id, ExactnessNote::Fields { fields });
                }
            }
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
        let field = FieldName::try_from(field.into())
            .map_err(|_| "an exactness field name cannot be empty")?;
        if exactness == Exactness::ByteExact {
            let Some(mut note) = self.annotations.exactness.remove(&id) else {
                return Ok(self);
            };
            match &mut note {
                ExactnessNote::Entity { .. } => {
                    note.fields_mut().insert(field, Exactness::ByteExact);
                    self.annotations.exactness.insert(id, note);
                }
                ExactnessNote::Fields { fields } => {
                    let mut fields = fields.get().clone();
                    fields.remove(&field);
                    if let Ok(fields) = NonEmptyMap::try_from(fields) {
                        self.annotations
                            .exactness
                            .insert(id, ExactnessNote::Fields { fields });
                    }
                }
            }
            return Ok(self);
        }
        match self.annotations.exactness.entry(id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ExactnessNote::Fields {
                    fields: NonEmptyMap(BTreeMap::from([(field, exactness)])),
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().fields_mut().insert(field, exactness);
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

        let expected_fields = BTreeMap::from([(
            FieldName::try_from("param_range".to_string()).expect("nonempty path"),
            Exactness::ByteExact,
        )]);
        assert_eq!(
            builder.annotations().exactness["f3d:edge#0"],
            ExactnessNote::Entity {
                entity: Inexactness::Inferred,
                fields: expected_fields,
            }
        );

        builder.exactness("f3d:edge#0", Exactness::ByteExact);
        assert!(builder.annotations().exactness.is_empty());
    }

    #[test]
    fn an_entity_note_refuses_byte_exact_and_a_field_note_refuses_an_empty_map() {
        let error = serde_json::from_value::<ExactnessNote>(serde_json::json!({
            "scope": "entity",
            "entity": "byte_exact"
        }))
        .expect_err("byte-exact is the implicit entity exactness");
        assert!(error.to_string().contains("unknown variant"), "{error}");

        let error = serde_json::from_value::<ExactnessNote>(serde_json::json!({
            "scope": "fields",
            "fields": {}
        }))
        .expect_err("a field note carries at least one override");
        assert!(
            error.to_string().contains("needs a field override"),
            "{error}"
        );
    }

    #[test]
    fn explicit_field_exactness_survives_an_equal_entity_note() {
        let wire = serde_json::json!({
            "scope": "entity",
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
                    serde_json::json!({"scope": "entity", "entity": "derived"})
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
            assert!(error.contains("field name cannot be empty"), "{error}");
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
        for wire in [
            serde_json::json!({"scope": "entity", "entity": "derived", "fields": {"": "derived"}}),
            serde_json::json!({"scope": "fields", "fields": {"": "derived"}}),
        ] {
            let error = serde_json::from_value::<ExactnessNote>(wire).expect_err("empty field key");
            assert!(
                error.to_string().contains("field name cannot be empty"),
                "{error}"
            );
        }
        let wire = serde_json::json!({
            "scope": "entity",
            "entity": "derived",
            "fields": {"position.x": "byte_exact"}
        });
        let admitted: ExactnessNote = serde_json::from_value(wire.clone()).expect("nonempty path");
        assert_eq!(
            serde_json::to_value(admitted).expect("serialize note"),
            wire
        );
    }
}
