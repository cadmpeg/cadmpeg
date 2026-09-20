// SPDX-License-Identifier: Apache-2.0
//! Atomic staging for neutral entity transfer.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::annotations::{AnnotationBuilder, Annotations};
use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::document::{CadIr, Model};
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
};
use crate::geometry::{pcurve::Pcurve, Curve, ProceduralCurve, ProceduralSurface, Surface};
use crate::index::identity_hash;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCheckpoint {
    lengths: [usize; EntityKind::ALL.len()],
    feature_parents: crate::document::FeatureRegenerationParents,
}

impl ModelCheckpoint {
    /// Captures every neutral arena length.
    pub fn capture(model: &Model) -> Self {
        macro_rules! capture_lengths {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                [$(model.$field.len()),*]
            };
        }
        let lengths = crate::document::arena_registry!(capture_lengths);
        Self {
            lengths,
            feature_parents: model.feature_regeneration_parents.clone(),
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
    pub fn discard_appended(&self, model: &mut Model) {
        macro_rules! truncate_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(model.$field.truncate(self.length::<$ty>());)*
            };
        }
        crate::document::arena_registry!(truncate_arenas);
        model.feature_regeneration_parents = self.feature_parents.clone();
    }
}

macro_rules! impl_arena_entities {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
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

fn identity_index_contains(model: &Model, index: &IdentityIndex, identity: &str) -> bool {
    index
        .get(&identity_hash(identity))
        .into_iter()
        .flatten()
        .any(|slot| model.identity_at(slot.kind, slot.index) == Some(identity))
}

fn index_model_identities(model: &Model) -> Result<IdentityIndex, DraftError> {
    let mut identity_index = IdentityIndex::new();
    macro_rules! index_arenas {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
            $(for (slot_index, entity) in model.$field.iter().enumerate() {
                if identity_index_contains(model, &identity_index, entity.identity()) {
                    return Err(DraftError::IdentityCollision(entity.identity().to_owned()));
                }
                identity_index
                    .entry(identity_hash(entity.identity()))
                    .or_default()
                    .push(IdentitySlot {
                        kind: <$ty as EntitySchema>::KIND,
                        index: slot_index,
                    });
            })*
        };
    }
    crate::document::arena_registry!(index_arenas);
    Ok(identity_index)
}

/// Error returned before an atomic draft commit mutates its destination.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DraftError {
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
    /// A staged entity cannot state its own typed references.
    #[error("staged entity {owner} cannot state its typed references: {source}")]
    ReferenceWalk {
        /// Staged entity whose schema walk failed.
        owner: String,
        /// Walk failure raised by the entity's own serialization.
        #[source]
        source: crate::schema::ReferenceWalkError,
    },
}

/// Transactional collection of staged model entities.
///
/// A plain draft carries no accounting and can commit through `commit_model`.
/// [`with_accounting`](Self::with_accounting) adds accounting and requires the
/// commit route that accepts every accounting destination.
#[derive(Debug)]
pub struct ModelDraft<A = ()> {
    model: Model,
    identity_index: Option<IdentityIndex>,
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
            identity_index: Some(IdentityIndex::new()),
            accounting: (),
        }
    }
}

impl ModelDraft {
    /// Creates an empty draft.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add accounting while preserving all staged entities and their index.
    ///
    /// Accounted drafts cannot use a model-only commit:
    ///
    /// ```compile_fail
    /// let draft = cadmpeg_ir::draft::ModelDraft::new().with_accounting();
    /// draft.commit_model(&mut cadmpeg_ir::CadIr::empty()).unwrap();
    /// ```
    ///
    /// A commit session also requires a draft with no accounting:
    ///
    /// ```compile_fail
    /// let draft = cadmpeg_ir::draft::ModelDraft::new().with_accounting();
    /// let mut document = cadmpeg_ir::CadIr::empty();
    /// cadmpeg_ir::draft::CommitSession::new(&mut document).commit_model(draft).unwrap();
    /// ```
    pub fn with_accounting(self) -> ModelDraft<DraftAccounting> {
        ModelDraft {
            model: self.model,
            identity_index: self.identity_index,
            accounting: DraftAccounting::default(),
        }
    }

    /// Commit a draft that carries model entities only.
    pub fn commit_model(mut self, base: &mut CadIr) -> Result<(), DraftError> {
        self.validate_against(base)?;
        base.model.append(self.model);
        Ok(())
    }
}

