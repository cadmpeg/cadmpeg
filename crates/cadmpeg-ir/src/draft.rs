// SPDX-License-Identifier: Apache-2.0
//! Atomic staging for neutral entity transfer.

use std::borrow::BorrowMut;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;

use crate::annotations::Annotations;
use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::document::{CadIr, Model};
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
};
use crate::geometry::{pcurve::Pcurve, Curve, ProceduralCurve, ProceduralSurface, Surface};
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Occurrence, ProductDefinition};
use crate::provenance::Exactness;
use crate::schema::{EntityKind, EntitySchema};
use crate::semantic_annotations::SemanticAnnotation;
use crate::sketches::{
    Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchConstraint,
    SpatialSketchEntity,
};
use crate::spreadsheets::Spreadsheet;
use crate::subd::SubdSurface;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};

mod private {
    pub trait Sealed {}
}

/// Entity type owned by one registry-declared model arena.
pub trait ArenaEntity: private::Sealed + EntitySchema + Sized {
    /// Returns this entity type's arena.
    fn arena(model: &Model) -> &Vec<Self>;

    /// Returns this entity type's mutable arena.
    fn arena_mut(model: &mut Model) -> &mut Vec<Self>;
}

/// Registry-complete arena lengths captured at a model transaction boundary.
#[derive(Debug)]
pub struct ModelCheckpoint {
    lengths: [usize; EntityKind::ALL.len()],
    feature_parents: crate::document::FeatureRegenerationParents,
}

impl ModelCheckpoint {
    /// Capture a decoded model's arena lengths and feature-parent table.
    pub fn capture(model: &Model) -> Self {
        Self::with_parents(model, model.feature_regeneration_parents.clone())
    }

    fn with_parents(
        model: &Model,
        feature_parents: crate::document::FeatureRegenerationParents,
    ) -> Self {
        macro_rules! capture_lengths {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { [$(model.$field.len()),*] };
        }
        Self {
            lengths: crate::document::arena_registry!(capture_lengths),
            feature_parents,
        }
    }

    fn length<T: ArenaEntity>(&self) -> usize {
        self.lengths[T::KIND.index()]
    }

    /// Returns the captured length of one typed arena.
    pub fn arena_len<T: ArenaEntity>(&self) -> usize {
        self.length::<T>()
    }

    /// Returns mutable entities of `T` added since this checkpoint.
    pub fn added_mut<'a, T: ArenaEntity>(&self, model: &'a mut Model) -> Option<&'a mut [T]> {
        T::arena_mut(model).get_mut(self.length::<T>()..)
    }

    /// Discards appended entities and restores captured feature-parent relations.
    ///
    /// This is an append-only checkpoint, not a snapshot of existing entities.
    /// Between capture and discard, callers must not remove, reorder, or modify
    /// entities that preceded the checkpoint.
    pub fn discard_appended(&self, model: &mut Model) -> Result<(), CodecError> {
        let parents = self.feature_parents.clone();
        macro_rules! admit_truncation {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                if model.$field.len() < self.length::<$ty>() {
                    return Err(CodecError::malformed("checkpoint cannot restore removed entities"));
                }
            )*};
        }
        crate::document::arena_registry!(admit_truncation);
        self.restore(model, parents);
        Ok(())
    }

    /// Compare captured arena lengths and feature-parent tables.
    pub fn same_state(&self, other: &Self) -> bool {
        self.lengths == other.lengths && self.feature_parents == other.feature_parents
    }

    fn restore(&self, model: &mut Model, parents: crate::document::FeatureRegenerationParents) {
        macro_rules! truncate_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(model.$field.truncate(self.length::<$ty>());)* };
        }
        crate::document::arena_registry!(truncate_arenas);
        model.feature_regeneration_parents = parents;
    }
}

macro_rules! impl_arena_entities {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        $(
            impl private::Sealed for $ty {}

            impl ArenaEntity for $ty {
                fn arena(model: &Model) -> &Vec<Self> {
                    &model.$field
                }

                fn arena_mut(model: &mut Model) -> &mut Vec<Self> {
                    &mut model.$field
                }
            }
        )*
    };
}

crate::document::arena_registry!(impl_arena_entities);

/// One neutral identity location. The arena owns the identity string; the
/// index stores only its stable slot, so transaction checks do not allocate a
/// second copy of every model identity.
#[derive(Debug, Clone, Copy)]
struct IdentitySlot {
    kind: EntityKind,
    index: usize,
}

type IdentityIndex = HashMap<u64, Vec<IdentitySlot>>;

fn identity_hash(identity: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut hasher);
    hasher.finish()
}

fn insert_identity<T>(index: &mut HashMap<u64, Vec<T>>, hash: u64, value: T) {
    index.entry(hash).or_default().push(value);
}

fn index_model_identities(model: &Model) -> Result<IdentityIndex, &str> {
    let mut identity_index = IdentityIndex::new();
    macro_rules! index_arenas {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for (slot_index, entity) in model.$field.iter().enumerate() {
                if identity_index_contains(model, &identity_index, entity.identity()) {
                    return Err(entity.identity());
                }
                insert_identity(&mut identity_index, identity_hash(entity.identity()), IdentitySlot { kind: <$ty as EntitySchema>::KIND, index: slot_index });
            })*
        };
    }
    crate::document::arena_registry!(index_arenas);
    Ok(identity_index)
}

fn identity_index_contains(model: &Model, index: &IdentityIndex, identity: &str) -> bool {
    let hash = identity_hash(identity);

    for slot in index.get(&hash).into_iter().flatten() {
        if let Some(candidate) = model.identity_at(slot.kind, slot.index) {
            if candidate == identity {
                return true;
            }
        }
    }
    false
}

