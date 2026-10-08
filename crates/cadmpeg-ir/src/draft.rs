// SPDX-License-Identifier: Apache-2.0
//! Atomic staging for neutral entity transfer.

use std::borrow::BorrowMut;
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::annotations::{AnnotationBuilder, Annotations};
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
    /// Capture a decoded model with a live reservation for the copied parent table.
    pub fn capture<'ctx>(
        model: &Model,
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<(Self, ScopedReservation<'ctx>), CodecError> {
        ctx.with_scoped_storage("model checkpoint storage", || {
            Ok(Self::with_parents(
                model,
                model
                    .feature_regeneration_parents
                    .try_clone_for_decode(ctx, "model checkpoint parents")?,
            ))
        })
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
    /// Parent-table copies use the caller's context. Dropping arena suffixes releases admitted values.
    pub fn discard_appended(
        &self,
        model: &mut Model,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        let parents = self
            .feature_parents
            .try_clone_for_decode(ctx, "model checkpoint restored parents")?;
        macro_rules! check_append_bounds {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                if model.$field.len() < self.length::<$ty>() {
                    return Err(CodecError::malformed("checkpoint cannot restore removed entities"));
                }
            )*};
        }
        crate::document::arena_registry!(check_append_bounds);
        self.restore(model, parents);
        Ok(())
    }

    /// Compare captured state after admitting the parent-table comparisons.
    pub fn same_state(&self, other: &Self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        ctx.charge_work(0, "compare model checkpoint lengths")?;
        if self.lengths != other.lengths {
            return Ok(false);
        }
        self.feature_parents.equivalent(&other.feature_parents, ctx)
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

type IdentityIndex = BTreeMap<u64, Vec<IdentitySlot>>;

fn index_model_identities<'a>(
    model: &'a Model,
    ctx: &DecodeContext<'_>,
) -> Result<Result<IdentityIndex, &'a str>, CodecError> {
    let mut identity_index = IdentityIndex::new();
    macro_rules! index_arenas {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for (slot_index, entity) in ctx.admit_iter(&model.$field, "draft identity scan")?.enumerate() {
                let hash = ctx.hash_value(entity.identity(), "draft identity scan")?;
                if identity_index_contains(model, &identity_index, hash, entity.identity(), ctx)? {
                    return Ok(Err(entity.identity()));
                }
                ctx.push_btree_group(&mut identity_index, hash, IdentitySlot { kind: <$ty as EntitySchema>::KIND, index: slot_index }, "draft identity slots", "draft identity slots")?;
            })*
        };
    }
    crate::document::arena_registry!(index_arenas);
    Ok(Ok(identity_index))
}

fn identity_index_contains(
    model: &Model,
    index: &IdentityIndex,
    hash: u64,
    identity: &str,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(slots) = ctx.get_btree_map(index, &hash, "draft identity bucket lookup")? else {
        return Ok(false);
    };
    ctx.any_by(
        slots,
        |slot| match model.identity_at(slot.kind, slot.index) {
            Some(candidate) => {
                identities_equal(ctx, candidate, identity, "compare draft identities")
            }
            None => Ok(false),
        },
        "draft identity collision scan",
    )
}

