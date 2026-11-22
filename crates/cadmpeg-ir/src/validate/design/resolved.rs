// SPDX-License-Identifier: Apache-2.0
//! Resolved states (§5.4, §5.5): an evaluation with content is its chain of
//! bases with each delta applied, down to `Empty` or to the extent of a
//! current state. Every check of a state's names, owners, members, instance
//! bodies, poses, and closure runs against the resolved state.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::evaluations::check_path_steps;
use super::records::{DesignRecords, MemberKey, OfKey, PositionKey};
use crate::design::evaluation::{Evaluation, StateBase, StateDelta};
use crate::design::feature::FeatureOperation;
use crate::design::names::{NameEntry, NameKind};
use crate::design::refs::{EntityRef, RecordRef};
use crate::design::selection::OccurrencePath;
use crate::document::Topology;
use crate::features::ConfigurationId;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::index::identities::IdentitySet;
use crate::index::ModelIndex;
use crate::process::state::{ProcessBase, ProcessState};
use crate::report::check::{Check, Finding};
use crate::schema::EntitySchema;
use crate::topology::ChildLists;
use crate::transform::Transform;

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

/// One state with every delta of its chain applied.
#[derive(Default, Clone)]
struct ResolvedState<'a> {
    records: BTreeMap<&'a str, Located<'a>>,
    names: BTreeMap<&'a EntityRef, &'a NameEntry>,
    owners: BTreeMap<&'a BodyId, &'a ProductDefinitionId>,
    members: BTreeMap<&'a str, &'a ProductDefinitionId>,
    instance_bodies: BTreeMap<&'a OccurrencePath, &'a [BodyId]>,
    poses: BTreeMap<&'a OccurrencePath, &'a Transform>,
}

impl ResolvedState<'_> {
    fn holds(&self, arena: Arena, id: &str) -> bool {
        self.records
            .get(id)
            .is_some_and(|located| located.arena == arena)
    }
}

/// A resolved state that the records resting on it share; `None` where the
/// chain of bases does not resolve.
type Shared<'a> = Option<Rc<ResolvedState<'a>>>;

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
                .map(|body| self.children.body_regions(&body.id))
                .unwrap_or_default()
                .iter()
                .map(|region| region.id.as_str())
                .collect(),
            Arena::Shell => located
                .topology
                .shells
                .get(located.index)
                .map(|shell| self.children.shell_faces(&shell.id))
                .unwrap_or_default()
                .iter()
                .map(|face| face.id.as_str())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The extent of a scope's current state at `position`: its bodies and the
    /// records of the current geometry they reach, through references and
    /// through the derived child lists, their name entries, and its poses.
    /// `None` where the scope has no current state there.
    fn extent(
        &self,
        scope: &str,
        position: PositionKey<'_>,
    ) -> Result<Option<ResolvedState<'a>>, CodecError> {
        let Some(current) = self.index.scopes.get(scope).and_then(|scope| {
            scope
                .current
                .iter()
                .find(|current| PositionKey::of(&current.at) == position)
        }) else {
            return Ok(None);
        };
        let mut state = ResolvedState::default();
        let mut pending: Vec<&'a str> = current
            .bodies
            .iter()
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
            if state.records.insert(id, located).is_some() {
                continue;
            }
            pending.extend(self.derived_children(located));
            located.visit_references(self.ctx, &mut |target| {
                if let Some((target, _)) = self.geometry.get_key_value(target) {
                    if !state.records.contains_key(target) {
                        pending.push(target);
                    }
                }
                Ok(())
            })?;
        }
        for (entity, entry) in self.index.ir.design.names.entries() {
            let (arena, id) = Arena::of_entity(entity);
            if state.holds(arena, id) {
                state.names.insert(entity, entry);
            }
        }
        state.poses = current.poses.iter().collect();
        Ok(Some(state))
    }

    /// The state an evaluation's `Empty` or `Current` base names. `None`
    /// where a `Current` base names no current state.
    fn below_evaluation(&self, evaluation: &'a Evaluation) -> Result<Shared<'a>, CodecError> {
        let Some(content) = &evaluation.content else {
            return Ok(None);
        };
        match &content.base {
            StateBase::Empty => Ok(Some(Rc::default())),
            StateBase::Current(position) => Ok(self
                .extent(evaluation.scope.as_str(), PositionKey::of(position))?
                .map(Rc::new)),
            StateBase::Evaluation(_) => Ok(None),
        }
    }
}

