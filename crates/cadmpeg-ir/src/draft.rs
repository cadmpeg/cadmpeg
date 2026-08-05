// SPDX-License-Identifier: Apache-2.0
//! Atomic staging for neutral entity transfer.

use std::collections::{BTreeMap, HashSet};

use crate::annotations::{Annotations, ExactnessNote};
use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::document::{CadIr, Model};
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
};
use crate::geometry::{Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface};
use crate::index::ModelIndex;
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Occurrence, ProductDefinition};
use crate::provenance::Exactness;
use crate::report::{LossNote, TransferLedger};
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
}

impl ModelCheckpoint {
    /// Captures every neutral arena length.
    pub fn capture(model: &Model) -> Self {
        let mut lengths = Vec::with_capacity(EntityKind::ALL.len());
        macro_rules! capture_lengths {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(lengths.push(model.$field.len());)*
            };
        }
        crate::document::arena_registry!(capture_lengths);
        Self {
            lengths: lengths
                .try_into()
                .expect("arena registry and EntityKind::ALL have equal length"),
        }
    }

    fn length<T: ArenaEntity>(&self) -> usize {
        let index = EntityKind::ALL
            .iter()
            .position(|kind| *kind == T::KIND)
            .expect("arena entity kind is registered");
        self.lengths[index]
    }

    /// Returns the captured length of one typed arena.
    pub fn arena_len<T: ArenaEntity>(&self) -> usize {
        self.length::<T>()
    }

    /// Returns entities of `T` added since this checkpoint.
    pub fn added<'a, T: ArenaEntity>(&self, model: &'a Model) -> Option<&'a [T]> {
        T::arena(model).get(self.length::<T>()..)
    }

    /// Returns mutable entities of `T` added since this checkpoint.
    pub fn added_mut<'a, T: ArenaEntity>(&self, model: &'a mut Model) -> Option<&'a mut [T]> {
        T::arena_mut(model).get_mut(self.length::<T>()..)
    }

    /// Counts all entities added since this checkpoint, rejecting arena shrinkage.
    pub fn added_count(&self, model: &Model) -> Option<usize> {
        let after = Self::capture(model);
        after
            .lengths
            .into_iter()
            .zip(self.lengths)
            .try_fold(0_usize, |total, (after, before)| {
                total.checked_add(after.checked_sub(before)?)
            })
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
    /// A B-rep assembly does not contain exactly one closed body graph.
    #[error("invalid B-rep assembly: {0}")]
    InvalidBrep(String),
}

/// Transactional collection of staged model entities and decode accounting.
#[derive(Debug, Default)]
pub struct ModelDraft {
    model: Model,
    identities: HashSet<String>,
    exactness: BTreeMap<String, ExactnessNote>,
    notes: Vec<LossNote>,
    ledger: TransferLedger,
}

impl ModelDraft {
    /// Creates an empty draft.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts one entity, rejecting draft-local identity collisions immediately.
    pub fn insert<T: ArenaEntity>(&mut self, entity: T) -> Result<(), DraftError> {
        let identity = entity.identity().to_owned();
        if !self.identities.insert(identity.clone()) {
            return Err(DraftError::IdentityCollision(identity));
        }
        T::arena_mut(&mut self.model).push(entity);
        Ok(())
    }

    /// Returns the staged arena for one entity type.
    pub fn arena<T: ArenaEntity>(&self) -> &Vec<T> {
        T::arena(&self.model)
    }

