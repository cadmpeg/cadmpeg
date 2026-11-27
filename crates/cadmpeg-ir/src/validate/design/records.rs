// SPDX-License-Identifier: Apache-2.0
//! Lookups over the design layer that the part 10 rules share.
//!
//! Every lookup keeps the first record of an identity; a repeated identity is
//! reported by the scope-and-linkage rules and does not stop the others.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use crate::design::configuration::{Arrangement, DesignConfiguration, FeatureStateOverride};
use crate::design::container::{Container, Rollback};
use crate::design::evaluation::Evaluation;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::ops::sketch::{
    Planar, Sketch, SketchConstraint, SketchEntity, Spatial, SpatialSketch,
};
use crate::design::parameter::DesignParameter;
use crate::design::refs::{Evaluated, Member, Position};
use crate::design::scope::DesignScope;
use crate::document::CadIr;
use crate::features::{ConfigurationId, FeatureId};
use crate::products::{Occurrence, ProductDefinition, Storage};

/// A member of a container, by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::validate) enum MemberKey<'a> {
    /// A feature.
    Feature(&'a str),
    /// A nested container.
    Container(&'a str),
}

impl<'a> MemberKey<'a> {
    pub(in crate::validate) fn of(member: &'a Member) -> Self {
        match member {
            Member::Feature(id) => Self::Feature(id.as_str()),
            Member::Container(id) => Self::Container(id.as_str()),
        }
    }

    pub(in crate::validate) const fn id(self) -> &'a str {
        match self {
            Self::Feature(id) | Self::Container(id) => id,
        }
    }
}

impl fmt::Display for MemberKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Feature(id) => write!(f, "feature {id}"),
            Self::Container(id) => write!(f, "container {id}"),
        }
    }
}

/// A position, by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::validate) enum PositionKey<'a> {
    /// The start of an ordered container.
    Start(&'a str),
    /// The position after a member.
    After(MemberKey<'a>),
}

impl<'a> PositionKey<'a> {
    pub(in crate::validate) fn of(position: &'a Position) -> Self {
        match position {
            Position::Start(id) => Self::Start(id.as_str()),
            Position::After(member) => Self::After(MemberKey::of(member)),
        }
    }
}

impl fmt::Display for PositionKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(id) => write!(f, "the start of container {id}"),
            Self::After(member) => write!(f, "the position after {member}"),
        }
    }
}

/// What an evaluation evaluates, by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::validate) enum OfKey<'a> {
    /// The state at a position.
    State(PositionKey<'a>),
    /// The tool body of a feature.
    Tool(&'a str),
}

impl<'a> OfKey<'a> {
    pub(in crate::validate) fn of(of: &'a Evaluated) -> Self {
        match of {
            Evaluated::State(position) => Self::State(PositionKey::of(position)),
            Evaluated::Tool(feature) => Self::Tool(feature.as_str()),
        }
    }
}

#[derive(Default)]
struct ScopeEvaluations<'a> {
    start: HashMap<&'a str, Vec<&'a str>>,
    after_feature: HashMap<&'a str, Vec<&'a str>>,
    after_container: HashMap<&'a str, Vec<&'a str>>,
}