impl<A> ModelDraft<A> {
    /// Inserts one entity, rejecting draft-local identity collisions immediately.
    pub fn insert<T: ArenaEntity>(&mut self, entity: T) -> Result<(), DraftError> {
        let mut identity_index = self.take_identity_index()?;
        let identity = entity.identity();
        if identity_index_contains(&self.model, &identity_index, identity) {
            self.identity_index = Some(identity_index);
            return Err(DraftError::IdentityCollision(identity.to_owned()));
        }
        let index = T::arena(&self.model).len();
        identity_index
            .entry(identity_hash(identity))
            .or_default()
            .push(IdentitySlot {
                kind: T::KIND,
                index,
            });
        T::arena_mut(&mut self.model).push(entity);
        self.identity_index = Some(identity_index);
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
        self.identity_index = None;
        &mut self.model
    }

    /// Number of staged entities.
    pub fn entity_count(&self) -> usize {
        self.model.entity_count()
    }

    // Validation needs only membership in the identity universe, so this
    // collects the universe directly — every neutral arena in the registry
    // plus every native record id — instead of building a full `ModelIndex`
    // whose typed lookup maps would all go unused. The set must stay equal
    // to `ModelIndex::contains`'s universe, or identity-collision detection
    // goes unsound; both derive their arena list from `arena_registry!`, so
    // only an identity source added to `ModelIndex` outside the registry
    // arenas and the native namespaces could split them.
    fn validate_against(&mut self, base: &CadIr) -> Result<(), DraftError> {
        let mut identities = HashSet::with_capacity(base.model.entity_count());
        macro_rules! collect_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(for entity in &base.model.$field {
                    identities.insert(entity.identity());
                })*
            };
        }
        crate::document::arena_registry!(collect_identities);
        for record in base
            .native
            .0
            .values()
            .flat_map(|namespace| namespace.arenas().values().flatten())
        {
            identities.insert(record.id());
        }
        self.validate_with_contains(&base.model, |identity| identities.contains(identity))
    }

    fn take_identity_index(&mut self) -> Result<IdentityIndex, DraftError> {
        match self.identity_index.take() {
            Some(identity_index) => Ok(identity_index),
            None => index_model_identities(&self.model),
        }
    }

    /// Validates staged identities and references against one identity universe.
    ///
    /// The caller supplies the identities already committed outside this draft;
    /// references may also target another entity staged in the same draft. The
    /// index populated by `insert` is reused. Direct mutable staging invalidates
    /// that index and causes one complete rebuild before validation.
    fn validate_with_contains(
        &mut self,
        base: &Model,
        contains: impl Fn(&str) -> bool,
    ) -> Result<(), DraftError> {
        let identity_index = self.take_identity_index()?;
        macro_rules! check_external_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(for entity in &self.model.$field {
                    if contains(entity.identity()) {
                        return Err(DraftError::IdentityCollision(entity.identity().to_owned()));
                    }
                })*
            };
        }
        crate::document::arena_registry!(check_external_identities);
        macro_rules! validate_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(for entity in &self.model.$field {
                    let owner = entity.identity();
                    let mut missing = None;
                    entity.visit_references(&mut |reference| {
                        if missing.is_none()
                            && !contains(&reference.target)
                            && !identity_index_contains(&self.model, &identity_index, &reference.target)
                        {
                            missing = Some(reference.target);
                        }
                    })
                    .map_err(|source| DraftError::ReferenceWalk {
                        owner: owner.to_owned(),
                        source,
                    })?;
                    if let Some(target) = missing {
                        return Err(DraftError::UnresolvedReference {
                            owner: owner.to_owned(),
                            target,
                        });
                    }
                })*
            };
        }
        crate::document::arena_registry!(validate_arenas);
        if !self.model.features.is_empty() || self.model.has_feature_regeneration_parents() {
            crate::document::validate_feature_parents(&[base, &self.model]).map_err(|error| {
                DraftError::FeatureParents {
                    owner: error.owner,
                    message: error.message,
                }
            })?;
        }
        self.identity_index = Some(identity_index);
        Ok(())
    }
}

impl ModelDraft<DraftAccounting> {
    /// Records sparse exactness for a staged entity.
    pub fn exactness(&mut self, identity: impl Into<String>, exactness: Exactness) {
        let identity = identity.into();
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

    /// Validates and atomically extends a document and its exactness annotations.
    pub fn commit(
        mut self,
        base: &mut CadIr,
        annotations: &mut Annotations,
    ) -> Result<(), DraftError> {
        self.validate_against(base)?;
        let Self {
            model,
            identity_index: _,
            accounting: DraftAccounting { exactness },
        } = self;
        base.model.append(model);
        let mut annotation_builder = AnnotationBuilder::resume(std::mem::take(annotations));
        for (identity, exactness) in exactness {
            annotation_builder.exactness(identity, exactness);
        }
        *annotations = annotation_builder.build();
        Ok(())
    }
}

#[derive(Debug)]
enum CommittedIdentity {
    Neutral(IdentitySlot),
    Native(String),
}

type CommittedIdentityIndex = HashMap<u64, Vec<CommittedIdentity>>;

/// An exclusive document borrow for a sequence of checked draft commits.
///
/// The identity index is built on the first lookup or commit. The document is
/// available only for shared reads until the session ends, so another writer
/// cannot invalidate the cached identities.
///
/// ```compile_fail
/// use cadmpeg_ir::{document::CadIr, draft::CommitSession};
/// let mut ir = CadIr::empty();
/// let mut session = CommitSession::new(&mut ir);
/// ir.model.points.clear();
/// session.contains("test:model:point#1");
/// ```
///
/// ```compile_fail
/// use cadmpeg_ir::{document::CadIr, draft::CommitSession};
/// let mut ir = CadIr::empty();
/// let session = CommitSession::new(&mut ir);
/// session.document().model.points.clear();
/// ```
#[derive(Debug)]
pub struct CommitSession<'a> {
    base: &'a mut CadIr,
    identities: Option<CommittedIdentityIndex>,
}