/// Error returned before an atomic draft commit mutates its destination.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DraftError {
    /// Decode storage or work exceeds its resource allowance.
    #[error("resource refusal: {0:?}")]
    Resource(ResourceLimit),
    /// A fallible formatter or allocator cannot complete admission.
    #[error("{0}")]
    Admission(String),
    /// An identity already exists in the base model or this draft.
    #[error("entity identity collision: {0}")]
    IdentityCollision(String),
    /// A staged typed reference cannot resolve after commit.
    #[error("staged entity {owner} has unresolved reference {target}")]
    UnresolvedReference {
        /// Staged entity holding the reference.
        owner: String,
        /// Missing target identity.
        target: String,
    },
    /// Model-owned feature parents do not form an admitted combined graph.
    #[error("staged feature {owner} has invalid parent relations: {message}")]
    FeatureParents {
        /// Feature whose ownership or predecessor relation is invalid.
        owner: crate::features::FeatureId,
        /// The violated graph invariant and affected identities.
        message: String,
    },
}

impl From<CodecError> for DraftError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::ResourceLimit(limit) => Self::Resource(limit),
            error => Self::Admission(error.to_string()),
        }
    }
}

/// Transactional collection of staged model entities.
///
/// A plain draft carries no accounting and can commit through `commit_model`.
/// [`with_accounting`](Self::with_accounting) adds accounting and requires the
/// commit route that accepts every accounting destination.
#[derive(Debug)]
pub struct ModelDraft<A = ()> {
    model: Model,
    insertion_index: Option<IdentityIndex>,
    accounting: A,
}

/// Exactness annotations that must accompany a draft commit.
#[derive(Debug, Default)]
pub struct DraftAccounting {
    exactness: BTreeMap<String, Exactness>,
}

impl Default for ModelDraft {
    fn default() -> Self {
        Self {
            model: Model::default(),
            insertion_index: Some(IdentityIndex::new()),
            accounting: (),
        }
    }
}

impl ModelDraft {
    /// Creates an empty draft.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add accounting while preserving all staged entities.
    ///
    /// Accounted drafts cannot use a model-only commit:
    ///
    /// ```compile_fail
    /// let arena = cadmpeg_core::decode::DecodeArena::new();
    /// let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::default()).unwrap();
    /// let draft = cadmpeg_ir::draft::ModelDraft::new().with_accounting();
    /// draft.commit_model(&mut cadmpeg_ir::CadIr::empty(), &ctx).unwrap();
    /// ```
    ///
    /// A commit session also requires a draft with no accounting:
    ///
    /// ```compile_fail
    /// let draft = cadmpeg_ir::draft::ModelDraft::new().with_accounting();
    /// let mut document = cadmpeg_ir::CadIr::empty();
    /// let arena = cadmpeg_core::decode::DecodeArena::new();
    /// let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::default()).unwrap();
    /// cadmpeg_ir::draft::CommitSession::new(&mut document, &ctx, None).unwrap().commit_model(draft).unwrap();
    /// ```
    pub fn with_accounting(self) -> ModelDraft<DraftAccounting> {
        ModelDraft {
            model: self.model,
            insertion_index: self.insertion_index,
            accounting: DraftAccounting::default(),
        }
    }

    /// Validate and append one decoded draft through the charged session owner.
    pub fn commit_model(
        self,
        base: &mut CadIr,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), DraftError>, CodecError> {
        CommitSession::new(base, ctx, None)?.commit_model(self)
    }
}

impl<A> ModelDraft<A> {
    /// Admit the entity's arena storage and check its identity before insertion.
    pub fn insert<T: ArenaEntity>(&mut self, entity: T) -> Result<(), DraftError> {
        let identity = entity.identity();
        let index = match self.insertion_index.take() {
            Some(index) => index,
            None => match index_model_identities(&self.model) {
                Ok(index) => index,
                Err(identity) => return Err(DraftError::IdentityCollision(identity.to_owned())),
            },
        };
        let index = self.insertion_index.insert(index);
        if identity_index_contains(&self.model, index, identity) {
            return Err(DraftError::IdentityCollision(identity.to_owned()));
        }

        let hash = identity_hash(identity);
        let slot = IdentitySlot {
            kind: T::KIND,
            index: T::arena(&self.model).len(),
        };

        insert_identity(index, hash, slot);
        T::arena_mut(&mut self.model).push(entity);
        Ok(())
    }

    /// Returns the staged model for coordinated multi-arena construction.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Returns the staged model for coordinated multi-arena construction.
    ///
    /// Commit still checks all identities and references, including entities inserted
    /// through this lower-level surface.
    pub fn model_mut(&mut self) -> &mut Model {
        self.insertion_index = None;
        &mut self.model
    }

    /// Number of staged entities.
    pub fn entity_count(&self) -> usize {
        self.model.entity_count()
    }

    fn validate_with_contains(
        &mut self,
        base: &Model,
        contains: impl Fn(&str) -> bool,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), DraftError>, CodecError> {
        let indexed = index_model_identities(&self.model);
        let identity_index = match &indexed {
            Ok(index) => index,
            Err(identity) => return Ok(Err(DraftError::IdentityCollision((*identity).to_owned()))),
        };
        macro_rules! check_external_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(for entity in &self.model.$field {
                    if contains(entity.identity()) {
                        let identity = entity.identity().to_owned();
                        return Ok(Err(DraftError::IdentityCollision(identity)));
                    }
                })*
            };
        }
        crate::document::arena_registry!(check_external_identities);
        macro_rules! validate_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(for entity in &self.model.$field {
                    let owner = entity.identity();
                    let mut missing = None;
                    entity.visit_references(ctx, &mut |target| {
                        if missing.is_some() {
                            return Ok(());
                        }
                        if !contains(target) && !identity_index_contains(&self.model, identity_index, target) {
                            missing = Some(target.to_owned());
                        }
                        Ok(())
                    })?;
                    if let Some(target) = missing {
                        let owner = owner.to_owned();
                        return Ok(Err(DraftError::UnresolvedReference { owner, target }));
                    }
                })*
            };
        }
        crate::document::arena_registry!(validate_arenas);
        if !self.model.features.is_empty() || self.model.has_feature_regeneration_parents() {
            if let Err(error) = crate::document::feature_parents::validate(&[base, &self.model]) {
                return Ok(Err(DraftError::FeatureParents {
                    owner: error.owner().clone(),
                    message: format!("{error}"),
                }));
            }
        }
        Ok(Ok(()))
    }
}