fn identities_equal(
    ctx: &DecodeContext<'_>,
    left: &str,
    right: &str,
    operation: &'static str,
) -> Result<bool, CodecError> {
    ctx.equal_bytes(left.as_bytes(), right.as_bytes(), operation)
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
    pub fn insert<T: ArenaEntity>(
        &mut self,
        entity: T,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), DraftError> {
        let identity = entity.identity();
        macro_rules! check_identity {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(if !self.model.$field.is_empty() && ctx.any_by(&self.model.$field, |existing| identities_equal(ctx, existing.identity(), identity, "compare draft insertion identities"), "draft insertion identity scan")? {
                    return Err(DraftError::IdentityCollision(ctx.copy_retained_text(identity, "draft identity collision")?));
                })*
            };
        }
        crate::document::arena_registry!(check_identity);
        ctx.reserve_vec(T::arena_mut(&mut self.model), 1, "draft entity arena")?;
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
        &mut self.model
    }

    /// Number of staged entities.
    pub fn entity_count(&self) -> usize {
        self.model.entity_count()
    }

    fn validate_with_contains(
        &mut self,
        base: &Model,
        contains: impl Fn(&str) -> Result<bool, CodecError>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), DraftError>, CodecError> {
        let indexed = ctx.with_scoped_storage("draft identity storage", || {
            index_model_identities(&self.model, ctx)
        })?;
        let identity_index = match &indexed.0 {
            Ok(index) => index,
            Err(identity) => {
                return Ok(Err(DraftError::IdentityCollision(
                    ctx.copy_retained_text(identity, "draft identity collision")?,
                )))
            }
        };
        macro_rules! check_external_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(if !self.model.$field.is_empty() { if let Some(entity) = ctx.find_by(&self.model.$field, |entity| contains(entity.identity()), "draft external identity scan")? {
                    let identity = ctx.copy_retained_text(entity.identity(), "draft external identity collision")?;
                    return Ok(Err(DraftError::IdentityCollision(identity)));
                } })*
            };
        }
        crate::document::arena_registry!(check_external_identities);
        macro_rules! validate_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(for entity in ctx.admit_iter(&self.model.$field, "draft reference scan")? {
                    let owner = entity.identity();
                    let mut missing = None;
                    entity.visit_references(ctx, &mut |target| {
                        if missing.is_some() {
                            return Ok(());
                        }
                        if !contains(target)? && !identity_index_contains(&self.model, identity_index, ctx.hash_value(target, "hash draft identity lookup")?, target, ctx)? {
                            missing = Some(ctx.copy_retained_text(target, "draft missing reference")?);
                        }
                        Ok(())
                    })?;
                    if let Some(target) = missing {
                        let owner = ctx.copy_retained_text(owner, "draft missing reference owner")?;
                        return Ok(Err(DraftError::UnresolvedReference { owner, target }));
                    }
                })*
            };
        }
        crate::document::arena_registry!(validate_arenas);
        if !self.model.features.is_empty() || self.model.has_feature_regeneration_parents() {
            if let Err(error) =
                crate::document::feature_parents::validate(ctx, &[base, &self.model])?
            {
                return Ok(Err(DraftError::FeatureParents {
                    owner: error
                        .owner()
                        .try_clone_for_decode(ctx, "feature parent diagnostic owner")?,
                    message: ctx
                        .format_retained(format_args!("{error}"), "feature parent diagnostic")?,
                }));
            }
        }
        Ok(Ok(()))
    }
}

impl ModelDraft<DraftAccounting> {
    /// Record staged exactness after admitting the retained key and record.
    pub fn exactness(
        &mut self,
        ctx: &DecodeContext<'_>,
        identity: impl std::fmt::Display,
        exactness: Exactness,
    ) -> Result<(), CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "draft exactness lookup")?;
        let identity = ctx.format_scoped_text(
            &mut scratch,
            format_args!("{identity}"),
            "draft exactness lookup",
        )?;
        if exactness == Exactness::ByteExact {
            ctx.remove_btree_map(
                &mut self.accounting.exactness,
                &identity,
                "draft exactness comparisons",
            )?;
        } else {
            let identity = if ctx.contains_key_btree_map(
                &self.accounting.exactness,
                &identity,
                "draft exactness comparisons",
            )? {
                identity
            } else {
                ctx.copy_retained_text(&identity, "draft exactness identity")?
            };
            ctx.insert_btree_map(
                &mut self.accounting.exactness,
                identity,
                exactness,
                "draft exactness records",
            )?;
        }
        Ok(())
    }

    /// Retains exactness notes selected by identity.
    pub fn retain_exactness(
        &mut self,
        ctx: &DecodeContext<'_>,
        keep: impl FnMut(&str) -> Result<bool, CodecError>,
    ) -> Result<(), CodecError> {
        crate::annotations::retain_identity_entries(
            ctx,
            &mut self.accounting.exactness,
            keep,
            "draft exactness decisions",
            "draft exactness predicate scan",
            "draft exactness retention scan",
        )
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

type CommittedIdentityIndex = BTreeMap<u64, Vec<CommittedIdentity>>;

/// A document owner or exclusive borrow with a caller-owned identity cache.
///
/// The cache is built on the first lookup or commit. The context stays borrowed
/// until the session ends. Each successful commit extends an existing cache.
/// Speculative append prepares cache transfers inside the document transaction.
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
    storage: ScopedReservation<'ctx>,
}