/// The design records of one document, by identity.
pub(in crate::validate) struct DesignRecords<'a> {
    pub(in crate::validate) ir: &'a CadIr,
    /// Each scope, by the definition whose design it is.
    pub(in crate::validate) scopes: BTreeMap<&'a str, &'a DesignScope>,
    pub(in crate::validate) containers: BTreeMap<&'a str, &'a Container>,
    pub(in crate::validate) features: BTreeMap<&'a str, &'a Feature>,
    pub(in crate::validate) configurations: BTreeMap<&'a str, &'a DesignConfiguration>,
    pub(in crate::validate) arrangements: BTreeMap<&'a str, &'a Arrangement>,
    pub(in crate::validate) evaluations: BTreeMap<&'a str, &'a Evaluation>,
    pub(in crate::validate) parameters: BTreeMap<&'a str, &'a DesignParameter>,
    pub(in crate::validate) sketches: BTreeMap<&'a str, &'a Sketch>,
    pub(in crate::validate) spatial_sketches: BTreeMap<&'a str, &'a SpatialSketch>,
    pub(in crate::validate) sketch_entities: BTreeMap<&'a str, &'a SketchEntity<Planar>>,
    pub(in crate::validate) spatial_sketch_entities: BTreeMap<&'a str, &'a SketchEntity<Spatial>>,
    pub(in crate::validate) definitions: BTreeMap<&'a str, &'a ProductDefinition>,
    pub(in crate::validate) occurrences: BTreeMap<&'a str, &'a Occurrence>,
    /// The container that lists each member.
    pub(in crate::validate) parents: BTreeMap<MemberKey<'a>, &'a str>,
    feature_parents: HashMap<&'a str, &'a str>,
    container_parents: HashMap<&'a str, &'a str>,
    /// The scope whose roots list each root container.
    pub(in crate::validate) roots: BTreeMap<&'a str, &'a str>,
    /// The configurations of each scope.
    pub(in crate::validate) scope_configurations: BTreeMap<&'a str, Vec<&'a DesignConfiguration>>,
    /// The first product definition that lists each occurrence id.
    occurrence_owners: HashMap<&'a str, &'a str>,
    /// Memoized `container -> (scope, hops_to_root)`.
    container_scopes: OnceCell<HashMap<&'a str, (&'a str, usize)>>,
    /// Memoized scopes holding a feature whose `component != scope`.
    several_scopes: OnceCell<HashSet<&'a str>>,
    /// Memoized prototypes placed by `self.occurrences`.
    placed: OnceCell<BTreeSet<&'a str>>,
    /// Memoized prototypes placed by any `ir.model.occurrences` entry.
    all_placed: OnceCell<HashSet<&'a str>>,
    /// Memoized `spanned_definitions` per scope.
    spanned: RefCell<HashMap<&'a str, Rc<BTreeSet<&'a str>>>>,
    empty_spanned: Rc<BTreeSet<&'a str>>,
    /// Memoized `moved_occurrences(ir)`.
    moved: OnceCell<BTreeSet<&'a str>>,
    /// Memoized body ids in `ir.geometry.bodies` with `transform.is_some()`.
    transformed_bodies: OnceCell<HashSet<&'a str>>,
    /// Memoized evaluation ids with content keyed by `scope` then position.
    evaluations_by_scope_position: OnceCell<HashMap<&'a str, ScopeEvaluations<'a>>>,
    /// Memoized planar sketches keyed by `owner`.
    sketches_by_owner: OnceCell<HashMap<&'a str, Vec<&'a Sketch>>>,
    /// Memoized planar sketch constraints keyed by `sketch`.
    constraints_by_sketch: OnceCell<HashMap<&'a str, Vec<&'a SketchConstraint<Planar>>>>,
    /// Memoized planar sketch entities keyed by `sketch`.
    entities_by_sketch: OnceCell<HashMap<&'a str, Vec<&'a SketchEntity<Planar>>>>,
}

/// How a feature names an occurrence (§6.7).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::validate) enum Namer {
    /// A `Combine`'s `new_component`.
    Combine,
    /// A `Place` or a `PlaceSet`.
    Placement,
    /// A component pattern or mirror.
    Creator,
}

/// A container's members: in order for an ordered container.
pub(in crate::validate) fn members(container: &Container) -> Vec<&Member> {
    match container {
        Container::Ordered { members, .. } => members.iter().collect(),
        Container::Unordered { members, .. } => members.iter().collect(),
    }
}

/// The one spelling of the state entering the member of ordered container
/// `container` that `before` precedes (§5.3): `After` the nearest feature of
/// `before`, else `Start(container)`. A container member has no result in
/// its parent.
pub(super) fn entering_at<'a>(container: &'a str, before: &'a [Member]) -> PositionKey<'a> {
    before
        .iter()
        .rev()
        .find(|member| matches!(member, Member::Feature(_)))
        .map_or(PositionKey::Start(container), |member| {
            PositionKey::After(MemberKey::of(member))
        })
}

pub(super) fn by_id<'a, T>(
    records: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
) -> BTreeMap<&'a str, &'a T> {
    let mut index = BTreeMap::new();
    for record in records {
        index.entry(id(record)).or_insert(record);
    }
    index
}