impl ModelDraft<DraftAccounting> {
    /// Record staged exactness.
    pub fn exactness(&mut self, identity: impl std::fmt::Display, exactness: Exactness) {
        let identity = identity.to_string();
        if exactness == Exactness::ByteExact {
            self.accounting.exactness.remove(&identity);
        } else {
            self.accounting.exactness.insert(identity, exactness);
        }
    }

    /// Retains exactness notes selected by identity.
    pub fn retain_exactness(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.accounting
            .exactness
            .retain(|identity, _| keep(identity));
    }

    /// Commit decoded entities and transfer their owned exactness entries.
    pub fn commit(
        self,
        base: &mut CadIr,
        annotations: &mut Annotations,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), DraftError>, CodecError> {
        CommitSession::new(base, ctx, None)?.commit(self, annotations)
    }
}

#[derive(Debug)]
enum CommittedIdentity {
    Neutral(IdentitySlot),
    Native {
        namespace_hash: u64,
        arena_hash: u64,
        record: usize,
    },
    StagedUnknown(usize),
}

type CommittedIdentityIndex = HashMap<u64, Vec<CommittedIdentity>>;

/// A document owner or exclusive borrow with a caller-owned identity cache.
///
/// The cache is built on the first lookup or commit. The context stays borrowed
/// until the session ends. Each successful commit extends the existing cache.
///
/// ```compile_fail
/// use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
/// use cadmpeg_ir::{document::CadIr, draft::CommitSession};
/// let arena = DecodeArena::new();
/// let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
/// let mut ir = CadIr::empty();
/// let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();
/// ir.model.points.clear();
/// session.contains("test:model:point#1").unwrap();
/// ```
///
/// ```compile_fail
/// use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
/// use cadmpeg_ir::{document::CadIr, draft::CommitSession};
/// let arena = DecodeArena::new();
/// let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
/// let mut ir = CadIr::empty();
/// let session = CommitSession::new(&mut ir, &ctx, None).unwrap();
/// session.document().model.points.clear();
/// ```
#[derive(Debug)]
pub struct CommitSession<'ctx, D: BorrowMut<CadIr>> {
    state: CommitState<'ctx, D>,
    ctx: &'ctx DecodeContext<'ctx>,
}

#[derive(Debug)]
struct CommitState<'ctx, D: BorrowMut<CadIr>> {
    base: D,
    unknowns: Vec<crate::unknown::UnknownRecord>,
    unknown_namespace: Option<&'ctx str>,
    identities: Option<CommittedIdentityIndex>,
    admission: Option<crate::validate::admit::incremental::AdmittedState>,
    unindexed_start: Option<[usize; EntityKind::ALL.len()]>,
    source_admission: Option<crate::validate::admit::NativeUnknownAdmission>,
}