#[derive(Debug)]
struct CommitState<'ctx, D: BorrowMut<CadIr>> {
    base: D,
    unknowns: Vec<crate::unknown::UnknownRecord>,
    unknown_namespace: Option<&'ctx str>,
    identities: Option<CommittedIdentityIndex>,
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
            },
            ctx,
            storage: ctx.reserve_scoped(0, "committed identity storage")?,
        })
    }

    /// Read the committed document while its cache reservation remains live.
    pub fn document(&self) -> &CadIr {
        self.state.base.borrow()
    }

    /// Invalidate cached positions before exposing the document for mutation.
    pub fn document_mut(&mut self) -> Result<&mut CadIr, CodecError> {
        let storage = self.ctx.reserve_scoped(0, "committed identity storage")?;
        self.state.identities = None;
        self.storage = storage;
        Ok(self.state.base.borrow_mut())
    }

    /// Release the cache and transfer the document and staged source records.
    pub fn into_parts(self) -> (D, Vec<crate::unknown::UnknownRecord>) {
        let Self {
            state,
            ctx: _,
            storage,
        } = self;
        let CommitState {
            base,
            unknowns,
            unknown_namespace: _,
            identities,
        } = state;
        drop(identities);
        drop(storage);
        (base, unknowns)
    }

    /// Borrow staged source records without constructing native product records.
    pub fn unknowns(&self) -> &[crate::unknown::UnknownRecord] {
        &self.state.unknowns
    }

    /// Mutate outgoing links while keeping cached identity positions stable.
    pub fn unknown_links_mut(&mut self, index: usize) -> Option<(&str, &mut Vec<String>)> {
        self.state
            .unknowns
            .get_mut(index)
            .map(crate::unknown::UnknownRecord::id_and_links_mut)
    }

    /// Move an owned source population into the session and invalidate cached positions.
    pub fn replace_unknowns(
        &mut self,
        records: Vec<crate::unknown::UnknownRecord>,
    ) -> Result<(), CodecError> {
        let storage = self.ctx.reserve_scoped(0, "committed identity storage")?;
        self.state.identities = None;
        self.storage = storage;
        self.state.unknowns = records;
        Ok(())
    }

    /// Admit a moved source record and extend a previously built identity cache.
    pub fn push_unknown(
        &mut self,
        record: crate::unknown::UnknownRecord,
    ) -> Result<(), CodecError> {
        self.ctx
            .reserve_vec(&mut self.state.unknowns, 1, "staged unknown record slots")?;
        if let Some(index) = &mut self.state.identities {
            let hash = self
                .ctx
                .hash_value(record.id().as_str(), "hash staged unknown identity")?;
            self.storage.with_storage(|| {
                self.ctx.push_btree_group(
                    index,
                    hash,
                    CommittedIdentity::StagedUnknown(self.state.unknowns.len()),
                    "committed identity slots",
                    "committed identity slots",
                )
            })?;
        }
        self.state.unknowns.push(record);
        Ok(())
    }

    /// Validate and commit one model draft under this session's context.
    pub fn commit_model(
        &mut self,
        draft: ModelDraft,
    ) -> Result<Result<(), DraftError>, CodecError> {
        self.state
            .commit_with_storage(draft, self.ctx, &mut self.storage, || Ok(()))
    }

    /// Admit an accounted draft and its annotation transfer before either is applied.
    pub fn commit(
        &mut self,
        draft: ModelDraft<DraftAccounting>,
        annotations: &mut Annotations,
    ) -> Result<Result<(), DraftError>, CodecError> {
        let ctx = self.ctx;
        let transaction = annotations.copy_transaction(ctx, "draft annotation transaction")?;
        let ModelDraft { model, accounting } = draft;
        let ((), transaction) = transaction.update(|annotations| {
            let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
            for (identity, exactness) in
                ctx.admit_iter(accounting.exactness, "draft exactness transfer scan")?
            {
                builder.exactness_owned(ctx, identity, exactness)?;
            }
            *annotations = builder.build();
            Ok::<_, CodecError>(())
        })?;
        match self.state.commit_with_storage(
            ModelDraft {
                model,
                accounting: (),
            },
            ctx,
            &mut self.storage,
            || transaction.into_retained(),
        )? {
            Ok(merged) => {
                *annotations = merged;
                Ok(Ok(()))
            }
            Err(error) => Ok(Err(error)),
        }
    }

    /// Append a candidate and extend live cache positions inside its transaction.
    pub fn try_append<T, E>(
        &mut self,
        model: Model,
        native: crate::native::Native,
        admit: impl FnOnce(&CadIr, &[crate::unknown::UnknownRecord]) -> Result<Result<T, E>, CodecError>,
    ) -> Result<Result<T, E>, CodecError> {
        let ctx = self.ctx;
        let unknowns = &self.state.unknowns;
        let Some(index) = &mut self.state.identities else {
            return self
                .state
                .base
                .borrow_mut()
                .try_append(ctx, model, native, |combined| admit(combined, unknowns));
        };
        let staged = ctx.with_scoped_storage("candidate committed identity staging", || {
            stage_committed_identities(
                self.state.base.borrow(),
                &model,
                Some(&native),
                self.state.unknown_namespace,
                ctx,
            )
        })?;
        let cache = &mut self.storage;
        self.state
            .base
            .borrow_mut()
            .try_append(ctx, model, native, |combined| {
                let result = admit(combined, unknowns)?;
                if result.is_ok() {
                    for (hash, mut group) in
                        ctx.admit_iter(staged.0, "committed identity transfer scan")?
                    {
                        cache.with_storage(|| {
                            let target = ctx
                                .entry_btree_map(index, hash, "committed identity slots")?
                                .or_default();
                            ctx.append_vec(target, &mut group, "committed identity transfer moves")
                        })?;
                    }
                }
                Ok(result)
            })
    }

    /// Look up an identity through the live caller-accounted cache.
    pub fn contains(&mut self, identity: &str) -> Result<bool, CodecError> {
        self.state
            .lookup_with_storage(identity, self.ctx, &mut self.storage)
    }
}

