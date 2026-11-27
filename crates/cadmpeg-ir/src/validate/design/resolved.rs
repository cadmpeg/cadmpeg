// SPDX-License-Identifier: Apache-2.0
//! Resolved states (§5.4, §5.5): an evaluation with content is its chain of
//! bases with each delta applied, down to `Empty` or to the extent of a
//! current state. Every check of a state's names, owners, members, instance
//! bodies, poses, and closure runs against the resolved state.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::records::{DesignRecords, MemberKey, OfKey, PositionKey};
use super::selections::check_path;
use crate::design::evaluation::{Evaluation, EvaluationStatus};
use crate::design::feature::FeatureOperation;
use crate::design::names::{NameEntry, NameKind};
use crate::design::refs::{EntityRef, RecordRef};
use crate::document::Topology;
use crate::features::ConfigurationId;
use crate::ids::BodyId;
use crate::index::identities::IdentitySet;
use crate::index::ModelIndex;
use crate::process::state::ProcessState;
use crate::report::check::{Check, Finding};
use crate::schema::EntitySchema;
use crate::topology::ChildLists;
use state::{Closure, State};
use state_topology::StateRules;
use walk::{Bottom, Forest, Stored};

macro_rules! topology_arenas {
    ($target:ident) => {
        $target! {
            bodies: Body,
            regions: Region,
            shells: Shell,
            faces: Face,
            loops: Loop,
            coedges: Coedge,
            edges: Edge,
            vertices: Vertex,
            points: Point,
            surfaces: Surface,
            curves: Curve,
            subds: Subd,
            pcurves: Pcurve,
            procedural_surfaces: ProceduralSurface,
            procedural_curves: ProceduralCurve,
        }
    };
}

macro_rules! declare_arenas {
    ($($field:ident: $arena:ident,)*) => {
        /// A `Topology` arena.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(super) enum Arena {
            $($arena,)*
        }

        impl Arena {
            const fn label(self) -> &'static str {
                match self {
                    $(Self::$arena => stringify!($field),)*
                }
            }

            pub(super) fn of_record(record: &RecordRef) -> (Self, &str) {
                match record {
                    $(RecordRef::$arena(id) => (Self::$arena, id.as_str()),)*
                }
            }
        }

        /// Each record of `topology`: its arena, identity, and index.
        pub(super) fn records(topology: &Topology) -> Vec<(Arena, &str, usize)> {
            let mut records = Vec::new();
            $(for (at, record) in topology.$field.iter().enumerate() {
                records.push((Arena::$arena, EntitySchema::identity(record), at));
            })*
            records
        }

        impl Located<'_> {
            /// Appends a copy of the record to its arena of `into`.
            fn copy_into(self, into: &mut Topology) {
                match self.arena {
                    $(Arena::$arena => into
                        .$field
                        .extend(self.topology.$field.get(self.index).cloned()),)*
                }
            }

            fn visit_references(
                self,
                ctx: &DecodeContext<'_>,
                visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
            ) -> Result<(), CodecError> {
                match self.arena {
                    $(Arena::$arena => match self.topology.$field.get(self.index) {
                        Some(record) => record.visit_references(ctx, visitor),
                        None => Ok(()),
                    },)*
                }
            }
        }
    };
}
topology_arenas!(declare_arenas);

impl Arena {
    pub(super) fn of_entity(entity: &EntityRef) -> (Self, &str) {
        match entity {
            EntityRef::Body(id) => (Self::Body, id.as_str()),
            EntityRef::Face(id) => (Self::Face, id.as_str()),
            EntityRef::Edge(id) => (Self::Edge, id.as_str()),
            EntityRef::Vertex(id) => (Self::Vertex, id.as_str()),
            EntityRef::Surface(id) => (Self::Surface, id.as_str()),
            EntityRef::Curve(id) => (Self::Curve, id.as_str()),
            EntityRef::Subd(id) => (Self::Subd, id.as_str()),
        }
    }

