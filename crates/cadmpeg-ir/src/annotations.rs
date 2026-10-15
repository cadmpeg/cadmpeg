// SPDX-License-Identifier: Apache-2.0
//! Sparse document-wide provenance and exactness annotations.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::Arc;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;

use crate::provenance::{AnnotationProvenance, Exactness, StreamName};

mod sparse;
pub use sparse::{PreparedAnnotationDelta, SparseAnnotationTransaction};

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

/// Candidate annotation tables held until a transaction commits or is discarded.
#[derive(Debug)]
pub struct AnnotationTransaction {
    annotations: Annotations,
}

impl AnnotationTransaction {
    /// Read the candidate annotation tables.
    pub fn annotations(&self) -> &Annotations {
        &self.annotations
    }

    /// Apply mutations to the candidate tables. A failed mutation discards them.
    pub fn update<T, E: From<CodecError>>(
        mut self,
        apply: impl FnOnce(&mut Annotations) -> Result<T, E>,
    ) -> Result<(T, Self), E> {
        let result = apply(&mut self.annotations);
        if result.is_err() {
            self.annotations = Annotations::default();
        }
        Ok((result?, self))
    }

    /// Transfer the candidate tables to the document.
    pub fn into_retained(self) -> Result<Annotations, CodecError> {
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Allocate one shared stream name under the caller's decode budget.
    ///
    /// ```compile_fail
    /// fn invalid(ctx: &cadmpeg_core::decode::DecodeContext<'_>) {
    ///     let _ = cadmpeg_ir::annotations::StreamHandle::new(ctx, "", "stream handle");
    /// }
    /// ```
    pub fn new(
        _ctx: &DecodeContext<'_>,
        stream: StreamName,
        _operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self(Arc::new(stream)))
    }
}

/// Update policy for one annotation builder.
pub trait AnnotationStorage {
    /// Whether a failed update discards temporary annotation values.
    const SCOPED: bool;
}

impl AnnotationStorage for () {
    const SCOPED: bool = false;
}

/// A transaction builder: a failed update discards its candidate tables.
#[derive(Debug)]
pub struct TransactionStorage<'ctx>(std::marker::PhantomData<&'ctx ()>);

impl AnnotationStorage for TransactionStorage<'_> {
    const SCOPED: bool = true;
}

/// Incrementally constructs provenance and exactness with owned storage.
#[derive(Debug, Default, Clone)]
pub struct AnnotationBuilder<Storage = ()> {
    state: AnnotationState,
    storage: std::marker::PhantomData<Storage>,
}

impl AnnotationBuilder {
    /// Create an empty retained annotation builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Continue building retained annotation tables.
    pub fn resume(annotations: Annotations) -> Self {
        Self {
            state: AnnotationState::resume(annotations),
            storage: std::marker::PhantomData,
        }
    }

    /// Finish a builder whose storage is retained.
    pub fn build(self) -> Annotations {
        self.state.build()
    }
}

impl<Storage: AnnotationStorage> AnnotationBuilder<Storage> {
    fn update<E: From<CodecError>>(
        &mut self,
        apply: impl FnOnce(&mut AnnotationState) -> Result<(), E>,
    ) -> Result<(), E> {
        let result = apply(&mut self.state);
        if Storage::SCOPED && result.is_err() {
            self.state.annotations = Annotations::default();
        }
        result
    }

    /// Borrow the annotation tables held by this builder.
    pub fn annotations(&self) -> &Annotations {
        self.state.annotations()
    }

    /// Copy the tables into a transaction builder.
    pub fn copy_transaction<'ctx>(
        &self,
        _ctx: &'ctx DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<AnnotationBuilder<TransactionStorage<'ctx>>, CodecError> {
        Ok(AnnotationBuilder {
            state: AnnotationState::resume(self.state.annotations.clone()),
            storage: std::marker::PhantomData,
        })
    }

    /// Record provenance and exactness within this builder's storage policy.
    pub fn annotate(
        &mut self,
        _ctx: &DecodeContext<'_>,
        id: impl Display,
        stream: impl Display,
        offset: u64,
        tag: &str,
        exactness: Exactness,
    ) -> Result<(), CodecError> {
        self.update(|state| state.annotate(id, stream, offset, tag, exactness))
    }