impl<'ctx, D: BorrowMut<CadIr>> CommitSession<'ctx, D> {
    /// Hold the document and caller's context without scanning identity arenas.
    pub fn new(
        base: D,
        ctx: &'ctx DecodeContext<'ctx>,
        unknown_namespace: Option<&'ctx str>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            state: CommitState {
                base,
                unknowns: Vec::new(),
                unknown_namespace,
                identities: None,
                admission: None,
                unindexed_start: None,
                source_admission: None,
            },
            ctx,
        })
    }

    /// Read the committed document while its cache reservation remains live.
    pub fn document(&self) -> &CadIr {
        self.state.base.borrow()
    }

    /// Invalidate cached positions before exposing the document for mutation.
    pub fn document_mut(&mut self) -> Result<&mut CadIr, CodecError> {
        self.state.identities = None;
        self.state.unindexed_start = None;
        self.state.admission = None;
        Ok(self.state.base.borrow_mut())
    }

    /// Release the cache and transfer the document and staged source records.
    pub fn into_parts(self) -> (D, Vec<crate::unknown::UnknownRecord>) {
        let Self { state, ctx: _ } = self;
        let CommitState {
            base,
            unknowns,
            unknown_namespace: _,
            identities,
            admission,
            unindexed_start: _,
            source_admission,
        } = state;
        drop(source_admission);
        drop(admission);
        drop(identities);
        (base, unknowns)
    }

    /// Borrow staged source records without constructing native product records.
    pub fn unknowns(&self) -> &[crate::unknown::UnknownRecord] {
        &self.state.unknowns
    }

    /// Mutate outgoing links while keeping cached identity positions stable.
    pub fn unknown_links_mut(
        &mut self,
        index: usize,
    ) -> Result<Option<(&str, &mut Vec<String>)>, CodecError> {
        if self.state.unknowns.get(index).is_some() {
            if let Some(admission) = &mut self.state.admission {
                admission.dirty_unknown(index);
            }
        }
        if self.state.unknowns.get(index).is_some() {
            if let Some(source) = &mut self.state.source_admission {
                source.dirty(index);
            }
        }
        Ok(self
            .state
            .unknowns
            .get_mut(index)
            .map(crate::unknown::UnknownRecord::id_and_links_mut))
    }

    /// Expose model arenas for append-only staging. Existing entities and native
    /// records must remain unchanged. Candidate admission checks the new suffix.
    pub fn append_model_mut(&mut self) -> Result<&mut Model, CodecError> {
        if self.state.identities.is_some() && self.state.unindexed_start.is_none() {
            macro_rules! lengths {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {[$(self.state.base.borrow().model.$field.len()),*]};
            }
            self.state.unindexed_start = Some(crate::document::arena_registry!(lengths));
        }
        Ok(&mut self.state.base.borrow_mut().model)
    }

    /// Edit only entities appended after `checkpoint`, invalidating admission
    /// facts for that suffix. Existing prefix entities must remain unchanged.
    pub fn edit_appended_model(
        &mut self,
        checkpoint: &ModelCheckpoint,
    ) -> Result<&mut Model, CodecError> {
        if let Some(admission) = &mut self.state.admission {
            admission.rewind(&self.state.base.borrow().model, checkpoint)?;
        }
        self.state.identities = None;
        self.state.unindexed_start = None;
        Ok(&mut self.state.base.borrow_mut().model)
    }

    /// Move an owned source population into the session and invalidate cached positions.
    pub fn replace_unknowns(
        &mut self,
        records: Vec<crate::unknown::UnknownRecord>,
    ) -> Result<(), CodecError> {
        self.state.identities = None;
        self.state.unindexed_start = None;
        self.state.admission = None;
        self.state.source_admission = None;
        self.state.unknowns = records;
        Ok(())
    }

    /// Admit a moved source record and extend a previously built identity cache.
    pub fn push_unknown(
        &mut self,
        record: crate::unknown::UnknownRecord,
    ) -> Result<(), CodecError> {
        if let Some(index) = &mut self.state.identities {
            insert_identity(
                index,
                identity_hash(record.id().as_str()),
                CommittedIdentity::StagedUnknown(self.state.unknowns.len()),
            );
        }
        self.state.admission = None;
        self.state.source_admission = None;
        self.state.unknowns.push(record);
        Ok(())
    }

    /// Check source identity uniqueness once and recheck only edited link lists.
    pub fn validate_source_records(
        &mut self,
    ) -> Result<Result<(), crate::native::NativeConvertError>, CodecError> {
        match &mut self.state.source_admission {
            Some(admission) => admission.check(&self.state.unknowns),
            None => {
                match crate::validate::admit::NativeUnknownAdmission::build(&self.state.unknowns) {
                    Ok(admission) => {
                        self.state.source_admission = Some(admission);
                        Ok(Ok(()))
                    }
                    Err(error) => Ok(Err(error)),
                }
            }
        }
    }

    /// Validate and commit one model draft under this session's context.
    pub fn commit_model(
        &mut self,
        draft: ModelDraft,
    ) -> Result<Result<(), DraftError>, CodecError> {
        self.state.commit_draft(draft, self.ctx, || ())
    }

    /// Admit an accounted draft and its annotation transfer before either is applied.
    pub fn commit(
        &mut self,
        draft: ModelDraft<DraftAccounting>,
        annotations: &mut Annotations,
    ) -> Result<Result<(), DraftError>, CodecError> {
        let ctx = self.ctx;
        let mut transaction = annotations.sparse_transaction();
        let ModelDraft {
            model,
            accounting,
            insertion_index,
        } = draft;
        let mut annotations_admitted = self
            .state
            .admission
            .as_ref()
            .is_some_and(|state| state.annotations_admitted);
        let annotation_owners = if annotations_admitted && !accounting.exactness.is_empty() {
            Some(index_model_identities(&model))
        } else {
            None
        };
        for (identity, exactness) in accounting.exactness {
            if let Some(Ok(owners)) = &annotation_owners {
                annotations_admitted &= identity_index_contains(&model, owners, &identity);
            } else if annotation_owners.is_some() {
                annotations_admitted = false;
            }
            transaction.exactness(&identity, exactness);
        }
        match self.state.commit_draft(
            ModelDraft {
                model,
                insertion_index,
                accounting: (),
            },
            ctx,
            || transaction.prepare(),
        )? {
            Ok(merged) => {
                merged.apply(annotations);
                if let Some(state) = &mut self.state.admission {
                    state.annotations_admitted = annotations_admitted;
                }
                Ok(Ok(()))
            }
            Err(error) => Ok(Err(error)),
        }
    }

    /// Append a candidate under read-only admission and extend only accepted cache positions.
    pub fn try_append<T, E>(
        &mut self,
        model: Model,
        native: crate::native::Native,
        admit: impl FnOnce(&CadIr, &[crate::unknown::UnknownRecord]) -> Result<Result<T, E>, CodecError>,
    ) -> Result<Result<T, E>, CodecError> {
        self.state.ensure_identities()?;
        let index = self
            .state
            .identities
            .as_mut()
            .ok_or_else(|| CodecError::malformed("committed identity index is absent"))?;
        let staged = stage_committed_identities(
            self.state.base.borrow(),
            &model,
            Some(&native),
            self.state.unknown_namespace,
        );
        let unknowns = &self.state.unknowns;
        let result = self
            .state
            .base
            .borrow_mut()
            .try_append(model, native, |combined| admit(combined, unknowns))?;
        if result.is_ok() {
            for (hash, group) in staged {
                index.entry(hash).or_default().extend(group);
            }
        }
        Ok(result)
    }

    /// Admit an independent candidate against reusable facts. Shared references,
    /// arbitrary document mutation and native additions use combined validation.
    /// Native additions leave admission facts empty. A later neutral candidate
    /// can establish a new reusable state.
    /// The callback runs before the append is accepted, so transfer refusal rolls
    /// back the model and the identity cache together. Local reports contain
    /// candidate counts and findings; fallback reports describe the combined model.
    /// Annotation tables must change only through accepted sparse transfers or
    /// annotations of new suffix entities while admission facts remain cached.
    pub fn try_admit_append<'ann, T, E>(
        &mut self,
        mut candidate: CadIr,
        changes: crate::annotations::SparseAnnotationTransaction<'ann>,
        allowed: &[crate::report::check::Check],
        accept: impl FnOnce(
            Result<&crate::report::check::ValidationReport, &crate::native::NativeConvertError>,
            crate::annotations::SparseAnnotationTransaction<'ann>,
        ) -> Result<Result<T, E>, CodecError>,
    ) -> Result<Result<T, E>, CodecError> {
        use crate::validate::admit::incremental::{AdmittedState, CandidateScope};
        let ctx = self.ctx;
        let format = self
            .state
            .unknown_namespace
            .ok_or_else(|| CodecError::malformed("candidate admission needs a source namespace"))?;
        let annotations = changes.base();
        let reuse =
            allowed == crate::validate::admit::RHINO_DRAFT_CHECKS && candidate.native.0.is_empty();
        let mut state = self
            .state
            .admission
            .take()
            .filter(|state| reuse && state.annotations_admitted);
        if let Some(admitted) = &mut state {
            let mut suffix_annotations = annotations.sparse_transaction();
            macro_rules! select_suffix_annotations {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    for entity in self.state.base.borrow().model.$field.get(admitted.lengths[<$ty as EntitySchema>::KIND.index()]..).ok_or_else(|| CodecError::malformed("admitted prefix was removed"))? {
                        suffix_annotations.select(entity.identity());
                    }
                )*};
            }
            crate::document::arena_registry!(select_suffix_annotations);
            let starts = admitted.lengths;
            let report = admitted.probe(
                ctx,
                self.state.base.borrow_mut(),
                (format, &self.state.unknowns),
                suffix_annotations.annotations(),
                allowed,
                (starts, CandidateScope::Appended),
            )?;
            if report.as_ref().is_some_and(|result| {
                result
                    .as_ref()
                    .is_ok_and(crate::report::check::ValidationReport::is_ok)
            }) {
                admitted.accept(self.state.base.borrow())?;
            } else {
                state = None;
            }
        }
        let local = match &mut state {
            Some(admitted) => admitted.probe(
                ctx,
                &mut candidate,
                (format, &self.state.unknowns),
                changes.annotations(),
                allowed,
                ([0; EntityKind::ALL.len()], CandidateScope::Detached),
            )?,
            None => None,
        };
        let local = local.filter(|result| {
            result
                .as_ref()
                .is_ok_and(crate::report::check::ValidationReport::is_ok)
        });
        let is_local = local.is_some();
        let result = self.try_append(candidate.model, candidate.native, |combined, unknowns| {
            let report = match local {
                Some(report) => report,
                None => crate::validate::admit::admit_annotation_delta(
                    ctx,
                    combined,
                    (format, unknowns),
                    &changes,
                    allowed,
                )?,
            };
            if reuse
                && report
                    .as_ref()
                    .is_ok_and(crate::report::check::ValidationReport::is_ok)
            {
                match &mut state {
                    Some(admitted) if is_local => admitted.accept(combined)?,
                    _ => state = Some(AdmittedState::build(combined, format, unknowns)?),
                }
            }
            accept(report.as_ref(), changes)
        });
        if result.as_ref().is_ok_and(Result::is_ok) {
            self.state.admission = state;
        }
        result
    }

    /// Admit staged model appends after in-place editing of their new suffix.
    /// Both Rhino routes retain the shared core. Other check sets rebuild facts.
    pub fn admit_appended(
        &mut self,
        annotations: &Annotations,
        allowed: &[crate::report::check::Check],
    ) -> Result<
        Result<crate::report::check::ValidationReport, crate::native::NativeConvertError>,
        CodecError,
    > {
        use crate::validate::admit::{
            admit_with_native_unknowns,
            incremental::{AdmittedState, CandidateScope},
        };
        let ctx = self.ctx;
        let format = self
            .state
            .unknown_namespace
            .ok_or_else(|| CodecError::malformed("candidate admission needs a source namespace"))?;
        let reuse = allowed == crate::RHINO_DRAFT_CHECKS || allowed == crate::RHINO_INSTANCE_CHECKS;
        let mut state = self.state.admission.take().filter(|state| {
            reuse && (allowed != crate::RHINO_DRAFT_CHECKS || state.annotations_admitted)
        });
        let local = if let Some(admitted) = &mut state {
            let mut suffix = annotations.sparse_transaction();
            macro_rules! select_annotations {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    for entity in self.state.base.borrow().model.$field.get(admitted.lengths[<$ty as EntitySchema>::KIND.index()]..).ok_or_else(|| CodecError::malformed("admitted prefix was removed"))? { suffix.select(entity.identity()); }
                )*};
            }
            if allowed.contains(&crate::report::check::Check::Annotations) {
                crate::document::arena_registry!(select_annotations);
            }
            let starts = admitted.lengths;
            admitted.probe(
                ctx,
                self.state.base.borrow_mut(),
                (format, &self.state.unknowns),
                suffix.annotations(),
                allowed,
                (starts, CandidateScope::Appended),
            )?
        } else {
            None
        };
        let local = local.filter(|report| {
            report
                .as_ref()
                .is_ok_and(crate::report::check::ValidationReport::is_ok)
        });
        let is_local = local.is_some();
        let report = match local {
            Some(report) => report,
            None => admit_with_native_unknowns(
                ctx,
                self.state.base.borrow(),
                (format, &self.state.unknowns),
                allowed
                    .contains(&crate::report::check::Check::Annotations)
                    .then_some(annotations),
                allowed,
                Vec::new(),
            )?,
        };
        if reuse
            && report
                .as_ref()
                .is_ok_and(crate::report::check::ValidationReport::is_ok)
        {
            match &mut state {
                Some(admitted) if is_local => admitted.accept(self.state.base.borrow())?,
                _ => {
                    state = Some(AdmittedState::build(
                        self.state.base.borrow(),
                        format,
                        &self.state.unknowns,
                    )?);
                }
            }
            if let Some(admitted) = &mut state {
                admitted.annotations_admitted = allowed == crate::RHINO_DRAFT_CHECKS
                    || admitted.annotation_owners_resolve(annotations);
            }
            self.state.admission = state;
        }
        Ok(report)
    }

    /// Look up an identity through the live caller-accounted cache.
    pub fn contains(&mut self, identity: &str) -> Result<bool, CodecError> {
        self.state.lookup(identity)
    }
}