    /// Whether the arena holds nameable entities: an `EntityRef` names its
    /// records.
    pub(super) const fn is_entity(self) -> bool {
        matches!(
            self,
            Self::Body
                | Self::Face
                | Self::Edge
                | Self::Vertex
                | Self::Surface
                | Self::Curve
                | Self::Subd
        )
    }

    /// The name kind of an entity of this arena; a subdivision cage and a
    /// record no `EntityRef` names have none.
    pub(super) const fn name_kind(self) -> Option<NameKind> {
        match self {
            Self::Body => Some(NameKind::Body),
            Self::Face => Some(NameKind::Face),
            Self::Edge => Some(NameKind::Edge),
            Self::Vertex => Some(NameKind::Vertex),
            Self::Surface => Some(NameKind::Surface),
            Self::Curve => Some(NameKind::Curve),
            _ => None,
        }
    }
}

impl fmt::Display for Arena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A topology record of a state: the `Topology` that holds it and its slot.
#[derive(Clone, Copy)]
struct Located<'a> {
    arena: Arena,
    topology: &'a Topology,
    index: usize,
}

/// The records that a current state's bodies reach, and their name entries.
struct Extent<'a> {
    records: BTreeMap<&'a str, Located<'a>>,
    names: BTreeMap<&'a EntityRef, &'a NameEntry>,
}

/// Each record of `topology`, by identity, keeping the first of a repeated one.
fn locate(topology: &Topology) -> BTreeMap<&str, Located<'_>> {
    let mut located = BTreeMap::new();
    for (arena, id, index) in records(topology) {
        located.entry(id).or_insert(Located {
            arena,
            topology,
            index,
        });
    }
    located
}

struct Resolver<'a, 'c, 'd> {
    ctx: &'c DecodeContext<'d>,
    index: &'c DesignRecords<'a>,
    geometry: BTreeMap<&'a str, Located<'a>>,
    children: ChildLists<'a>,
}

impl<'a> Resolver<'a, '_, '_> {
    /// The children of a current-geometry record that only their parent
    /// pointers list (D29): a body's regions and a shell's faces.
    fn derived_children(&self, located: Located<'a>) -> Vec<&'a str> {
        match located.arena {
            Arena::Body => located
                .topology
                .bodies
                .get(located.index)
                .map(|body| self.children.stated_body_regions(&body.id))
                .unwrap_or_default()
                .iter()
                .map(|region| region.id.as_str())
                .collect(),
            Arena::Shell => located
                .topology
                .shells
                .get(located.index)
                .map(|shell| self.children.stated_shell_faces(&shell.id))
                .unwrap_or_default()
                .iter()
                .map(|face| face.id.as_str())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The state a chain of bases ends on: nothing, or the extent of a
    /// scope's current state at a position, its bodies and the records of the
    /// current geometry they reach, through references and through the
    /// derived child lists, their name entries, and its poses. `None` where
    /// the scope has no current state there.
    fn bottom(
        &self,
        closure: &Closure<'_, 'a, '_, '_>,
        bottom: Bottom<'a>,
    ) -> Result<Option<State<'a>>, CodecError> {
        let Bottom::Current(scope, position) = bottom else {
            return Ok(Some(State::default()));
        };
        let Some(current) = self.index.scopes.get(scope).and_then(|scope| {
            scope
                .current
                .iter()
                .find(|current| PositionKey::of(&current.at) == position)
        }) else {
            return Ok(None);
        };
        let extent = self.geometry_extent(&current.bodies)?;
        State::bottom(
            closure,
            extent.records,
            extent.names,
            current.poses.iter().collect(),
        )
        .map(Some)
    }

    /// The topology records that `bodies` reach through the derived child
    /// lists and references, with their name entries.
    fn geometry_extent(
        &self,
        bodies: impl IntoIterator<Item = &'a BodyId>,
    ) -> Result<Extent<'a>, CodecError> {
        let mut records: BTreeMap<&'a str, Located<'a>> = BTreeMap::new();
        let mut pending: Vec<&'a str> = bodies
            .into_iter()
            .filter_map(|body| {
                self.geometry
                    .get_key_value(body.as_str())
                    .map(|(id, _)| *id)
            })
            .collect();
        while let Some(id) = pending.pop() {
            let Some(located) = self.geometry.get(id).copied() else {
                continue;
            };
            if records.insert(id, located).is_some() {
                continue;
            }
            pending.extend(self.derived_children(located));
            located.visit_references(self.ctx, &mut |target| {
                if let Some((target, _)) = self.geometry.get_key_value(target) {
                    if !records.contains_key(target) {
                        pending.push(target);
                    }
                }
                Ok(())
            })?;
        }
        let mut names = BTreeMap::new();
        for (entity, entry) in self.index.ir.design.names.entries() {
            let (arena, id) = Arena::of_entity(entity);
            if records
                .get(id)
                .is_some_and(|located| located.arena == arena)
            {
                names.insert(entity, entry);
            }
        }
        Ok(Extent { records, names })
    }
}

/// Whether a reference to `target` from a state leaves it: the state does
/// not hold `target` (`held` false), and `target` is a record of a
/// `Topology` of the document or no record of it.
pub(crate) fn leaves_state(
    held: bool,
    target: &str,
    topology: &BTreeSet<&str>,
    ids: &ModelIndex<'_>,
) -> bool {
    !held && (topology.contains(target) || !IdentitySet::contains(ids, target))
}

/// The ids of every `Topology` of the document: the current geometry's and
/// those of the deltas of `deltas`.
fn topology_ids<'a>(
    geometry: &BTreeMap<&'a str, Located<'a>>,
    deltas: impl Iterator<Item = &'a crate::design::evaluation::StateDelta>,
) -> BTreeSet<&'a str> {
    let mut topology: BTreeSet<&str> = geometry.keys().copied().collect();
    for delta in deltas {
        topology.extend(records(&delta.records).into_iter().map(|(_, id, _)| id));
    }
    topology
}