    /// Record provenance under the caller's decode context.
    pub fn note(
        &mut self,
        _ctx: &DecodeContext<'_>,
        id: impl Display,
        stream: &StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) -> Result<(), CodecError> {
        self.update(|state| state.note(id, stream, offset, tag))
    }

    /// Record provenance with an already admitted owned identity.
    pub fn note_owned(
        &mut self,
        id: String,
        stream: &StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) -> Result<(), CodecError> {
        self.update(|state| {
            state.insert_provenance(std::borrow::Cow::Owned(id), stream, offset, tag)
        })
    }

    /// Set entity exactness under the caller's context.
    pub fn exactness(
        &mut self,
        _ctx: &DecodeContext<'_>,
        id: impl Display,
        exactness: Exactness,
    ) -> Result<&mut Self, CodecError> {
        self.update(|state| {
            state.exactness(id, exactness);
            Ok::<(), CodecError>(())
        })?;
        Ok(self)
    }

    /// Set exactness with an already admitted identity.
    pub fn exactness_owned(
        &mut self,
        id: String,
        exactness: Exactness,
    ) -> Result<&mut Self, CodecError> {
        self.update(|state| {
            state.exactness_owned(id, exactness);
            Ok::<(), CodecError>(())
        })?;
        Ok(self)
    }

    /// Mark a field as derived under the caller's context.
    pub fn derived(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: impl Display,
        field: &str,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.field_exactness(ctx, id, field, Exactness::Derived)
    }

    /// Set field exactness under the caller's context.
    pub fn field_exactness<'f>(
        &mut self,
        _ctx: &DecodeContext<'_>,
        id: impl Display,
        field: impl Into<std::borrow::Cow<'f, str>>,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.update(|state| state.field_exactness(id, field, exactness).map(|_| ()))?;
        Ok(self)
    }

    /// Set field exactness with already admitted owned keys.
    pub fn field_exactness_owned(
        &mut self,
        id: String,
        field: String,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.update(|state| {
            state
                .field_exactness_owned(id, field, exactness)
                .map(|_| ())
        })?;
        Ok(self)
    }

    /// Retain exactness selected by identity, deciding every entry first.
    pub fn retain_exactness(
        &mut self,
        _ctx: &DecodeContext<'_>,
        keep: impl FnMut(&str) -> Result<bool, CodecError>,
    ) -> Result<&mut Self, CodecError> {
        self.update(|state| retain_identity_entries(&mut state.annotations.exactness, keep))?;
        Ok(self)
    }

    /// Remove annotations for an entity.
    pub fn remove_entity(&mut self, _ctx: &DecodeContext<'_>, id: &str) -> Result<(), CodecError> {
        self.update(|state| {
            state.annotations.provenance.remove(id);
            state.annotations.exactness.remove(id);
            Ok(())
        })
    }
}

impl AnnotationBuilder<TransactionStorage<'_>> {
    /// Transfer a transaction's tables to a retained builder.
    pub fn into_retained(self) -> Result<AnnotationBuilder, CodecError> {
        Ok(AnnotationBuilder::resume(self.state.annotations))
    }
}

/// Incrementally constructs document provenance and exactness annotations.
#[derive(Debug, Default, Clone)]
struct AnnotationState {
    annotations: Annotations,
}

impl AnnotationState {
    /// Continue building an existing annotation set.
    fn resume(annotations: Annotations) -> Self {
        Self { annotations }
    }

    /// Borrow the annotations accumulated so far.
    #[must_use]
    fn annotations(&self) -> &Annotations {
        &self.annotations
    }

    /// Record provenance and entity exactness under the decode budget.
    fn annotate(
        &mut self,
        id: impl Display,
        stream: impl Display,
        offset: u64,
        tag: &str,
        exactness: Exactness,
    ) -> Result<(), CodecError> {
        let id = id.to_string();
        let stream = format!("{stream}");
        let stream = StreamName::try_from(stream)
            .map_err(|_| CodecError::malformed("annotation stream name is empty"))?;
        let stream = StreamHandle(Arc::new(stream));
        self.note(&id, &stream, offset, Some(tag))?;
        self.exactness(&id, exactness);
        Ok(())
    }

