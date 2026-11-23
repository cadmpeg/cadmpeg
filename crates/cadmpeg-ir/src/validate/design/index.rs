// SPDX-License-Identifier: Apache-2.0
//! Lookups and the dependency order (§5.3) shared by the design rules.
//!
//! A state at a position depends on the features at or before the position
//! in its container chain, and on every feature those depend on through
//! their definitions' references (D26). A feature's references are the
//! features its definition, its sketches, and its parameters pick, and the
//! features the names it picks by mention.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use crate::design::container::{Container, Rollback};
use crate::design::feature::FeatureOperation;
use crate::design::ops::sketch::{Planar, Sketch, SketchEntity, Spatial, SpatialSketch};
use crate::design::refs::{Member, Position};
use crate::design::selection::{NativeOwner, OccurrencePath, PickTarget, StepRef};
use crate::document::CadIr;
use crate::ids::ProductDefinitionId;

use super::names::mentioned_features;
use super::records::{by_id, members, DesignRecords, MemberKey, PositionKey};
use super::Held;

/// A state whose dependencies a rule reads.
#[derive(Debug, Clone, Copy)]
pub(super) enum StateAt<'a> {
    /// The state entering a feature of an ordered container.
    Entering(&'a str),
    /// The state at a position.
    Position(&'a Position),
}

/// Cache key for [`DesignIndex::state_features`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StateKey<'a> {
    Entering(&'a str),
    Position(PositionKey<'a>),
}

impl<'a> StateKey<'a> {
    fn of(at: StateAt<'a>) -> Self {
        match at {
            StateAt::Entering(feature) => Self::Entering(feature),
            StateAt::Position(position) => Self::Position(PositionKey::of(position)),
        }
    }
}

/// The design records by identity, container membership, and dependencies.
pub(super) struct DesignIndex<'a> {
    pub(super) records: DesignRecords<'a>,
    /// The container holding each member, and the member's index in an
    /// ordered container's list.
    places: BTreeMap<MemberKey<'a>, (&'a Container, usize)>,
    pub(super) sketches: BTreeMap<&'a str, &'a Sketch>,
    pub(super) spatial_sketches: BTreeMap<&'a str, &'a SpatialSketch>,
    pub(super) sketch_entities: BTreeMap<&'a str, &'a SketchEntity<Planar>>,
    pub(super) spatial_sketch_entities: BTreeMap<&'a str, &'a SketchEntity<Spatial>>,
    pub(super) publications: BTreeSet<&'a str>,
    /// The features each feature's references name.
    dependencies: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// Memoized [`Self::state_features`] results, keyed by state.
    state_cache: RefCell<HashMap<StateKey<'a>, Option<Rc<HashSet<&'a str>>>>>,
    /// Memoized [`Self::current_features`] results, keyed by scope id.
    current_cache: RefCell<HashMap<&'a str, Option<Rc<HashSet<&'a str>>>>>,
}

