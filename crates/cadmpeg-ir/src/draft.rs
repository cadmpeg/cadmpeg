// SPDX-License-Identifier: Apache-2.0
//! Atomic staging for neutral entity transfer.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::annotations::{Annotations, ExactnessNote};
use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::document::{CadIr, Model};
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
};
use crate::geometry::{Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface};
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
    /// A B-rep assembly does not contain exactly one closed body graph.
    #[error("invalid B-rep assembly: {0}")]
    InvalidBrep(String),
}

/// Transactional collection of staged model entities and decode accounting.
///
/// Plan prose calls this stage `DocumentDraft`. Prefer that name in docs; this
/// type remains the runtime draft that commits into [`CadIr`].
#[derive(Debug, Default)]
pub struct ModelDraft {
    model: Model,
    identity_index: IdentityIndex,
    identities_synced: bool,
    exactness: BTreeMap<String, ExactnessNote>,
    notes: Vec<LossNote>,
    ledger: TransferLedger,
}

/// Plan name for [`ModelDraft`] in the
/// `DocumentDraft → CadIr → ValidationReport` state machine.
pub type DocumentDraft = ModelDraft;

impl ModelDraft {
    /// Creates an empty draft.
    pub fn new() -> Self {
        Self {
            identities_synced: true,
            ..Self::default()
        }
    }

    /// Inserts one entity, rejecting draft-local identity collisions immediately.
    pub fn insert<T: ArenaEntity>(&mut self, entity: T) -> Result<(), DraftError> {
        self.synchronize_identities()?;
        let identity = entity.identity();
        if identity_index_contains(&self.model, &self.identity_index, identity) {
            return Err(DraftError::IdentityCollision(identity.to_owned()));
        }
        let index = T::arena(&self.model).len();
        self.identity_index
            .entry(identity_hash(identity))
            .or_default()
            .push(IdentitySlot {
                kind: T::KIND,
                index,
            });
        T::arena_mut(&mut self.model).push(entity);
        Ok(())
    }

    /// Returns the staged arena for one entity type.
    pub fn arena<T: ArenaEntity>(&self) -> &Vec<T> {
        T::arena(&self.model)
    }

    /// Returns the mutable staged arena for one entity type.
    pub fn arena_mut<T: ArenaEntity>(&mut self) -> &mut Vec<T> {
        self.identities_synced = false;
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
        self.identities_synced = false;
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
            .flat_map(|namespace| namespace.arenas.values().flatten())
        {
            identities.insert(record.id());
        }
        self.validate_with_contains(|identity| identities.contains(identity))
    }

    fn synchronize_identities(&mut self) -> Result<(), DraftError> {
        if self.identities_synced {
            return Ok(());
        }
        self.identity_index = index_model_identities(&self.model)?;
        self.identities_synced = true;
        Ok(())
    }

    /// Validates staged identities and references against one identity universe.
    ///
    /// The caller supplies the identities already committed outside this draft;
    /// references may also target another entity staged in the same draft. The
    /// index populated by `insert` is reused. Direct mutable staging invalidates
    /// that index and causes one complete rebuild before validation.
    fn validate_with_contains(
        &mut self,
        contains: impl Fn(&str) -> bool,
    ) -> Result<(), DraftError> {
        self.synchronize_identities()?;
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
                            && !identity_index_contains(
                                &self.model,
                                &self.identity_index,
                                &reference.target,
                            )
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
        Ok(())
    }