    /// Format a temporary lookup identity before admitting its provenance.
    fn note(
        &mut self,
        id: impl Display,
        stream: &StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) -> Result<(), CodecError> {
        let id = id.to_string();
        self.insert_provenance(std::borrow::Cow::Borrowed(&id), stream, offset, tag)
    }

    /// Admit one provenance record, moving an owned identity when it is new.
    fn insert_provenance(
        &mut self,
        mut id: std::borrow::Cow<'_, str>,
        stream: &StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) -> Result<(), CodecError> {
        let previous = self.annotations.provenance.get_mut(id.as_ref());
        let provenance = || {
            let tag = tag.map(str::to_owned);

            Ok::<_, CodecError>(AnnotationProvenance::annotation(
                stream.0.clone(),
                offset,
                tag,
            ))
        };
        if let Some(previous) = previous {
            *previous = provenance()?;
        } else {
            if let std::borrow::Cow::Borrowed(text) = id {
                id = std::borrow::Cow::Owned(text.to_owned());
            }
            // The lookup proved absence. Admit the new slot without another lookup.
            self.annotations
                .provenance
                .insert(id.into_owned(), provenance()?);
        }
        Ok(())
    }

    /// Set entity exactness after admitting any new retained record.
    fn exactness(&mut self, id: impl Display, exactness: Exactness) -> &mut Self {
        let id = id.to_string();
        self.insert_exactness(std::borrow::Cow::Borrowed(&id), exactness)
    }

    /// Move an admitted identity into an exactness entry after destination admission.
    fn exactness_owned(&mut self, id: String, exactness: Exactness) -> &mut Self {
        self.insert_exactness(std::borrow::Cow::Owned(id), exactness)
    }