fn index_committed_identities(base: &CadIr) -> CommittedIdentityIndex {
    let mut identities = CommittedIdentityIndex::new();
    macro_rules! collect_model_identities {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
            $(for (index, entity) in base.model.$field.iter().enumerate() {
                identities
                    .entry(identity_hash(entity.identity()))
                    .or_default()
                    .push(CommittedIdentity::Neutral(IdentitySlot {
                        kind: <$ty as EntitySchema>::KIND,
                        index,
                    }));
            })*
        };
    }
    crate::document::arena_registry!(collect_model_identities);
    for record in base
        .native
        .0
        .values()
        .flat_map(|namespace| namespace.arenas().values().flatten())
    {
        identities
            .entry(identity_hash(record.id()))
            .or_default()
            .push(CommittedIdentity::Native(record.id().to_owned()));
    }
    identities
}

fn committed_identity_contains(
    base: &CadIr,
    identities: &CommittedIdentityIndex,
    identity: &str,
) -> bool {
    identities
        .get(&identity_hash(identity))
        .into_iter()
        .flatten()
        .any(|owner| match owner {
            CommittedIdentity::Neutral(slot) => {
                base.model.identity_at(slot.kind, slot.index) == Some(identity)
            }
            CommittedIdentity::Native(candidate) => candidate == identity,
        })
}

impl<'a> CommitSession<'a> {
    /// Exclusively borrows a document without scanning its identity arenas yet.
    pub fn new(base: &'a mut CadIr) -> Self {
        Self {
            base,
            identities: None,
        }
    }

    /// Reads the current document, including every successful session commit.
    pub fn document(&self) -> &CadIr {
        self.base
    }

    /// Reports whether any neutral or native arena owns `identity`.
    pub fn contains(&mut self, identity: &str) -> bool {
        let identities = self
            .identities
            .get_or_insert_with(|| index_committed_identities(self.base));
        committed_identity_contains(self.base, identities, identity)
    }

    /// Validates and commits one model draft into the borrowed document.
    ///
    /// A rejected draft leaves both the document and its cached identity
    /// population unchanged and does not poison a later commit.
    pub fn commit_model(&mut self, mut draft: ModelDraft) -> Result<(), DraftError> {
        let identities = self
            .identities
            .get_or_insert_with(|| index_committed_identities(self.base));
        draft.validate_with_contains(&self.base.model, |identity| {
            committed_identity_contains(self.base, identities, identity)
        })?;
        // Validation has succeeded. Register the admitted draft's append slots
        // directly, before moving its arenas; no checkpoint slice can be absent.
        macro_rules! register_draft {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(for (offset, entity) in draft.model.$field.iter().enumerate() {
                    identities
                        .entry(identity_hash(entity.identity()))
                        .or_default()
                        .push(CommittedIdentity::Neutral(IdentitySlot {
                            kind: <$ty as EntitySchema>::KIND,
                            index: self.base.model.$field.len() + offset,
                        }));
                })*
            };
        }
        crate::document::arena_registry!(register_draft);
        self.base.model.append(draft.model);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    mod accounting;
    mod feature_parents;

    use super::{CommitSession, DraftError, ModelCheckpoint, ModelDraft};
    use crate::annotations::Annotations;
    use crate::document::CadIr;
    use crate::ids::PointId;
    use crate::math::Point3;
    use crate::native::NativeRecord;
    use crate::topology::{Point, Vertex};

