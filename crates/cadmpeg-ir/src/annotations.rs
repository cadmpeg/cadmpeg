// SPDX-License-Identifier: Apache-2.0
//! Sparse document-wide provenance and exactness annotations.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::Arc;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provenance::{AnnotationProvenance, Exactness, StreamName};

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
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub provenance: BTreeMap<String, AnnotationProvenance>,
    /// Non-byte-exact entity or field annotations.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    exactness: BTreeMap<String, ExactnessNote>,
}

/// Two source annotation identities would become one identity.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("annotation identity collision at {id}")]
pub struct AnnotationIdentityCollision {
    /// The identity that would replace an existing annotation.
    pub id: String,
}

impl From<AnnotationIdentityCollision> for cadmpeg_core::CodecError {
    fn from(error: AnnotationIdentityCollision) -> Self {
        Self::Malformed(error.to_string())
    }
}

/// Non-empty serde field path keying an exactness override.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FieldName(String);

#[cfg(feature = "schema")]
impl JsonSchema for FieldName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FieldName".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // The map schema generator uses key patterns to constrain property names.
        schemars::json_schema!({"type": "string", "minLength": 1, "pattern": "[\\s\\S]"})
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "BTreeMap<FieldName, Exactness>")]
pub struct NonEmptyMap(BTreeMap<FieldName, Exactness>);

#[cfg(feature = "schema")]
impl JsonSchema for NonEmptyMap {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "NonEmptyMap".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = BTreeMap::<FieldName, Exactness>::json_schema(generator);
        schema.insert("minProperties".into(), 1.into());
        schema
    }
}

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

impl<'de> Deserialize<'de> for NonEmptyMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::try_from(cadmpeg_core::distinct_keys::btree_map(deserializer)?)
            .map_err(serde::de::Error::custom)
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
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
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

/// Shared ownership of an admitted source stream name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamHandle(Arc<StreamName>);

impl StreamHandle {
    /// Own a stream name that can be reused across annotation builders.
    ///
    /// ```compile_fail
    /// let _ = cadmpeg_ir::annotations::StreamHandle::new("");
    /// ```
    #[must_use]
    pub fn new(stream: StreamName) -> Self {
        Self(Arc::new(stream))
    }
}

/// Incrementally constructs document provenance and exactness annotations.
#[derive(Debug, Default, Clone)]
pub struct AnnotationBuilder {
    annotations: Annotations,
}

impl AnnotationBuilder {
    /// Copy a speculative annotation set under the active decode budget.
    pub fn copy_charged(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut annotations = Annotations::default();
        for (id, source) in &self.annotations.provenance {
            ctx.charge_collection_items(1, operation)?;
            let id = ctx.copy_retained_text(id, operation)?;
            annotations
                .provenance
                .insert(id, source.copy_charged(ctx, operation)?);
        }
        for (id, note) in &self.annotations.exactness {
            ctx.charge_collection_items(1, operation)?;
            let id = ctx.copy_retained_text(id, operation)?;
            let mut fields = BTreeMap::new();
            for (field, exactness) in note.fields() {
                ctx.charge_collection_items(1, operation)?;
                let field = FieldName(ctx.copy_retained_text(field.as_str(), operation)?);
                fields.insert(field, *exactness);
            }
            let note = match note {
                ExactnessNote::Entity { entity, .. } => ExactnessNote::Entity {
                    entity: *entity,
                    fields,
                },
                ExactnessNote::Fields { .. } => ExactnessNote::Fields {
                    fields: NonEmptyMap(fields),
                },
            };
            annotations.exactness.insert(id, note);
        }
        Ok(Self { annotations })
    }

    /// Create an empty annotation builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Continue building an existing annotation set.
    pub fn resume(annotations: Annotations) -> Self {
        Self { annotations }
    }