    /// Returns the mutable staged arena for one entity type.
    pub fn arena_mut<T: ArenaEntity>(&mut self) -> &mut Vec<T> {
        T::arena_mut(&mut self.model)
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

    /// Records sparse exactness for a staged entity.
    pub fn exactness(&mut self, identity: impl Into<String>, exactness: Exactness) {
        let identity = identity.into();
        if exactness == Exactness::ByteExact {
            self.exactness.remove(&identity);
        } else {
            self.exactness.insert(
                identity,
                ExactnessNote {
                    entity: exactness,
                    fields: BTreeMap::new(),
                },
            );
        }
    }

    /// Retains exactness notes selected by identity.
    pub fn retain_exactness(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.exactness.retain(|identity, _| keep(identity));
    }

    /// Adds a staged loss note.
    pub fn note(&mut self, note: LossNote) {
        self.notes.push(note);
    }

    /// Returns the mutable staged transfer ledger.
    pub fn ledger_mut(&mut self) -> &mut TransferLedger {
        &mut self.ledger
    }

    /// Number of staged entities.
    pub fn entity_count(&self) -> usize {
        self.model.entity_count()
    }

    /// Commits only staged model entities, discarding empty ancillary staging surfaces.
    pub fn commit_model(self, base: &mut CadIr) -> Result<(), DraftError> {
        self.commit(
            base,
            &mut Annotations::default(),
            &mut Vec::new(),
            &mut TransferLedger::default(),
        )
    }

    fn validate_against(&self, base: &CadIr) -> Result<(), DraftError> {
        let index = ModelIndex::new(base);
        self.validate_with_contains(|identity| index.contains(identity))
            .map(|_| ())
    }

    /// Validates staged identities and references against one identity universe.
    ///
    /// The caller supplies the identities already committed outside this draft;
    /// references may also target another entity staged in the same draft. The
    /// returned set owns the identities collected from every arena so a session
    /// can absorb it after the commit without allocating them again.
    fn validate_with_contains(
        &self,
        contains: impl Fn(&str) -> bool,
    ) -> Result<HashSet<String>, DraftError> {
        let mut staged_identities = HashSet::with_capacity(self.model.entity_count());
        macro_rules! collect_staged_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(for entity in &self.model.$field {
                    let identity = entity.identity().to_owned();
                    if !staged_identities.insert(identity.clone()) {
                        return Err(DraftError::IdentityCollision(identity));
                    }
                })*
            };
        }
        crate::document::arena_registry!(collect_staged_identities);
        for identity in &staged_identities {
            if contains(identity) {
                return Err(DraftError::IdentityCollision(identity.clone()));
            }
        }
        macro_rules! validate_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(for entity in &self.model.$field {
                    let owner = entity.identity();
                    let mut missing = None;
                    entity.visit_references(&mut |reference| {
                        if missing.is_none()
                            && !contains(&reference.target)
                            && !staged_identities.contains(&reference.target)
                        {
                            missing = Some(reference.target);
                        }
                    });
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
        Ok(staged_identities)
    }

    /// Validates and atomically extends a document, annotations, notes, and ledger.
    pub fn commit(
        self,
        base: &mut CadIr,
        annotations: &mut Annotations,
        notes: &mut Vec<LossNote>,
        ledger: &mut TransferLedger,
    ) -> Result<(), DraftError> {
        self.validate_against(base)?;
        self.commit_validated(base, annotations, notes, ledger);
        Ok(())
    }

    /// Keeps selected staged entities, then validates and commits the resulting salvage graph.
    pub fn commit_incomplete(
        mut self,
        base: &mut CadIr,
        annotations: &mut Annotations,
        notes: &mut Vec<LossNote>,
        ledger: &mut TransferLedger,
        mut keep: impl FnMut(EntityKind, &str) -> bool,
    ) -> Result<(), DraftError> {
        macro_rules! retain_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(self.model.$field.retain(|entity| keep(<$ty>::KIND, entity.identity()));)*
            };
        }
        crate::document::arena_registry!(retain_arenas);
        self.identities.clear();
        macro_rules! collect_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(self.identities.extend(
                    self.model.$field.iter().map(|entity| entity.identity().to_owned())
                );)*
            };
        }
        crate::document::arena_registry!(collect_identities);
        self.exactness
            .retain(|identity, _| self.identities.contains(identity));
        self.commit(base, annotations, notes, ledger)
    }

    fn commit_validated(
        self,
        base: &mut CadIr,
        annotations: &mut Annotations,
        notes: &mut Vec<LossNote>,
        ledger: &mut TransferLedger,
    ) {
        let Self {
            mut model,
            identities: _,
            exactness,
            notes: staged_notes,
            ledger: staged_ledger,
        } = self;
        macro_rules! extend_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(base.model.$field.append(&mut model.$field);)*
            };
        }
        crate::document::arena_registry!(extend_arenas);
        annotations.exactness.extend(exactness);
        notes.extend(staged_notes);
        ledger.entries.extend(staged_ledger.entries);
    }
}

/// Owned identity universe for one decode-scoped sequence of draft commits.
///
/// A session is valid only while the associated [`CadIr`] is mutated through
/// this session. Inserting a neutral or native record directly into `base`
/// after construction makes the session stale; topology decoding observes this
/// invariant by routing every topology draft through one session and performing
/// no other record insertion during that phase.
#[derive(Debug)]
pub struct CommitSession {
    identities: HashSet<String>,
}