fn index_committed_identities(
    base: &CadIr,
    unknowns: &[crate::unknown::UnknownRecord],
    unknown_namespace: Option<&str>,
    ctx: &DecodeContext<'_>,
) -> Result<CommittedIdentityIndex, CodecError> {
    let mut identities = CommittedIdentityIndex::new();
    macro_rules! collect_model_identities {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for (index, entity) in ctx.admit_iter(&base.model.$field, "committed identity scan")?.enumerate() {
                let hash = ctx.hash_value(entity.identity(), "committed identity scan")?;
                ctx.push_btree_group(&mut identities, hash, CommittedIdentity::Neutral(IdentitySlot { kind: <$ty as EntitySchema>::KIND, index }), "committed identity slots", "committed identity slots")?;
            })*
        };
    }
    crate::document::arena_registry!(collect_model_identities);
    for (format, records) in ctx.admit_iter(&base.native.0, "committed native namespace scan")? {
        let replaces_unknowns = match unknown_namespace {
            Some(replacement) => identities_equal(
                ctx,
                format,
                replacement,
                "compare committed unknown namespace",
            )?,
            None => false,
        };
        let namespace = ctx.hash_value(format, "hash committed native namespace")?;
        for (arena, records) in ctx.admit_iter(records.arenas(), "committed native arena scan")? {
            if replaces_unknowns
                && identities_equal(ctx, arena, "unknowns", "compare committed unknown arena")?
            {
                continue;
            }
            let arena = ctx.hash_value(arena, "hash committed native arena")?;
            for (record, value) in ctx
                .admit_iter(records, "committed native identity scan")?
                .enumerate()
            {
                let hash = ctx.hash_value(value.id(), "committed native identity scan")?;
                ctx.push_btree_group(
                    &mut identities,
                    hash,
                    CommittedIdentity::Native {
                        namespace_hash: namespace,
                        arena_hash: arena,
                        record,
                    },
                    "committed identity slots",
                    "committed identity slots",
                )?;
            }
        }
    }
    for (index, record) in ctx
        .admit_iter(unknowns, "committed unknown identity scan")?
        .enumerate()
    {
        let hash = ctx.hash_value(record.id().as_str(), "committed unknown identity scan")?;
        ctx.push_btree_group(
            &mut identities,
            hash,
            CommittedIdentity::StagedUnknown(index),
            "committed identity slots",
            "committed identity slots",
        )?;
    }
    Ok(identities)
}