impl<'a> DesignRecords<'a> {
    pub(in crate::validate) fn new(ir: &'a CadIr) -> Self {
        let design = &ir.design;
        let mut parents = BTreeMap::new();
        let mut feature_parents = HashMap::new();
        let mut container_parents = HashMap::new();
        for container in &design.containers {
            let parent_id = container.id().as_str();
            for member in members(container) {
                let key = MemberKey::of(member);
                parents.entry(key).or_insert(parent_id);
                match key {
                    MemberKey::Feature(id) => {
                        feature_parents.entry(id).or_insert(parent_id);
                    }
                    MemberKey::Container(id) => {
                        container_parents.entry(id).or_insert(parent_id);
                    }
                }
            }
        }
        let mut roots = BTreeMap::new();
        for scope in &design.scopes {
            for root in &scope.roots {
                roots
                    .entry(root.as_str())
                    .or_insert(scope.definition.as_str());
            }
        }
        let mut scope_configurations: BTreeMap<&str, Vec<&DesignConfiguration>> = BTreeMap::new();
        for configuration in &design.configurations {
            scope_configurations
                .entry(configuration.scope.as_str())
                .or_default()
                .push(configuration);
        }
        let mut occurrence_owners: HashMap<&'a str, &'a str> = HashMap::new();
        for definition in &ir.model.product_definitions {
            let def_id = definition.id.as_str();
            for occurrence in &definition.occurrences {
                occurrence_owners
                    .entry(occurrence.as_str())
                    .or_insert(def_id);
            }
        }
        Self {
            ir,
            scopes: by_id(&design.scopes, |scope| scope.definition.as_str()),
            containers: by_id(&design.containers, |container| container.id().as_str()),
            features: by_id(&design.features, |feature| feature.id.as_str()),
            configurations: by_id(&design.configurations, |configuration| {
                configuration.id.as_str()
            }),
            arrangements: by_id(&design.arrangements, |arrangement| arrangement.id.as_str()),
            evaluations: by_id(&design.evaluations, |evaluation| evaluation.id.as_str()),
            parameters: by_id(&design.parameters, |parameter| parameter.id.as_str()),
            sketches: by_id(&design.sketches, |sketch| sketch.id.as_str()),
            spatial_sketches: by_id(&design.spatial_sketches, |sketch| sketch.id.as_str()),
            sketch_entities: by_id(&design.sketch_entities, |entity| entity.id.as_str()),
            spatial_sketch_entities: by_id(&design.spatial_sketch_entities, |entity| {
                entity.id.as_str()
            }),
            definitions: by_id(&ir.model.product_definitions, |definition| {
                definition.id.as_str()
            }),
            occurrences: by_id(&ir.model.occurrences, |occurrence| occurrence.id.as_str()),
            parents,
            feature_parents,
            container_parents,
            roots,
            scope_configurations,
            occurrence_owners,
            container_scopes: OnceCell::new(),
            several_scopes: OnceCell::new(),
            placed: OnceCell::new(),
            all_placed: OnceCell::new(),
            spanned: RefCell::new(HashMap::new()),
            empty_spanned: Rc::new(BTreeSet::new()),
            moved: OnceCell::new(),
            transformed_bodies: OnceCell::new(),
            evaluations_by_scope_position: OnceCell::new(),
            sketches_by_owner: OnceCell::new(),
            constraints_by_sketch: OnceCell::new(),
            entities_by_sketch: OnceCell::new(),
        }
    }