impl CommitSession {
    /// Builds an owned identity universe from every neutral and native arena.
    pub fn new(base: &CadIr) -> Self {
        let mut identities = HashSet::with_capacity(base.model.entity_count());
        macro_rules! collect_model_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(identities.extend(
                    base.model.$field.iter().map(|entity| entity.identity().to_owned())
                );)*
            };
        }
        crate::document::arena_registry!(collect_model_identities);
        identities.extend(
            base.native
                .0
                .values()
                .flat_map(|namespace| namespace.arenas.values().flatten())
                .map(|record| record.id().to_owned()),
        );
        Self { identities }
    }

    /// Validates and commits one model draft into `base`.
    ///
    /// Validation is completed before either destination is changed. On
    /// success, the draft's owned identities are moved into this session. A
    /// rejected draft therefore leaves both the document and the session
    /// unchanged and does not poison a later commit.
    pub fn commit_model(&mut self, draft: ModelDraft, base: &mut CadIr) -> Result<(), DraftError> {
        let staged_identities =
            draft.validate_with_contains(|identity| self.identities.contains(identity))?;
        draft.commit_validated(
            base,
            &mut Annotations::default(),
            &mut Vec::new(),
            &mut TransferLedger::default(),
        );
        self.identities.extend(staged_identities);
        Ok(())
    }
}

/// A staged graph containing exactly one body and all of its typed dependencies.
#[derive(Debug)]
pub struct BrepAssembly(ModelDraft);

