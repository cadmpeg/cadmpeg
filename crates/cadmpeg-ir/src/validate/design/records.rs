// SPDX-License-Identifier: Apache-2.0
//! Lookups over the design layer that the part 10 rules share.
//!
//! Every lookup keeps the first record of an identity; a repeated identity is
//! reported by the scope-and-linkage rules and does not stop the others.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::design::configuration::{Arrangement, DesignConfiguration};
use crate::design::container::{Container, Rollback};
use crate::design::evaluation::Evaluation;
use crate::design::feature::Feature;
use crate::design::parameter::DesignParameter;
use crate::design::refs::{Evaluated, Member, Position};
use crate::design::scope::DesignScope;
use crate::document::CadIr;
use crate::features::ConfigurationId;
use crate::products::{Occurrence, ProductDefinition, PrototypeReference};

/// A member of a container, by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum MemberKey<'a> {
    /// A feature.
    Feature(&'a str),
    /// A nested container.
    Container(&'a str),
}

impl<'a> MemberKey<'a> {
    pub(super) fn of(member: &'a Member) -> Self {
        match member {
            Member::Feature(id) => Self::Feature(id.as_str()),
            Member::Container(id) => Self::Container(id.as_str()),
        }
    }

    pub(super) const fn id(self) -> &'a str {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PositionKey<'a> {
    /// The start of an ordered container.
    Start(&'a str),
    /// The position after a member.
    After(MemberKey<'a>),
}

impl<'a> PositionKey<'a> {
    pub(super) fn of(position: &'a Position) -> Self {
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
pub(super) enum OfKey<'a> {
    /// The state at a position.
    State(PositionKey<'a>),
    /// The tool body of a feature.
    Tool(&'a str),
}

impl<'a> OfKey<'a> {
    pub(super) fn of(of: &'a Evaluated) -> Self {
        match of {
            Evaluated::State(position) => Self::State(PositionKey::of(position)),
            Evaluated::Tool(feature) => Self::Tool(feature.as_str()),
        }
    }
}

/// The design records of one document, by identity.
pub(super) struct DesignRecords<'a> {
    pub(super) ir: &'a CadIr,
    /// Each scope, by the definition whose design it is.
    pub(super) scopes: BTreeMap<&'a str, &'a DesignScope>,
    pub(super) containers: BTreeMap<&'a str, &'a Container>,
    pub(super) features: BTreeMap<&'a str, &'a Feature>,
    pub(super) configurations: BTreeMap<&'a str, &'a DesignConfiguration>,
    pub(super) arrangements: BTreeMap<&'a str, &'a Arrangement>,
    pub(super) evaluations: BTreeMap<&'a str, &'a Evaluation>,
    pub(super) parameters: BTreeMap<&'a str, &'a DesignParameter>,
    pub(super) definitions: BTreeMap<&'a str, &'a ProductDefinition>,
    pub(super) occurrences: BTreeMap<&'a str, &'a Occurrence>,
    /// The container that lists each member.
    pub(super) parents: BTreeMap<MemberKey<'a>, &'a str>,
    /// The scope whose roots list each root container.
    pub(super) roots: BTreeMap<&'a str, &'a str>,
    /// The configurations of each scope.
    pub(super) scope_configurations: BTreeMap<&'a str, Vec<&'a DesignConfiguration>>,
}

/// A container's members: in order for an ordered container.
pub(super) fn members(container: &Container) -> Vec<&Member> {
    match container {
        Container::Ordered { members, .. } => members.iter().collect(),
        Container::Unordered { members, .. } => members.iter().collect(),
    }
}

fn by_id<'a, T>(records: &'a [T], id: impl Fn(&'a T) -> &'a str) -> BTreeMap<&'a str, &'a T> {
    let mut index = BTreeMap::new();
    for record in records {
        index.entry(id(record)).or_insert(record);
    }
    index
}

impl<'a> DesignRecords<'a> {
    pub(super) fn new(ir: &'a CadIr) -> Self {
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
        }
    }

    /// Whether the member's record exists.
    pub(super) fn member_exists(&self, member: MemberKey<'_>) -> bool {
        match member {
            MemberKey::Feature(id) => self.features.contains_key(id),
            MemberKey::Container(id) => self.containers.contains_key(id),
        }
    }

    /// The scope a member belongs to through the containers above it: the
    /// scope whose roots list the topmost container. `None` where the chain
    /// reaches no root or does not end.
    pub(super) fn scope_of(&self, member: MemberKey<'a>) -> Option<&'a str> {
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
    pub(super) fn container_of(&self, position: PositionKey<'a>) -> Option<&'a Container> {
        let id = match position {
            PositionKey::Start(id) => id,
            PositionKey::After(member) => self.parents.get(&member).copied()?,
        };
        self.containers.get(id).copied()
    }

    /// The rollback position of an ordered container, spelled through its
    /// member. `None` for an unordered container, or where `Before` names no
    /// member of it.
    pub(super) fn rollback_position(container: &'a Container) -> Option<PositionKey<'a>> {
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
            Rollback::End => members.len(),
            Rollback::Before(member) => members.iter().position(|listed| listed == member)?,
        };
        Some(match at.checked_sub(1) {
            Some(previous) => PositionKey::After(MemberKey::of(&members[previous])),
            None => PositionKey::Start(id.as_str()),
        })
    }

    /// The configuration and its ancestors through `parent`, nearest first,
    /// stopping before a configuration already listed.
    pub(super) fn configuration_chain(
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

    /// The definition an occurrence places, where it is a definition of this
    /// document.
    pub(super) fn prototype(&self, occurrence: &str) -> Option<&'a str> {
        match &self.occurrences.get(occurrence)?.prototype {
            PrototypeReference::Local { definition } => Some(definition.as_str()),
            PrototypeReference::External { .. } | PrototypeReference::Unresolved {} => None,
        }
    }

    /// Whether the scope has configurations.
    pub(super) fn has_configurations(&self, scope: &str) -> bool {
        self.scope_configurations.contains_key(scope)
    }
}