    /// The first product definition that lists `occurrence`.
    pub(in crate::validate) fn occurrence_owner(&self, occurrence: &str) -> Option<&'a str> {
        self.occurrence_owners.get(occurrence).copied()
    }

    /// The features that name each occurrence, in feature order, each with
    /// how it names it and whether it is a `Place` (§6.7).
    pub(in crate::validate) fn occurrence_namers(
        &self,
    ) -> BTreeMap<&'a str, Vec<(&'a str, Namer, bool)>> {
        let mut named: BTreeMap<&str, Vec<(&str, Namer, bool)>> = BTreeMap::new();
        for feature in &self.ir.design.features {
            let id = feature.id.as_str();
            let found: Vec<(&str, Namer, bool)> = match &feature.definition.operation {
                FeatureOperation::Combine(combine) => combine
                    .new_component
                    .iter()
                    .map(|new| (new.occurrence.as_str(), Namer::Combine, false))
                    .collect(),
                FeatureOperation::Place(place) => {
                    vec![(place.occurrence.as_str(), Namer::Placement, true)]
                }
                FeatureOperation::PlaceSet(set) => set
                    .occurrences
                    .iter()
                    .map(|occurrence| (occurrence.as_str(), Namer::Placement, false))
                    .collect(),
                FeatureOperation::ComponentPattern(pattern) => pattern
                    .instances
                    .iter()
                    .flat_map(|instance| &instance.occurrences)
                    .map(|occurrence| (occurrence.as_str(), Namer::Creator, false))
                    .collect(),
                FeatureOperation::MirrorComponents(mirror) => mirror
                    .results
                    .iter()
                    .map(|result| (result.occurrence.as_str(), Namer::Creator, false))
                    .collect(),
                _ => Vec::new(),
            };
            for (occurrence, namer, place) in found {
                named
                    .entry(occurrence)
                    .or_default()
                    .push((id, namer, place));
            }
        }
        named
    }

    /// Whether the member's record exists.
    pub(in crate::validate) fn member_exists(&self, member: MemberKey<'_>) -> bool {
        match member {
            MemberKey::Feature(id) => self.features.contains_key(id),
            MemberKey::Container(id) => self.containers.contains_key(id),
        }
    }

    fn container_scopes(&self) -> &HashMap<&'a str, (&'a str, usize)> {
        self.container_scopes.get_or_init(|| {
            let mut scopes: HashMap<&'a str, (&'a str, usize)> = HashMap::new();
            for (&root, &scope) in &self.roots {
                scopes.insert(root, (scope, 0));
            }
            let candidates: Vec<&'a str> = self
                .parents
                .keys()
                .filter_map(|key| match key {
                    MemberKey::Container(id) => Some(*id),
                    MemberKey::Feature(_) => None,
                })
                .collect();
            let max_steps = self.containers.len();
            for start in candidates {
                if scopes.contains_key(start) {
                    continue;
                }
                let mut container = start;
                for step in 0..=max_steps {
                    if let Some(&scope) = self.roots.get(container) {
                        scopes.insert(start, (scope, step));
                        break;
                    }
                    if let Some(&(scope, rem)) = scopes.get(container) {
                        if step + rem <= max_steps {
                            scopes.insert(start, (scope, step + rem));
                        }
                        break;
                    }
                    let Some(&parent) = self.parents.get(&MemberKey::Container(container)) else {
                        break;
                    };
                    container = parent;
                }
            }
            scopes
        })
    }

    /// The scope a member belongs to through the containers above it: the
    /// scope whose roots list the topmost container. `None` where the chain
    /// reaches no root or does not end.
    pub(in crate::validate) fn parent_of(&self, member: MemberKey<'_>) -> Option<&'a str> {
        match member {
            MemberKey::Feature(id) => self.feature_parents.get(id).copied(),
            MemberKey::Container(id) => self.container_parents.get(id).copied(),
        }
    }

    pub(in crate::validate) fn scope_of(&self, member: MemberKey<'_>) -> Option<&'a str> {
        let container = match member {
            MemberKey::Container(id) => id,
            MemberKey::Feature(id) => self.feature_parents.get(id).copied()?,
        };
        self.container_scopes()
            .get(container)
            .map(|&(scope, _)| scope)
    }

    /// The container a position lies in: the container itself for `Start`,
    /// and the container that lists the member for `After`.
    pub(in crate::validate) fn container_of(
        &self,
        position: PositionKey<'_>,
    ) -> Option<&'a Container> {
        let id = match position {
            PositionKey::Start(id) => id,
            PositionKey::After(MemberKey::Feature(id)) => self.feature_parents.get(id).copied()?,
            PositionKey::After(MemberKey::Container(id)) => {
                self.container_parents.get(id).copied()?
            }
        };
        self.containers.get(id).copied()
    }

    /// Whether `position` is `After(Container(c))` for an ordered container
    /// `c` whose rollback is unstated, the one spelling of the state at it
    /// (§5.3).
    pub(in crate::validate) fn at_unstated_rollback(&self, position: PositionKey<'_>) -> bool {
        matches!(
            position,
            PositionKey::After(MemberKey::Container(id))
                if matches!(
                    self.containers.get(id),
                    Some(Container::Ordered { rollback: None, .. })
                )
        )
    }

    /// The rollback position of an ordered container in its one spelling
    /// (§5.3): through the nearest feature member before it, else `Start` of
    /// the container; `After(Container(c))` where the rollback is unstated.
    /// `None` for an unordered container, or where `Before` names no member
    /// of it.
    pub(in crate::validate) fn rollback_position(
        container: &'a Container,
    ) -> Option<PositionKey<'a>> {
        let Container::Ordered {
            id,
            members,
            rollback,
            ..
        } = container
        else {
            return None;
        };
        let at = match rollback {
            None => return Some(PositionKey::After(MemberKey::Container(id.as_str()))),
            Some(Rollback::End) => members.len(),
            Some(Rollback::Before(member)) => members.iter().position(|listed| listed == member)?,
        };
        Some(entering_at(id.as_str(), &members[..at]))
    }

    /// The configuration and its ancestors through `parent`, nearest first,
    /// stopping before a configuration already listed.
    pub(in crate::validate) fn configuration_chain(
        &self,
        configuration: Option<&str>,
    ) -> Vec<&'a DesignConfiguration> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut next = configuration;
        while let Some(id) = next {
            let Some(configuration) = self.configurations.get(id).copied() else {
                break;
            };
            if !seen.insert(configuration.id.as_str()) {
                break;
            }
            chain.push(configuration);
            next = configuration.parent.as_ref().map(ConfigurationId::as_str);
        }
        chain
    }

    /// The definition an occurrence places.
    pub(in crate::validate) fn prototype(&self, occurrence: &str) -> Option<&'a str> {
        self.occurrences
            .get(occurrence)
            .map(|occurrence| occurrence.prototype.as_str())
    }

    /// Whether the scope has configurations.
    pub(in crate::validate) fn has_configurations(&self, scope: &str) -> bool {
        self.scope_configurations.contains_key(scope)
    }

    /// The overrides of `feature` that apply in `configuration` under
    /// `arrangement`, nearest first: the arrangement's, then those of the
    /// configuration and its parents (§5.1, §5.10, §5.11).
    pub(in crate::validate) fn feature_overrides<'f>(
        &'f self,
        feature: &FeatureId,
        configuration: Option<&str>,
        arrangement: Option<&'f Arrangement>,
    ) -> Vec<&'f FeatureStateOverride> {
        arrangement
            .map(|arrangement| &arrangement.overrides)
            .into_iter()
            .chain(
                self.configuration_chain(configuration)
                    .into_iter()
                    .map(|configuration| &configuration.overrides),
            )
            .filter_map(|overrides| overrides.features.get(feature))
            .collect()
    }

    /// The definition of `feature` that applies in `configuration` under
    /// `arrangement`: the nearest definition override, else the feature's
    /// own.
    pub(in crate::validate) fn definition_in<'f>(
        &'f self,
        feature: &'f Feature,
        configuration: Option<&str>,
        arrangement: Option<&'f Arrangement>,
    ) -> &'f FeatureDefinition {
        self.feature_overrides(&feature.id, configuration, arrangement)
            .into_iter()
            .find_map(|stated| stated.definition.as_ref())
            .unwrap_or(&feature.definition)
    }

    /// Every feature definition the design states, with the record that
    /// states it and the feature it defines: each feature's own, and each
    /// definition override of a configuration or an arrangement.
    pub(in crate::validate) fn feature_definitions(
        &self,
    ) -> Vec<(&'a str, &'a str, &'a FeatureDefinition)> {
        let design = &self.ir.design;
        let overrides = design
            .configurations
            .iter()
            .map(|configuration| (configuration.id.as_str(), &configuration.overrides))
            .chain(
                design
                    .arrangements
                    .iter()
                    .map(|arrangement| (arrangement.id.as_str(), &arrangement.overrides)),
            );
        design
            .features
            .iter()
            .map(|feature| {
                (
                    feature.id.as_str(),
                    feature.id.as_str(),
                    &feature.definition,
                )
            })
            .chain(overrides.flat_map(|(holder, overrides)| {
                overrides
                    .features
                    .iter()
                    .filter_map(move |(feature, state)| {
                        Some((holder, feature.as_str(), state.definition.as_ref()?))
                    })
            }))
            .collect()
    }

    fn holds_several_definitions(&self, scope: &str) -> bool {
        self.several_scopes
            .get_or_init(|| {
                let mut scopes = HashSet::new();
                for (&id, feature) in &self.features {
                    let Some(component) = feature.component.as_ref() else {
                        continue;
                    };
                    if let Some(scope) = self.scope_of(MemberKey::Feature(id)) {
                        if component.as_str() != scope {
                            scopes.insert(scope);
                        }
                    }
                }
                scopes
            })
            .contains(scope)
    }

    fn placed_prototypes(&self) -> &BTreeSet<&'a str> {
        self.placed.get_or_init(|| {
            self.occurrences
                .values()
                .map(|occurrence| occurrence.prototype.as_str())
                .collect()
        })
    }

    /// The scope's definition and the definitions it spans (§5.1).
    pub(in crate::validate) fn spanned_definitions(&self, scope: &str) -> Rc<BTreeSet<&'a str>> {
        if let Some(cached) = self.spanned.borrow().get(scope) {
            return Rc::clone(cached);
        }
        let Some((&scope, root)) = self.definitions.get_key_value(scope) else {
            return Rc::clone(&self.empty_spanned);
        };
        let mut spanned = BTreeSet::from([scope]);
        let several = self.holds_several_definitions(scope);
        let mut pending = vec![scope];
        if matches!(root.storage, Storage::File { .. }) {
            let key = scope.split_once('#').map(|(_, key)| key);
            let placed = self.placed_prototypes();
            for (&id, definition) in &self.definitions {
                let file_key = id
                    .split(':')
                    .nth(1)
                    .and_then(|scope| scope.split_once('@'))
                    .map(|(key, _)| key);
                let same_file = match file_key {
                    Some(file_key) => Some(file_key) == key,
                    None => root.is_root(),
                };
                if same_file
                    && matches!(definition.storage, Storage::Embedded)
                    && !self.scopes.contains_key(id)
                    && !placed.contains(id)
                    && spanned.insert(id)
                {
                    pending.push(id);
                }
            }
        }
        while let Some(definition) = pending.pop() {
            let owned = self
                .definitions
                .get(definition)
                .map_or(&[][..], |definition| &definition.occurrences[..]);
            for occurrence in owned {
                let Some((&prototype, placed)) = self
                    .prototype(occurrence.as_str())
                    .and_then(|prototype| self.definitions.get_key_value(prototype))
                else {
                    continue;
                };
                if (matches!(placed.storage, Storage::Embedded)
                    || (several && matches!(placed.storage, Storage::File { .. })))
                    && !self.scopes.contains_key(prototype)
                    && spanned.insert(prototype)
                {
                    pending.push(prototype);
                }
            }
        }
        let spanned = Rc::new(spanned);
        self.spanned.borrow_mut().insert(scope, Rc::clone(&spanned));
        spanned
    }

    /// The occurrences whose owner some state restates, whose membership the
    /// current listing does not decide.
    pub(in crate::validate) fn moved_occurrences(&self) -> &BTreeSet<&'a str> {
        self.moved.get_or_init(|| {
            self.ir
                .design
                .evaluations
                .iter()
                .flat_map(|evaluation| evaluation.content.iter())
                .flat_map(|content| content.delta.members.keys())
                .map(crate::ids::OccurrenceId::as_str)
                .collect()
        })
    }

    /// Whether any occurrence of the model places `definition`.
    pub(in crate::validate) fn is_placed(&self, definition: &str) -> bool {
        self.all_placed
            .get_or_init(|| {
                self.ir
                    .model
                    .occurrences
                    .iter()
                    .map(|occurrence| occurrence.prototype.as_str())
                    .collect()
            })
            .contains(definition)
    }

    /// Whether any body of `ir.geometry.bodies` with id `body` states a
    /// transform.
    pub(in crate::validate) fn has_transformed_body(&self, body: &str) -> bool {
        self.transformed_bodies
            .get_or_init(|| {
                self.ir
                    .geometry
                    .bodies
                    .iter()
                    .filter(|record| record.transform.is_some())
                    .map(|record| record.id.as_str())
                    .collect()
            })
            .contains(body)
    }

    /// Stored evaluation ids with content at `(scope, position)`, in arena
    /// order.
    pub(in crate::validate) fn state_evaluations(
        &self,
        scope: &str,
        position: PositionKey<'_>,
    ) -> &[&'a str] {
        let Some(by_pos) = self
            .evaluations_by_scope_position
            .get_or_init(|| {
                let mut map: HashMap<&'a str, ScopeEvaluations<'a>> = HashMap::new();
                for evaluation in &self.ir.design.evaluations {
                    if evaluation.content.is_some() {
                        if let Evaluated::State(of) = &evaluation.of {
                            let entry = map.entry(evaluation.scope.as_str()).or_default();
                            match PositionKey::of(of) {
                                PositionKey::Start(id) => entry.start.entry(id).or_default(),
                                PositionKey::After(MemberKey::Feature(id)) => {
                                    entry.after_feature.entry(id).or_default()
                                }
                                PositionKey::After(MemberKey::Container(id)) => {
                                    entry.after_container.entry(id).or_default()
                                }
                            }
                            .push(evaluation.id.as_str());
                        }
                    }
                }
                map
            })
            .get(scope)
        else {
            return &[];
        };
        let list = match position {
            PositionKey::Start(id) => by_pos.start.get(id),
            PositionKey::After(MemberKey::Feature(id)) => by_pos.after_feature.get(id),
            PositionKey::After(MemberKey::Container(id)) => by_pos.after_container.get(id),
        };
        list.map_or(&[], Vec::as_slice)
    }

    /// Planar sketches owned by `owner`, in arena order.
    pub(in crate::validate) fn owned_sketches(&self, owner: &str) -> &[&'a Sketch] {
        self.sketches_by_owner
            .get_or_init(|| {
                let mut map: HashMap<&'a str, Vec<&'a Sketch>> = HashMap::new();
                for sketch in &self.ir.design.sketches {
                    map.entry(sketch.owner.as_str()).or_default().push(sketch);
                }
                map
            })
            .get(owner)
            .map_or(&[][..], Vec::as_slice)
    }

    /// Planar sketch constraints of `sketch`, in arena order.
    pub(in crate::validate) fn sketch_constraints(
        &self,
        sketch: &str,
    ) -> &[&'a SketchConstraint<Planar>] {
        self.constraints_by_sketch
            .get_or_init(|| {
                let mut map: HashMap<&'a str, Vec<&'a SketchConstraint<Planar>>> = HashMap::new();
                for constraint in &self.ir.design.sketch_constraints {
                    map.entry(constraint.sketch.as_str())
                        .or_default()
                        .push(constraint);
                }
                map
            })
            .get(sketch)
            .map_or(&[][..], Vec::as_slice)
    }

    /// Planar sketch entities of `sketch`, in arena order.
    pub(in crate::validate) fn sketch_entities_of(
        &self,
        sketch: &str,
    ) -> &[&'a SketchEntity<Planar>] {
        self.entities_by_sketch
            .get_or_init(|| {
                let mut map: HashMap<&'a str, Vec<&'a SketchEntity<Planar>>> = HashMap::new();
                for entity in &self.ir.design.sketch_entities {
                    map.entry(entity.sketch.as_str()).or_default().push(entity);
                }
                map
            })
            .get(sketch)
            .map_or(&[][..], Vec::as_slice)
    }
}

impl CadIr {
    /// The definition `scope` and the definitions its design scope spans
    /// (§5.1): embedded definitions reached through occurrences and, for a
    /// file-stored scope, the unplaced embedded definitions of its file. A
    /// definition with a scope of its own is not spanned.
    #[must_use]
    pub fn spanned_definitions(&self, scope: &crate::ids::ProductDefinitionId) -> BTreeSet<&str> {
        let spanned = DesignRecords::new(self).spanned_definitions(scope.as_str());
        Rc::try_unwrap(spanned).unwrap_or_else(|rc| (*rc).clone())
    }

    /// The span of every design scope, keyed by the scope's definition, from
    /// one index of the document; each value is what `spanned_definitions`
    /// returns for that scope.
    #[must_use]
    pub fn spanned_definitions_of_scopes(&self) -> BTreeMap<String, BTreeSet<String>> {
        let records = DesignRecords::new(self);
        self.design
            .scopes
            .iter()
            .map(|scope| {
                let definition = scope.definition.as_str();
                let span = records
                    .spanned_definitions(definition)
                    .iter()
                    .map(|spanned| (*spanned).to_owned())
                    .collect();
                (definition.to_owned(), span)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