pub(super) fn check_resolved_states(
    ctx: &DecodeContext<'_>,
    index: &DesignRecords<'_>,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let ir = index.ir;
    let has_names = !ir.design.names.entries().is_empty();
    let has_evaluations = ir
        .design
        .evaluations
        .iter()
        .any(|evaluation| evaluation.content.is_some());
    if !has_names && !has_evaluations {
        return Ok(());
    }
    let geometry = locate(&ir.geometry);
    if has_names {
        check_current_names(ir, &geometry, findings);
    }
    let unproduced: BTreeSet<&BodyId> = ir
        .design
        .scopes
        .iter()
        .flat_map(|scope| &scope.unproduced)
        .collect();
    if !has_evaluations && unproduced.is_empty() {
        return Ok(());
    }
    let resolver = Resolver {
        ctx,
        index,
        geometry,
        children: ir.geometry.child_lists(),
    };
    if has_names && !unproduced.is_empty() {
        let extent = resolver.geometry_extent(unproduced.iter().copied())?;
        for (entity, entry) in &extent.names {
            if entry.name.is_some() {
                super::error(
                    findings,
                    entity.id(),
                    crate::rule!(
                        "design.entity-unproduced-body-must-no-structured",
                        "an entity of an unproduced body must have no structured name"
                    ),
                );
            }
        }
    }
    if !has_evaluations {
        return Ok(());
    }
    let evaluations = &ir.design.evaluations;
    let topology = topology_ids(
        &resolver.geometry,
        evaluations
            .iter()
            .filter_map(|evaluation| evaluation.content.as_ref().map(|content| &content.delta)),
    );
    let closure = Closure {
        ctx,
        topology: &topology,
        ids,
    };
    let moved = index.moved_occurrences();
    let rules = StateRules::new(ctx, ids.native_view());
    let forest = Forest::of_evaluations(evaluations);
    let mut found: Vec<Vec<Finding>> = vec![Vec::new(); evaluations.len()];
    forest.walk(
        ctx,
        |bottom| resolver.bottom(&closure, bottom),
        |at, record, state| {
            let (Stored::Evaluation(evaluation), Some(delta)) = (record, record.delta()) else {
                return Ok(());
            };
            let id = evaluation.id.as_str();
            let findings = &mut found[at];
            let applied = state.apply(&closure, delta, Some((id, &mut *findings)))?;
            for body in &unproduced {
                if state.holds(Arena::Body, body.as_str()) {
                    super::error(
                        findings,
                        id,
                        crate::rule!(
                            "design.evaluation-states-unproduced-body",
                            "evaluation states unproduced body {}",
                            body.as_str()
                        ),
                    );
                }
            }
            let root = Some(evaluation.scope.as_str());
            check_entries(index, state, root, moved, id, findings);
            check_closure(state, id, findings);
            let caches = evaluation.status == EvaluationStatus::Current;
            rules.check(state, &applied, id, caches, findings)?;
            check_stored_bodies(index, evaluation, state, findings);
            Ok(())
        },
    )?;
    findings.extend(found.into_iter().flatten());
    Ok(())
}

