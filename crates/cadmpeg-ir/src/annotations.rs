// SPDX-License-Identifier: Apache-2.0
//! Sparse document-wide provenance and exactness annotations.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::Arc;

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_core::CodecError;

use crate::provenance::{AnnotationProvenance, Exactness, StreamName};

mod key;
mod named;
mod sparse;
pub use key::{AnnotationKey, AnnotationTarget, Qualifier};
pub(crate) use named::{delta_records, deltas, design_and_process_records, Removed};
pub use sparse::{PreparedAnnotationDelta, SparseAnnotationTransaction};

/// Document-wide provenance and exactness tables keyed by annotation key.
///
/// The key of a top-level entity is its globally unique entity id. The key
/// of a record inside the delta of an evaluation or a process state is the
/// id of that evaluation or process state, one space, and the record's id.
/// Validation reports a key whose qualifier names no evaluation or process
/// state with a delta, or whose record that delta does not hold, as it
/// reports a key that names no entity.
///
/// An entity absent from `exactness` is byte-exact.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Annotations {
    /// Source location for each annotated entity.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "BTreeMap<String, AnnotationProvenance>")
    )]
    pub provenance: BTreeMap<AnnotationKey, AnnotationProvenance>,
    /// Non-byte-exact entity or field annotations.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeMap<String, ExactnessNote>"))]
    exactness: BTreeMap<AnnotationKey, ExactnessNote>,
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
    pub fn into_retained(self) -> Annotations {
        self.annotations
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
pub struct FieldName(std::borrow::Cow<'static, str>);

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
    /// The field path `name`.
    ///
    /// # Panics
    ///
    /// Panics where `name` is empty; in a constant, the crate does not build.
    #[must_use]
    pub const fn from_static(name: &'static str) -> Self {
        assert!(!name.is_empty(), "an exactness field name cannot be empty");
        Self(std::borrow::Cow::Borrowed(name))
    }

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
        Ok(Self(std::borrow::Cow::Owned(name)))
    }
}