impl BrepAssembly {
    /// Checks that `draft` is one internally closed body graph.
    pub fn new(draft: ModelDraft) -> Result<Self, DraftError> {
        if draft.model.bodies.len() != 1 {
            return Err(DraftError::InvalidBrep(format!(
                "expected one body, found {}",
                draft.model.bodies.len()
            )));
        }
        let mut identities = HashSet::with_capacity(draft.model.entity_count());
        macro_rules! collect_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(identities.extend(
                    draft.model.$field.iter().map(|entity| entity.identity().to_owned())
                );)*
            };
        }
        crate::document::arena_registry!(collect_identities);
        macro_rules! check_closure {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(for entity in &draft.model.$field {
                    let mut missing = None;
                    entity.visit_references(&mut |reference| {
                        if missing.is_none() && !identities.contains(&reference.target) {
                            missing = Some(reference.target);
                        }
                    });
                    if let Some(target) = missing {
                        return Err(DraftError::InvalidBrep(format!(
                            "entity {} references external identity {target}", entity.identity()
                        )));
                    }
                })*
            };
        }
        crate::document::arena_registry!(check_closure);
        Ok(Self(draft))
    }

    /// Returns the validated draft for atomic commit.
    pub fn into_draft(self) -> ModelDraft {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{BrepAssembly, CommitSession, DraftError, ModelDraft};
    use crate::annotations::Annotations;
    use crate::document::CadIr;
    use crate::ids::{BodyId, PointId, RegionId};
    use crate::math::Point3;
    use crate::native::NativeRecord;
    use crate::report::{TransferDisposition, TransferLedger};
    use crate::topology::{Body, BodyKind, Point, Region, Vertex};
    use crate::units::Units;

    fn point(id: &str) -> Point {
        Point {
            id: PointId(id.into()),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        }
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
                id: id.into(),
                point: point.into(),
                tolerance: None,
            })
            .expect("insert vertex into draft");
        draft
    }

    #[test]
    fn collision_refuses_without_mutating_any_destination() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.push(point("test:model:point#1"));
        let mut draft = ModelDraft::new();
        draft
            .insert(point("test:model:point#1"))
            .expect("insert point into empty draft");
        let mut annotations = Annotations::default();
        let mut notes = Vec::new();
        let mut ledger = TransferLedger::default();

        assert!(matches!(
            draft.commit(&mut ir, &mut annotations, &mut notes, &mut ledger),
            Err(DraftError::IdentityCollision(_))
        ));
        assert_eq!(ir.model.points.len(), 1);
        assert!(annotations.exactness.is_empty());
        assert!(notes.is_empty());
        assert!(ledger.is_empty());
    }

    #[test]
    fn brep_assembly_requires_one_closed_body_and_commits_without_cloning() {
        let body_id = BodyId("test:model:body#1".into());
        let mut draft = ModelDraft::new();
        draft
            .insert(Body {
                id: body_id.clone(),
                kind: BodyKind::Wire,
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            })
            .expect("insert body into empty draft");
        draft.ledger_mut().record(
            "source-body",
            Some(body_id.0.clone()),
            TransferDisposition::Emitted,
            None,
        );
        let assembly = BrepAssembly::new(draft).expect("single body draft is closed");
        let mut ir = CadIr::empty(Units::default());
        let mut annotations = Annotations::default();
        let mut notes = Vec::new();
        let mut ledger = TransferLedger::default();
        assembly
            .into_draft()
            .commit(&mut ir, &mut annotations, &mut notes, &mut ledger)
            .expect("commit closed body draft");

        assert_eq!(ir.model.bodies.len(), 1);
        ledger
            .verify(&crate::index::ModelIndex::new(&ir))
            .expect("committed ledger targets resolve");
    }

    #[test]
    fn brep_assembly_checks_entities_added_through_the_model_surface() {
        let body_id = BodyId("test:model:body#direct".into());
        let region_id = RegionId("test:model:region#direct".into());
        let mut draft = ModelDraft::new();
        draft.model_mut().bodies.push(Body {
            id: body_id.clone(),
            kind: BodyKind::Wire,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        draft.model_mut().regions.push(Region {
            id: region_id,
            body: body_id,
            shells: Vec::new(),
        });

        BrepAssembly::new(draft).expect("directly staged closed body");
    }

    #[test]
    fn commit_session_matches_sequential_model_commits() {
        let mut session_ir = CadIr::empty(Units::default());
        let mut session = CommitSession::new(&session_ir);
        session
            .commit_model(point_draft("test:model:point#1"), &mut session_ir)
            .expect("first session commit");
        session
            .commit_model(point_draft("test:model:point#2"), &mut session_ir)
            .expect("second session commit");

        let mut sequential_ir = CadIr::empty(Units::default());
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
        let mut ir = CadIr::empty(Units::default());
        let mut session = CommitSession::new(&ir);
        let identity = "test:model:point#cross-draft";
        session
            .commit_model(point_draft(identity), &mut ir)
            .expect("first session commit");

        assert_eq!(
            session.commit_model(point_draft(identity), &mut ir),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn commit_session_rejects_pre_existing_neutral_identity() {
        let identity = "test:model:point#existing";
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.push(point(identity));
        let mut session = CommitSession::new(&ir);

        assert_eq!(
            session.commit_model(point_draft(identity), &mut ir),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        assert_eq!(ir.model.points.len(), 1);
    }

    #[test]
    fn commit_session_rejects_native_identity() {
        let identity = "test:native:record#1";
        let mut ir = CadIr::empty(Units::default());
        ir.native.namespace_mut("test").arenas.insert(
            "records".into(),
            vec![NativeRecord::new(identity, serde_json::Map::new())],
        );
        let mut session = CommitSession::new(&ir);

        assert_eq!(
            session.commit_model(point_draft(identity), &mut ir),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        assert!(ir.model.points.is_empty());
    }

    #[test]
    fn commit_session_rejects_unresolved_reference() {
        let owner = "test:model:vertex#missing";
        let target = "test:model:point#missing";
        let mut ir = CadIr::empty(Units::default());
        let mut session = CommitSession::new(&ir);

        assert_eq!(
            session.commit_model(vertex_draft(owner, target), &mut ir),
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
        let mut ir = CadIr::empty(Units::default());
        let mut session = CommitSession::new(&ir);
        session
            .commit_model(point_draft(point_id), &mut ir)
            .expect("point commit");
        session
            .commit_model(vertex_draft("test:model:vertex#later", point_id), &mut ir)
            .expect("reference into committed draft resolves");

        assert_eq!(ir.model.points.len(), 1);
        assert_eq!(ir.model.vertices.len(), 1);
    }

    #[test]
    fn rejected_session_commit_leaves_session_and_base_usable() {
        let rejected_identity = "test:model:vertex#rejected";
        let mut ir = CadIr::empty(Units::default());
        let before = ir.clone();
        let mut session = CommitSession::new(&ir);

        assert!(session
            .commit_model(
                vertex_draft(rejected_identity, "test:model:point#never-committed"),
                &mut ir,
            )
            .is_err());
        assert_eq!(ir, before);

        session
            .commit_model(point_draft(rejected_identity), &mut ir)
            .expect("rejected identity was not absorbed into the session");
        assert_eq!(ir.model.points.len(), 1);
    }
}