/// Every name entry of the current geometry names an entity of it.
fn check_current_names(
    ir: &crate::document::CadIr,
    geometry: &BTreeMap<&str, Located<'_>>,
    findings: &mut Vec<Finding>,
) {
    for entity in ir.design.names.entries().keys() {
        let (arena, id) = Arena::of_entity(entity);
        let held = geometry
            .get(id)
            .is_some_and(|located| located.arena == arena);
        if !held {
            super::error(
                findings,
                id,
                crate::rule!(
                    "design.name-entry-names-no-record-current",
                    "name entry names {id}, which is no record of the current geometry's {arena}"
                ),
            );
        }
    }
}

/// The resolved state's name, owner, member, instance-body, and pose entries
/// name what the state holds; instance-body and pose paths resolve from
/// `root`, the definition of the state's scope.
fn check_entries(
    index: &DesignRecords<'_>,
    state: &State<'_>,
    root: Option<&str>,
    moved: &BTreeSet<&str>,
    evaluation: &str,
    findings: &mut Vec<Finding>,
) {
    for entity in &state.unheld {
        let (arena, id) = Arena::of_entity(entity);
        super::error(
            findings,
            evaluation,
            crate::rule!(
                "design.name-entry-names-no-record-resolved",
                "name entry names {id}, which is no record of the resolved state's {arena}"
            ),
        );
    }
    for (body, owner) in &state.owners {
        if !state.holds(Arena::Body, body.as_str()) {
            super::error(
                findings,
                evaluation,
                crate::rule!(
                    "design.owner-entry-names-no-body-resolved",
                    "owner entry names {}, which is no body of the resolved state",
                    body.as_str()
                ),
            );
        }
        if !index.definitions.contains_key(owner.as_str()) {
            super::error(
                findings,
                evaluation,
                crate::rule!(
                    "design.owner-entry-names-no-product-definition",
                    "owner entry names {}, which is no product definition",
                    owner.as_str()
                ),
            );
        }
    }
    for (occurrence, owner) in &state.members {
        if !index.occurrences.contains_key(occurrence) {
            super::error(
                findings,
                evaluation,
                crate::rule!(
                    "design.member-entry-names-no-occurrence-model",
                    "member entry names {occurrence}, which is no occurrence of the model"
                ),
            );
        }
        if !index.definitions.contains_key(owner.as_str()) {
            super::error(
                findings,
                evaluation,
                crate::rule!(
                    "design.member-entry-names-no-product-definition",
                    "member entry names {}, which is no product definition",
                    owner.as_str()
                ),
            );
        }
    }
    for (path, bodies) in &state.instance_bodies {
        check_path(index, root, path, moved, evaluation, findings);
        for body in *bodies {
            if !state.holds(Arena::Body, body.as_str()) {
                super::error(
                    findings,
                    evaluation,
                    crate::rule!(
                        "design.instance-body-entry-lists-no-body",
                        "instance-body entry lists {}, which is no body of the resolved state",
                        body.as_str()
                    ),
                );
            }
        }
    }
    for path in state.poses.keys() {
        check_path(index, root, path, moved, evaluation, findings);
    }
}