/// A record whose content is a delta on a base: an evaluation or a process
/// state.
trait Stored {
    fn id(&self) -> &str;
    /// The delta of the record's content; `None` without content.
    fn delta(&self) -> Option<&StateDelta>;
    /// The id of the record of the same arena that the content's base names.
    fn base(&self) -> Option<&str>;
}

impl Stored for Evaluation {
    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn delta(&self) -> Option<&StateDelta> {
        self.content.as_ref().map(|content| &content.delta)
    }

    fn base(&self) -> Option<&str> {
        match &self.content.as_ref()?.base {
            StateBase::Evaluation(base) => Some(base.as_str()),
            StateBase::Empty | StateBase::Current(_) => None,
        }
    }
}

impl Stored for ProcessState {
    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn delta(&self) -> Option<&StateDelta> {
        self.content.as_ref().map(|content| &content.delta)
    }

    fn base(&self) -> Option<&str> {
        match &self.content.as_ref()?.base {
            ProcessBase::State(base) => Some(base.as_str()),
            ProcessBase::Design(_) => None,
        }
    }
}

/// The resolved states of the records of one arena. Each record's state is
/// resolved once, from the resolved state of its base, and is kept while a
/// record that names it as base is still to be checked.
struct Chain<'a, R> {
    /// Each record by id; the first of a repeated id.
    by_id: BTreeMap<&'a str, &'a R>,
    /// For each id, the number of records still to be checked whose base
    /// names it, while that number is above zero.
    pending: BTreeMap<&'a str, usize>,
    /// The kept states; `None` where the chain of bases does not resolve.
    states: BTreeMap<&'a str, Shared<'a>>,
}

impl<'a, R: Stored> Chain<'a, R> {
    /// The chains of `records`. `bases` holds the id that the base of each
    /// record still to be checked names.
    fn new(records: &'a [R], bases: impl IntoIterator<Item = &'a str>) -> Self {
        let mut by_id = BTreeMap::new();
        for record in records {
            by_id.entry(record.id()).or_insert(record);
        }
        let mut pending: BTreeMap<&'a str, usize> = BTreeMap::new();
        for base in bases {
            *pending.entry(base).or_default() += 1;
        }
        Self {
            by_id,
            pending,
            states: BTreeMap::new(),
        }
    }

    fn is_first(&self, record: &R) -> bool {
        self.by_id
            .get(record.id())
            .is_some_and(|first| std::ptr::eq(*first, record))
    }

    /// The resolved state of `record`'s base, which `below` gives where the
    /// base lies outside the arena. `None` where the chain of bases does not
    /// resolve (an absent base, a base with no content, a cycle, a chain
    /// through the record's own id, or a base `below` does not resolve),
    /// which the evaluation rules report. Releases the base.
    fn base_state(
        &mut self,
        record: &'a R,
        mut below: impl FnMut(&'a R) -> Result<Shared<'a>, CodecError>,
    ) -> Result<Shared<'a>, CodecError> {
        let Some(base) = record.base() else {
            return below(record);
        };
        let through_own_id = !self.is_first(record) && self.reaches(base, record.id());
        let state = if through_own_id {
            None
        } else {
            self.state(base, &mut below)?
        };
        self.release(base);
        Ok(state)
    }

