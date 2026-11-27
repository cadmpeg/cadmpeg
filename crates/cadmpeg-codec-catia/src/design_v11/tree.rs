// SPDX-License-Identifier: Apache-2.0
//! The part's feature tree as the projection states it: the bodies whose
//! kind a stored fact fixes, the members of the part's own group, and the
//! tree records no design record can state.

use std::collections::{BTreeSet, HashMap};

use crate::decode::ModelingGraphScope;
use crate::native::feature_tree::{
    CatiaFeatureContainer, CatiaFeatureMember, CatiaFeatureReference, CatiaSegmentValue,
};
use crate::native::CatiaNative;

/// The tree of one part root.
pub(super) struct Tree<'a> {
    /// Byte offset of the graph that holds the root, the key of its identities.
    pub(super) graph_offset: u64,
    /// The part root.
    pub(super) root: &'a CatiaFeatureContainer,
    /// Features the root lists outside every set.
    pub(super) group: Vec<Item<'a>>,
    /// The root's bodies, in list order.
    pub(super) bodies: Vec<Body<'a>>,
    /// The entity the root's `CurrentFeature` selects: the in-work object.
    pub(super) current: Option<u32>,
    /// Tree records the projection does not state.
    pub(super) unstated: Vec<Unstated>,
}

/// A body: an ordered container that `MainTool` selects or whose class is
/// `MechanicalTool`.
pub(super) struct Body<'a> {
    pub(super) container: &'a CatiaFeatureContainer,
    pub(super) members: Vec<Item<'a>>,
}

/// One stated member, in stored order.
pub(super) enum Item<'a> {
    Feature {
        member: &'a CatiaFeatureMember,
        class: &'a str,
    },
    Body(Body<'a>),
}

impl Item<'_> {
    /// The entity identity of the item's record.
    pub(super) fn entity_id(&self) -> u32 {
        match self {
            Item::Feature { member, .. } => member.entity_id,
            Item::Body(body) => body.container.entity_id,
        }
    }
}

/// A tree record the projection does not state, and why.
pub(super) struct Unstated {
    pub(super) record: String,
    pub(super) reason: &'static str,
}

const ORDERED_BODY_CLASS: &str = "MechanicalTool";
const MAIN_BODY_CLASS: &str = "HybridBody";

/// The tree of the one part root in the modelling graphs, or `None` where
/// the decoder reads no root list.
pub(super) fn read<'a>(native: &'a CatiaNative, scope: &ModelingGraphScope) -> Option<Tree<'a>> {
    let mut roots = native.feature_containers.iter().filter(|container| {
        container.class.name == "MechanicalPart" && scope.contains(&container.object_graph)
    });
    let root = roots.next()?;
    if roots.next().is_some() {
        return None;
    }
    let graph_offset = native
        .object_graphs
        .iter()
        .find(|graph| graph.id == root.object_graph)?
        .byte_offset;
    let listed = member_list(root)?;
    let containers = native
        .feature_containers
        .iter()
        .filter(|container| {
            container.object_graph == root.object_graph
                && container.object_record != root.object_record
        })
        .map(|container| (container.object_record.as_str(), container))
        .collect::<HashMap<_, _>>();
    let main = token(root, "MainTool")
        .and_then(|reference| reference.target.as_ref())
        .map(|target| target.object_record.as_str());
    let mut walk = Walk {
        containers: &containers,
        main,
        placed: BTreeSet::new(),
        unstated: Vec::new(),
    };
    walk.placed.insert(root.object_record.as_str());
    let mut group = Vec::new();
    let mut bodies = Vec::new();
    for member in listed {
        match walk.item(member) {
            Some(Item::Body(body)) => bodies.push(body),
            Some(feature) => group.push(feature),
            None => {}
        }
    }
    for container in containers.values() {
        if !walk.placed.contains(container.object_record.as_str()) {
            walk.unstated.push(Unstated {
                record: container.object_record.clone(),
                reason: "no set lists it",
            });
        }
    }
    walk.unstated.sort_by(|a, b| a.record.cmp(&b.record));
    Some(Tree {
        graph_offset,
        root,
        group,
        bodies,
        current: token(root, "CurrentFeature").map(|reference| reference.entity_id),
        unstated: walk.unstated,
    })
}

struct Walk<'a, 'm> {
    containers: &'m HashMap<&'a str, &'a CatiaFeatureContainer>,
    main: Option<&'a str>,
    placed: BTreeSet<&'a str>,
    unstated: Vec<Unstated>,
}

impl<'a> Walk<'a, '_> {
    /// The stated item for one listed member: a feature, a body, or `None`
    /// where the member is not stated.
    fn item(&mut self, member: &'a CatiaFeatureMember) -> Option<Item<'a>> {
        let Some(target) = member.target.as_ref() else {
            self.unstated.push(Unstated {
                record: format!("entity {}", member.entity_id),
                reason: "selects no record",
            });
            return None;
        };
        let record = target.object_record.as_str();
        if !self.placed.insert(record) {
            self.unstated.push(Unstated {
                record: record.to_owned(),
                reason: "is listed more than once",
            });
            return None;
        }
        let Some(container) = self.containers.get(record).copied() else {
            let Some(class) = target.class.as_ref() else {
                self.unstated.push(Unstated {
                    record: record.to_owned(),
                    reason: "has no compact class",
                });
                return None;
            };
            return Some(Item::Feature {
                member,
                class: class.name.as_str(),
            });
        };
        let is_body = container.class.name == ORDERED_BODY_CLASS
            || (container.class.name == MAIN_BODY_CLASS && self.main == Some(record));
        if !is_body {
            self.unstated.push(Unstated {
                record: record.to_owned(),
                reason: "is a set whose ordered or unordered kind is unread",
            });
            return None;
        }
        let Some(listed) = member_list(container) else {
            self.unstated.push(Unstated {
                record: record.to_owned(),
                reason: "has an unread member list",
            });
            return None;
        };
        let members = listed
            .into_iter()
            .filter_map(|member| self.item(member))
            .collect();
        Some(Item::Body(Body { container, members }))
    }
}

/// The container's `_FtrList` members, an empty list for a complete walk
/// that stores no segment, or `None` where the list is unread.
fn member_list(container: &CatiaFeatureContainer) -> Option<Vec<&CatiaFeatureMember>> {
    let mut lists = container
        .segments
        .iter()
        .filter(|segment| segment.name == "_FtrList");
    match (lists.next(), lists.next()) {
        (Some(segment), None) => match &segment.value {
            CatiaSegmentValue::List(members) => Some(members.iter().collect()),
            CatiaSegmentValue::Token(_) => None,
        },
        (None, _)
            if container.segments.is_empty() && container.unsegmented_payload_offset.is_none() =>
        {
            Some(Vec::new())
        }
        _ => None,
    }
}

/// The value of the container's one single-token segment `name`.
fn token<'a>(
    container: &'a CatiaFeatureContainer,
    name: &str,
) -> Option<&'a CatiaFeatureReference> {
    let mut found = container
        .segments
        .iter()
        .filter(|segment| segment.name == name);
    match (found.next(), found.next()) {
        (Some(segment), None) => match &segment.value {
            CatiaSegmentValue::Token(reference) => Some(reference),
            CatiaSegmentValue::List(_) => None,
        },
        _ => None,
    }
}