fn committed_identity_contains(
    base: &CadIr,
    unknowns: &[crate::unknown::UnknownRecord],
    identities: &CommittedIdentityIndex,
    identity: &str,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    ctx.charge_work(0, "committed identity lookup")?;
    if identities.is_empty() {
        return Ok(false);
    }
    let hash = ctx.hash_value(identity, "committed identity lookup")?;
    let Some(owners) = ctx.get_btree_map(identities, &hash, "committed identity bucket lookup")?
    else {
        return Ok(false);
    };
    ctx.any_by(
        owners,
        |owner| {
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
                    return ctx.any_by(
                        &base.native.0,
                        |(format, native)| {
                            if ctx.hash_value(format, "find committed native namespace")?
                                != *namespace_hash
                            {
                                return Ok(false);
                            }
                            ctx.any_by(
                                native.arenas(),
                                |(name, records)| {
                                    if ctx.hash_value(name, "find committed native arena")?
                                        != *arena_hash
                                    {
                                        return Ok(false);
                                    }
                                    match records.get(*record) {
                                        Some(candidate) => identities_equal(
                                            ctx,
                                            candidate.id(),
                                            identity,
                                            "compare committed identities",
                                        ),
                                        None => Ok(false),
                                    }
                                },
                                "committed native arena scan",
                            )
                        },
                        "committed native namespace scan",
                    );
                }
            };
            match candidate {
                Some(candidate) => {
                    identities_equal(ctx, candidate, identity, "compare committed identities")
                }
                None => Ok(false),
            }
        },
        "committed identity collision scan",
    )
}