    /// Records that a record whose base names `id` is checked, and drops the
    /// kept state of `id` when no record still to be checked names it.
    fn release(&mut self, id: &str) {
        let Some(count) = self.pending.get_mut(id) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            self.pending.remove(id);
            self.states.remove(id);
        }
    }

    /// Whether the chain of bases from the record `from` reaches `id`.
    fn reaches(&self, from: &str, id: &str) -> bool {
        let mut at = from;
        for _ in 0..=self.by_id.len() {
            if at == id {
                return true;
            }
            let Some(base) = self.by_id.get(at).and_then(|&record| record.base()) else {
                return false;
            };
            at = base;
        }
        false
    }

    /// The resolved state of the record `id`: down its chain of bases to a
    /// kept state or to the state `below` gives, then each delta applied.
    /// Keeps the state of each record of the chain that a record still to be
    /// checked names as base.
    fn state(
        &mut self,
        id: &'a str,
        mut below: impl FnMut(&'a R) -> Result<Shared<'a>, CodecError>,
    ) -> Result<Shared<'a>, CodecError> {
        let mut chain: Vec<&'a R> = Vec::new();
        let mut walked = BTreeSet::new();
        let mut at = id;
        let mut state = loop {
            if let Some(state) = self.states.get(at) {
                break state.clone();
            }
            let Some(&record) = self.by_id.get(at) else {
                break None;
            };
            if !walked.insert(at) {
                break None;
            }
            chain.push(record);
            if record.delta().is_none() {
                break None;
            }
            match record.base() {
                Some(base) => at = base,
                None => break below(record)?,
            }
        };
        for record in chain.into_iter().rev() {
            if let (Some(resolved), Some(delta)) = (&mut state, record.delta()) {
                apply(Rc::make_mut(resolved), delta, None);
            }
            if self.pending.contains_key(record.id()) {
                self.states.insert(record.id(), state.clone());
            }
        }
        Ok(state)
    }

    /// Keeps `state` as the resolved state of `record` where `record` is the
    /// first of its id and a record still to be checked names it as base.
    fn keep(&mut self, record: &'a R, state: ResolvedState<'a>) {
        let id = record.id();
        if self.is_first(record) && self.pending.contains_key(id) {
            self.states
                .entry(id)
                .or_insert_with(|| Some(Rc::new(state)));
        }
    }
}

/// Applies `delta` to `state`. With `report`, a record of the delta whose
/// identity another record of the delta holds, or a base record of another
/// arena holds, is reported against the named evaluation.
fn apply<'a>(
    state: &mut ResolvedState<'a>,
    delta: &'a StateDelta,
    mut report: Option<(&str, &mut Vec<Finding>)>,
) {
    for removed in &delta.removed {
        let (arena, id) = Arena::of_record(removed);
        if state.holds(arena, id) {
            state.records.remove(id);
        }
    }
    let mut inserted = BTreeSet::new();
    for (arena, id, index) in records(&delta.records) {
        let previous = state.records.insert(
            id,
            Located {
                arena,
                topology: &delta.records,
                index,
            },
        );
        let repeated = !inserted.insert(id);
        let clash = previous.filter(|previous| repeated || previous.arena != arena);
        if let (Some(previous), Some((evaluation, findings))) = (clash, report.as_mut()) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "two records of the resolved state share id {id}: a {} record and a {arena} record",
                    previous.arena
                ),
            );
        }
    }
    merge(&mut state.names, &delta.names, |entry| entry.as_ref());
    merge(&mut state.owners, &delta.owners, |owner| owner.as_ref());
    for (occurrence, owner) in &delta.members {
        match owner {
            Some(owner) => state.members.insert(occurrence.as_str(), owner),
            None => state.members.remove(occurrence.as_str()),
        };
    }
    merge(
        &mut state.instance_bodies,
        &delta.instance_bodies,
        |bodies| bodies.as_deref(),
    );
    merge(&mut state.poses, &delta.poses, |pose| pose.as_ref());
}

