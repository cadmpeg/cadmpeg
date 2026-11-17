// SPDX-License-Identifier: Apache-2.0
//! Lookups and the dependency order (§5.3) shared by the design rules.
//!
//! A state at a position depends on the features at or before the position
//! in its container chain, and on every feature those depend on through
//! their definitions' references (D26). A feature's references are the
//! features its definition, its sketches, and its parameters pick, and the
//! features the names it picks by mention.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::container::{Container, Rollback};
use crate::design::feature::{Feature, FeatureOperation};
use crate::design::ops::sketch::{Sketch, SpatialSketch};
use crate::design::refs::{Member, Position};
use crate::design::scope::DesignScope;
use crate::design::selection::{NativeOwner, OccurrencePath, PickTarget, StepRef};
use crate::document::CadIr;
use crate::ids::ProductDefinitionId;
use crate::products::{Occurrence, PrototypeReference};

use super::names::mentioned_features;
use super::Held;

/// A member of a container, by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Key<'a> {
    Feature(&'a str),
    Container(&'a str),
}

impl<'a> From<&'a Member> for Key<'a> {
    fn from(member: &'a Member) -> Self {
        match member {
            Member::Feature(id) => Self::Feature(id.as_str()),
            Member::Container(id) => Self::Container(id.as_str()),
        }
    }
}

/// A state whose dependencies a rule reads.
#[derive(Debug, Clone, Copy)]
pub(super) enum StateAt<'a> {
    /// The state entering a feature of an ordered container.
    Entering(&'a str),
    /// The state at a position.
    Position(&'a Position),
}

/// The design records by identity, container membership, and dependencies.
pub(super) struct DesignIndex<'a> {
    pub(super) ir: &'a CadIr,
    pub(super) features: BTreeMap<&'a str, &'a Feature>,
    pub(super) containers: BTreeMap<&'a str, &'a Container>,
    /// The container holding each member, and the member's index in an
    /// ordered container's list.
    places: BTreeMap<Key<'a>, (&'a Container, usize)>,
    /// The scope definition of each root container.
    roots: BTreeMap<&'a str, &'a ProductDefinitionId>,
    /// The scopes, by definition.
    pub(super) scopes: BTreeMap<&'a str, &'a DesignScope>,
    pub(super) sketches: BTreeMap<&'a str, &'a Sketch>,
    pub(super) spatial_sketches: BTreeMap<&'a str, &'a SpatialSketch>,
    /// The sketch of each planar sketch entity.
    pub(super) sketch_entities: BTreeMap<&'a str, &'a str>,
    /// The spatial sketch of each spatial sketch entity.
    pub(super) spatial_sketch_entities: BTreeMap<&'a str, &'a str>,
    pub(super) publications: BTreeSet<&'a str>,
    pub(super) occurrences: BTreeMap<&'a str, &'a Occurrence>,
    pub(super) definitions: BTreeSet<&'a str>,
    /// The features each feature's references name.
    dependencies: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

impl<'a> DesignIndex<'a> {
    pub(super) fn new(ir: &'a CadIr) -> Self {
        let design = &ir.design;
        let mut places = BTreeMap::new();
        for container in &design.containers {
            match container {
                Container::Ordered { members, .. } => {
                    for (index, member) in members.iter().enumerate() {
                        places
                            .entry(Key::from(member))
                            .or_insert((container, index));
                    }
                }
                Container::Unordered { members, .. } => {
                    for (index, member) in members.iter().enumerate() {
                        places
                            .entry(Key::from(member))
                            .or_insert((container, index));
                    }
                }
            }
        }
        let mut roots = BTreeMap::new();
        for scope in &design.scopes {
            for root in &scope.roots {
                roots.entry(root.as_str()).or_insert(&scope.definition);
            }
        }
        Self {
            ir,
            features: by_id(&design.features, |feature| feature.id.as_str()),
            containers: by_id(&design.containers, |container| container.id().as_str()),
            places,
            roots,
            scopes: by_id(&design.scopes, |scope| scope.definition.as_str()),
            sketches: by_id(&design.sketches, |sketch| sketch.id.as_str()),
            spatial_sketches: by_id(&design.spatial_sketches, |sketch| sketch.id.as_str()),
            sketch_entities: design
                .sketch_entities
                .iter()
                .map(|entity| (entity.id.as_str(), entity.sketch.as_str()))
                .collect(),
            spatial_sketch_entities: design
                .spatial_sketch_entities
                .iter()
                .map(|entity| (entity.id.as_str(), entity.sketch.as_str()))
                .collect(),
            publications: design
                .publications
                .iter()
                .map(|publication| publication.id.as_str())
                .collect(),
            occurrences: by_id(&ir.model.occurrences, |occurrence| occurrence.id.as_str()),
            definitions: ir
                .model
                .product_definitions
                .iter()
                .map(|definition| definition.id.as_str())
                .collect(),
            dependencies: BTreeMap::new(),
        }
    }