fn index_committed_identities(
    base: &CadIr,
    unknowns: &[crate::unknown::UnknownRecord],
    unknown_namespace: Option<&str>,
) -> CommittedIdentityIndex {
    let mut identities = CommittedIdentityIndex::new();
    macro_rules! collect_model_identities {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for (index, entity) in base.model.$field.iter().enumerate() {
                insert_identity(&mut identities, identity_hash(entity.identity()), CommittedIdentity::Neutral(IdentitySlot { kind: <$ty as EntitySchema>::KIND, index }));
            })*
        };
    }
    crate::document::arena_registry!(collect_model_identities);
    for (format, records) in &base.native.0 {
        let replaces_unknowns = match unknown_namespace {
            Some(replacement) => format == replacement,
            None => false,
        };

        let namespace = identity_hash(format);
        for (arena, records) in records.arenas() {
            if replaces_unknowns && arena == "unknowns" {
                continue;
            }

            let arena = identity_hash(arena);
            for (record, value) in records.iter().enumerate() {
                insert_identity(
                    &mut identities,
                    identity_hash(value.id()),
                    CommittedIdentity::Native {
                        namespace_hash: namespace,
                        arena_hash: arena,
                        record,
                    },
                );
            }
        }
    }
    for (index, record) in unknowns.iter().enumerate() {
        insert_identity(
            &mut identities,
            identity_hash(record.id().as_str()),
            CommittedIdentity::StagedUnknown(index),
        );
    }
    identities
}