    /// Record one provenance and exactness annotation through the caller's decode budget.
    pub fn annotate_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: impl Display,
        stream: impl Display,
        offset: u64,
        tag: &str,
        exactness: Exactness,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let id = ctx.format_retained(format_args!("{id}"), "annotation identity")?;
        let stream = ctx.format_retained(format_args!("{stream}"), "annotation stream name")?;
        let stream = StreamName::try_from(stream)
            .map_err(|_| cadmpeg_core::CodecError::malformed("annotation stream name is empty"))?;
        ctx.charge_collection_items(1, "annotation stream handles")?;
        let stream = Arc::new(stream);
        let tag = ctx.copy_retained_text(tag, "annotation tag")?;
        if !self.annotations.provenance.contains_key(&id) {
            ctx.charge_collection_items(1, "annotation provenance nodes")?;
        }
        let entity_exactness = Inexactness::try_from(exactness).ok();
        let retained_fields = self.annotations.exactness.get(&id).is_some_and(|note| {
            let fields = match note {
                ExactnessNote::Entity { fields, .. } => fields,
                ExactnessNote::Fields {
                    fields: NonEmptyMap(fields),
                } => fields,
            };
            fields.values().any(|value| *value != exactness)
        });
        let exactness_id = if entity_exactness.is_some() || retained_fields {
            let copied = ctx.copy_retained_text(&id, "annotation exactness identity")?;
            ctx.charge_collection_items(1, "annotation exactness nodes")?;
            Some(copied)
        } else {
            None
        };
        let mut fields = match self.annotations.exactness.remove(&id) {
            Some(
                ExactnessNote::Entity { fields, .. }
                | ExactnessNote::Fields {
                    fields: NonEmptyMap(fields),
                },
            ) => fields,
            None => BTreeMap::new(),
        };
        fields.retain(|_, value| *value != exactness);
        self.annotations.provenance.insert(
            id,
            AnnotationProvenance::annotation(stream, offset, Some(tag)),
        );
        if let Some(entity) = entity_exactness {
            if let Some(exactness_id) = exactness_id {
                self.annotations
                    .exactness
                    .insert(exactness_id, ExactnessNote::Entity { entity, fields });
            }
        } else if let Ok(fields) = NonEmptyMap::try_from(fields) {
            if let Some(exactness_id) = exactness_id {
                self.annotations
                    .exactness
                    .insert(exactness_id, ExactnessNote::Fields { fields });
            }
        }
        Ok(())
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
        self.note_owned(id.to_string(), stream, offset)
    }

    /// Record a source location after admitting its identity, tag and map entry.
    pub fn note_charged(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &str,
        stream: &StreamHandle,
        offset: u64,
        tag: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.note_charged_optional(ctx, id, stream, offset, Some(tag))
    }

    /// Record an optionally tagged source location under the caller's budget.
    pub fn note_charged_optional(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &str,
        stream: &StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if !self.annotations.provenance.contains_key(id) {
            ctx.charge_collection_items(1, "collect source provenance")?;
        }
        let id = ctx.copy_retained_text(id, "retain source provenance identity")?;
        let tag = tag
            .map(|tag| ctx.copy_retained_text(tag, "retain source provenance tag"))
            .transpose()?;
        let note = self.note_owned(id, stream, offset);
        if let Some(tag) = tag {
            note.tag(tag);
        }
        Ok(())
    }

    /// Record a source location with an already admitted identity.
    pub fn note_owned(
        &mut self,
        id: String,
        stream: &StreamHandle,
        offset: u64,
    ) -> ProvenanceNote<'_> {
        let provenance = match self.annotations.provenance.entry(id) {
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(
                AnnotationProvenance::annotation(stream.0.clone(), offset, None),
            ),
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(AnnotationProvenance::annotation(
                    stream.0.clone(),
                    offset,
                    None,
                ));
                entry.into_mut()
            }
        };
        ProvenanceNote { provenance }
    }

    /// Set entity-level exactness. Byte-exact entries are removed to preserve
    /// the table's sparse absent-means-byte-exact representation.
    pub fn exactness(&mut self, id: impl Display, exactness: Exactness) -> &mut Self {
        self.exactness_owned(id.to_string(), exactness)
    }

    /// Set entity exactness with an already admitted identity.
    pub fn exactness_owned(&mut self, id: String, exactness: Exactness) -> &mut Self {
        let fields = match self.annotations.exactness.remove(&id) {
            Some(
                ExactnessNote::Entity { mut fields, .. }
                | ExactnessNote::Fields {
                    fields: NonEmptyMap(mut fields),
                },
            ) => {
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

    /// Mark a derived field after admitting its identity and map entries.
    pub fn derived_charged(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &str,
        field: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let existing = self.annotations.exactness.get(id);
        if existing.is_none() {
            ctx.charge_collection_items(1, "collect source exactness entities")?;
        }
        if existing.is_none_or(|note| !note.fields().contains_key(field)) {
            ctx.charge_collection_items(1, "collect source exactness fields")?;
        }
        let id = ctx.copy_retained_text(id, "retain source exactness identity")?;
        let field = ctx.copy_retained_text(field, "retain source exactness field")?;
        self.derived_owned(id, field)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        Ok(())
    }

    fn derived_owned(&mut self, id: String, field: String) -> Result<&mut Self, &'static str> {
        self.field_exactness_owned(id, field, Exactness::Derived)
    }

    /// Set a serialized field's exactness.
    ///
    /// A byte-exact override is retained for an inexact entity. For a
    /// byte-exact entity, the override is omitted and an empty note is removed.
    pub fn field_exactness(
        &mut self,
        id: impl Display,
        field: impl Into<String>,
        exactness: Exactness,
    ) -> Result<&mut Self, &'static str> {
        self.field_exactness_owned(id.to_string(), field.into(), exactness)
    }

    /// Set field exactness with already admitted identity and field strings.
    pub fn field_exactness_owned(
        &mut self,
        id: String,
        field: String,
        exactness: Exactness,
    ) -> Result<&mut Self, &'static str> {
        let field =
            FieldName::try_from(field).map_err(|_| "an exactness field name cannot be empty")?;
        if exactness == Exactness::ByteExact {
            let Some(note) = self.annotations.exactness.remove(&id) else {
                return Ok(self);
            };
            match note {
                ExactnessNote::Entity { entity, mut fields } => {
                    fields.insert(field, Exactness::ByteExact);
                    self.annotations
                        .exactness
                        .insert(id, ExactnessNote::Entity { entity, fields });
                }
                ExactnessNote::Fields {
                    fields: NonEmptyMap(mut fields),
                } => {
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

    /// Report which exactness map entries a derived field would add.
    #[must_use]
    pub fn derived_field_admission(&self, id: &str, field: &str) -> (bool, bool) {
        match self.annotations.exactness.get(id) {
            Some(note) => (false, !note.fields().contains_key(field)),
            None => (true, true),
        }
    }

    /// Retain exactness annotations selected by identity.
    pub fn retain_exactness(&mut self, mut keep: impl FnMut(&str) -> bool) -> &mut Self {
        self.annotations.exactness.retain(|id, _| keep(id));
        self
    }

    /// Remove all annotations for an entity that was removed from the model.
    pub fn remove_entity(&mut self, id: impl Display) {
        let id = id.to_string();
        self.remove_entity_str(&id);
    }

    /// Remove all annotations for an entity whose identity is already borrowed.
    pub fn remove_entity_str(&mut self, id: &str) {
        self.annotations.provenance.remove(id);
        self.annotations.exactness.remove(id);
    }

    /// Finish building and return the annotation tables.
    pub fn build(self) -> Annotations {
        self.annotations
    }
}

impl Annotations {
    /// Remap both tables together. A collision leaves both tables unchanged.
    /// The callback runs once for each distinct source identity.
    pub fn map_ids(
        &mut self,
        mut map: impl FnMut(&str) -> String,
    ) -> Result<(), AnnotationIdentityCollision> {
        let ids = self
            .provenance
            .keys()
            .chain(self.exactness.keys())
            .collect::<std::collections::BTreeSet<_>>();
        let mut targets = std::collections::BTreeSet::new();
        let mut remapping = Vec::new();
        for id in ids {
            let target = map(id);
            if !targets.insert(target.clone()) {
                return Err(AnnotationIdentityCollision { id: target });
            }
            remapping.push((id.clone(), target));
        }
        let mut remapped = Self::default();
        for (id, target) in remapping {
            if let Some(provenance) = self.provenance.remove(&id) {
                remapped.provenance.insert(target.clone(), provenance);
            }
            if let Some(exactness) = self.exactness.remove(&id) {
                remapped.exactness.insert(target, exactness);
            }
        }
        *self = remapped;
        Ok(())
    }

    /// Remap identities after charging each index entry and retained key copy.
    pub fn map_ids_charged(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut map: impl FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
        operation: &'static str,
    ) -> Result<Result<(), AnnotationIdentityCollision>, cadmpeg_core::CodecError> {
        let mut ids = std::collections::BTreeSet::new();
        for id in self.provenance.keys().chain(self.exactness.keys()) {
            if !ids.contains(id) {
                ctx.charge_collection_items(1, operation)?;
                ids.insert(id);
            }
        }
        let mut targets = std::collections::BTreeSet::new();
        let mut remapping = Vec::new();
        ctx.charge_collection_items(cadmpeg_core::decode::u64_from_index(ids.len()), operation)?;
        remapping.try_reserve(ids.len()).map_err(|_| {
            cadmpeg_core::CodecError::ResourceLimit(
                cadmpeg_core::decode::ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::Codec(operation),
                    u64::MAX,
                    u64::MAX,
                    operation,
                ),
            )
        })?;
        for id in ids {
            let target = map(id)?;
            if targets.contains(&target) {
                return Ok(Err(AnnotationIdentityCollision { id: target }));
            }
            ctx.charge_collection_items(1, operation)?;
            targets.insert(ctx.copy_retained_text(&target, operation)?);
            let provenance_target = if self.provenance.contains_key(id) {
                ctx.charge_collection_items(1, operation)?;
                Some(ctx.copy_retained_text(&target, operation)?)
            } else {
                None
            };
            if self.exactness.contains_key(id) {
                ctx.charge_collection_items(1, operation)?;
            }
            remapping.push((
                ctx.copy_retained_text(id, operation)?,
                target,
                provenance_target,
            ));
        }
        let mut remapped = Self::default();
        for (id, target, provenance_target) in remapping {
            if let Some(provenance) = self.provenance.remove(&id) {
                let Some(provenance_target) = provenance_target else {
                    return Err(cadmpeg_core::CodecError::malformed(
                        "missing charged annotation key",
                    ));
                };
                remapped.provenance.insert(provenance_target, provenance);
            }
            if let Some(exactness) = self.exactness.remove(&id) {
                remapped.exactness.insert(target, exactness);
            }
        }
        *self = remapped;
        Ok(Ok(()))
    }

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

    /// Append annotations with disjoint identities without a decode context.
    /// A collision leaves this annotation set unchanged.
    pub fn append(&mut self, mut other: Self) -> Result<(), AnnotationIdentityCollision> {
        for id in other.provenance.keys().chain(other.exactness.keys()) {
            if self.provenance.contains_key(id) || self.exactness.contains_key(id) {
                return Err(AnnotationIdentityCollision { id: id.clone() });
            }
        }
        self.provenance.append(&mut other.provenance);
        self.exactness.append(&mut other.exactness);
        Ok(())
    }

    /// Append disjoint tables after charging their destination nodes.
    pub fn append_charged(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut other: Self,
        operation: &'static str,
    ) -> Result<Result<(), AnnotationIdentityCollision>, cadmpeg_core::CodecError> {
        for id in other.provenance.keys().chain(other.exactness.keys()) {
            if self.provenance.contains_key(id) || self.exactness.contains_key(id) {
                return Ok(Err(AnnotationIdentityCollision {
                    id: ctx.copy_retained_text(id, operation)?,
                }));
            }
        }
        let count = other
            .provenance
            .len()
            .checked_add(other.exactness.len())
            .map(cadmpeg_core::decode::u64_from_index)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        ctx.charge_collection_items(count, operation)?;
        self.provenance.append(&mut other.provenance);
        self.exactness.append(&mut other.exactness);
        Ok(Ok(()))
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
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[test]
    fn admitted_annotation_refuses_each_retained_value_and_tree_node() {
        let id = "test:model:entity#1";
        let stream = "creo:VisibGeom";
        let tag = "face";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let mut total = 0u64;
        for (value, operation) in [
            (id, "annotation identity"),
            (stream, "annotation stream name"),
            (tag, "annotation tag"),
            (id, "annotation exactness identity"),
        ] {
            total += cadmpeg_core::decode::u64_from_index(value.len());
            policy.limits.max_retained_bytes = total - 1;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut builder = super::AnnotationBuilder::new();
            let error = builder
                .annotate_admitted(&ctx, id, stream, 42, tag, super::Exactness::Derived)
                .expect_err("retained value exceeds cap");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::RetainedBytes
                    && resource.operation == operation),
                "{error}"
            );
        }
        policy.limits.max_retained_bytes = total;
        for (limit, operation) in [
            (0, "annotation stream handles"),
            (1, "annotation provenance nodes"),
            (2, "annotation exactness nodes"),
        ] {
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut builder = super::AnnotationBuilder::new();
            let error = builder
                .annotate_admitted(&ctx, id, stream, 42, tag, super::Exactness::Derived)
                .expect_err("collection node exceeds cap");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation),
                "{error}"
            );
        }
        policy.limits.max_collection_items = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut admitted = super::AnnotationBuilder::new();
        admitted
            .annotate_admitted(&ctx, id, stream, 42, tag, super::Exactness::Derived)
            .expect("exact caps admit annotation");
        let mut original = super::AnnotationBuilder::new();
        let handle = super::StreamHandle::new(
            super::StreamName::try_from(stream.to_string()).expect("nonempty stream"),
        );
        original.note(id, &handle, 42).tag(tag);
        original.exactness(id, super::Exactness::Derived);
        assert_eq!(admitted.build(), original.build());
    }

    #[test]
    fn annotation_remap_refuses_nested_collection_and_retained_limits() {
        let run = |collection_limit, retained_limit| {
            let mut builder = super::AnnotationBuilder::new();
            let stream = super::StreamHandle::new(crate::stream_name!("test"));
            builder.note("test:point#0", &stream, 0);
            let mut annotations = builder.build();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = collection_limit;
            policy.limits.max_retained_bytes = retained_limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let outcome = annotations.map_ids_charged(
                &ctx,
                |_| Ok(String::from("test:point#mapped")),
                "test_annotation_remap",
            );
            (outcome, annotations)
        };
        assert!(
            matches!(run(0, u64::MAX).0, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "test_annotation_remap")
        );
        assert!(
            matches!(run(u64::MAX, 0).0, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "test_annotation_remap")
        );
        let (outcome, annotations) = run(u64::MAX, u64::MAX);
        assert!(matches!(outcome, Ok(Ok(()))));
        assert!(annotations.provenance.contains_key("test:point#mapped"));
    }

    #[test]
    fn annotation_append_refuses_destination_node_limit() {
        let run = |limit| {
            let mut builder = super::AnnotationBuilder::new();
            let stream = super::StreamHandle::new(crate::stream_name!("test"));
            builder.note("test:point#0", &stream, 0);
            let mut target = super::Annotations::default();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let result = target.append_charged(&ctx, builder.build(), "test_annotation_append");
            (result, target)
        };
        assert!(
            matches!(run(0).0, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "test_annotation_append")
        );
        let (result, target) = run(u64::MAX);
        assert!(matches!(result, Ok(Ok(()))));
        assert!(target.provenance.contains_key("test:point#0"));
    }

    #[test]
    fn annotation_copy_charges_nested_entries_and_retained_text() {
        let mut builder = super::AnnotationBuilder::new();
        let stream = super::StreamHandle::new(crate::stream_name!("test"));
        builder.note("test:point#0", &stream, 7).tag("point");
        builder
            .derived("test:point#0", "position")
            .expect("field path");

        let run = |collection_limit: u64, retained_limit: u64| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = collection_limit;
            policy.limits.max_retained_bytes = retained_limit;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root");
            builder.copy_charged(&ctx, "test_annotation_copy")
        };
        assert!(
            matches!(run(0, u64::MAX), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "test_annotation_copy")
        );
        assert!(
            matches!(run(u64::MAX, 0), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "test_annotation_copy")
        );
        assert_eq!(
            run(u64::MAX, u64::MAX).expect("service copy").build(),
            builder.clone().build()
        );
    }

    mod identity_merges;

    use std::collections::BTreeMap;

    #[cfg(feature = "schema")]
    use super::NonEmptyMap;
    use super::{
        AnnotationBuilder, Annotations, ExactnessNote, FieldName, Inexactness, StreamHandle,
    };
    use crate::provenance::Exactness;

    #[cfg(feature = "schema")]
    #[test]
    fn exactness_schemas_preserve_field_and_population_admission() {
        let schema = serde_json::to_value(schemars::schema_for!(NonEmptyMap)).unwrap();
        assert_eq!(schema["minProperties"], 1);
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema["patternProperties"].get("[\\s\\S]").is_some());
        let schema = serde_json::to_value(schemars::schema_for!(FieldName)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["minLength"], 1);

        // Whitespace and line breaks are legal field text; only absence is refused.
        for field in [" ", "\n", "geometry.radius"] {
            let key = FieldName::try_from(field.to_owned()).unwrap();
            let map = NonEmptyMap::try_from(BTreeMap::from([(key, Exactness::Derived)])).unwrap();
            let wire = serde_json::to_value(&map).unwrap();
            assert_eq!(serde_json::from_value::<NonEmptyMap>(wire).unwrap(), map);
        }
        assert!(FieldName::try_from(String::new()).is_err());
        assert!(NonEmptyMap::try_from(BTreeMap::new()).is_err());
    }

    #[test]
    fn builder_names_streams_and_records_provenance() {
        let mut builder = AnnotationBuilder::new();
        let first = StreamHandle::new(crate::stream_name!("f3d:Breps.BlobParts/body.smbh"));
        let second = StreamHandle::new(crate::stream_name!("f3d:Breps.BlobParts/body.smbh"));

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
    fn repeated_note_replaces_location_and_clears_the_previous_tag() {
        let mut builder = AnnotationBuilder::new();
        let first = StreamHandle::new(crate::stream_name!("first"));
        let second = StreamHandle::new(crate::stream_name!("second"));

        builder.note("entity", &first, 7).tag("stale");
        builder.note("entity", &second, 11);

        let annotations = builder.build();
        let provenance = &annotations.provenance["entity"];
        assert_eq!(provenance.stream(), "second");
        assert_eq!(provenance.offset, 11);
        assert_eq!(provenance.tag, None);

        let wire = serde_json::to_value(&annotations).expect("serialize annotations");
        let restored: Annotations =
            serde_json::from_value(wire).expect("annotation wire round-trips");
        assert_eq!(restored, annotations);
    }

    #[test]
    fn a_nonempty_whitespace_stream_name_is_preserved_on_the_annotation_wire() {
        let mut builder = AnnotationBuilder::new();
        let stream = StreamHandle::new(crate::stream_name!(" \t"));
        builder.note("whitespace", &stream, 3);

        let annotations = builder.build();
        assert_eq!(annotations.provenance["whitespace"].stream(), " \t");
        let wire = serde_json::to_value(&annotations).expect("serialize annotations");
        assert_eq!(wire["provenance"]["whitespace"]["stream"], " \t");
        assert_eq!(
            serde_json::from_value::<Annotations>(wire).unwrap(),
            annotations
        );
    }

    #[test]
    fn annotation_provenance_names_its_stream_and_refuses_the_deleted_index_table() {
        let mut builder = AnnotationBuilder::new();
        let stream = StreamHandle::new(crate::stream_name!("f3d:Breps.BlobParts/body.smbh"));
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
        let first = AnnotationBuilder::new();
        let handle = StreamHandle::new(crate::stream_name!("first"));
        let mut second = AnnotationBuilder::new();
        second.note("foreign", &handle, 1);
        let mut cloned = first.clone();
        cloned.note("cloned", &handle, 2);
        let mut resumed = AnnotationBuilder::resume(first.build());
        resumed.note("resumed", &handle, 3);
        let mut empty = AnnotationBuilder::new();
        empty.note("empty", &handle, 4);
        let mut same_name = AnnotationBuilder::new();
        same_name.note("same-name", &handle, 5);
        assert_eq!(same_name.annotations.stream_count(), 1);
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
            builder.annotations.exactness["f3d:edge#0"],
            ExactnessNote::Entity {
                entity: Inexactness::Inferred,
                fields: expected_fields,
            }
        );

        builder.exactness("f3d:edge#0", Exactness::ByteExact);
        assert!(builder.annotations.exactness.is_empty());
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
                serde_json::to_value(&builder.annotations.exactness["nx:model:surface#1"]).unwrap(),
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
        let stream = StreamHandle::new(crate::stream_name!("catia:e5_0d_03"));
        builder.note("catia:e5:curve#0", &stream, 42).tag("circle");
        builder
            .derived("catia:e5:curve#0", "geometry")
            .expect("nonempty exactness field");

        builder.remove_entity("catia:e5:curve#0");

        assert!(!builder
            .annotations
            .provenance
            .contains_key("catia:e5:curve#0"));
        assert!(!builder
            .annotations
            .exactness
            .contains_key("catia:e5:curve#0"));
    }

    #[test]
    fn exactness_field_admission_rejects_empty_keys_without_mutation() {
        let id = "test:model:point#0";
        let mut builder = AnnotationBuilder::new();
        builder.exactness(id, Exactness::Inferred);
        builder.derived(id, "position.x").expect("nonempty path");
        let before = serde_json::to_value(&builder.annotations).expect("serialize annotations");
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
                serde_json::to_value(&builder.annotations).expect("serialize annotations"),
                before
            );
        }
        assert!(builder.derived(id, "").is_err());
        assert_eq!(
            serde_json::to_value(&builder.annotations).expect("serialize annotations"),
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
    #[test]
    fn repeated_exactness_field_keys_are_refused_through_the_decode_sidecar() {
        let ir = crate::examples::unit_cube().expect("valid cube");
        let ir_json = ir.to_canonical_json().expect("serialize CADIR");
        let id = ir.model.points[0].id.as_str();
        for scope in ["entity", "fields"] {
            let mut builder = AnnotationBuilder::new();
            if scope == "entity" {
                builder.exactness(id, Exactness::Inferred);
            }
            builder.derived(id, "position").expect("nonempty field");
            let report = crate::report::decode::DecodeReport::unclassified(
                "test",
                crate::report::decode::DecodeTransfer::full(true),
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                crate::report::decode::TransferLedger::default(),
            );
            let sidecar = crate::source_fidelity::DecodeSidecar::bind_sha256(
                crate::hash::digest::Sha256Digest::digest(ir_json.as_bytes()),
                report,
                crate::SourceFidelity::with_annotations(builder.build()),
            );
            let text = serde_json::to_string(&sidecar).expect("serialize sidecar");
            assert_eq!(
                crate::source_fidelity::DecodeSidecar::from_json(&text).unwrap(),
                sidecar
            );
            assert_eq!(text.matches(r#""position":"derived""#).count(), 1);
            let duplicate = text.replace(
                r#""position":"derived""#,
                r#""position":"derived","position":"unknown""#,
            );
            let error = crate::source_fidelity::DecodeSidecar::from_json(&duplicate)
                .expect_err("a repeated field key cannot replace its first value");
            assert!(
                error.to_string().contains("duplicate key position"),
                "{scope}: {error}"
            );
        }
    }
}