    fn point(id: &str) -> Point {
        Point::new(
            PointId::mint(id).expect("valid identity"),
            Point3::new(0.0, 0.0, 0.0),
            None,
        )
        .expect("a finite position is a point")
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
                ordinal: ordinal as u64,
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
                "test:checkpoint:feature#child".try_into().unwrap(),
                "test:checkpoint:feature#parent".try_into().unwrap(),
            )
            .unwrap();
        checkpoint.discard_appended(&mut model);
        assert_eq!(model, original);
    }

    #[test]
    fn draft_commit_preserves_admitted_feature_regeneration_parents() {
        use crate::features::{Feature, FeatureDefinition, FeatureOperation};

        let mut draft = ModelDraft::new();
        for (ordinal, key) in ["parent", "child"].into_iter().enumerate() {
            draft
                .insert(Feature {
                    id: format!("test:draft:feature#{key}").try_into().unwrap(),
                    ordinal: ordinal as u64,
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
            .set_feature_regeneration_parent(child, parent)
            .unwrap();
        let expected = draft.model().clone();
        let mut ir = CadIr::empty();
        draft.commit_model(&mut ir).unwrap();
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
            draft.commit(&mut ir, &mut annotations),
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
            draft.commit_model(&mut ir),
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
            draft.commit_model(&mut ir),
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
        let mut session = CommitSession::new(&mut session_ir);
        session
            .commit_model(point_draft("test:model:point#1"))
            .expect("first session commit");
        session
            .commit_model(point_draft("test:model:point#2"))
            .expect("second session commit");

        let mut sequential_ir = CadIr::empty();
        point_draft("test:model:point#1")
            .commit_model(&mut sequential_ir)
            .expect("first sequential commit");
        point_draft("test:model:point#2")
            .commit_model(&mut sequential_ir)
            .expect("second sequential commit");

        assert_eq!(session_ir, sequential_ir);
    }

    #[test]
    fn commit_session_rejects_cross_draft_identity_collision() {
        let mut ir = CadIr::empty();
        let mut session = CommitSession::new(&mut ir);
        let identity = "test:model:point#cross-draft";
        session
            .commit_model(point_draft(identity))
            .expect("first session commit");

        assert_eq!(
            session.commit_model(point_draft(identity)),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn commit_session_rejects_pre_existing_neutral_identity() {
        let identity = "test:model:point#existing";
        let mut ir = CadIr::empty();
        ir.model.points.push(point(identity));
        let mut session = CommitSession::new(&mut ir);

        assert_eq!(
            session.commit_model(point_draft(identity)),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn commit_session_rejects_native_identity() {
        let identity = "test:native:record#1";
        let mut ir = CadIr::empty();
        ir.native.namespace_mut("test").arenas_mut().insert(
            "records".into(),
            vec![
                NativeRecord::new(identity, serde_json::Map::new()).expect("valid native identity")
            ],
        );
        let mut session = CommitSession::new(&mut ir);

        assert_eq!(
            session.commit_model(point_draft(identity)),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        assert!(ir.model.points.is_empty());
    }

    #[test]
    fn commit_session_rejects_unresolved_reference() {
        let owner = "test:model:vertex#missing";
        let target = "test:model:point#missing";
        let mut ir = CadIr::empty();
        let mut session = CommitSession::new(&mut ir);

        assert_eq!(
            session.commit_model(vertex_draft(owner, target)),
            Err(DraftError::UnresolvedReference {
                owner: owner.into(),
                target: target.into(),
            })
        );
        assert!(ir.model.vertices.is_empty());
    }

    #[test]
    fn commit_session_resolves_reference_into_earlier_draft() {
        let point_id = "test:model:point#earlier";
        let mut ir = CadIr::empty();
        let mut session = CommitSession::new(&mut ir);
        session
            .commit_model(point_draft(point_id))
            .expect("point commit");
        session
            .commit_model(vertex_draft("test:model:vertex#later", point_id))
            .expect("reference into committed draft resolves");

        assert_eq!(ir.model.points.len(), 1);
        assert_eq!(ir.model.vertices.len(), 1);
    }

    #[test]
    fn rejected_session_commit_leaves_session_and_base_usable() {
        let rejected_identity = "test:model:vertex#rejected";
        let mut ir = CadIr::empty();
        let before = ir.clone();
        let mut session = CommitSession::new(&mut ir);

        assert!(session
            .commit_model(vertex_draft(
                rejected_identity,
                "test:model:point#never-committed",
            ))
            .is_err());
        assert_eq!(session.document(), &before);

        session
            .commit_model(point_draft(rejected_identity))
            .expect("rejected identity was not absorbed into the session");
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn commit_session_contains_tracks_only_successful_commits() {
        let committed_identity = "test:model:point#committed";
        let rejected_identity = "test:model:point#rejected";
        let mut ir = CadIr::empty();
        let mut session = CommitSession::new(&mut ir);

        assert!(!session.contains(committed_identity));
        session
            .commit_model(point_draft(committed_identity))
            .expect("point commit");
        assert!(session.contains(committed_identity));

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
        assert!(!session.contains(rejected_identity));
        assert!(session.commit_model(rejected).is_err());
        assert!(!session.contains(rejected_identity));
    }
}