/// Each record of the resolved state references only topology records of the
/// state: a reference to an id that the state does not hold is reported
/// unless the id is a record of the document outside every `Topology`, which
/// the reference checks resolve.
fn check_closure(state: &State<'_>, evaluation: &str, findings: &mut Vec<Finding>) {
    for (id, target) in &state.open {
        super::error(
            findings,
            evaluation,
            crate::rule!("design.record-resolved-state-references-state-does", "record {id} of the resolved state references {target}, which the state does not hold"),
        );
    }
}

/// Where the state after a stored-geometry feature is stored, it holds each
/// body that the feature's definition in the evaluation's configuration
/// names (§5.10).
fn check_stored_bodies(
    index: &DesignRecords<'_>,
    evaluation: &Evaluation,
    state: &State<'_>,
    findings: &mut Vec<Finding>,
) {
    let OfKey::State(PositionKey::After(MemberKey::Feature(feature))) = OfKey::of(&evaluation.of)
    else {
        return;
    };
    let Some(feature) = index.features.get(feature) else {
        return;
    };
    let configuration = evaluation
        .configuration
        .as_ref()
        .map(ConfigurationId::as_str);
    let FeatureOperation::Stored {
        bodies: Some(bodies),
        ..
    } = &index.definition_in(feature, configuration, None).operation
    else {
        return;
    };
    for body in bodies {
        if !state.holds(Arena::Body, body.as_str()) {
            super::error(
                findings,
                evaluation.id.as_str(),
                crate::rule!(
                    "design.stored-feature-names-no-body-state",
                    "stored feature {} names {}, which is no body of the state after it",
                    feature.id.as_str(),
                    body.as_str()
                ),
            );
        }
    }
}

/// Each process state with content resolves like an evaluation (§7.9): its
/// chain of process states, down to a design base, with each delta applied.
/// `scope` gives the design scope of a state's setup, which a `Current` base
/// reads. Every resolved state's entries name what it holds, and it is
/// closed. Findings are reported under [`Check::Process`].
pub(in crate::validate) fn check_process_states<'a>(
    ctx: &DecodeContext<'_>,
    index: &DesignRecords<'a>,
    ids: &ModelIndex<'_>,
    scope: impl Fn(&ProcessState) -> Option<&'a str>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let ir = index.ir;
    let states = &ir.process.states;
    if states.iter().all(|state| state.content.is_none()) {
        return Ok(());
    }
    let resolver = Resolver {
        ctx,
        index,
        geometry: locate(&ir.geometry),
        children: ir.geometry.child_lists(),
    };
    let topology = topology_ids(
        &resolver.geometry,
        ir.design
            .evaluations
            .iter()
            .filter_map(|evaluation| evaluation.content.as_ref().map(|content| &content.delta))
            .chain(
                states
                    .iter()
                    .filter_map(|state| state.content.as_ref().map(|content| &content.delta)),
            ),
    );
    let closure = Closure {
        ctx,
        topology: &topology,
        ids,
    };
    let moved = index.moved_occurrences();
    let rules = StateRules::new(ctx, ids.native_view());
    let forest = Forest::of_process_states(&ir.design.evaluations, states, &scope);
    let mut found: Vec<Vec<Finding>> = vec![Vec::new(); states.len()];
    let offset = ir.design.evaluations.len();
    forest.walk(
        ctx,
        |bottom| resolver.bottom(&closure, bottom),
        |at, record, state| {
            let Some(delta) = record.delta() else {
                return Ok(());
            };
            let Stored::Process(process) = record else {
                state.apply(&closure, delta, None)?;
                return Ok(());
            };
            let id = process.id.as_str();
            let findings = &mut found[at - offset];
            let applied = state.apply(&closure, delta, Some((id, &mut *findings)))?;
            check_entries(index, state, scope(process), moved, id, findings);
            check_closure(state, id, findings);
            let caches = process.status == EvaluationStatus::Current;
            rules.check(state, &applied, id, caches, findings)?;
            Ok(())
        },
    )?;
    findings.extend(found.into_iter().flatten().map(|finding| Finding {
        check: Check::Process,
        ..finding
    }));
    Ok(())
}

mod state;
mod state_topology;
#[cfg(test)]
mod tests;
pub(crate) mod visit;
mod walk;