    fn insert_exactness(
        &mut self,
        id: std::borrow::Cow<'_, str>,
        exactness: Exactness,
    ) -> &mut Self {
        if let Some(note) = self.annotations.exactness.get_mut(id.as_ref()) {
            let mut remove = exactness == Exactness::ByteExact;
            if remove {
                for value in note.fields().values() {
                    if *value != exactness {
                        remove = false;
                        break;
                    }
                }
            }
            if remove {
                // Removal performs its own key comparisons. Refuse before changing fields.
                self.annotations.exactness.remove(id.as_ref());
            } else {
                note.fields_mut().retain(|_, value| *value != exactness);
                let fields = std::mem::take(note.fields_mut());
                *note = match Inexactness::try_from(exactness) {
                    Ok(entity) => ExactnessNote::Entity { entity, fields },
                    Err(_) => ExactnessNote::Fields {
                        fields: NonEmptyMap(fields),
                    },
                };
            }
        } else if let Ok(entity) = Inexactness::try_from(exactness) {
            let id = match id {
                std::borrow::Cow::Owned(id) => id,
                std::borrow::Cow::Borrowed(id) => id.to_owned(),
            };
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

    /// Set field exactness after admitting retained keys and records.
    fn field_exactness<'f>(
        &mut self,
        id: impl Display,
        field: impl Into<std::borrow::Cow<'f, str>>,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        let id = id.to_string();
        self.insert_field_exactness(id, field.into(), exactness)
    }

    /// Admit map records for already owned identity and field strings.
    fn field_exactness_owned(
        &mut self,
        id: String,
        field: String,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.insert_field_exactness(id, std::borrow::Cow::Owned(field), exactness)
    }

    fn insert_field_exactness(
        &mut self,
        id: String,
        field: std::borrow::Cow<'_, str>,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        if field.is_empty() {
            return Err(AnnotationFieldError::Invalid(
                "an exactness field name cannot be empty",
            ));
        }
        let field = FieldName(field.into_owned());
        self.set_field_exactness(id, field, exactness);
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

    /// Finish building and return the annotation tables.
    fn build(self) -> Annotations {
        self.annotations
    }
}

/// Complete all fallible decisions before changing an identity-keyed map.
pub(crate) fn retain_identity_entries<V>(
    entries: &mut BTreeMap<String, V>,
    mut keep: impl FnMut(&str) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let mut decisions = Vec::new();
    for identity in entries.keys() {
        decisions.push(keep(identity)?);
    }
    let mut decisions = decisions.into_iter();
    entries.retain(|_, _| decisions.next() == Some(true));
    Ok(())
}

impl Annotations {
    /// Copy the annotation tables into a transaction.
    pub fn copy_transaction(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<AnnotationTransaction, CodecError> {
        Ok(AnnotationTransaction {
            annotations: self.clone(),
        })
    }

    /// Retain provenance selected by identity, deciding every entry first.
    pub fn retain_provenance(
        &mut self,
        _ctx: &DecodeContext<'_>,
        keep: impl FnMut(&str) -> Result<bool, CodecError>,
    ) -> Result<(), CodecError> {
        retain_identity_entries(&mut self.provenance, keep)
    }

    /// Remap both tables. A collision leaves both tables unchanged.
    pub fn map_ids(
        &mut self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut map: impl FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
        _operation: &'static str,
    ) -> Result<Result<(), AnnotationIdentityCollision>, cadmpeg_core::CodecError> {
        enum Destination {
            Provenance(String),
            Exactness(String),
            Both {
                provenance: String,
                exactness: String,
            },
        }
        let mut ids = std::collections::BTreeSet::new();
        for id in self.provenance.keys().chain(self.exactness.keys()) {
            ids.insert(id);
        }
        let mut targets = std::collections::BTreeSet::new();
        let mut remapping = Vec::new();
        for id in ids {
            let target = map(id)?;
            if targets.contains(&target) {
                return Ok(Err(AnnotationIdentityCollision { id: target }));
            }
            targets.insert(target.clone());
            remapping.push((id.to_owned(), target));
        }
        let mut destinations = Vec::new();
        for (id, target) in remapping {
            let destination = match (
                self.provenance.contains_key(&id),
                self.exactness.contains_key(&id),
            ) {
                (true, true) => Destination::Both {
                    provenance: target.clone(),
                    exactness: target,
                },
                (true, false) => Destination::Provenance(target),
                (false, true) => Destination::Exactness(target),
                (false, false) => continue,
            };

            destinations.push((id, destination));
        }
        let mut remapped = Self::default();
        for (id, destination) in destinations {
            match destination {
                Destination::Provenance(target) => {
                    if let Some(provenance) = self.provenance.remove(&id) {
                        remapped.provenance.insert(target, provenance);
                    }
                }
                Destination::Exactness(target) => {
                    if let Some(exactness) = self.exactness.remove(&id) {
                        remapped.exactness.insert(target, exactness);
                    }
                }
                Destination::Both {
                    provenance,
                    exactness,
                } => {
                    if let Some(value) = self.provenance.remove(&id) {
                        remapped.provenance.insert(provenance, value);
                    }
                    if let Some(value) = self.exactness.remove(&id) {
                        remapped.exactness.insert(exactness, value);
                    }
                }
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

    /// Append disjoint tables.
    pub fn append(
        &mut self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut other: Self,
        _operation: &'static str,
    ) -> Result<Result<(), AnnotationIdentityCollision>, cadmpeg_core::CodecError> {
        for id in other.provenance.keys().chain(other.exactness.keys()) {
            if self.provenance.contains_key(id) || self.exactness.contains_key(id) {
                return Ok(Err(AnnotationIdentityCollision { id: id.to_owned() }));
            }
        }
        self.provenance.append(&mut other.provenance);
        self.exactness.append(&mut other.exactness);
        Ok(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn annotation_remap_moves_nested_records_to_the_mapped_identity() {
        const MAPPED: &str = "test:point#mapped";
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut builder = super::AnnotationBuilder::new();
        let stream =
            super::StreamHandle::new(&ctx, crate::stream_name!("test"), "fixture stream handle")
                .unwrap();
        builder
            .note(&ctx, "test:point#0", &stream, 0, None)
            .unwrap();
        builder
            .exactness(&ctx, "test:point#0", super::Exactness::Inferred)
            .unwrap();
        let mut annotations = builder.build();
        let outcome = annotations.map_ids(&ctx, |_| Ok(MAPPED.to_owned()), "test_annotation_remap");
        assert!(matches!(outcome, Ok(Ok(()))));
        assert!(annotations.provenance.contains_key(MAPPED));
    }

    #[test]
    fn exactness_update_and_removal_allocate_no_retained_records() {
        let mut builder = super::AnnotationBuilder::new();
        builder
            .exactness(
                &cadmpeg_test_support::service_decode_context(),
                "test:point#0",
                super::Exactness::Derived,
            )
            .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        builder
            .exactness(&ctx, "test:point#0", super::Exactness::Inferred)
            .unwrap();
        assert_eq!(
            builder.state.annotations.exactness["test:point#0"].entity(),
            super::Exactness::Inferred
        );
        builder
            .exactness(&ctx, "test:point#0", super::Exactness::ByteExact)
            .unwrap();
        builder
            .exactness(&ctx, "test:point#missing", super::Exactness::ByteExact)
            .unwrap();
        assert!(builder.state.annotations.exactness.is_empty());
    }

    #[test]
    fn annotation_copy_preserves_nested_entries() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut builder = super::AnnotationBuilder::new();
        let stream =
            super::StreamHandle::new(&ctx, crate::stream_name!("test"), "fixture stream handle")
                .unwrap();
        builder
            .note(&ctx, "test:point#0", &stream, 7, Some("point"))
            .unwrap();
        builder
            .derived(&ctx, "test:point#0", "position")
            .expect("field path");
        let copied = builder
            .copy_transaction(&ctx, "test_annotation_copy")
            .and_then(super::AnnotationBuilder::into_retained)
            .expect("copy");
        assert_eq!(copied.build(), builder.clone().build());
        let original = builder.build();
        let transaction = original
            .copy_transaction(&ctx, "annotation transaction copy")
            .unwrap();
        assert_eq!(transaction.annotations(), &original);
    }

    #[test]
    fn annotate_matches_note_and_exactness() {
        let id = "test:model:entity#1";
        let stream = "creo:VisibGeom";
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut annotated = super::AnnotationBuilder::new();
        annotated
            .annotate(&ctx, id, stream, 42, "face", super::Exactness::Derived)
            .unwrap();
        let handle = super::StreamHandle::new(
            &ctx,
            super::StreamName::try_from(stream.to_owned()).unwrap(),
            "fixture stream handle",
        )
        .unwrap();
        let mut expected = super::AnnotationBuilder::new();
        expected.note(&ctx, id, &handle, 42, Some("face")).unwrap();
        expected
            .exactness(&ctx, id, super::Exactness::Derived)
            .unwrap();
        assert_eq!(annotated.build(), expected.build());
    }

    mod entity_exactness;
    mod identity_merges;
    mod provenance;
    mod retention;
    mod stream_handles;

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
        let first = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("f3d:Breps.BlobParts/body.smbh"),
            "fixture stream handle",
        )
        .unwrap();
        let second = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("f3d:Breps.BlobParts/body.smbh"),
            "fixture stream handle",
        )
        .unwrap();

        assert_eq!(first, second);
        builder
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "f3d:body#0",
                &first,
                42,
                Some("body"),
            )
            .unwrap();

        let annotations = builder.build();
        assert_eq!(annotations.stream_count(), 1);
        let provenance = &annotations.provenance["f3d:body#0"];
        assert_eq!(provenance.stream(), "f3d:Breps.BlobParts/body.smbh");
        assert_eq!(provenance.offset, 42);
        assert_eq!(provenance.tag.as_deref(), Some("body"));
    }

    #[test]
    fn owned_annotation_keys_preserve_note_and_exactness_semantics() {
        let stream = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("native-stream"),
            "fixture stream handle",
        )
        .unwrap();
        let mut formatted = AnnotationBuilder::new();
        let mut owned = AnnotationBuilder::new();
        for (offset, exactness) in [(7, Exactness::Derived), (11, Exactness::ByteExact)] {
            formatted
                .note(
                    &cadmpeg_test_support::service_decode_context(),
                    "entity",
                    &stream,
                    offset,
                    Some("tag"),
                )
                .unwrap();
            formatted
                .exactness(
                    &cadmpeg_test_support::service_decode_context(),
                    "entity",
                    exactness,
                )
                .unwrap();
            owned
                .note_owned(String::from("entity"), &stream, offset, Some("tag"))
                .unwrap();
            owned
                .exactness_owned(String::from("entity"), exactness)
                .unwrap();
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
        let first = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("first"),
            "fixture stream handle",
        )
        .unwrap();
        let second = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("second"),
            "fixture stream handle",
        )
        .unwrap();