fn committed_identity_contains(
    base: &CadIr,
    unknowns: &[crate::unknown::UnknownRecord],
    identities: &CommittedIdentityIndex,
    identity: &str,
) -> bool {
    let hash = identity_hash(identity);

    for owner in identities.get(&hash).into_iter().flatten() {
        let candidate = match owner {
            CommittedIdentity::Neutral(slot) => base.model.identity_at(slot.kind, slot.index),
            CommittedIdentity::StagedUnknown(index) => {
                unknowns.get(*index).map(|record| record.id().as_str())
            }
            CommittedIdentity::Native {
                namespace_hash,
                arena_hash,
                record,
            } => {
                for (format, native) in &base.native.0 {
                    if identity_hash(format) != *namespace_hash {
                        continue;
                    }
                    for (name, records) in native.arenas() {
                        if identity_hash(name) != *arena_hash {
                            continue;
                        }

                        if let Some(candidate) = records.get(*record) {
                            if candidate.id() == identity {
                                return true;
                            }
                        }
                    }
                }
                None
            }
        };
        if let Some(candidate) = candidate {
            if candidate == identity {
                return true;
            }
        }
    }
    false
}

fn stage_committed_identities(
    base: &CadIr,
    model: &Model,
    native: Option<&crate::native::Native>,
    unknown_namespace: Option<&str>,
) -> CommittedIdentityIndex {
    let mut staged = CommittedIdentityIndex::new();
    macro_rules! stage_model {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
            for (offset, entity) in model.$field.iter().enumerate() {
                let index = base.model.$field.len() + offset;
                insert_identity(&mut staged, identity_hash(entity.identity()), CommittedIdentity::Neutral(IdentitySlot { kind: <$ty as EntitySchema>::KIND, index }));
            }
        )*};
    }
    crate::document::arena_registry!(stage_model);
    if let Some(native) = native {
        for (format, namespace) in &native.0 {
            let namespace_hash = identity_hash(format);
            let replaces_unknowns = match unknown_namespace {
                Some(replacement) => format == replacement,
                None => false,
            };
            for (arena, records) in namespace.arenas() {
                if replaces_unknowns && (arena == "unknowns") {
                    continue;
                }

                let arena_hash = identity_hash(arena);
                let mut start = 0;
                for (existing_format, existing_namespace) in &base.native.0 {
                    if existing_format != format {
                        continue;
                    }
                    for (existing_arena, existing_records) in existing_namespace.arenas() {
                        if existing_arena == arena {
                            start = existing_records.len();
                            break;
                        }
                    }
                    break;
                }
                for (offset, record) in records.iter().enumerate() {
                    let record_position = start + offset;
                    insert_identity(
                        &mut staged,
                        identity_hash(record.id()),
                        CommittedIdentity::Native {
                            namespace_hash,
                            arena_hash,
                            record: record_position,
                        },
                    );
                }
            }
        }
    }
    staged
}

impl<D: BorrowMut<CadIr>> CommitState<'_, D> {
    fn lookup(&mut self, identity: &str) -> Result<bool, CodecError> {
        self.ensure_identities()?;
        match &self.identities {
            Some(index) => Ok(committed_identity_contains(
                self.base.borrow(),
                &self.unknowns,
                index,
                identity,
            )),
            None => Err(CodecError::malformed("committed identity index is absent")),
        }
    }

    fn ensure_identities(&mut self) -> Result<(), CodecError> {
        if self.identities.is_none() {
            self.identities = Some(index_committed_identities(
                self.base.borrow(),
                &self.unknowns,
                self.unknown_namespace,
            ));
        }
        if let Some(starts) = self.unindexed_start.take() {
            let identities = self
                .identities
                .as_mut()
                .ok_or_else(|| CodecError::malformed("committed identity index is absent"))?;
            macro_rules! index_appended {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    let start = starts[<$ty as EntitySchema>::KIND.index()];
                    for (offset, entity) in self.base.borrow().model.$field.get(start..).ok_or_else(|| CodecError::malformed("append-only staging removed an arena prefix"))?.iter().enumerate() {
                        insert_identity(identities, identity_hash(entity.identity()), CommittedIdentity::Neutral(IdentitySlot { kind: <$ty as EntitySchema>::KIND, index: start + offset }));
                    }
                )*};
            }
            crate::document::arena_registry!(index_appended);
        }
        Ok(())
    }

    /// Validates and commits a model draft.
    ///
    /// The inner result reports a rejected candidate without changing the
    /// committed model.
    fn commit_draft<T>(
        &mut self,
        mut draft: ModelDraft,
        ctx: &DecodeContext<'_>,
        before_apply: impl FnOnce() -> T,
    ) -> Result<Result<T, DraftError>, CodecError> {
        self.ensure_identities()?;
        let base = self.base.borrow_mut();
        let identities = self
            .identities
            .as_mut()
            .ok_or_else(|| CodecError::Malformed("committed identity index is absent".into()))?;
        if let Err(error) = draft.validate_with_contains(
            &base.model,
            |identity| committed_identity_contains(base, &self.unknowns, identities, identity),
            ctx,
        )? {
            return Ok(Err(error));
        }
        let staged = stage_committed_identities(base, &draft.model, None, self.unknown_namespace);
        let transferred = before_apply();
        for (hash, group) in staged {
            identities.entry(hash).or_default().extend(group);
        }
        base.model.append(draft.model);
        Ok(Ok(transferred))
    }
}

#[cfg(test)]
mod tests {
    mod accounting;
    mod feature_parents;
    mod native_identity_slots;
    mod owned_session;
    mod scaling;

