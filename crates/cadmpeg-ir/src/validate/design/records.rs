// SPDX-License-Identifier: Apache-2.0
//! Lookups over the design layer that the part 10 rules share.
//!
//! Every lookup keeps the first record of an identity; a repeated identity is
//! reported by the scope-and-linkage rules and does not stop the others.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::design::configuration::{Arrangement, DesignConfiguration};
use crate::design::container::{Container, Rollback};
use crate::design::evaluation::Evaluation;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::parameter::DesignParameter;
use crate::design::refs::{Evaluated, Member, Position};
use crate::design::scope::DesignScope;
use crate::document::CadIr;
use crate::features::ConfigurationId;
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
    pub(in crate::validate) definitions: BTreeMap<&'a str, &'a ProductDefinition>,
    pub(in crate::validate) occurrences: BTreeMap<&'a str, &'a Occurrence>,
    /// The container that lists each member.
    pub(in crate::validate) parents: BTreeMap<MemberKey<'a>, &'a str>,
    /// The scope whose roots list each root container.
    pub(in crate::validate) roots: BTreeMap<&'a str, &'a str>,
    /// The configurations of each scope.
    pub(in crate::validate) scope_configurations: BTreeMap<&'a str, Vec<&'a DesignConfiguration>>,
    /// The first product definition that lists each occurrence id.
    occurrence_owners: HashMap<&'a str, &'a str>,
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
        for container in &design.containers {
            for member in members(container) {
                parents
                    .entry(MemberKey::of(member))
                    .or_insert(container.id().as_str());
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
            definitions: by_id(&ir.model.product_definitions, |definition| {
                definition.id.as_str()
            }),
            occurrences: by_id(&ir.model.occurrences, |occurrence| occurrence.id.as_str()),
            parents,
            roots,
            scope_configurations,
            occurrence_owners,
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

    /// The scope a member belongs to through the containers above it: the
    /// scope whose roots list the topmost container. `None` where the chain
    /// reaches no root or does not end.
    pub(in crate::validate) fn scope_of(&self, member: MemberKey<'a>) -> Option<&'a str> {
        let mut container = match member {
            MemberKey::Container(id) => id,
            MemberKey::Feature(_) => self.parents.get(&member).copied()?,
        };
        for _ in 0..=self.containers.len() {
            if let Some(scope) = self.roots.get(container) {
                return Some(scope);
            }
            container = self
                .parents
                .get(&MemberKey::Container(container))
                .copied()?;
        }
        None
    }

    /// The container a position lies in: the container itself for `Start`,
    /// and the container that lists the member for `After`.
    pub(in crate::validate) fn container_of(
        &self,
        position: PositionKey<'a>,
    ) -> Option<&'a Container> {
        let id = match position {
            PositionKey::Start(id) => id,
            PositionKey::After(member) => self.parents.get(&member).copied()?,
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

    /// The definition of `feature` that applies in `configuration` (§5.10,
    /// §5.11): the nearest definition override on the configuration's chain,
    /// else the feature's own.
    pub(in crate::validate) fn definition_in(
        &self,
        feature: &'a Feature,
        configuration: Option<&str>,
    ) -> &'a FeatureDefinition {
        self.configuration_chain(configuration)
            .into_iter()
            .find_map(|configuration| {
                configuration
                    .overrides
                    .features
                    .get(&feature.id)?
                    .definition
                    .as_ref()
            })
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
        self.features.iter().any(|(&id, feature)| {
            feature
                .component
                .as_ref()
                .is_some_and(|component| component.as_str() != scope)
                && self.scope_of(MemberKey::Feature(id)) == Some(scope)
        })
    }

    /// The scope's definition and the definitions it spans (§5.1).
    pub(in crate::validate) fn spanned_definitions(&self, scope: &str) -> BTreeSet<&'a str> {
        let Some((&scope, root)) = self.definitions.get_key_value(scope) else {
            return BTreeSet::new();
        };
        let mut spanned = BTreeSet::from([scope]);
        let several = self.holds_several_definitions(scope);
        let mut pending = vec![scope];
        if matches!(root.storage, Storage::File { .. }) {
            let key = scope.split_once('#').map(|(_, key)| key);
            let placed: BTreeSet<&str> = self
                .occurrences
                .values()
                .map(|occurrence| occurrence.prototype.as_str())
                .collect();
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
        spanned
    }
}

impl CadIr {
    /// The definition `scope` and the definitions its design scope spans
    /// (§5.1): embedded definitions reached through occurrences and, for a
    /// file-stored scope, the unplaced embedded definitions of its file. A
    /// definition with a scope of its own is not spanned.
    #[must_use]
    pub fn spanned_definitions(&self, scope: &crate::ids::ProductDefinitionId) -> BTreeSet<&str> {
        DesignRecords::new(self).spanned_definitions(scope.as_str())
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
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                (definition.to_owned(), span)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