fn merge<'a, K: Ord, V, T: ?Sized>(
    into: &mut BTreeMap<&'a K, &'a T>,
    entries: &'a BTreeMap<K, V>,
    value: impl Fn(&'a V) -> Option<&'a T>,
) {
    for (key, entry) in entries {
        match value(entry) {
            Some(value) => into.insert(key, value),
            None => into.remove(key),
        };
    }
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
    if !has_evaluations {
        check_current_names(ir, &geometry, findings);
        return Ok(());
    }
    let resolver = Resolver {
        ctx,
        index,
        geometry,
        children: ir.geometry.child_lists(),
    };
    if has_names {
        check_current_names(ir, &resolver.geometry, findings);
    }
    let mut topology: BTreeSet<&str> = resolver.geometry.keys().copied().collect();
    for evaluation in &ir.design.evaluations {
        if let Some(content) = &evaluation.content {
            topology.extend(
                records(&content.delta.records)
                    .into_iter()
                    .map(|(_, id, _)| id),
            );
        }
    }
    let evaluations = &ir.design.evaluations;
    let mut chain = Chain::new(evaluations, evaluations.iter().filter_map(Stored::base));
    for evaluation in evaluations {
        let Some(content) = &evaluation.content else {
            continue;
        };
        let Some(base) = chain.base_state(evaluation, |below| resolver.below_evaluation(below))?
        else {
            continue;
        };
        let id = evaluation.id.as_str();
        let mut state = (*base).clone();
        apply(&mut state, &content.delta, Some((id, &mut *findings)));
        check_entries(index, &state, id, findings);
        check_closure(ctx, &state, &topology, ids, id, findings)?;
        state_topology::check_state_topology(
            ctx,
            ir.tolerances.linear,
            &base,
            &state,
            &content.delta.records,
            id,
            findings,
        )?;
        check_stored_bodies(index, evaluation, &state, findings);
        chain.keep(evaluation, state);
    }
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
                format_args!(
                    "name entry names {id}, which is no record of the current geometry's {arena}"
                ),
            );
        }
    }
}

/// The resolved state's name, owner, member, instance-body, and pose entries
/// name what the state holds.
fn check_entries(
    index: &DesignRecords<'_>,
    state: &ResolvedState<'_>,
    evaluation: &str,
    findings: &mut Vec<Finding>,
) {
    for entity in state.names.keys() {
        let (arena, id) = Arena::of_entity(entity);
        if !state.holds(arena, id) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "name entry names {id}, which is no record of the resolved state's {arena}"
                ),
            );
        }
    }
    for (body, owner) in &state.owners {
        if !state.holds(Arena::Body, body.as_str()) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "owner entry names {}, which is no body of the resolved state",
                    body.as_str()
                ),
            );
        }
        if !index.definitions.contains_key(owner.as_str()) {
            super::error(
                findings,
                evaluation,
                format_args!(
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
                format_args!(
                    "member entry names {occurrence}, which is no occurrence of the model"
                ),
            );
        }
        if !index.definitions.contains_key(owner.as_str()) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "member entry names {}, which is no product definition",
                    owner.as_str()
                ),
            );
        }
    }
    for (path, bodies) in &state.instance_bodies {
        check_path_steps(index, path, evaluation, "instance-body", findings);
        for body in *bodies {
            if !state.holds(Arena::Body, body.as_str()) {
                super::error(
                    findings,
                    evaluation,
                    format_args!(
                        "instance-body entry lists {}, which is no body of the resolved state",
                        body.as_str()
                    ),
                );
            }
        }
    }
    for path in state.poses.keys() {
        check_path_steps(index, path, evaluation, "pose", findings);
    }
}

/// Each record of the resolved state references only topology records of the
/// state: a reference to an id that the state does not hold is reported
/// unless the id is a record of the document outside every `Topology`, which
/// the reference checks resolve.
fn check_closure(
    ctx: &DecodeContext<'_>,
    state: &ResolvedState<'_>,
    topology: &BTreeSet<&str>,
    ids: &ModelIndex<'_>,
    evaluation: &str,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for (id, located) in &state.records {
        let mut outside = None;
        located.visit_references(ctx, &mut |target| {
            if outside.is_none()
                && !state.records.contains_key(target)
                && (topology.contains(target) || !IdentitySet::contains(ids, target))
            {
                outside = Some(target.to_owned());
            }
            Ok(())
        })?;
        if let Some(target) = outside {
            super::error(
                findings,
                evaluation,
                format_args!("record {id} of the resolved state references {target}, which the state does not hold"),
            );
        }
    }
    Ok(())
}

