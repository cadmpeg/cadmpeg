// SPDX-License-Identifier: Apache-2.0
//! Sparse document-wide provenance and exactness annotations.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::Arc;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceLimit, ScopedReservation,
};
use cadmpeg_core::CodecError;

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

/// Annotation storage held until a transaction commits or is discarded.
#[derive(Debug)]
pub struct AnnotationTransaction<'ctx> {
    annotations: Annotations,
    storage: ScopedReservation<'ctx>,
}

impl AnnotationTransaction<'_> {
    /// Read the candidate annotation tables.
    pub fn annotations(&self) -> &Annotations {
        &self.annotations
    }

    /// Admit mutations into the transaction's temporary storage.
    pub fn update<T, E: From<CodecError>>(
        mut self,
        apply: impl FnOnce(&mut Annotations) -> Result<T, E>,
    ) -> Result<(T, Self), E> {
        let result = self.storage.with_storage(|| apply(&mut self.annotations))?;
        Ok((result, self))
    }

    /// Admit the final owned tables before transferring them to the document.
    pub fn into_retained(self) -> Result<Annotations, CodecError> {
        self.storage.commit()?;
        Ok(self.annotations)
    }
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

/// Failure to admit annotation identity changes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnnotationIdentityError {
    /// Two identities would name one annotation.
    #[error("{0}")]
    Collision(AnnotationIdentityCollision),
    /// The budget or allocator refused admission.
    #[error("decode resource limit: {0:?}")]
    Resource(ResourceLimit),
    /// The remapping callback refused an identity.
    #[error("{0}")]
    Callback(String),
}

impl From<ResourceLimit> for AnnotationIdentityError {
    fn from(limit: ResourceLimit) -> Self {
        Self::Resource(limit)
    }
}

impl From<AnnotationIdentityError> for CodecError {
    fn from(error: AnnotationIdentityError) -> Self {
        match error {
            AnnotationIdentityError::Collision(error) => error.into(),
            AnnotationIdentityError::Resource(limit) => limit.into(),
            AnnotationIdentityError::Callback(message) => Self::Malformed(message),
        }
    }
}

/// Failure to admit field exactness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AnnotationFieldError {
    /// A field name is empty.
    #[error("{0}")]
    Invalid(&'static str),
    /// The budget or allocator refused admission.
    #[error("decode resource limit: {0:?}")]
    Resource(ResourceLimit),
}

impl From<ResourceLimit> for AnnotationFieldError {
    fn from(limit: ResourceLimit) -> Self {
        Self::Resource(limit)
    }
}

impl From<CodecError> for AnnotationFieldError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::ResourceLimit(limit) => Self::Resource(limit),
            _ => Self::Invalid("cannot format annotation identity"),
        }
    }
}

impl From<AnnotationFieldError> for CodecError {
    fn from(error: AnnotationFieldError) -> Self {
        match error {
            AnnotationFieldError::Invalid(message) => Self::malformed(message),
            AnnotationFieldError::Resource(limit) => limit.into(),
        }
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

    /// Allocate one shared stream name under the caller's decode budget.
    pub fn new_for_decode(
        ctx: &DecodeContext<'_>,
        stream: StreamName,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let bytes = std::mem::size_of::<StreamName>() + 2 * std::mem::size_of::<usize>();
        ctx.charge_retained(u64_from_index(bytes), operation)?;
        ctx.charge_collection_items(1, operation)?;
        Ok(Self::new(stream))
    }
}

/// Incrementally constructs document provenance and exactness annotations.
#[derive(Debug, Default, Clone)]
pub struct AnnotationBuilder {
    annotations: Annotations,
}

impl AnnotationBuilder {
    /// Copy the annotation tables under the caller's decode budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        self.annotations
            .try_clone_for_decode(ctx, operation)
            .map(|annotations| Self { annotations })
    }

    /// Create an empty annotation builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Continue building an existing annotation set.
    pub fn resume(annotations: Annotations) -> Self {
        Self { annotations }
    }