impl<'a> DesignIndex<'a> {
    pub(super) fn new(ir: &'a CadIr) -> Self {
        let design = &ir.design;
        let mut places = BTreeMap::new();
        for container in &design.containers {
            for (index, member) in members(container).into_iter().enumerate() {
                places
                    .entry(MemberKey::of(member))
                    .or_insert((container, index));
            }
        }
        Self {
            records: DesignRecords::new(ir),
            places,
            sketches: by_id(&design.sketches, |sketch| sketch.id.as_str()),
            spatial_sketches: by_id(&design.spatial_sketches, |sketch| sketch.id.as_str()),
            sketch_entities: design
                .sketch_entities
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
            spatial_sketch_entities: design
                .spatial_sketch_entities
                .iter()
                .map(|entity| (entity.id.as_str(), entity))
                .collect(),
            publications: design
                .publications
                .iter()
                .map(|publication| publication.id.as_str())
                .collect(),
            dependencies: BTreeMap::new(),
            state_cache: RefCell::new(HashMap::new()),
            current_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Records the references of every feature: the features its held
    /// records pick, and the features the names they pick by mention.
    pub(super) fn add_dependencies(&mut self, held: &[Held<'a>]) {
        for feature in &self.records.ir.design.features {
            if let FeatureOperation::SheetMetal(sheet) = &feature.definition.operation {
                use crate::design::ops::sheet_metal::SheetMetalKind;
                let target = match &sheet.kind {
                    SheetMetalKind::AttachedBend { plate, .. } => Some(plate),
                    SheetMetalKind::UnchangedCorner { bend } => Some(bend),
                    _ => None,
                };
                if let Some(target) = target {
                    self.dependencies
                        .entry(feature.id.as_str())
                        .or_default()
                        .insert(target.as_str());
                }
            }
        }
        self.state_cache.borrow_mut().clear();
        self.current_cache.borrow_mut().clear();
        for record in held {
            let Some(feature) = record.holder.feature() else {
                continue;
            };
            let mut found = BTreeSet::new();
            for selection in &record.selections {
                for pick in selection.picks() {
                    if let Some(target) = self.target_feature(&pick.target) {
                        found.insert(target);
                    }
                    if let PickTarget::Entity(name) = &pick.target {
                        found.extend(
                            mentioned_features(name)
                                .into_iter()
                                .map(crate::features::FeatureId::as_str),
                        );
                    }
                }
            }
            self.dependencies.entry(feature).or_default().extend(found);
        }
    }

    /// The container holding a member and the member's index.
    pub(super) fn place(&self, key: MemberKey<'_>) -> Option<(&'a Container, usize)> {
        self.places.get(&key).copied()
    }

    /// Whether the member's container is ordered.
    pub(super) fn in_ordered(&self, key: MemberKey<'_>) -> bool {
        matches!(self.place(key), Some((Container::Ordered { .. }, _)))
    }

    /// The definition of the scope a member belongs to, through its
    /// container chain.
    pub(super) fn scope_of(&self, key: MemberKey<'a>) -> Option<&'a ProductDefinitionId> {
        let scope = self.records.scope_of(key)?;
        self.records
            .scopes
            .get(scope)
            .map(|scope| &scope.definition)
    }

    /// The definition a feature's selections resolve in: its `component`
    /// where stated, else its scope's definition (§5.8).
    pub(super) fn owner_definition(&self, feature: &str) -> Option<&'a ProductDefinitionId> {
        let (&feature, record) = self.records.features.get_key_value(feature)?;
        record
            .component
            .as_ref()
            .or_else(|| self.scope_of(MemberKey::Feature(feature)))
    }

    /// The feature a pick target depends on.
    pub(super) fn target_feature(&self, target: &'a PickTarget) -> Option<&'a str> {
        match target {
            PickTarget::Sketch(sketch)
            | PickTarget::SketchRegion { sketch, .. }
            | PickTarget::SketchFrame { sketch, .. } => self.sketch_owner(sketch.as_str()),
            PickTarget::SketchEntity(entity) => self
                .sketch_entities
                .get(entity.as_str())
                .and_then(|entity| self.sketch_owner(entity.sketch.as_str())),
            PickTarget::SpatialSketchEntity(entity) => self
                .spatial_sketch_entities
                .get(entity.as_str())
                .and_then(|entity| self.spatial_sketch_owner(entity.sketch.as_str())),
            PickTarget::Datum { feature, .. }
            | PickTarget::Feature(feature)
            | PickTarget::FeatureResult(feature)
            | PickTarget::TemplateInput {
                instance: feature, ..
            } => Some(feature.as_str()),
            PickTarget::Native { within } => match within.as_ref()? {
                NativeOwner::Sketch(sketch) => self.sketch_owner(sketch.as_str()),
                NativeOwner::SpatialSketch(sketch) => self.spatial_sketch_owner(sketch.as_str()),
                NativeOwner::Feature(feature) => Some(feature.as_str()),
            },
            PickTarget::Entity(_)
            | PickTarget::Principal { .. }
            | PickTarget::Component
            | PickTarget::Publication(_) => None,
        }
    }

    fn sketch_owner(&self, sketch: &str) -> Option<&'a str> {
        self.sketches
            .get(sketch)
            .map(|sketch| sketch.owner.as_str())
    }

    fn spatial_sketch_owner(&self, sketch: &str) -> Option<&'a str> {
        self.spatial_sketches
            .get(sketch)
            .map(|sketch| sketch.owner.as_str())
    }

    /// The features a state depends on (§5.3), or `None` where a member the
    /// state names is in no container.
    pub(super) fn state_features(&self, at: StateAt<'a>) -> Option<Rc<HashSet<&'a str>>> {
        let key = StateKey::of(at);
        if let Some(cached) = self.state_cache.borrow().get(&key) {
            return cached.clone();
        }
        let computed = self.compute_state_features(at).map(Rc::new);
        self.state_cache.borrow_mut().insert(key, computed.clone());
        computed
    }

    fn compute_state_features(&self, at: StateAt<'a>) -> Option<HashSet<&'a str>> {
        let mut found = HashSet::new();
        let (container, upto) = match at {
            StateAt::Entering(feature) => {
                let (container, index) = self.place(MemberKey::Feature(feature))?;
                (container, Some(index))
            }
            StateAt::Position(Position::Start(container)) => {
                let container = self.records.containers.get(container.as_str()).copied()?;
                (container, Some(0))
            }
            StateAt::Position(Position::After(member)) => {
                let key = MemberKey::of(member);
                if let MemberKey::Container(id) = key {
                    let inner = self.records.containers.get(id).copied()?;
                    self.add_subtree(inner, &mut found);
                }
                let (container, index) = self.place(key)?;
                if let (Container::Unordered { .. }, MemberKey::Feature(feature)) = (container, key)
                {
                    found.insert(feature);
                }
                (container, index.checked_add(1))
            }
        };
        self.add_chain(container, upto, &mut found);
        Some(self.closure(found))
    }

    /// The one spelling of the state entering a feature of an ordered
    /// container (§5.3): `Start` of the container before its first member,
    /// else the position after the member before it.
    pub(super) fn entering(&self, feature: &str) -> Option<PositionKey<'a>> {
        let (container, index) = self.place(MemberKey::Feature(feature))?;
        let Container::Ordered { id, members, .. } = container else {
            return None;
        };
        Some(match index.checked_sub(1) {
            None => PositionKey::Start(id.as_str()),
            Some(before) => PositionKey::After(MemberKey::of(members.get(before)?)),
        })
    }

    /// The position whose state a position names (§5.3): for
    /// `After(Container(c))` of an ordered container `c`, the position at
    /// `c`'s rollback, which has no state of its own; any other position is
    /// itself. `None` where a rollback names no member of its container.
    pub(super) fn rollback_position<'p>(
        &self,
        mut position: PositionKey<'p>,
    ) -> Option<PositionKey<'p>>
    where
        'a: 'p,
    {
        for _ in 0..=self.records.containers.len() {
            let PositionKey::After(MemberKey::Container(id)) = position else {
                return Some(position);
            };
            let Some(container @ Container::Ordered { .. }) =
                self.records.containers.get(id).copied()
            else {
                return Some(position);
            };
            position = DesignRecords::rollback_position(container)?;
        }
        None
    }

    /// The features the current geometry of a scope depends on: the union
    /// over its current states, or `None` where it lists none.
    pub(super) fn current_features(&self, scope: &'a str) -> Option<Rc<HashSet<&'a str>>> {
        if let Some(cached) = self.current_cache.borrow().get(scope) {
            return cached.clone();
        }
        let computed = self.compute_current_features(scope).map(Rc::new);
        self.current_cache
            .borrow_mut()
            .insert(scope, computed.clone());
        computed
    }

    fn compute_current_features(&self, scope: &str) -> Option<HashSet<&'a str>> {
        let scope = self.records.scopes.get(scope)?;
        if scope.current.is_empty() {
            return None;
        }
        let mut found = HashSet::new();
        for current in &scope.current {
            found.extend(
                self.state_features(StateAt::Position(&current.at))?
                    .iter()
                    .copied(),
            );
        }
        Some(found)
    }

    /// Adds the members before `upto` of an ordered container, and of every
    /// ordered container above it, before the member that holds it.
    fn add_chain(
        &self,
        mut container: &'a Container,
        mut upto: Option<usize>,
        found: &mut HashSet<&'a str>,
    ) {
        for _ in 0..=self.records.containers.len() {
            if let (Container::Ordered { members, .. }, Some(upto)) = (container, upto) {
                for member in members.iter().take(upto) {
                    self.add_member(member, found);
                }
            }
            let Some((parent, index)) = self.place(MemberKey::Container(container.id().as_str()))
            else {
                return;
            };
            container = parent;
            upto = Some(index);
        }
    }

    fn add_member(&self, member: &'a Member, found: &mut HashSet<&'a str>) {
        match member {
            Member::Feature(feature) => {
                found.insert(feature.as_str());
            }
            Member::Container(container) => {
                if let Some(container) = self.records.containers.get(container.as_str()) {
                    self.add_subtree(container, found);
                }
            }
        }
    }

    /// Adds every feature of a container's tree, up to each ordered
    /// container's rollback position.
    fn add_subtree(&self, container: &'a Container, found: &mut HashSet<&'a str>) {
        let mut stack = vec![container];
        let mut seen = HashSet::new();
        while let Some(container) = stack.pop() {
            if !seen.insert(container.id().as_str()) {
                continue;
            }
            match container {
                Container::Ordered {
                    members, rollback, ..
                } => {
                    let end = match rollback {
                        Rollback::End => members.len(),
                        Rollback::Before(at) => members
                            .iter()
                            .position(|member| member == at)
                            .unwrap_or(members.len()),
                    };
                    for member in &members[..end] {
                        match member {
                            Member::Feature(feature) => {
                                found.insert(feature.as_str());
                            }
                            Member::Container(inner) => {
                                stack.extend(self.records.containers.get(inner.as_str()).copied());
                            }
                        }
                    }
                }
                Container::Unordered { members, .. } => {
                    for member in members {
                        match member {
                            Member::Feature(feature) => {
                                found.insert(feature.as_str());
                            }
                            Member::Container(inner) => {
                                stack.extend(self.records.containers.get(inner.as_str()).copied());
                            }
                        }
                    }
                }
            }
        }
    }

    /// The features `start` holds and every feature they depend on.
    fn closure(&self, start: HashSet<&'a str>) -> HashSet<&'a str> {
        let mut found = start;
        let mut stack: Vec<&'a str> = found.iter().copied().collect();
        while let Some(feature) = stack.pop() {
            for &next in self.dependencies.get(feature).into_iter().flatten() {
                if found.insert(next) {
                    stack.push(next);
                }
            }
        }
        found
    }

    /// The features a `Tool(feature)` state depends on: the feature and what
    /// its definition depends on (§5.3). `None` where the feature does not
    /// exist.
    pub(super) fn tool_features(&self, feature: &str) -> Option<HashSet<&'a str>> {
        let (&feature, _) = self.records.features.get_key_value(feature)?;
        Some(self.closure(HashSet::from([feature])))
    }

    /// The prototype definitions the occurrences of a path place.
    pub(super) fn path_prototypes(&self, path: &OccurrencePath) -> Vec<&'a str> {
        path.0
            .iter()
            .filter_map(|step| match &step.occurrence {
                StepRef::Occurrence(occurrence) => self.records.prototype(occurrence.as_str()),
                StepRef::Native(_) => None,
            })
            .collect()
    }

    /// Every feature of the scope of a definition.
    pub(super) fn scope_features(&self, definition: &str) -> Vec<&'a str> {
        self.records
            .features
            .keys()
            .copied()
            .filter(|feature| {
                self.scope_of(MemberKey::Feature(feature))
                    .is_some_and(|scope| scope.as_str() == definition)
            })
            .collect()
    }

    /// Whether `from` depends on `on` through references, directly or
    /// transitively.
    pub(super) fn depends_on(&self, from: &str, on: &str) -> bool {
        let direct: HashSet<&'a str> = self
            .dependencies
            .get(from)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        self.closure(direct).contains(on)
    }

    /// Whether a feature's operation is the given family's.
    pub(super) fn operation(&self, feature: &str) -> Option<&'a FeatureOperation> {
        self.records
            .features
            .get(feature)
            .map(|feature| &feature.definition.operation)
    }
}