    use super::{CommitSession, DraftError, ModelCheckpoint, ModelDraft};
    use crate::annotations::Annotations;
    use crate::document::CadIr;
    use crate::ids::PointId;
    use crate::math::Point3;
    use crate::native::NativeRecord;
    use crate::topology::{Point, Vertex};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    fn point(id: &str) -> Point {
        Point::new(
            PointId::mint(id).expect("valid identity"),
            crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        )
    }

    fn point_draft(id: &str) -> ModelDraft {
        let mut draft = ModelDraft::new();
        draft.insert(point(id)).expect("insert point into draft");
        draft
    }

    fn vertex_draft(id: &str, point: &str) -> ModelDraft {
        let mut draft = ModelDraft::new();
        draft
            .insert(Vertex {
                id: id.try_into().expect("valid identity"),
                point: point.try_into().expect("valid identity"),
                tolerance: None,
            })
            .expect("insert vertex into draft");
        draft
    }

    #[test]
    fn large_draft_insertion_fits_a_linear_work_budget() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut draft = ModelDraft::new();
        for index in 0..4096 {
            draft
                .insert(point(&format!("test:model:point#{index}")))
                .unwrap();
        }
        assert_eq!(draft.entity_count(), 4096);
        assert!(matches!(
            draft.insert(point("test:model:point#4095")),
            Err(DraftError::IdentityCollision(_))
        ));
        ctx.finish_session().unwrap();
    }

    #[test]
    fn direct_draft_edits_invalidate_insertion_identity_slots() {
        let mut draft = ModelDraft::new();
        draft.insert(point("test:model:point#old")).unwrap();
        draft.model_mut().points.clear();
        draft.insert(point("test:model:point#old")).unwrap();
        draft
            .model_mut()
            .points
            .push(point("test:model:point#direct"));
        assert!(matches!(
            draft.insert(point("test:model:point#direct")),
            Err(DraftError::IdentityCollision(_))
        ));
        assert_eq!(draft.entity_count(), 2);
    }

    #[test]
    fn admitted_draft_commit_reports_a_missing_reference() {
        let missing = "test:model:point#missing";
        let mut ir = CadIr::empty();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let result = CommitSession::new(&mut ir, &ctx, None)
            .unwrap()
            .commit_model(vertex_draft("test:model:vertex#new", missing))
            .unwrap();
        assert!(matches!(
            result,
            Err(DraftError::UnresolvedReference { .. })
        ));
    }

    #[test]
    fn checkpoint_discards_appends_outside_the_geometry_arenas() {
        use crate::assets::{Asset, AssetContent, AssetData};
        use crate::features::{Feature, FeatureDefinition, FeatureOperation};

        let mut model = crate::document::Model::default();
        model.points.push(point("test:checkpoint:point#existing"));
        let original = model.clone();
        let checkpoint = ModelCheckpoint::capture(&model);
        model.assets.push(Asset {
            id: "test:checkpoint:asset#new".try_into().unwrap(),
            name: None,
            media_type: None,
            content: AssetContent::Embedded {
                data: AssetData::new(vec![9]).unwrap(),
            },
            native_ref: None,
        });
        for (ordinal, key) in ["parent", "child"].into_iter().enumerate() {
            model.features.push(Feature {
                id: format!("test:checkpoint:feature#{key}").try_into().unwrap(),
                ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
                name: None,
                suppressed: None,
                dependencies: crate::features::DistinctMembers::default(),
                source_properties: std::collections::BTreeMap::default(),
                source_tag: None,
                source_text: None,
                source_content: crate::features::FeatureContent::default(),
                evaluation: crate::features::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
                ),
                native_ref: None,
            });
        }
        model
            .set_feature_regeneration_parent(
                &("test:checkpoint:feature#child".try_into().unwrap()),
                &("test:checkpoint:feature#parent".try_into().unwrap()),
            )
            .unwrap();
        checkpoint.discard_appended(&mut model).unwrap();
        assert_eq!(model, original);
    }

    #[test]
    fn decoded_draft_accepts_the_same_feature_parent_graph() {
        crate::test_support::with_service_decode_context(|ctx| {
            let draft = feature_parents::parent_draft();
            let expected = draft.model().clone();
            let mut ir = CadIr::empty();
            let mut session = CommitSession::new(&mut ir, ctx, None).unwrap();
            session.commit_model(draft).unwrap().unwrap();
            assert_eq!(session.document().model, expected);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn draft_commit_preserves_admitted_feature_regeneration_parents() {
        use crate::features::{Feature, FeatureDefinition, FeatureOperation};

        let mut draft = ModelDraft::new();
        for (ordinal, key) in ["parent", "child"].into_iter().enumerate() {
            draft
                .insert(Feature {
                    id: format!("test:draft:feature#{key}").try_into().unwrap(),
                    ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
                    name: None,
                    suppressed: None,
                    dependencies: crate::features::DistinctMembers::default(),
                    source_properties: std::collections::BTreeMap::default(),
                    source_tag: None,
                    source_text: None,
                    source_content: crate::features::FeatureContent::default(),
                    evaluation: crate::features::FeatureEvaluation::from_definition(
                        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
                    ),
                    native_ref: None,
                })
                .unwrap();
        }
        let child = "test:draft:feature#child".try_into().unwrap();
        let parent = "test:draft:feature#parent".try_into().unwrap();
        draft
            .model_mut()
            .set_feature_regeneration_parent(&(child), &(parent))
            .unwrap();
        let expected = draft.model().clone();
        let mut ir = CadIr::empty();
        draft
            .commit_model(&mut ir, &cadmpeg_test_support::service_decode_context())
            .unwrap()
            .unwrap();
        assert_eq!(ir.model, expected);
        let round_trip = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
        assert_eq!(
            round_trip
                .model
                .feature_regeneration_parent(&"test:draft:feature#child".try_into().unwrap()),
            Some(&"test:draft:feature#parent".try_into().unwrap())
        );
    }

    #[test]
    fn collision_refuses_without_mutating_any_destination() {
        let mut ir = CadIr::empty();
        ir.model.points.push(point("test:model:point#1"));
        let mut draft = ModelDraft::new().with_accounting();
        draft
            .insert(point("test:model:point#1"))
            .expect("insert point into empty draft");
        let mut annotations = Annotations::default();

        assert!(matches!(
            draft
                .commit(
                    &mut ir,
                    &mut annotations,
                    &cadmpeg_test_support::service_decode_context()
                )
                .unwrap(),
            Err(DraftError::IdentityCollision(_))
        ));
        assert_eq!(ir.model.points.len(), 1);
        assert!(annotations.exactness().is_empty());
    }

    #[test]
    fn direct_model_staging_rebuilds_identity_cache_before_commit() {
        let identity = "test:model:point#direct-duplicate";
        let mut draft = ModelDraft::new();
        draft.model_mut().points.push(point(identity));
        draft.model_mut().points.push(point(identity));
        let mut ir = CadIr::empty();

        assert_eq!(
            draft
                .commit_model(&mut ir, &cadmpeg_test_support::service_decode_context())
                .unwrap(),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        assert!(ir.model.points.is_empty());
    }

    #[test]
    fn direct_model_staging_revalidates_references_before_commit() {
        let owner = "test:model:vertex#direct-missing";
        let target = "test:model:point#direct-missing";
        let mut draft = ModelDraft::new();
        draft.model_mut().vertices.push(Vertex {
            id: owner.try_into().expect("valid identity"),
            point: target.try_into().expect("valid identity"),
            tolerance: None,
        });
        let mut ir = CadIr::empty();

        assert_eq!(
            draft
                .commit_model(&mut ir, &cadmpeg_test_support::service_decode_context())
                .unwrap(),
            Err(DraftError::UnresolvedReference {
                owner: owner.into(),
                target: target.into(),
            })
        );
        assert!(ir.model.vertices.is_empty());
    }

    #[test]
    fn commit_session_matches_sequential_model_commits() {
        let mut session_ir = CadIr::empty();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut session = CommitSession::new(&mut session_ir, &ctx, None).unwrap();
        session
            .commit_model(point_draft("test:model:point#1"))
            .unwrap()
            .expect("first session commit");
        session
            .commit_model(point_draft("test:model:point#2"))
            .unwrap()
            .expect("second session commit");

        let mut sequential_ir = CadIr::empty();
        point_draft("test:model:point#1")
            .commit_model(
                &mut sequential_ir,
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap()
            .expect("first sequential commit");
        point_draft("test:model:point#2")
            .commit_model(
                &mut sequential_ir,
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap()
            .expect("second sequential commit");

        drop(session);
        assert_eq!(session_ir, sequential_ir);
    }

    #[test]
    fn commit_session_rejects_cross_draft_identity_collision() {
        let mut ir = CadIr::empty();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();
        let identity = "test:model:point#cross-draft";
        session
            .commit_model(point_draft(identity))
            .unwrap()
            .expect("first session commit");

        assert_eq!(
            session.commit_model(point_draft(identity)).unwrap(),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        drop(session);
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn commit_session_rejects_pre_existing_neutral_identity() {
        let identity = "test:model:point#existing";
        let mut ir = CadIr::empty();
        ir.model.points.push(point(identity));
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();

        assert_eq!(
            session.commit_model(point_draft(identity)).unwrap(),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        drop(session);
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn commit_session_rejects_native_identity() {
        let identity = "test:native:record#1";
        let mut ir = CadIr::empty();
        ir.native.namespace_mut("test").arenas_mut().insert(
            "records".into(),
            vec![NativeRecord::new(
                crate::ids::Identity::new(identity).expect("valid identity"),
                serde_json::Map::new(),
            )
            .expect("valid native identity")],
        );
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();

        assert_eq!(
            session.commit_model(point_draft(identity)).unwrap(),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        drop(session);
        assert!(ir.model.points.is_empty());
    }

    #[test]
    fn commit_session_rejects_unresolved_reference() {
        let owner = "test:model:vertex#missing";
        let target = "test:model:point#missing";
        let mut ir = CadIr::empty();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();

        assert_eq!(
            session.commit_model(vertex_draft(owner, target)).unwrap(),
            Err(DraftError::UnresolvedReference {
                owner: owner.into(),
                target: target.into(),
            })
        );
        drop(session);
        assert!(ir.model.vertices.is_empty());
    }

    #[test]
    fn commit_session_resolves_reference_into_earlier_draft() {
        let point_id = "test:model:point#earlier";
        let mut ir = CadIr::empty();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();
        session
            .commit_model(point_draft(point_id))
            .unwrap()
            .expect("point commit");
        session
            .commit_model(vertex_draft("test:model:vertex#later", point_id))
            .unwrap()
            .expect("reference into committed draft resolves");

        drop(session);
        assert_eq!(ir.model.points.len(), 1);
        assert_eq!(ir.model.vertices.len(), 1);
    }

    #[test]
    fn rejected_session_commit_leaves_session_and_base_usable() {
        let rejected_identity = "test:model:vertex#rejected";
        let mut ir = CadIr::empty();
        let before = ir.clone();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();

        assert!(session
            .commit_model(vertex_draft(
                rejected_identity,
                "test:model:point#never-committed",
            ))
            .unwrap()
            .is_err());
        assert_eq!(session.document(), &before);

        session
            .commit_model(point_draft(rejected_identity))
            .unwrap()
            .expect("rejected identity was not absorbed into the session");
        drop(session);
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn commit_session_contains_tracks_only_successful_commits() {
        let committed_identity = "test:model:point#committed";
        let rejected_identity = "test:model:point#rejected";
        let mut ir = CadIr::empty();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();

        assert!(!session.contains(committed_identity).unwrap());
        session
            .commit_model(point_draft(committed_identity))
            .unwrap()
            .expect("point commit");
        assert!(session.contains(committed_identity).unwrap());

        let mut rejected = ModelDraft::new();
        rejected
            .insert(Vertex {
                id: rejected_identity.try_into().expect("valid identity"),
                point: "test:model:point#missing"
                    .try_into()
                    .expect("valid identity"),
                tolerance: None,
            })
            .expect("insert rejected vertex");
        assert!(!session.contains(rejected_identity).unwrap());
        assert!(session.commit_model(rejected).unwrap().is_err());
        assert!(!session.contains(rejected_identity).unwrap());
    }
}