    /// Borrow the annotations accumulated so far.
    #[must_use]
    pub fn annotations(&self) -> &Annotations {
        &self.annotations
    }

    /// Record provenance and entity exactness under the decode budget.
    pub fn annotate(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: impl Display,
        stream: impl Display,
        offset: u64,
        tag: &str,
        exactness: Exactness,
    ) -> Result<(), CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "annotation identity")?;
        let id =
            ctx.format_scoped_text(&mut scratch, format_args!("{id}"), "annotation identity")?;
        let stream = ctx.format_retained(format_args!("{stream}"), "annotation stream name")?;
        let stream = StreamName::try_from(stream)
            .map_err(|_| CodecError::malformed("annotation stream name is empty"))?;
        let stream = StreamHandle::new_for_decode(ctx, stream, "annotation stream handles")?;
        self.note_for_decode(ctx, &id, &stream, offset, Some(tag))?;
        self.exactness_for_decode(ctx, &id, exactness)?;
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

    /// Record an optional provenance tag under the caller's decode budget.
    pub fn note_for_decode(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: impl Display,
        stream: &StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) -> Result<(), CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "source provenance lookup")?;
        let id = ctx.format_scoped_text(
            &mut scratch,
            format_args!("{id}"),
            "source provenance lookup",
        )?;
        admit_identity_work(
            ctx,
            self.annotations.provenance.len(),
            id.len(),
            "collect source provenance",
        )?;
        let stored_id = if self.annotations.provenance.contains_key(&id) {
            id
        } else {
            ctx.admit_retained_btree_record::<String, AnnotationProvenance>(0, "collect source provenance",
            )?;
            ctx.copy_retained_text(&id, "retain source provenance identity")?
        };
        let tag = tag
            .map(|tag| ctx.copy_retained_text(tag, "retain source provenance tag"))
            .transpose()?;
        ctx.charge_work(1, "share source provenance stream")?;
        let note = self.note_owned(stored_id, stream, offset);
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
        if let Some(note) = self.annotations.exactness.get_mut(&id) {
            note.fields_mut().retain(|_, value| *value != exactness);
            let fields = std::mem::take(note.fields_mut());
            *note = match Inexactness::try_from(exactness) {
                Ok(entity) => ExactnessNote::Entity { entity, fields },
                Err(_) => ExactnessNote::Fields {
                    fields: NonEmptyMap(fields),
                },
            };
            if exactness == Exactness::ByteExact && note.fields().is_empty() {
                self.annotations.exactness.remove(&id);
            }
        } else if let Ok(entity) = Inexactness::try_from(exactness) {
            self.annotations.exactness.insert(
                id,
                ExactnessNote::Entity {
                    entity,
                    fields: BTreeMap::new(),
                },
            );
        }
        self
    }

    /// Set entity exactness after admitting any new retained record.
    pub fn exactness_for_decode(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: impl Display,
        exactness: Exactness,
    ) -> Result<&mut Self, CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "source exactness lookup")?;
        let id = ctx.format_scoped_text(
            &mut scratch,
            format_args!("{id}"),
            "source exactness lookup",
        )?;
        admit_identity_work(
            ctx,
            self.annotations.exactness.len(),
            id.len(),
            "collect source exactness entities",
        )?;
        let id =
            if exactness != Exactness::ByteExact && !self.annotations.exactness.contains_key(&id) {
                ctx.copy_retained_text(&id, "retain source exactness identity")?
            } else {
                id
            };
        self.exactness_owned_for_decode(ctx, id, exactness)
    }

    /// Move an admitted identity into an exactness entry after destination admission.
    pub fn exactness_owned_for_decode(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: String,
        exactness: Exactness,
    ) -> Result<&mut Self, CodecError> {
        admit_identity_work(
            ctx,
            self.annotations.exactness.len(),
            id.len(),
            "collect source exactness entities",
        )?;
        let fields = self
            .annotations
            .exactness
            .get(&id)
            .map_or(0, |note| note.fields().len());
        ctx.charge_work(u64_from_index(fields), "retain source exactness fields")?;
        if exactness != Exactness::ByteExact && !self.annotations.exactness.contains_key(&id) {
            ctx.admit_retained_btree_record::<String, ExactnessNote>(0, "collect source exactness entities",
            )?;
        }
        Ok(self.exactness_owned(id, exactness))
    }

    /// Mark one serialized field as deterministically derived.
    pub fn derived(
        &mut self,
        id: impl Display,
        field: impl Into<String>,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.field_exactness(id, field, Exactness::Derived)
    }

    /// Mark a derived field under the caller's decode budget.
    pub fn derived_for_decode(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: impl Display,
        field: &str,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.field_exactness_for_decode(ctx, id, field, Exactness::Derived)
    }

    /// Set field exactness through the default decode policy.
    pub fn field_exactness(
        &mut self,
        id: impl Display,
        field: impl Into<String>,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes_limit(&[], &arena, &policy)?;
        self.field_exactness_for_decode(&ctx, id, field.into(), exactness)
    }

    /// Set field exactness after admitting retained keys and records.
    pub fn field_exactness_for_decode<'f>(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: impl Display,
        field: impl Into<std::borrow::Cow<'f, str>>,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        let mut scratch = ctx.reserve_scoped(0, "source exactness lookup")?;
        let id = ctx.format_scoped_text(
            &mut scratch,
            format_args!("{id}"),
            "source exactness lookup",
        )?;
        self.insert_field_exactness(ctx, id, true, field.into(), exactness)
    }

    /// Set field exactness with owned input strings under the default policy.
    pub fn field_exactness_owned(
        &mut self,
        id: String,
        field: String,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes_limit(&[], &arena, &policy)?;
        self.field_exactness_owned_for_decode(&ctx, id, field, exactness)
    }

    /// Admit map records for already owned identity and field strings.
    pub fn field_exactness_owned_for_decode(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: String,
        field: String,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.insert_field_exactness(ctx, id, false, std::borrow::Cow::Owned(field), exactness)
    }

    fn insert_field_exactness(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: String,
        scoped_id: bool,
        field: std::borrow::Cow<'_, str>,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        if field.is_empty() {
            return Err(AnnotationFieldError::Invalid(
                "an exactness field name cannot be empty",
            ));
        }
        admit_identity_work(
            ctx,
            self.annotations.exactness.len(),
            id.len(),
            "collect source exactness entities",
        )?;
        let existing = self.annotations.exactness.get(&id);
        let keep_field = exactness != Exactness::ByteExact
            || matches!(existing, Some(ExactnessNote::Entity { .. }));
        let new_entity = existing.is_none() && keep_field;
        let new_field =
            keep_field && existing.is_none_or(|note| !note.fields().contains_key(field.as_ref()));
        if new_entity {
            ctx.admit_retained_btree_record::<String, ExactnessNote>(0, "collect source exactness entities",
            )?;
        }
        if new_field {
            ctx.admit_retained_btree_record::<FieldName, Exactness>(0, "collect source exactness fields",
            )?;
        }
        let fields = existing.map_or(0, |note| note.fields().len());
        admit_identity_work(ctx, fields, field.len(), "collect source exactness fields")?;
        let id = if new_entity && scoped_id {
            ctx.copy_retained_text(&id, "retain source exactness identity")?
        } else {
            id
        };
        let mut scratch = ctx.reserve_scoped(0, "source exactness field lookup")?;
        let field = match field {
            std::borrow::Cow::Owned(field) => field,
            std::borrow::Cow::Borrowed(field) if new_field => {
                ctx.copy_retained_text(field, "retain source exactness field")?
            }
            std::borrow::Cow::Borrowed(field) => scratch
                .with_storage(|| ctx.copy_retained_text(field, "source exactness field lookup"))?,
        };
        self.set_field_exactness(id, FieldName(field), exactness);
        Ok(self)
    }

    fn set_field_exactness(&mut self, id: String, field: FieldName, exactness: Exactness) {
        if exactness == Exactness::ByteExact {
            if let Some(note) = self.annotations.exactness.get_mut(&id) {
                match note {
                    ExactnessNote::Entity { fields, .. } => {
                        fields.insert(field, exactness);
                    }
                    ExactnessNote::Fields { fields } => {
                        fields.0.remove(&field);
                    }
                }
                if matches!(note, ExactnessNote::Fields { fields } if fields.0.is_empty()) {
                    self.annotations.exactness.remove(&id);
                }
            }
        } else {
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
        }
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

fn admit_identity_work(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entries: usize,
    bytes: usize,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    // A binary height bound covers node comparisons and one key copy.
    let levels = u64::from(usize::BITS - entries.leading_zeros()) + 1;
    let work = levels
        .checked_mul(32)
        .and_then(|work| work.checked_add(4))
        .and_then(|work| work.checked_mul(cadmpeg_core::decode::u64_from_index(bytes)))
        .and_then(|work| work.checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)
}

impl Annotations {
    /// Copy a speculative annotation set under the active decode budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        self.copy_transaction(ctx, operation)?.into_retained()
    }

    /// Copy annotation tables while holding their temporary storage reservation.
    pub fn copy_transaction<'ctx>(
        &self,
        ctx: &'ctx DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<AnnotationTransaction<'ctx>, CodecError> {
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut annotations = Self::default();
        for (id, source) in &self.provenance {
            admit_identity_work(ctx, annotations.provenance.len(), id.len(), operation)?;
            let id = storage.with_storage(|| ctx.copy_retained_text(id, operation))?;
            let source = storage.with_storage(|| source.try_clone_for_decode(ctx, operation))?;
            ctx.insert_scoped_btree_map_if_vacant(
                &mut storage, &mut annotations.provenance, id, source, operation, operation,
            )?;
        }
        for (id, note) in &self.exactness {
            admit_identity_work(ctx, annotations.exactness.len(), id.len(), operation)?;
            let id = storage.with_storage(|| ctx.copy_retained_text(id, operation))?;
            let mut fields = BTreeMap::new();
            for (field, exactness) in note.fields() {
                admit_identity_work(ctx, fields.len(), field.as_str().len(), operation)?;
                let field = FieldName(storage.with_storage(|| ctx.copy_retained_text(field.as_str(), operation))?);
                ctx.insert_scoped_btree_map_if_vacant(
                    &mut storage, &mut fields, field, *exactness, operation, operation,
                )?;
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
            ctx.insert_scoped_btree_map_if_vacant(
                &mut storage, &mut annotations.exactness, id, note, operation, operation,
            )?;
        }
        Ok(AnnotationTransaction { annotations, storage })
    }

    /// Remap both tables together. A collision leaves both tables unchanged.
    /// The callback runs once for each distinct source identity.
    pub fn map_ids(
        &mut self,
        mut map: impl FnMut(&str) -> String,
    ) -> Result<(), AnnotationIdentityError> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes_limit(&[], &arena, &policy)?;
        match self.map_ids_for_decode(&ctx, |id| Ok(map(id)), "remap annotation identities") {
            Ok(result) => result.map_err(AnnotationIdentityError::Collision),
            Err(CodecError::ResourceLimit(limit)) => Err(limit.into()),
            Err(error) => Err(AnnotationIdentityError::Callback(error.to_string())),
        }
    }

    /// Remap both tables while charging temporary indices and retained keys.
    /// The callback returns each target identity with its retained bytes
    /// already charged; a target stored in both tables is copied and charged
    /// once more. A collision leaves both tables unchanged.
    pub fn map_ids_for_decode(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut map: impl FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
        operation: &'static str,
    ) -> Result<Result<(), AnnotationIdentityCollision>, cadmpeg_core::CodecError> {
        let mut scratch = ctx.reserve_scoped(0, operation)?;
        let mut ids = std::collections::BTreeSet::new();
        for id in self.provenance.keys().chain(self.exactness.keys()) {
            admit_identity_work(ctx, ids.len(), id.len(), operation)?;
            if !ids.contains(id) {
                ctx.insert_scoped_btree_value(&mut scratch, &mut ids, id, operation)?;
            }
        }
        let mut targets = std::collections::BTreeSet::new();
        let mut remapping = Vec::new();
        ctx.reserve_scoped_vec(&mut scratch, &mut remapping, ids.len(), operation)?;
        for id in ids {
            ctx.charge_work(1, operation)?;
            let target = map(id)?;
            admit_identity_work(ctx, targets.len(), target.len(), operation)?;
            if targets.contains(&target) {
                return Ok(Err(AnnotationIdentityCollision { id: target }));
            }
            let target_check =
                scratch.with_storage(|| ctx.copy_retained_text(&target, operation))?;
            ctx.insert_scoped_btree_value(&mut scratch, &mut targets, target_check, operation)?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(id.len()), operation)?;
            let source = scratch.with_storage(|| ctx.copy_retained_text(id, operation))?;
            remapping.push((source, target));
        }
        let mut remapped = Self::default();
        for (id, target) in remapping {
            admit_identity_work(ctx, self.provenance.len(), id.len(), operation)?;
            admit_identity_work(ctx, self.exactness.len(), id.len(), operation)?;
            admit_identity_work(ctx, remapped.provenance.len(), target.len(), operation)?;
            admit_identity_work(ctx, remapped.exactness.len(), target.len(), operation)?;
            let provenance = self.provenance.remove(&id);
            let exactness = self.exactness.remove(&id);
            match (provenance, exactness) {
                (Some(provenance), Some(exactness)) => {
                    let provenance_key = ctx.copy_retained_text(&target, operation)?;
                    ctx.admit_retained_btree_record::<String, AnnotationProvenance>(0, operation)?;
                    remapped.provenance.insert(provenance_key, provenance);
                    ctx.admit_retained_btree_record::<String, ExactnessNote>(0, operation)?;
                    remapped.exactness.insert(target, exactness);
                }
                (Some(provenance), None) => {
                    ctx.admit_retained_btree_record::<String, AnnotationProvenance>(0, operation)?;
                    remapped.provenance.insert(target, provenance);
                }
                (None, Some(exactness)) => {
                    ctx.admit_retained_btree_record::<String, ExactnessNote>(0, operation)?;
                    remapped.exactness.insert(target, exactness);
                }
                (None, None) => {}
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
    pub fn append(&mut self, other: Self) -> Result<(), AnnotationIdentityError> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes_limit(&[], &arena, &policy)?;
        match self.append_for_decode(&ctx, other, "append annotation identities") {
            Ok(result) => result.map_err(AnnotationIdentityError::Collision),
            Err(CodecError::ResourceLimit(limit)) => Err(limit.into()),
            Err(error) => Err(AnnotationIdentityError::Callback(error.to_string())),
        }
    }

    /// Append disjoint tables after charging their destination nodes.
    pub fn append_for_decode(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut other: Self,
        operation: &'static str,
    ) -> Result<Result<(), AnnotationIdentityCollision>, cadmpeg_core::CodecError> {
        for id in other.provenance.keys().chain(other.exactness.keys()) {
            admit_identity_work(ctx, self.provenance.len(), id.len(), operation)?;
            admit_identity_work(ctx, self.exactness.len(), id.len(), operation)?;
            if self.provenance.contains_key(id) || self.exactness.contains_key(id) {
                return Ok(Err(AnnotationIdentityCollision {
                    id: ctx.copy_retained_text(id, operation)?,
                }));
            }
        }
        if !self.provenance.is_empty() && !other.provenance.is_empty() {
            for _entry in self.provenance.iter().chain(other.provenance.iter()) {
                ctx.admit_retained_btree_record::<String, AnnotationProvenance>(0, operation)?;
            }
        }
        if !self.exactness.is_empty() && !other.exactness.is_empty() {
            for _entry in self.exactness.iter().chain(other.exactness.iter()) {
                ctx.admit_retained_btree_record::<String, ExactnessNote>(0, operation)?;
            }
        }
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
    use cadmpeg_core::CodecError;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[test]
    fn admitted_annotation_refuses_each_retained_value_and_tree_node() {
        let id = "test:model:entity#1";
        let stream = "creo:VisibGeom";
        let tag = "face";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let mut total = 0u64;
        for (bytes, operation) in [
            (stream.len(), "annotation stream name"),
            (
                std::mem::size_of::<super::StreamName>() + 2 * std::mem::size_of::<usize>(),
                "annotation stream handles",
            ),
            (
                std::mem::size_of::<(String, super::AnnotationProvenance)>(),
                "collect source provenance",
            ),
            (id.len(), "retain source provenance identity"),
            (tag.len(), "retain source provenance tag"),
            (id.len(), "retain source exactness identity"),
            (
                std::mem::size_of::<(String, super::ExactnessNote)>(),
                "collect source exactness entities",
            ),
        ] {
            total += cadmpeg_core::decode::u64_from_index(bytes);
            policy.limits.max_retained_bytes = total - 1;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut builder = super::AnnotationBuilder::new();
            let error = builder
                .annotate(&ctx, id, stream, 42, tag, super::Exactness::Derived)
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
            (1, "collect source provenance"),
            (2, "collect source exactness entities"),
        ] {
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut builder = super::AnnotationBuilder::new();
            let error = builder
                .annotate(&ctx, id, stream, 42, tag, super::Exactness::Derived)
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
            .annotate(&ctx, id, stream, 42, tag, super::Exactness::Derived)
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
        const MAPPED: &str = "test:point#mapped";
        let run = |collection_limit, retained_limit| {
            let mut builder = super::AnnotationBuilder::new();
            let stream = super::StreamHandle::new(crate::stream_name!("test"));
            builder.note("test:point#0", &stream, 0);
            builder.exactness("test:point#0", super::Exactness::Inferred);
            let mut annotations = builder.build();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = collection_limit;
            policy.limits.max_retained_bytes = retained_limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let outcome = annotations.map_ids_for_decode(
                &ctx,
                |_| ctx.copy_retained_text(MAPPED, "test_annotation_target"),
                "test_annotation_remap",
            );
            (outcome, annotations)
        };
        assert!(
            matches!(run(0, u64::MAX).0, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "test_annotation_remap")
        );
        // The callback's own copy fits; the second table's key copy does not.
        let mapped_bytes = u64::try_from(MAPPED.len()).expect("short identity");
        assert!(
            matches!(run(u64::MAX, mapped_bytes).0, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
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
            let result = target.append_for_decode(&ctx, builder.build(), "test_annotation_append");
            (result, target)
        };
        let (result, target) = run(0);
        assert!(matches!(result, Ok(Ok(()))));
        assert!(target.provenance.contains_key("test:point#0"));
        let (result, target) = run(u64::MAX);
        assert!(matches!(result, Ok(Ok(()))));
        assert!(target.provenance.contains_key("test:point#0"));
    }

    #[test]
    fn annotation_append_rebuilds_only_nonempty_destination_maps() {
        let stream = super::StreamHandle::new(crate::stream_name!("test"));
        let mut target = super::AnnotationBuilder::new();
        target.note("test:point#0", &stream, 0);
        let mut source = super::AnnotationBuilder::new();
        source.note("test:point#1", &stream, 1);
        let mut target = target.build();
        let before = target.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = target
            .append_for_decode(&ctx, source.build(), "append nonempty annotation maps")
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
        );
        assert_eq!(target, before);
    }

    #[test]
    fn exactness_update_and_removal_allocate_no_retained_records() {
        let mut builder = super::AnnotationBuilder::new();
        builder.exactness("test:point#0", super::Exactness::Derived);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        builder
            .exactness_for_decode(&ctx, "test:point#0", super::Exactness::Inferred)
            .unwrap();
        assert_eq!(
            builder.annotations.exactness["test:point#0"].entity(),
            super::Exactness::Inferred
        );
        builder
            .exactness_for_decode(&ctx, "test:point#0", super::Exactness::ByteExact)
            .unwrap();
        builder
            .exactness_for_decode(&ctx, "test:point#missing", super::Exactness::ByteExact)
            .unwrap();
        assert!(builder.annotations.exactness.is_empty());
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
            builder.try_clone_for_decode(&ctx, "test_annotation_copy")
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

    #[test]
    fn annotation_transaction_copy_uses_scoped_storage_until_commit() {
        let mut builder = super::AnnotationBuilder::new();
        let stream = super::StreamHandle::new(crate::stream_name!("test"));
        builder.note("test:point#0", &stream, 7).tag("point");
        builder.derived("test:point#0", "position").unwrap();
        let original = builder.build();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = original.copy_transaction(&ctx, "annotation transaction copy").unwrap();
        assert_eq!(first.annotations(), &original);
        drop(first);
        let second = original.copy_transaction(&ctx, "annotation transaction copy").unwrap();
        let error = second.into_retained().unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.operation == "annotation transaction copy"));
    }

    #[test]
    fn annotation_transaction_refuses_copy_dimensions_and_releases_on_abort() {
        use cadmpeg_core::decode::ResourceDimension;
        let mut builder = super::AnnotationBuilder::new();
        let stream = super::StreamHandle::new(crate::stream_name!("test"));
        builder.note("test:point#0", &stream, 7).tag("point");
        builder.derived("test:point#0", "position").unwrap();
        let original = builder.build();
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("annotation copy dimensions"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let error = original.copy_transaction(&ctx, "annotation transaction copy").unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension && limit.operation == "annotation transaction copy"));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let transaction = original.copy_transaction(&ctx, "annotation transaction copy").unwrap();
        let aborted: Result<((), _), CodecError> = transaction.update(|annotations| {
            assert_eq!(annotations, &original);
            Err(CodecError::malformed("reject candidate"))
        });
        assert!(matches!(aborted, Err(CodecError::Malformed(_))));
        drop(aborted);
        let storage = ctx.reserve_scoped(4096, "all temporary storage released").unwrap();
        drop(storage);
        ctx.finish_session().unwrap();
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
    fn owned_annotation_keys_preserve_note_and_exactness_semantics() {
        let stream = StreamHandle::new(crate::stream_name!("native-stream"));
        let mut formatted = AnnotationBuilder::new();
        let mut owned = AnnotationBuilder::new();
        for (offset, exactness) in [(7, Exactness::Derived), (11, Exactness::ByteExact)] {
            formatted.note("entity", &stream, offset).tag("tag");
            formatted.exactness("entity", exactness);
            owned
                .note_owned(String::from("entity"), &stream, offset)
                .tag("tag");
            owned.exactness_owned(String::from("entity"), exactness);
        }
        let owned = owned.build();
        assert_eq!(owned, formatted.build());
        assert_eq!(owned.provenance["entity"].offset, 11);
        assert_eq!(owned.provenance["entity"].tag.as_deref(), Some("tag"));
        assert!(owned.exactness().is_empty());
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
    fn owned_field_exactness_matches_formatted_field_exactness() {
        for entity_first in [false, true] {
            let mut borrowed = AnnotationBuilder::new();
            let mut owned = AnnotationBuilder::new();
            for exactness in [
                Exactness::Derived,
                Exactness::Inferred,
                Exactness::ByteExact,
            ] {
                if entity_first {
                    borrowed.exactness("test:point#1", exactness);
                    owned.exactness_owned("test:point#1".to_string(), exactness);
                }
                borrowed
                    .field_exactness("test:point#1", "position.x", exactness)
                    .unwrap();
                owned
                    .field_exactness_owned(
                        "test:point#1".to_string(),
                        "position.x".to_string(),
                        exactness,
                    )
                    .unwrap();
                assert_eq!(borrowed.annotations(), owned.annotations());
            }
            let before = owned.annotations().clone();
            assert!(owned
                .field_exactness_owned(
                    "test:point#1".to_string(),
                    String::new(),
                    Exactness::Derived
                )
                .is_err());
            assert_eq!(owned.annotations(), &before);
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
            assert!(
                error.to_string().contains("field name cannot be empty"),
                "{error}"
            );
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