fn stage_committed_identities(
    base: &CadIr,
    model: &Model,
    native: Option<&crate::native::Native>,
    unknown_namespace: Option<&str>,
    ctx: &DecodeContext<'_>,
) -> Result<CommittedIdentityIndex, CodecError> {
    let mut staged = CommittedIdentityIndex::new();
    macro_rules! stage_model {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
            for (offset, entity) in ctx.admit_iter(&model.$field, "staged committed identity scan")?.enumerate() {
                let hash = ctx.hash_value(entity.identity(), "hash staged committed identity")?;
                let index = base.model.$field.len().checked_add(offset).ok_or_else(|| ctx.refuse_codec_limit("staged committed identity position", u64::MAX - 1, u64::MAX))?;
                ctx.push_btree_group(&mut staged, hash, CommittedIdentity::Neutral(IdentitySlot { kind: <$ty as EntitySchema>::KIND, index }), "draft committed identity slots", "draft committed identity slots")?;
            }
        )*};
    }
    crate::document::arena_registry!(stage_model);
    if let Some(native) = native {
        for (format, namespace) in ctx.admit_iter(&native.0, "staged native namespace scan")? {
            let namespace_hash = ctx.hash_value(format, "hash staged native namespace")?;
            let replaces_unknowns = match unknown_namespace {
                Some(replacement) => {
                    identities_equal(ctx, format, replacement, "compare staged unknown namespace")?
                }
                None => false,
            };
            let existing_namespace =
                ctx.get_btree_map(&base.native.0, format, "find staged native namespace")?;
            for (arena, records) in
                ctx.admit_iter(namespace.arenas(), "staged native arena scan")?
            {
                if replaces_unknowns
                    && identities_equal(ctx, arena, "unknowns", "compare staged unknown arena")?
                {
                    continue;
                }
                let arena_hash = ctx.hash_value(arena, "hash staged native arena")?;
                let start = match existing_namespace {
                    Some(namespace) => ctx
                        .get_btree_map(namespace.arenas(), arena, "find staged native arena")?
                        .map_or(0, Vec::len),
                    None => 0,
                };
                for (offset, record) in ctx
                    .admit_iter(records, "staged native identity scan")?
                    .enumerate()
                {
                    let hash = ctx.hash_value(record.id(), "hash staged native identity")?;
                    let position = start.checked_add(offset).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "staged native identity position",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
                    ctx.push_btree_group(
                        &mut staged,
                        hash,
                        CommittedIdentity::Native {
                            namespace_hash,
                            arena_hash,
                            record: position,
                        },
                        "draft committed identity slots",
                        "draft committed identity slots",
                    )?;
                }
            }
        }
    }
    Ok(staged)
}

impl<D: BorrowMut<CadIr>> CommitState<'_, D> {
    fn lookup_with_storage(
        &mut self,
        identity: &str,
        ctx: &DecodeContext<'_>,
        storage: &mut ScopedReservation<'_>,
    ) -> Result<bool, CodecError> {
        storage.with_storage(|| self.ensure_identities(ctx))?;
        match &self.identities {
            Some(index) => committed_identity_contains(
                self.base.borrow(),
                &self.unknowns,
                index,
                identity,
                ctx,
            ),
            None => Err(CodecError::malformed("committed identity index is absent")),
        }
    }

    fn ensure_identities(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        if self.identities.is_none() {
            self.identities = Some(index_committed_identities(
                self.base.borrow(),
                &self.unknowns,
                self.unknown_namespace,
                ctx,
            )?);
        }
        Ok(())
    }

    /// Validates and commits a model draft with allocation admission.
    ///
    /// The outer result reports a resource refusal. The inner result reports
    /// a rejected candidate without changing the committed model.
    fn commit_with_storage<T>(
        &mut self,
        mut draft: ModelDraft,
        ctx: &DecodeContext<'_>,
        cache: &mut ScopedReservation<'_>,
        before_apply: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<Result<T, DraftError>, CodecError> {
        cache.with_storage(|| self.ensure_identities(ctx))?;
        let base = self.base.borrow_mut();
        let identities = self
            .identities
            .as_mut()
            .ok_or_else(|| CodecError::Malformed("committed identity index is absent".into()))?;
        if let Err(error) = draft.validate_with_contains(
            &base.model,
            |identity| committed_identity_contains(base, &self.unknowns, identities, identity, ctx),
            ctx,
        )? {
            return Ok(Err(error));
        }
        macro_rules! reserve_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(if !draft.model.$field.is_empty() {
                    ctx.reserve_vec(&mut base.model.$field, draft.model.$field.len(), "committed model arena slots")?;
                })*
            };
        }
        crate::document::arena_registry!(reserve_arenas);
        base.model
            .feature_regeneration_parents
            .reserve_append(&draft.model.feature_regeneration_parents, ctx)?;
        let staged = ctx.with_scoped_storage("draft committed identity staging", || {
            stage_committed_identities(base, &draft.model, None, self.unknown_namespace, ctx)
        })?;
        for (hash, mut group) in ctx.admit_iter(staged.0, "committed identity transfer scan")? {
            cache.with_storage(|| {
                let target = ctx
                    .entry_btree_map(identities, hash, "committed identity slots")?
                    .or_default();
                ctx.append_vec(target, &mut group, "committed identity transfer moves")
            })?;
        }
        let transferred = before_apply()?;
        base.model.append(draft.model);
        Ok(Ok(transferred))
    }
}