    /// Validates and atomically extends a document, annotations, notes, and ledger.
    pub fn commit(
        mut self,
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
        let identity_index = index_model_identities(&self.model)?;
        self.exactness
            .retain(|identity, _| identity_index_contains(&self.model, &identity_index, identity));
        self.identity_index = identity_index;
        self.identities_synced = true;
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
            identity_index: _,
            identities_synced: _,
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

#[derive(Debug)]
enum CommittedIdentity {
    Neutral(IdentitySlot),
    Native(String),
}

type CommittedIdentityIndex = HashMap<u64, Vec<CommittedIdentity>>;

/// Identity universe for one decode-scoped sequence of draft commits.
///
/// A session is valid only while the associated [`CadIr`] is mutated through
/// this session. Inserting a neutral or native record directly into `base`
/// after construction makes the session stale; topology decoding observes this
/// invariant by routing every topology draft through one session and performing
/// no other record insertion during that phase.
#[derive(Debug)]
pub struct CommitSession {
    identities: CommittedIdentityIndex,
}

impl CommitSession {
    /// Builds an identity index over every neutral and native arena.
    pub fn new(base: &CadIr) -> Self {
        let mut identities = CommittedIdentityIndex::new();
        macro_rules! collect_model_identities {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(for (index, entity) in base.model.$field.iter().enumerate() {
                    Self::insert_neutral(
                        &mut identities,
                        entity.identity(),
                        IdentitySlot {
                            kind: <$ty as EntitySchema>::KIND,
                            index,
                        },
                    );
                })*
            };
        }
        crate::document::arena_registry!(collect_model_identities);
        for record in base
            .native
            .0
            .values()
            .flat_map(|namespace| namespace.arenas.values().flatten())
        {
            identities
                .entry(identity_hash(record.id()))
                .or_default()
                .push(CommittedIdentity::Native(record.id().to_owned()));
        }
        Self { identities }
    }

    fn insert_neutral(identities: &mut CommittedIdentityIndex, identity: &str, slot: IdentitySlot) {
        identities
            .entry(identity_hash(identity))
            .or_default()
            .push(CommittedIdentity::Neutral(slot));
    }

    /// Reports whether `identity` is already owned by `base` or a
    /// prior successful commit in this session.
    ///
    /// Identity ownership is kind-blind: this checks all neutral and native
    /// arenas, not whether a record exists in one particular arena.
    pub fn contains(&self, base: &CadIr, identity: &str) -> bool {
        self.identities
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

    fn register_added(&mut self, model: &Model, checkpoint: &ModelCheckpoint) {
        macro_rules! register_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
                $(if let Some(added) = checkpoint.added::<$ty>(model) {
                    for (offset, entity) in added.iter().enumerate() {
                        Self::insert_neutral(
                            &mut self.identities,
                            entity.identity(),
                            IdentitySlot {
                                kind: <$ty as EntitySchema>::KIND,
                                index: checkpoint.arena_len::<$ty>() + offset,
                            },
                        );
                    }
                })*
            };
        }
        crate::document::arena_registry!(register_arenas);
    }

    /// Validates and commits one model draft into `base`.
    ///
    /// Validation is completed before either destination is changed. On
    /// success, the draft's owned identities are moved into this session. A
    /// rejected draft therefore leaves both the document and the session
    /// unchanged and does not poison a later commit.
    pub fn commit_model(
        &mut self,
        mut draft: ModelDraft,
        base: &mut CadIr,
    ) -> Result<(), DraftError> {
        draft.validate_with_contains(|identity| self.contains(base, identity))?;
        let checkpoint = ModelCheckpoint::capture(&base.model);
        draft.commit_validated(
            base,
            &mut Annotations::default(),
            &mut Vec::new(),
            &mut TransferLedger::default(),
        );
        self.register_added(&base.model, &checkpoint);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CommitSession, DraftError, ModelDraft};
    use crate::annotations::Annotations;
    use crate::document::CadIr;
    use crate::ids::PointId;
    use crate::math::Point3;
    use crate::native::NativeRecord;
    use crate::report::TransferLedger;
    use crate::topology::{Point, Vertex};
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
    fn direct_model_staging_rebuilds_identity_cache_before_commit() {
        let identity = "test:model:point#direct-duplicate";
        let mut draft = ModelDraft::new();
        draft.model_mut().points.push(point(identity));
        draft.model_mut().points.push(point(identity));
        let mut ir = CadIr::empty(Units::default());

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
            id: owner.into(),
            point: target.into(),
            tolerance: None,
        });
        let mut ir = CadIr::empty(Units::default());

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
    fn incomplete_commit_rechecks_duplicate_identities() {
        let identity = "test:model:point#incomplete-duplicate";
        let mut draft = ModelDraft::new();
        draft.model_mut().points.push(point(identity));
        draft.model_mut().points.push(point(identity));
        let mut ir = CadIr::empty(Units::default());

        assert_eq!(
            draft.commit_incomplete(
                &mut ir,
                &mut Annotations::default(),
                &mut Vec::new(),
                &mut TransferLedger::default(),
                |_, _| true,
            ),
            Err(DraftError::IdentityCollision(identity.into()))
        );
        assert!(ir.model.points.is_empty());
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

    #[test]
    fn commit_session_contains_tracks_only_successful_commits() {
        let committed_identity = "test:model:point#committed";
        let rejected_identity = "test:model:point#rejected";
        let mut ir = CadIr::empty(Units::default());
        let mut session = CommitSession::new(&ir);

        assert!(!session.contains(&ir, committed_identity));
        session
            .commit_model(point_draft(committed_identity), &mut ir)
            .expect("point commit");
        assert!(session.contains(&ir, committed_identity));

        let mut rejected = ModelDraft::new();
        rejected
            .insert(Vertex {
                id: rejected_identity.into(),
                point: "test:model:point#missing".into(),
                tolerance: None,
            })
            .expect("insert rejected vertex");
        assert!(!session.contains(&ir, rejected_identity));
        assert!(session.commit_model(rejected, &mut ir).is_err());
        assert!(!session.contains(&ir, rejected_identity));
    }
}