impl From<FieldName> for String {
    fn from(name: FieldName) -> Self {
        name.0.into_owned()
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExactnessNote {
    /// The entity itself is not byte-exact; listed fields override it.
    Entity {
        /// Exactness of the entity except where overridden by `fields`.
        entity: Inexactness,
        /// Exactness overrides keyed by serde field path.
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
    /// Share one admitted stream name.
    ///
    /// ```compile_fail
    /// let _ = cadmpeg_ir::annotations::StreamHandle::new("");
    /// ```
    pub fn new(stream: StreamName) -> Self {
        Self(Arc::new(stream))
    }

    /// The shared stream name.
    #[must_use]
    pub fn name(&self) -> &StreamName {
        &self.0
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
pub struct TransactionStorage;

impl AnnotationStorage for TransactionStorage {
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
    pub fn copy_transaction(&self) -> AnnotationBuilder<TransactionStorage> {
        AnnotationBuilder {
            state: AnnotationState::resume(self.state.annotations.clone()),
            storage: std::marker::PhantomData,
        }
    }

    /// Record provenance and exactness within this builder's storage policy.
    pub fn annotate(
        &mut self,
        key: impl Into<AnnotationKey>,
        stream: impl Display,
        offset: u64,
        tag: &str,
        exactness: Exactness,
    ) -> Result<(), CodecError> {
        self.update(|state| state.annotate(key.into(), stream, offset, tag, exactness))
    }

    /// Record provenance.
    pub fn note(
        &mut self,
        key: impl Into<AnnotationKey>,
        stream: &StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) {
        self.state
            .insert_provenance(key.into(), stream, offset, tag);
    }

    /// Set entity exactness.
    pub fn exactness(&mut self, key: impl Into<AnnotationKey>, exactness: Exactness) -> &mut Self {
        self.state.insert_exactness(key.into(), exactness);
        self
    }

    /// Mark a field as derived.
    pub fn derived(
        &mut self,
        key: impl Into<AnnotationKey>,
        field: &str,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.field_exactness(key, field, Exactness::Derived)
    }

    /// Mark the field `field` of `key` derived.
    pub fn derived_field(&mut self, key: impl Into<AnnotationKey>, field: FieldName) -> &mut Self {
        self.state
            .set_field_exactness(key.into(), field, Exactness::Derived);
        self
    }

    /// Set field exactness.
    pub fn field_exactness<'f>(
        &mut self,
        key: impl Into<AnnotationKey>,
        field: impl Into<std::borrow::Cow<'f, str>>,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        self.update(|state| {
            state
                .insert_field_exactness(key.into(), field.into(), exactness)
                .map(|_| ())
        })?;
        Ok(self)
    }

    /// Retain the exactness entries whose owner [`AnnotationKey::owner`]
    /// `keep` selects, in increasing key order.
    pub fn retain_exactness(&mut self, mut keep: impl FnMut(&str) -> bool) -> &mut Self {
        self.state
            .annotations
            .exactness
            .retain(|key, _| keep(key.owner()));
        self
    }

    /// Remove annotations for an entity.
    pub fn remove_entity(&mut self, id: &str) {
        self.state.annotations.provenance.remove(id);
        self.state.annotations.exactness.remove(id);
    }
}

impl AnnotationBuilder<TransactionStorage> {
    /// Transfer a transaction's tables to a retained builder.
    pub fn into_retained(self) -> AnnotationBuilder {
        AnnotationBuilder::resume(self.state.annotations)
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

    /// Record provenance and entity exactness for one stream offset.
    fn annotate(
        &mut self,
        key: AnnotationKey,
        stream: impl Display,
        offset: u64,
        tag: &str,
        exactness: Exactness,
    ) -> Result<(), CodecError> {
        let stream = format!("{stream}");
        let stream = StreamName::try_from(stream)
            .map_err(|_| CodecError::malformed("annotation stream name is empty"))?;
        let stream = StreamHandle(Arc::new(stream));
        self.insert_exactness(key.clone(), exactness);
        self.insert_provenance(key, &stream, offset, Some(tag));
        Ok(())
    }

    /// Replace the provenance of `key`.
    fn insert_provenance(
        &mut self,
        key: AnnotationKey,
        stream: &StreamHandle,
        offset: u64,
        tag: Option<&str>,
    ) {
        let provenance =
            AnnotationProvenance::annotation(stream.0.clone(), offset, tag.map(str::to_owned));
        self.annotations.provenance.insert(key, provenance);
    }

    fn insert_exactness(&mut self, key: AnnotationKey, exactness: Exactness) -> &mut Self {
        if let Some(note) = self.annotations.exactness.get_mut(&key) {
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
                self.annotations.exactness.remove(&key);
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
            self.annotations.exactness.insert(
                key,
                ExactnessNote::Entity {
                    entity,
                    fields: BTreeMap::new(),
                },
            );
        }
        self
    }

    fn insert_field_exactness(
        &mut self,
        key: AnnotationKey,
        field: std::borrow::Cow<'_, str>,
        exactness: Exactness,
    ) -> Result<&mut Self, AnnotationFieldError> {
        if field.is_empty() {
            return Err(AnnotationFieldError::Invalid(
                "an exactness field name cannot be empty",
            ));
        }
        let field = FieldName(std::borrow::Cow::Owned(field.into_owned()));
        self.set_field_exactness(key, field, exactness);
        Ok(self)
    }

    fn set_field_exactness(&mut self, key: AnnotationKey, field: FieldName, exactness: Exactness) {
        if exactness == Exactness::ByteExact {
            if let Some(note) = self.annotations.exactness.get_mut(&key) {
                match note {
                    ExactnessNote::Entity { fields, .. } => {
                        fields.insert(field, exactness);
                    }
                    ExactnessNote::Fields { fields } => {
                        fields.0.remove(&field);
                    }
                }
                if matches!(note, ExactnessNote::Fields { fields } if fields.0.is_empty()) {
                    self.annotations.exactness.remove(&key);
                }
            }
        } else {
            match self.annotations.exactness.entry(key) {
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

impl Annotations {
    /// Copy the annotation tables into a transaction.
    pub fn copy_transaction(&self) -> AnnotationTransaction {
        AnnotationTransaction {
            annotations: self.clone(),
        }
    }

    /// Retain the provenance entries whose owner [`AnnotationKey::owner`]
    /// `keep` selects, in increasing key order.
    pub fn retain_provenance(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.provenance.retain(|key, _| keep(key.owner()));
    }

    /// Retain the provenance and exactness entries whose owner
    /// [`AnnotationKey::owner`] `keep` selects.
    pub fn retain_entities(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.provenance.retain(|key, _| keep(key.owner()));
        self.exactness.retain(|key, _| keep(key.owner()));
    }

    /// The annotations of each entity that `keep` selects, keyed as the
    /// record of that id in the delta of `qualifier` (§3.2).
    #[must_use]
    pub fn in_delta(&self, qualifier: &impl Qualifier, mut keep: impl FnMut(&str) -> bool) -> Self {
        let mut out = Self::default();
        let mut moved = |key: &AnnotationKey| match key.target() {
            AnnotationTarget::Entity(id) if keep(id) => {
                Some(AnnotationKey::record_in_delta(qualifier, id))
            }
            _ => None,
        };
        for (key, provenance) in &self.provenance {
            if let Some(key) = moved(key) {
                out.provenance.insert(key, provenance.clone());
            }
        }
        for (key, note) in &self.exactness {
            if let Some(key) = moved(key) {
                out.exactness.insert(key, note.clone());
            }
        }
        out
    }

    /// Replace each id that a key of either table holds by `map` of that id,
    /// calling `map` once per id in increasing order. A collision leaves both
    /// tables unchanged.
    pub fn map_ids(
        &mut self,
        mut map: impl FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
    ) -> Result<Result<(), AnnotationIdentityCollision>, cadmpeg_core::CodecError> {
        let keys = self
            .provenance
            .keys()
            .chain(self.exactness.keys())
            .collect::<std::collections::BTreeSet<_>>();
        let mut mapped = BTreeMap::new();
        for id in keys
            .iter()
            .flat_map(|key| key.ids())
            .collect::<std::collections::BTreeSet<_>>()
        {
            mapped.insert(id, map(id)?);
        }
        let mut targets = std::collections::BTreeSet::new();
        let mut remapping = Vec::with_capacity(keys.len());
        for key in keys {
            let target = key.map(|id| mapped[id].clone());
            if !targets.insert(target.clone()) {
                return Ok(Err(AnnotationIdentityCollision {
                    id: target.to_string(),
                }));
            }
            remapping.push((key.clone(), target));
        }
        let mut remapped = Self::default();
        for (key, target) in remapping {
            if let Some(provenance) = self.provenance.remove(&key) {
                remapped.provenance.insert(target.clone(), provenance);
            }
            if let Some(exactness) = self.exactness.remove(&key) {
                remapped.exactness.insert(target, exactness);
            }
        }
        *self = remapped;
        Ok(Ok(()))
    }

    /// Annotate `to` as the entity `from` is annotated, replacing the
    /// annotations of `to`.
    pub fn copy_entity(&mut self, from: &impl AsRef<crate::ids::Identity>, to: AnnotationKey) {
        self.copy_annotations(from.as_ref().as_str(), to);
    }

    /// Annotate `to` as the record whose annotation key reads `from` is
    /// annotated, replacing the annotations of `to`.
    pub(crate) fn copy_annotations(&mut self, from: &str, to: AnnotationKey) {
        match self.provenance.get(from).cloned() {
            Some(provenance) => self.provenance.insert(to.clone(), provenance),
            None => self.provenance.remove(&to),
        };
        match self.exactness.get(from).cloned() {
            Some(note) => self.exactness.insert(to, note),
            None => self.exactness.remove(&to),
        };
    }

    /// Sparse non-byte-exact annotations by annotation key.
    pub fn exactness(&self) -> &BTreeMap<AnnotationKey, ExactnessNote> {
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
    pub fn append(&mut self, mut other: Self) -> Result<(), AnnotationIdentityCollision> {
        for id in other.provenance.keys().chain(other.exactness.keys()) {
            if self.provenance.contains_key(id) || self.exactness.contains_key(id) {
                return Err(AnnotationIdentityCollision { id: id.to_string() });
            }
        }
        self.provenance.append(&mut other.provenance);
        self.exactness.append(&mut other.exactness);
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use cadmpeg_core::serde_erase::json;

    #[test]
    fn annotation_remap_moves_nested_records_to_the_mapped_identity() {
        const MAPPED: &str = "test:point#mapped";
        let mut builder = super::AnnotationBuilder::new();
        let stream = super::StreamHandle::new(crate::stream_name!("test"));
        builder.note("test:point#0", &stream, 0, None);
        builder.exactness("test:point#0", super::Exactness::Inferred);
        let mut annotations = builder.build();
        let outcome = annotations.map_ids(|_| Ok(MAPPED.to_owned()));
        assert!(matches!(outcome, Ok(Ok(()))));
        assert!(annotations.provenance.contains_key(MAPPED));
    }

    #[test]
    fn exactness_update_and_removal_allocate_no_retained_records() {
        let mut builder = super::AnnotationBuilder::new();
        builder.exactness("test:point#0", super::Exactness::Derived);
        builder.exactness("test:point#0", super::Exactness::Inferred);
        assert_eq!(
            builder.state.annotations.exactness["test:point#0"].entity(),
            super::Exactness::Inferred
        );
        builder.exactness("test:point#0", super::Exactness::ByteExact);
        builder.exactness("test:point#missing", super::Exactness::ByteExact);
        assert!(builder.state.annotations.exactness.is_empty());
    }

    #[test]
    fn annotation_copy_preserves_nested_entries() {
        let mut builder = super::AnnotationBuilder::new();
        let stream = super::StreamHandle::new(crate::stream_name!("test"));
        builder.note("test:point#0", &stream, 7, Some("point"));
        builder
            .derived("test:point#0", "position")
            .expect("field path");
        let copied = builder.copy_transaction().into_retained();
        assert_eq!(copied.build(), builder.clone().build());
        let original = builder.build();
        let transaction = original.copy_transaction();
        assert_eq!(transaction.annotations(), &original);
    }

    #[test]
    fn annotate_matches_note_and_exactness() {
        let id = "test:model:entity#1";
        let stream = "creo:VisibGeom";
        let mut annotated = super::AnnotationBuilder::new();
        annotated
            .annotate(id, stream, 42, "face", super::Exactness::Derived)
            .unwrap();
        let handle =
            super::StreamHandle::new(super::StreamName::try_from(stream.to_owned()).unwrap());
        let mut expected = super::AnnotationBuilder::new();
        expected.note(id, &handle, 42, Some("face"));
        expected.exactness(id, super::Exactness::Derived);
        assert_eq!(annotated.build(), expected.build());
    }

    mod delta_keys;
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
        let schema = json::to_value(schemars::schema_for!(NonEmptyMap)).unwrap();
        assert_eq!(schema["minProperties"], 1);
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema["patternProperties"].get("[\\s\\S]").is_some());
        let schema = json::to_value(schemars::schema_for!(FieldName)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["minLength"], 1);

        // Whitespace and line breaks are legal field text; only absence is refused.
        for field in [" ", "\n", "geometry.radius"] {
            let key = FieldName::try_from(field.to_owned()).unwrap();
            let map = NonEmptyMap::try_from(BTreeMap::from([(key, Exactness::Derived)])).unwrap();
            let wire = json::to_value(&map).unwrap();
            assert_eq!(json::from_value::<NonEmptyMap>(wire).unwrap(), map);
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
        builder.note("f3d:body#0", &first, 42, Some("body"));

        let annotations = builder.build();
        assert_eq!(annotations.stream_count(), 1);
        let provenance = &annotations.provenance["f3d:body#0"];
        assert_eq!(provenance.stream(), "f3d:Breps.BlobParts/body.smbh");
        assert_eq!(provenance.offset, 42);
        assert_eq!(provenance.tag.as_deref(), Some("body"));
    }

    #[test]
    fn a_repeated_note_and_exactness_keep_the_last_location_and_exactness() {
        let stream = StreamHandle::new(crate::stream_name!("native-stream"));
        let mut builder = AnnotationBuilder::new();
        for (offset, exactness) in [(7, Exactness::Derived), (11, Exactness::ByteExact)] {
            builder.note("entity", &stream, offset, Some("tag"));
            builder.exactness("entity", exactness);
        }
        let annotations = builder.build();
        assert_eq!(annotations.provenance["entity"].offset, 11);
        assert_eq!(annotations.provenance["entity"].tag.as_deref(), Some("tag"));
        assert!(annotations.exactness().is_empty());
    }

    #[test]
    fn repeated_note_replaces_location_and_clears_the_previous_tag() {
        let mut builder = AnnotationBuilder::new();
        let first = StreamHandle::new(crate::stream_name!("first"));
        let second = StreamHandle::new(crate::stream_name!("second"));

        builder.note("entity", &first, 7, Some("stale"));
        builder.note("entity", &second, 11, None);

        let annotations = builder.build();
        let provenance = &annotations.provenance["entity"];
        assert_eq!(provenance.stream(), "second");
        assert_eq!(provenance.offset, 11);
        assert_eq!(provenance.tag, None);

        let wire = json::to_value(&annotations).expect("serialize annotations");
        let restored: Annotations = json::from_value(wire).expect("annotation wire round-trips");
        assert_eq!(restored, annotations);
    }

    #[test]
    fn a_nonempty_whitespace_stream_name_is_preserved_on_the_annotation_wire() {
        let mut builder = AnnotationBuilder::new();
        let stream = StreamHandle::new(crate::stream_name!(" \t"));
        builder.note("whitespace", &stream, 3, None);

        let annotations = builder.build();
        assert_eq!(annotations.provenance["whitespace"].stream(), " \t");
        let wire = json::to_value(&annotations).expect("serialize annotations");
        assert_eq!(wire["provenance"]["whitespace"]["stream"], " \t");
        assert_eq!(json::from_value::<Annotations>(wire).unwrap(), annotations);
    }

    #[test]
    fn annotation_provenance_names_its_stream_and_refuses_the_deleted_index_table() {
        let mut builder = AnnotationBuilder::new();
        let stream = StreamHandle::new(crate::stream_name!("f3d:Breps.BlobParts/body.smbh"));
        builder.note("f3d:body#0", &stream, 42, Some("body"));
        let annotations = builder.build();

        let value = json::to_value(&annotations).unwrap();
        assert!(value.get("streams").is_none());
        assert_eq!(
            value["provenance"]["f3d:body#0"]["stream"],
            "f3d:Breps.BlobParts/body.smbh"
        );
        assert_eq!(json::from_value::<Annotations>(value).unwrap(), annotations);

        let error = json::from_value::<Annotations>(cadmpeg_core::json!({
            "streams": ["f3d:native"],
            "provenance": {
                "f3d:body#0": {"stream": "f3d:native", "offset": 42}
            }
        }))
        .unwrap_err()
        .to_string();
        assert!(error.contains("unknown field `streams`"), "{error}");

        let error = json::from_value::<Annotations>(cadmpeg_core::json!({
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
        second.note("foreign", &handle, 1, None);
        let mut cloned = first.clone();
        cloned.note("cloned", &handle, 2, None);
        let mut resumed = AnnotationBuilder::resume(first.build());
        resumed.note("resumed", &handle, 3, None);
        let mut empty = AnnotationBuilder::new();
        empty.note("empty", &handle, 4, None);
        let mut same_name = AnnotationBuilder::new();
        same_name.note("same-name", &handle, 5, None);
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
            let wire = json::to_value(&annotations).unwrap();
            let restored: Annotations = json::from_value(wire).unwrap();
            assert_eq!(restored, annotations);
            assert_eq!(restored.provenance[id].stream(), "first");
        }
    }

    #[test]
    fn an_empty_field_name_leaves_the_annotations_unchanged() {
        for entity_first in [false, true] {
            let mut builder = AnnotationBuilder::new();
            for exactness in [
                Exactness::Derived,
                Exactness::Inferred,
                Exactness::ByteExact,
            ] {
                if entity_first {
                    builder.exactness("test:point#1", exactness);
                }
                builder
                    .field_exactness("test:point#1", "position.x", exactness)
                    .unwrap();
            }
            let before = builder.annotations().clone();
            assert!(builder
                .field_exactness("test:point#1", String::new(), Exactness::Derived)
                .is_err());
            assert_eq!(builder.annotations(), &before);
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
            builder.state.annotations.exactness["f3d:edge#0"],
            ExactnessNote::Entity {
                entity: Inexactness::Inferred,
                fields: expected_fields,
            }
        );

        builder.exactness("f3d:edge#0", Exactness::ByteExact);
        assert!(builder.state.annotations.exactness.is_empty());
    }

    #[test]
    fn an_entity_note_refuses_byte_exact_and_a_field_note_refuses_an_empty_map() {
        let error = json::from_value::<ExactnessNote>(cadmpeg_core::json!({
            "kind": "entity", "entity": "byte_exact"
        }))
        .expect_err("byte-exact is the implicit entity exactness");
        assert!(error.to_string().contains("unknown variant"), "{error}");

        let error = json::from_value::<ExactnessNote>(cadmpeg_core::json!({
            "kind": "fields", "fields": {}
        }))
        .expect_err("a field note carries at least one override");
        assert!(
            error.to_string().contains("needs a field override"),
            "{error}"
        );
    }

    #[test]
    fn explicit_field_exactness_survives_an_equal_entity_note() {
        let wire = cadmpeg_core::json!({
            "kind": "entity", "entity": "derived",
            "fields": {"geometry": "derived"}
        });
        let note: ExactnessNote = json::from_value(wire.clone()).unwrap();
        assert_eq!(json::to_value(note).unwrap(), wire);

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
                json::to_value(&builder.state.annotations.exactness["nx:model:surface#1"]).unwrap(),
                if entity_first {
                    wire.clone()
                } else {
                    cadmpeg_core::json!({"kind": "entity", "entity": "derived", "fields": {}})
                }
            );
        }
    }

    #[test]
    fn removing_an_entity_removes_provenance_and_exactness() {
        let mut builder = AnnotationBuilder::new();
        let stream = StreamHandle::new(crate::stream_name!("catia:e5_0d_03"));
        builder.note("catia:e5:curve#0", &stream, 42, Some("circle"));
        builder
            .derived("catia:e5:curve#0", "geometry")
            .expect("nonempty exactness field");

        builder.remove_entity("catia:e5:curve#0");

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
        builder.exactness(id, Exactness::Inferred);
        builder.derived(id, "position.x").expect("nonempty path");
        let before = json::to_value(&builder.state.annotations).expect("serialize annotations");
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
                json::to_value(&builder.state.annotations).expect("serialize annotations"),
                before
            );
        }
        assert!(builder.derived(id, "").is_err());
        assert_eq!(
            json::to_value(&builder.state.annotations).expect("serialize annotations"),
            before
        );
        for wire in [
            cadmpeg_core::json!({"kind": "entity", "entity": "derived", "fields": {"": "derived"}}),
            cadmpeg_core::json!({"kind": "fields", "fields": {"": "derived"}}),
        ] {
            let error = json::from_value::<ExactnessNote>(wire).expect_err("empty field key");
            assert!(
                error.to_string().contains("field name cannot be empty"),
                "{error}"
            );
        }
        let wire = cadmpeg_core::json!({
            "kind": "entity", "entity": "derived",
            "fields": {"position.x": "byte_exact"}
        });
        let admitted: ExactnessNote = json::from_value(wire.clone()).expect("nonempty path");
        assert_eq!(json::to_value(admitted).expect("serialize note"), wire);
    }
    #[test]
    fn repeated_exactness_field_keys_are_refused_through_the_decode_sidecar() {
        let ir = crate::examples::unit_cube().expect("valid cube");
        let ir_json = ir.to_canonical_json().expect("serialize CADIR");
        let id = ir.geometry.points[0].id.as_str();
        for scope in ["entity", "fields"] {
            let mut builder = AnnotationBuilder::new();
            if scope == "entity" {
                builder.exactness(id, Exactness::Inferred);
            }
            builder.derived(id, "position").expect("nonempty field");
            let report = crate::report::decode::DecodeReport::unclassified(
                crate::codec_id!("test"),
                crate::report::decode::DecodeTransfer::full(true),
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                crate::report::decode::TransferLedger::default(),
            );
            let sidecar = crate::source_fidelity::DecodeSidecar::bind_sha256(
                crate::hash::digest::Sha256Digest::digest(ir_json.as_bytes()),
                report,
                crate::SourceFidelity::with_annotations(
                    builder.build(),
                    crate::hash::digest::Sha256Digest::digest(b"test source"),
                ),
            );
            let text = json::to_string(&sidecar).expect("serialize sidecar");
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
        let stream = StreamHandle::new(crate::stream_name!("test"));
        let mut original = AnnotationBuilder::new();
        original.note("test:model:point#prior", &stream, 0, Some("prior"));
        let mut candidate = original.copy_transaction();
        candidate
            .annotate(
                "test:model:point#candidate",
                "candidate stream",
                7,
                "candidate tag",
                Exactness::Derived,
            )
            .unwrap();
        candidate
            .derived("test:model:point#candidate", "position")
            .unwrap();
        assert!(candidate
            .annotations()
            .provenance
            .contains_key("test:model:point#candidate"));
        assert_eq!(original.annotations().provenance.len(), 1);
    }

    #[test]
    fn annotation_builder_transaction_commits_the_same_tables() {
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let stream = StreamHandle::new(crate::stream_name!("test"));
        let mut original = AnnotationBuilder::new();
        original.note("test:model:point#prior", &stream, 0, Some("prior"));
        let mut candidate = original.copy_transaction();
        candidate
            .annotate(
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
                "test:model:point#candidate",
                "candidate stream",
                7,
                "candidate tag",
                Exactness::Derived,
            )
            .unwrap();
        assert_eq!(candidate.into_retained().build(), expected.build());
        ctx.finish_session().unwrap();
    }
}

mod serialization;