#[cfg(test)]
mod tests {
    mod accounting;
    mod checkpoints;
    mod feature_parents;
    mod native_identity_slots;
    mod owned_session;

    use super::{CommitSession, DraftError, ModelCheckpoint, ModelDraft};
    use crate::annotations::Annotations;
    use crate::document::CadIr;
    use crate::ids::PointId;
    use crate::math::Point3;
    use crate::native::NativeRecord;
    use crate::topology::{Point, Vertex};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

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
        draft
            .insert(point(id), &cadmpeg_test_support::service_decode_context())
            .expect("insert point into draft");
        draft
    }

    fn vertex_draft(id: &str, point: &str) -> ModelDraft {
        let mut draft = ModelDraft::new();
        draft
            .insert(
                Vertex {
                    id: id.try_into().expect("valid identity"),
                    point: point.try_into().expect("valid identity"),
                    tolerance: None,
                },
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("insert vertex into draft");
        draft
    }

    #[test]
    fn draft_and_committed_identity_collisions_admit_string_comparison_work() {
        let candidate = "test:model:point#one";
        let target = "test:model:point#two";
        assert_eq!(candidate.len(), target.len());
        for committed in [false, true] {
            let mut ir = CadIr::empty();
            ir.model.points.push(point(candidate));
            let slot = super::IdentitySlot {
                kind: crate::schema::EntityKind::Point,
                index: 0,
            };
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                if committed {
                    "compare committed identities"
                } else {
                    "compare draft identities"
                },
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let hash = crate::index::identity_hash(target);
                    let result = if committed {
                        let index = std::collections::BTreeMap::from([(
                            hash,
                            vec![super::CommittedIdentity::Neutral(slot)],
                        )]);
                        super::committed_identity_contains(&ir, &[], &index, target, &ctx)
                    } else {
                        let index = std::collections::BTreeMap::from([(hash, vec![slot])]);
                        super::identity_index_contains(
                            &ir.model,
                            &index,
                            ctx.hash_value(target, "hash draft identity lookup")?,
                            target,
                            &ctx,
                        )
                    };
                    if let Err(CodecError::ResourceLimit(limit)) = &result {
                        assert!(
                            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit)
                        );
                    }
                    result
                },
            );
            let CodecError::ResourceLimit(limit) = error else {
                panic!("comparison admission must retain its refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.additional, u64::try_from(target.len()).unwrap());
            assert_eq!(
                limit.operation,
                if committed {
                    "compare committed identities"
                } else {
                    "compare draft identities"
                }
            );
        }
    }

    #[test]
    fn draft_identity_search_admits_only_the_matching_prefix() {
        let target = "test:model:point#target";
        let mut ir = CadIr::empty();
        ir.model.points.push(point(target));
        let slot = super::IdentitySlot {
            kind: crate::schema::EntityKind::Point,
            index: 0,
        };
        let index = std::collections::BTreeMap::from([(
            crate::index::identity_hash(target),
            vec![slot; 128],
        )]);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "draft search completed",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                assert!(super::identity_index_contains(
                    &ir.model,
                    &index,
                    ctx.hash_value(target, "hash draft identity lookup")?,
                    target,
                    &ctx
                )?);
                ctx.charge_work(1, "draft search completed")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("completion boundary");
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit.used;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let single =
            std::collections::BTreeMap::from([(crate::index::identity_hash(target), vec![slot])]);
        assert!(super::identity_index_contains(
            &ir.model,
            &single,
            ctx.hash_value(target, "hash draft identity lookup")
                .unwrap(),
            target,
            &ctx
        )
        .unwrap());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn admitted_draft_commit_refuses_identity_index_storage_before_mutating_model() {
        fn directly_staged_point() -> ModelDraft {
            let mut draft = ModelDraft::new();
            draft.model_mut().points.push(point("test:model:point#new"));
            draft
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = CadIr::empty();
        let result = CommitSession::new(&mut ir, &ctx, None)
            .unwrap()
            .commit_model(directly_staged_point());
        assert!(matches!(result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "draft identity slots"
                    && limit.used == 0
                    && limit.additional == 1
        ));
        assert!(ir.model.points.is_empty());

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        CommitSession::new(&mut ir, &ctx, None)
            .unwrap()
            .commit_model(directly_staged_point())
            .unwrap()
            .unwrap();
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn admitted_draft_commit_refuses_missing_reference_text_before_copy() {
        let missing = "test:model:point#missing";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(missing.len() - 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = CadIr::empty();
        let result = CommitSession::new(&mut ir, &ctx, None)
            .unwrap()
            .commit_model(vertex_draft("test:model:vertex#new", missing));
        assert!(matches!(result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "draft missing reference"
                    && limit.additional == u64::try_from(missing.len()).unwrap()
        ));
        assert!(ir.model.vertices.is_empty());

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
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let checkpoint = ModelCheckpoint::capture(&model, &ctx).unwrap();
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
                &cadmpeg_test_support::service_decode_context(),
                &("test:checkpoint:feature#child".try_into().unwrap()),
                &("test:checkpoint:feature#parent".try_into().unwrap()),
            )
            .unwrap();
        checkpoint.0.discard_appended(&mut model, &ctx).unwrap();
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
    fn decoded_draft_refuses_identity_lookup_work() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let draft = feature_parents::parent_draft();
        let mut ir = CadIr::empty();
        let before = ir.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = CommitSession::new(&mut ir, &ctx, None)
            .unwrap()
            .commit_model(draft);
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits)
        );
        assert_eq!(ir, before);
    }

    #[test]
    fn draft_commit_preserves_admitted_feature_regeneration_parents() {
        use crate::features::{Feature, FeatureDefinition, FeatureOperation};

        let mut draft = ModelDraft::new();
        for (ordinal, key) in ["parent", "child"].into_iter().enumerate() {
            draft
                .insert(
                    Feature {
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
                    },
                    &cadmpeg_test_support::service_decode_context(),
                )
                .unwrap();
        }
        let child = "test:draft:feature#child".try_into().unwrap();
        let parent = "test:draft:feature#parent".try_into().unwrap();
        draft
            .model_mut()
            .set_feature_regeneration_parent(
                &cadmpeg_test_support::service_decode_context(),
                &(child),
                &(parent),
            )
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
            .insert(
                point("test:model:point#1"),
                &cadmpeg_test_support::service_decode_context(),
            )
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
            .insert(
                Vertex {
                    id: rejected_identity.try_into().expect("valid identity"),
                    point: "test:model:point#missing"
                        .try_into()
                        .expect("valid identity"),
                    tolerance: None,
                },
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("insert rejected vertex");
        assert!(!session.contains(rejected_identity).unwrap());
        assert!(session.commit_model(rejected).unwrap().is_err());
        assert!(!session.contains(rejected_identity).unwrap());
    }
}