    /// Records the references of every feature: the features its held
    /// records pick, and the features the names they pick by mention.
    pub(super) fn add_dependencies(&mut self, held: &[Held<'a>]) {
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
    pub(super) fn place(&self, key: Key<'_>) -> Option<(&'a Container, usize)> {
        self.places.get(&key).copied()
    }

    /// Whether the member's container is ordered.
    pub(super) fn in_ordered(&self, key: Key<'_>) -> bool {
        matches!(self.place(key), Some((Container::Ordered { .. }, _)))
    }

    /// The scope a member belongs to, through its container chain.
    pub(super) fn scope_of(&self, key: Key<'_>) -> Option<&'a ProductDefinitionId> {
        let mut container = match key {
            Key::Container(id) => self.containers.get(id).copied()?,
            Key::Feature(_) => self.place(key)?.0,
        };
        for _ in 0..=self.containers.len() {
            let id = container.id().as_str();
            if let Some(scope) = self.roots.get(id) {
                return Some(scope);
            }
            container = self.place(Key::Container(id))?.0;
        }
        None
    }

    /// The definition a feature's selections resolve in: its `component`
    /// where stated, else its scope's definition (§5.8).
    pub(super) fn owner_definition(&self, feature: &str) -> Option<&'a ProductDefinitionId> {
        let record = self.features.get(feature)?;
        record
            .component
            .as_ref()
            .or_else(|| self.scope_of(Key::Feature(feature)))
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
                .and_then(|sketch| self.sketch_owner(sketch)),
            PickTarget::SpatialSketchEntity(entity) => self
                .spatial_sketch_entities
                .get(entity.as_str())
                .and_then(|sketch| self.spatial_sketch_owner(sketch)),
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
    pub(super) fn state_features(&self, at: StateAt<'a>) -> Option<BTreeSet<&'a str>> {
        let mut found = BTreeSet::new();
        let (container, upto) = match at {
            StateAt::Entering(feature) => {
                let (container, index) = self.place(Key::Feature(feature))?;
                (container, Some(index))
            }
            StateAt::Position(Position::Start(container)) => {
                let container = self.containers.get(container.as_str()).copied()?;
                (container, Some(0))
            }
            StateAt::Position(Position::After(member)) => {
                let key = Key::from(member);
                if let Key::Container(id) = key {
                    let inner = self.containers.get(id).copied()?;
                    self.add_subtree(inner, &mut found);
                }
                let (container, index) = self.place(key)?;
                if let (Container::Unordered { .. }, Key::Feature(feature)) = (container, key) {
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
    pub(super) fn entering(&self, feature: &str) -> Option<Position> {
        let (container, index) = self.place(Key::Feature(feature))?;
        let Container::Ordered { id, members, .. } = container else {
            return None;
        };
        Some(match index.checked_sub(1) {
            None => Position::Start(id.clone()),
            Some(before) => Position::After(members.get(before)?.clone()),
        })
    }

    /// The features the current geometry of a scope depends on: the union
    /// over its current states, or `None` where it lists none.
    pub(super) fn current_features(&self, scope: &str) -> Option<BTreeSet<&'a str>> {
        let scope = self.scopes.get(scope)?;
        if scope.current.is_empty() {
            return None;
        }
        let mut found = BTreeSet::new();
        for current in &scope.current {
            found.extend(self.state_features(StateAt::Position(&current.at))?);
        }
        Some(found)
    }

    /// Adds the members before `upto` of an ordered container, and of every
    /// ordered container above it, before the member that holds it.
    fn add_chain(
        &self,
        mut container: &'a Container,
        mut upto: Option<usize>,
        found: &mut BTreeSet<&'a str>,
    ) {
        for _ in 0..=self.containers.len() {
            if let (Container::Ordered { members, .. }, Some(upto)) = (container, upto) {
                for member in members.iter().take(upto) {
                    self.add_member(member, found);
                }
            }
            let Some((parent, index)) = self.place(Key::Container(container.id().as_str())) else {
                return;
            };
            container = parent;
            upto = Some(index);
        }
    }

    fn add_member(&self, member: &'a Member, found: &mut BTreeSet<&'a str>) {
        match member {
            Member::Feature(feature) => {
                found.insert(feature.as_str());
            }
            Member::Container(container) => {
                if let Some(container) = self.containers.get(container.as_str()) {
                    self.add_subtree(container, found);
                }
            }
        }
    }

    /// Adds every feature of a container's tree, up to each ordered
    /// container's rollback position.
    fn add_subtree(&self, container: &'a Container, found: &mut BTreeSet<&'a str>) {
        let mut stack = vec![container];
        let mut seen = BTreeSet::new();
        while let Some(container) = stack.pop() {
            if !seen.insert(container.id().as_str()) {
                continue;
            }
            let members: Vec<&'a Member> = match container {
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
                    members.iter().take(end).collect()
                }
                Container::Unordered { members, .. } => members.iter().collect(),
            };
            for member in members {
                match member {
                    Member::Feature(feature) => {
                        found.insert(feature.as_str());
                    }
                    Member::Container(inner) => {
                        stack.extend(self.containers.get(inner.as_str()).copied());
                    }
                }
            }
        }
    }

    /// The features `start` holds and every feature they depend on.
    fn closure(&self, start: BTreeSet<&'a str>) -> BTreeSet<&'a str> {
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
    pub(super) fn tool_features(&self, feature: &str) -> Option<BTreeSet<&'a str>> {
        let (&feature, _) = self.features.get_key_value(feature)?;
        Some(self.closure(BTreeSet::from([feature])))
    }

    /// The local prototype definitions the occurrences of a path place.
    pub(super) fn path_prototypes(&self, path: &OccurrencePath) -> Vec<&'a str> {
        path.0
            .iter()
            .filter_map(|step| match &step.occurrence {
                StepRef::Occurrence(occurrence) => self.occurrences.get(occurrence.as_str()),
                StepRef::Native(_) => None,
            })
            .filter_map(|occurrence| match &occurrence.prototype {
                PrototypeReference::Local { definition } => Some(definition.as_str()),
                PrototypeReference::External { .. } | PrototypeReference::Unresolved {} => None,
            })
            .collect()
    }

    /// Every feature of the scope of a definition.
    pub(super) fn scope_features(&self, definition: &str) -> Vec<&'a str> {
        self.features
            .keys()
            .copied()
            .filter(|feature| {
                self.scope_of(Key::Feature(feature))
                    .is_some_and(|scope| scope.as_str() == definition)
            })
            .collect()
    }

    /// Whether `from` depends on `on` through references, directly or
    /// transitively.
    pub(super) fn depends_on(&self, from: &str, on: &str) -> bool {
        let direct = self.dependencies.get(from).cloned().unwrap_or_default();
        self.closure(direct).contains(on)
    }

    /// Whether a feature's operation is the given family's.
    pub(super) fn operation(&self, feature: &str) -> Option<&'a FeatureOperation> {
        self.features
            .get(feature)
            .map(|feature| &feature.definition.operation)
    }
}

fn by_id<'a, T>(records: &'a [T], id: impl Fn(&'a T) -> &'a str) -> BTreeMap<&'a str, &'a T> {
    let mut map = BTreeMap::new();
    for record in records {
        map.entry(id(record)).or_insert(record);
    }
    map
}