        builder
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "entity",
                &first,
                7,
                Some("stale"),
            )
            .unwrap();
        builder
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "entity",
                &second,
                11,
                None,
            )
            .unwrap();

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
        let stream = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!(" \t"),
            "fixture stream handle",
        )
        .unwrap();
        builder
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "whitespace",
                &stream,
                3,
                None,
            )
            .unwrap();

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
        let stream = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("f3d:Breps.BlobParts/body.smbh"),
            "fixture stream handle",
        )
        .unwrap();
        builder
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "f3d:body#0",
                &stream,
                42,
                Some("body"),
            )
            .unwrap();
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
        let handle = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("first"),
            "fixture stream handle",
        )
        .unwrap();
        let mut second = AnnotationBuilder::new();
        second
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "foreign",
                &handle,
                1,
                None,
            )
            .unwrap();
        let mut cloned = first.clone();
        cloned
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "cloned",
                &handle,
                2,
                None,
            )
            .unwrap();
        let mut resumed = AnnotationBuilder::resume(first.build());
        resumed
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "resumed",
                &handle,
                3,
                None,
            )
            .unwrap();
        let mut empty = AnnotationBuilder::new();
        empty
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "empty",
                &handle,
                4,
                None,
            )
            .unwrap();
        let mut same_name = AnnotationBuilder::new();
        same_name
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "same-name",
                &handle,
                5,
                None,
            )
            .unwrap();
        assert_eq!(same_name.state.annotations.stream_count(), 1);
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
                    borrowed
                        .exactness(
                            &cadmpeg_test_support::service_decode_context(),
                            "test:point#1",
                            exactness,
                        )
                        .unwrap();
                    owned
                        .exactness_owned("test:point#1".to_string(), exactness)
                        .unwrap();
                }
                borrowed
                    .field_exactness(
                        &cadmpeg_test_support::service_decode_context(),
                        "test:point#1",
                        "position.x",
                        exactness,
                    )
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
            .derived(
                &cadmpeg_test_support::service_decode_context(),
                "f3d:edge#0",
                "param_range",
            )
            .expect("nonempty exactness field")
            .exactness(
                &cadmpeg_test_support::service_decode_context(),
                "f3d:edge#0",
                Exactness::Inferred,
            )
            .unwrap();
        builder
            .field_exactness(
                &cadmpeg_test_support::service_decode_context(),
                "f3d:edge#0",
                "param_range",
                Exactness::ByteExact,
            )
            .expect("nonempty exactness field");

        let expected_fields = BTreeMap::from([(
            FieldName::try_from("param_range".to_string()).expect("nonempty path"),
            Exactness::ByteExact,
        )]);
        assert_eq!(
            builder.state.annotations.exactness["f3d:edge#0"],
            ExactnessNote::Entity {
                entity: Inexactness::Inferred,
                fields: expected_fields,
            }
        );

        builder
            .exactness(
                &cadmpeg_test_support::service_decode_context(),
                "f3d:edge#0",
                Exactness::ByteExact,
            )
            .unwrap();
        assert!(builder.state.annotations.exactness.is_empty());
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
                builder
                    .exactness(
                        &cadmpeg_test_support::service_decode_context(),
                        "nx:model:surface#1",
                        Exactness::Derived,
                    )
                    .unwrap();
            }
            builder
                .derived(
                    &cadmpeg_test_support::service_decode_context(),
                    "nx:model:surface#1",
                    "geometry",
                )
                .expect("nonempty exactness field");
            if !entity_first {
                builder
                    .exactness(
                        &cadmpeg_test_support::service_decode_context(),
                        "nx:model:surface#1",
                        Exactness::Derived,
                    )
                    .unwrap();
            }
            assert_eq!(
                serde_json::to_value(&builder.state.annotations.exactness["nx:model:surface#1"])
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
        let stream = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("catia:e5_0d_03"),
            "fixture stream handle",
        )
        .unwrap();
        builder
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "catia:e5:curve#0",
                &stream,
                42,
                Some("circle"),
            )
            .unwrap();
        builder
            .derived(
                &cadmpeg_test_support::service_decode_context(),
                "catia:e5:curve#0",
                "geometry",
            )
            .expect("nonempty exactness field");

        builder
            .remove_entity(
                &cadmpeg_test_support::service_decode_context(),
                "catia:e5:curve#0",
            )
            .unwrap();

        assert!(!builder
            .state
            .annotations
            .provenance
            .contains_key("catia:e5:curve#0"));
        assert!(!builder
            .state
            .annotations
            .exactness
            .contains_key("catia:e5:curve#0"));
    }

    #[test]
    fn exactness_field_admission_rejects_empty_keys_without_mutation() {
        let id = "test:model:point#0";
        let mut builder = AnnotationBuilder::new();
        builder
            .exactness(
                &cadmpeg_test_support::service_decode_context(),
                id,
                Exactness::Inferred,
            )
            .unwrap();
        builder
            .derived(
                &cadmpeg_test_support::service_decode_context(),
                id,
                "position.x",
            )
            .expect("nonempty path");
        let before =
            serde_json::to_value(&builder.state.annotations).expect("serialize annotations");
        for exactness in [
            Exactness::ByteExact,
            Exactness::Derived,
            Exactness::Inferred,
        ] {
            let error = builder
                .field_exactness(
                    &cadmpeg_test_support::service_decode_context(),
                    id,
                    "",
                    exactness,
                )
                .expect_err("empty path");
            assert!(
                error.to_string().contains("field name cannot be empty"),
                "{error}"
            );
            assert_eq!(
                serde_json::to_value(&builder.state.annotations).expect("serialize annotations"),
                before
            );
        }
        assert!(builder
            .derived(&cadmpeg_test_support::service_decode_context(), id, "")
            .is_err());
        assert_eq!(
            serde_json::to_value(&builder.state.annotations).expect("serialize annotations"),
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
                builder
                    .exactness(
                        &cadmpeg_test_support::service_decode_context(),
                        id,
                        Exactness::Inferred,
                    )
                    .unwrap();
            }
            builder
                .derived(
                    &cadmpeg_test_support::service_decode_context(),
                    id,
                    "position",
                )
                .expect("nonempty field");
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

#[cfg(test)]
mod builder_storage_tests {
    use super::{AnnotationBuilder, Exactness, StreamHandle};
    #[test]
    fn annotation_builder_transaction_leaves_the_original_unchanged() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let stream =
            StreamHandle::new(&ctx, crate::stream_name!("test"), "fixture stream handle").unwrap();
        let mut original = AnnotationBuilder::new();
        original
            .note(&ctx, "test:model:point#prior", &stream, 0, Some("prior"))
            .unwrap();
        let mut candidate = original
            .copy_transaction(&ctx, "annotation candidate storage")
            .unwrap();
        candidate
            .annotate(
                &ctx,
                "test:model:point#candidate",
                "candidate stream",
                7,
                "candidate tag",
                Exactness::Derived,
            )
            .unwrap();
        candidate
            .derived(&ctx, "test:model:point#candidate", "position")
            .unwrap();
        assert!(candidate
            .annotations()
            .provenance
            .contains_key("test:model:point#candidate"));
        assert_eq!(original.annotations().provenance.len(), 1);
    }

    #[test]
    fn annotation_builder_transaction_commits_the_same_tables() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let stream = StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!("test"),
            "fixture stream handle",
        )
        .unwrap();
        let mut original = AnnotationBuilder::new();
        original
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "test:model:point#prior",
                &stream,
                0,
                Some("prior"),
            )
            .unwrap();
        let mut candidate = original
            .copy_transaction(&ctx, "annotation candidate storage")
            .unwrap();
        candidate
            .annotate(
                &ctx,
                "test:model:point#candidate",
                "candidate stream",
                7,
                "candidate tag",
                Exactness::Derived,
            )
            .unwrap();
        let mut expected = original.clone();
        expected
            .annotate(
                &ctx,
                "test:model:point#candidate",
                "candidate stream",
                7,
                "candidate tag",
                Exactness::Derived,
            )
            .unwrap();
        assert_eq!(candidate.into_retained().unwrap().build(), expected.build());
        ctx.finish_session().unwrap();
    }
}

mod serialization;
