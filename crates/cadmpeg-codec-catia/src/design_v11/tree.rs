// SPDX-License-Identifier: Apache-2.0
//! The part's feature tree as the projection states it: its bodies and
//! sets, the members of the part's own group, and the tree records no design
//! record can state.

use std::collections::{BTreeSet, HashMap};

use super::classes;
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
    /// Members the root lists outside every set, other than records that
    /// hold no operation.
    pub(super) group: Vec<Item<'a>>,
    /// The root's sets in list order, then the sets no read list holds.
    pub(super) sets: Vec<Set<'a>>,
    /// The entity the root's `CurrentFeature` selects: the in-work object.
    pub(super) current: Option<u32>,
    /// Tree records the projection does not state.
    pub(super) unstated: Vec<Unstated>,
    /// The sets among `sets` that no read list holds.
    pub(super) unlisted: Vec<String>,
}

/// A body or a geometrical set and its members in stored order.
pub(super) struct Set<'a> {
    pub(super) container: &'a CatiaFeatureContainer,
    pub(super) kind: SetKind,
    pub(super) members: Vec<Item<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SetKind {
    /// A `MechanicalTool`, or the `HybridBody` that `MainTool` selects.
    Body,
    /// Any other set: whether it is a body, an ordered geometrical set, or
    /// a geometrical set is unread.
    KindUnread,
}

/// One listed member, in stored order.
pub(super) enum Item<'a> {
    Feature {
        member: &'a CatiaFeatureMember,
        class: &'a str,
    },
    /// A member whose record the decoder cannot read; it keeps its place.
    Unread {
        member: &'a CatiaFeatureMember,
        reason: &'static str,
    },
    Set(Set<'a>),
}

impl Item<'_> {
    /// The entity identity of the item's record.
    pub(super) fn entity_id(&self) -> u32 {
        match self {
            Item::Feature { member, .. } | Item::Unread { member, .. } => member.entity_id,
            Item::Set(set) => set.container.entity_id,
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

/// The tree of the one part root in the modelling graphs: `Ok(None)` where
/// the graphs hold no feature-tree record, and `Err` with the reason where
/// they hold records but no tree is read.
pub(super) fn read<'a>(
    native: &'a CatiaNative,
    scope: &ModelingGraphScope,
) -> Result<Option<Tree<'a>>, &'static str> {
    let in_scope = native
        .feature_containers
        .iter()
        .filter(|container| scope.contains(&container.object_graph))
        .collect::<Vec<_>>();
    if in_scope.is_empty() {
        return Ok(None);
    }
    let mut roots = in_scope
        .iter()
        .filter(|container| container.class.name == "MechanicalPart");
    let (Some(root), None) = (roots.next().copied(), roots.next()) else {
        return Err("the modelling graphs store no part root or several");
    };
    let graph_offset = native
        .object_graphs
        .iter()
        .find(|graph| graph.id == root.object_graph)
        .ok_or("the part root's object graph is absent")?
        .byte_offset;
    let listed = member_list(root).ok_or("the part root's member list is unread")?;
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
        placed: BTreeSet::from([root.entity_id]),
        unstated: Vec::new(),
    };
    let mut group = Vec::new();
    let mut sets = Vec::new();
    for member in listed {
        match walk.item(member) {
            Some(Item::Set(set)) => sets.push(set),
            Some(Item::Feature { class, .. }) if classes::is_not_operation(class) => {}
            Some(item) => group.push(item),
            None => {}
        }
    }
    let listed_anywhere = containers
        .values()
        .filter_map(|container| member_list(container))
        .flatten()
        .map(|member| member.entity_id)
        .collect::<BTreeSet<_>>();
    let mut unlisted_sets = containers
        .values()
        .copied()
        .filter(|container| {
            !walk.placed.contains(&container.entity_id)
                && !listed_anywhere.contains(&container.entity_id)
        })
        .collect::<Vec<_>>();
    unlisted_sets.sort_by(|a, b| a.object_record.cmp(&b.object_record));
    let mut unlisted = Vec::new();
    for container in unlisted_sets {
        walk.placed.insert(container.entity_id);
        match walk.set(container) {
            Some(set) => {
                unlisted.push(format!(
                    "{} ({})",
                    container.object_record, container.class.name
                ));
                sets.push(set);
            }
            None => walk.unstated.push(Unstated {
                record: container.object_record.clone(),
                reason: "no set lists it, and its member list is unread",
            }),
        }
    }
    for container in containers.values() {
        if !walk.placed.contains(&container.entity_id) {
            walk.unstated.push(Unstated {
                record: container.object_record.clone(),
                reason: "no set lists it",
            });
        }
    }
    walk.unstated.sort_by(|a, b| a.record.cmp(&b.record));
    Ok(Some(Tree {
        graph_offset,
        root,
        group,
        sets,
        current: token(root, "CurrentFeature").map(|reference| reference.entity_id),
        unstated: walk.unstated,
        unlisted,
    }))
}

struct Walk<'a, 'm> {
    containers: &'m HashMap<&'a str, &'a CatiaFeatureContainer>,
    main: Option<&'a str>,
    /// The entities of the members walked so far.
    placed: BTreeSet<u32>,
    unstated: Vec<Unstated>,
}

impl<'a> Walk<'a, '_> {
    /// The item for one listed member, or `None` where the member repeats an
    /// earlier one.
    fn item(&mut self, member: &'a CatiaFeatureMember) -> Option<Item<'a>> {
        if !self.placed.insert(member.entity_id) {
            self.unstated.push(Unstated {
                record: format!("entity {}", member.entity_id),
                reason: "is listed more than once",
            });
            return None;
        }
        let unread = |reason| Some(Item::Unread { member, reason });
        let Some(target) = member.target.as_ref() else {
            return unread("selects no record");
        };
        let record = target.object_record.as_str();
        let Some(container) = self.containers.get(record).copied() else {
            return match target.class.as_ref() {
                None => unread("has no compact class"),
                Some(class) if class.name.trim().is_empty() => {
                    unread("selects a blank catalog entry")
                }
                Some(class) => Some(Item::Feature {
                    member,
                    class: class.name.as_str(),
                }),
            };
        };
        match self.set(container) {
            Some(set) => Some(Item::Set(set)),
            None => unread("is a set whose member list is unread"),
        }
    }

    /// The set a container states, or `None` where its member list is unread.
    fn set(&mut self, container: &'a CatiaFeatureContainer) -> Option<Set<'a>> {
        let listed = member_list(container)?;
        let kind = if container.class.name == ORDERED_BODY_CLASS
            || (container.class.name == MAIN_BODY_CLASS
                && self.main == Some(container.object_record.as_str()))
        {
            SetKind::Body
        } else {
            SetKind::KindUnread
        };
        let members = listed
            .into_iter()
            .filter_map(|member| self.item(member))
            .collect();
        Some(Set {
            container,
            kind,
            members,
        })
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