/// Where the state after a stored-geometry feature is stored, it holds each
/// body that the feature's definition in the evaluation's configuration
/// names (§5.10).
fn check_stored_bodies(
    index: &DesignRecords<'_>,
    evaluation: &Evaluation,
    state: &ResolvedState<'_>,
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
    } = &index.definition_in(feature, configuration).operation
    else {
        return;
    };
    for body in bodies {
        if !state.holds(Arena::Body, body.as_str()) {
            super::error(
                findings,
                evaluation.id.as_str(),
                format_args!(
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
    let mut topology: BTreeSet<&str> = resolver.geometry.keys().copied().collect();
    let deltas = ir
        .design
        .evaluations
        .iter()
        .filter_map(|evaluation| evaluation.content.as_ref().map(|content| &content.delta))
        .chain(
            states
                .iter()
                .filter_map(|state| state.content.as_ref().map(|content| &content.delta)),
        );
    for delta in deltas {
        topology.extend(records(&delta.records).into_iter().map(|(_, id, _)| id));
    }
    let mut evaluations = Chain::new(
        &ir.design.evaluations,
        states.iter().filter_map(design_base),
    );
    let mut chain = Chain::new(states, states.iter().filter_map(Stored::base));
    let mut process_findings = Vec::new();
    for state in states {
        let Some(content) = &state.content else {
            continue;
        };
        let base = chain.base_state(state, |below| {
            below_process_state(&resolver, &mut evaluations, &scope, below)
        })?;
        if let Some(evaluation) = design_base(state) {
            evaluations.release(evaluation);
        }
        let Some(base) = base else { continue };
        let id = state.id.as_str();
        let mut resolved = Rc::unwrap_or_clone(base);
        apply(
            &mut resolved,
            &content.delta,
            Some((id, &mut process_findings)),
        );
        check_entries(index, &resolved, id, &mut process_findings);
        check_closure(ctx, &resolved, &topology, ids, id, &mut process_findings)?;
        chain.keep(state, resolved);
    }
    findings.extend(process_findings.into_iter().map(|finding| Finding {
        check: Check::Process,
        ..finding
    }));
    Ok(())
}

/// The evaluation that a process state's design base names.
fn design_base(state: &ProcessState) -> Option<&str> {
    match &state.content.as_ref()?.base {
        ProcessBase::Design(StateBase::Evaluation(base)) => Some(base.as_str()),
        ProcessBase::Design(StateBase::Empty | StateBase::Current(_)) | ProcessBase::State(_) => {
            None
        }
    }
}

/// The state that a process state's design base names. `scope` gives the
/// design scope of a state's setup, which a `Current` base reads. `None`
/// where the base does not resolve.
fn below_process_state<'a>(
    resolver: &Resolver<'a, '_, '_>,
    evaluations: &mut Chain<'a, Evaluation>,
    scope: &impl Fn(&ProcessState) -> Option<&'a str>,
    state: &'a ProcessState,
) -> Result<Shared<'a>, CodecError> {
    let Some(ProcessBase::Design(base)) = state.content.as_ref().map(|content| &content.base)
    else {
        return Ok(None);
    };
    match base {
        StateBase::Empty => Ok(Some(Rc::default())),
        StateBase::Current(position) => match scope(state) {
            Some(scope) => Ok(resolver
                .extent(scope, PositionKey::of(position))?
                .map(Rc::new)),
            None => Ok(None),
        },
        StateBase::Evaluation(base) => evaluations.state(base.as_str(), |evaluation| {
            resolver.below_evaluation(evaluation)
        }),
    }
}

mod state_topology;
#[cfg(test)]
mod tests;
