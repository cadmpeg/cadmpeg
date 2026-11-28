// SPDX-License-Identifier: Apache-2.0
//! The part's feature tree as the projection states it: its bodies and
//! sets, the members of the part's own group, and the tree records no design
//! record can state.

use std::collections::{hash_map::Entry, BTreeSet, HashMap};

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
    /// Members the root lists outside every set.
    pub(super) group: Vec<Item<'a>>,
    /// The root's sets in list order, then the sets no read list holds.
    pub(super) sets: Vec<Set<'a>>,
    /// The entity the root's `CurrentFeature` selects: the in-work object.
    pub(super) current: Evidence<u32>,
    /// Tree records the projection does not state.
    pub(super) unstated: Vec<Unstated>,
    /// The sets among `sets` that no read list holds.
    pub(super) unlisted: Vec<String>,
    pub(super) unread_lists: Vec<&'a str>,
    /// The root body in each Boolean feature's tool slot, by Boolean entity.
    pub(super) tools: BooleanTools<'a>,
}

pub(super) type BooleanTools<'a> = HashMap<u32, Result<&'a CatiaFeatureContainer, &'static str>>;

/// A body or a geometrical set and its members in stored order.
pub(super) struct Set<'a> {
    pub(super) container: &'a CatiaFeatureContainer,
    pub(super) kind: SetKind,
    pub(super) members: Vec<Item<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SetKind {
    /// A `MechanicalTool`, the `HybridBody` that `MainTool` selects, or a
    /// `HybridBody` that lists a feature only a body holds.
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
    pub(super) reason: String,
}

const ORDERED_BODY_CLASS: &str = "MechanicalTool";
const MAIN_BODY_CLASS: &str = "HybridBody";
const SET_CLASSES: [&str; 3] = [ORDERED_BODY_CLASS, MAIN_BODY_CLASS, "GSMTool"];

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
    let containers = native
        .feature_containers
        .iter()
        .filter(|container| {
            container.object_graph == root.object_graph
                && SET_CLASSES.contains(&container.class.name.as_str())
        })
        .map(|container| (container.object_record.as_str(), container))
        .collect::<HashMap<_, _>>();
    let main = RecordEvidence(root)
        .token("MainTool")
        .value()
        .and_then(|reference| reference.target.as_ref())
        .map(|target| target.object_record.as_str());
    let mut walk = Walk {
        containers: &containers,
        main,
        placed: HashMap::from([(root.entity_id, root.entity_id)]),
        unstated: Vec::new(),
        unread_lists: Vec::new(),
    };
    let mut group = Vec::new();
    let mut sets = Vec::new();
    for (index, member) in walk.members(root).iter().enumerate() {
        match walk.item(root, index, member) {
            Some(Item::Set(set)) => sets.push(set),
            Some(item) => group.push(item),
            None => {}
        }
    }
    let listed_anywhere = containers
        .values()
        .filter_map(|container| RecordEvidence(container).members().value())
        .flatten()
        .map(|member| member.entity_id)
        .collect::<BTreeSet<_>>();
    let mut unlisted_sets = containers
        .values()
        .copied()
        .filter(|container| {
            !walk.placed.contains_key(&container.entity_id)
                && !listed_anywhere.contains(&container.entity_id)
        })
        .collect::<Vec<_>>();
    unlisted_sets.sort_by(|a, b| a.object_record.cmp(&b.object_record));
    let first_unlisted = sets.len();
    for container in unlisted_sets {
        walk.placed.insert(container.entity_id, root.entity_id);
        sets.push(walk.set(container));
    }
    for container in containers.values() {
        if !walk.placed.contains_key(&container.entity_id) {
            walk.unstated.push(Unstated {
                record: container.object_record.clone(),
                reason: "unreachable from every stated root".to_owned(),
            });
        }
    }
    walk.unstated.sort_by(|a, b| a.record.cmp(&b.record));
    let root_bodies = sets
        .iter()
        .filter(|set| set.kind == SetKind::Body)
        .map(|set| (set.container.object_record.as_str(), set.container))
        .collect::<HashMap<_, _>>();
    let mut holders = HashMap::<_, Vec<String>>::new();
    let mut tools = HashMap::new();
    for boolean in in_scope {
        if boolean.object_graph != root.object_graph
            || classes::boolean_op(&boolean.class.name).is_none()
        {
            continue;
        }
        let owner = walk.placed.get(&boolean.entity_id).copied();
        let tool = RecordEvidence(boolean).boolean_tool(&root_bodies, owner);
        if let Ok(tool) = tool {
            holders.entry(tool.entity_id).or_default().push(format!(
                ", the tool of {} ({})",
                boolean.object_record, boolean.class.name
            ));
        }
        tools.insert(boolean.entity_id, tool);
    }
    let unlisted = sets[first_unlisted..]
        .iter()
        .map(|set| {
            let container = set.container;
            let holders = holders
                .remove(&container.entity_id)
                .unwrap_or_default()
                .concat();
            format!(
                "{} ({}{holders})",
                container.object_record, container.class.name
            )
        })
        .collect();
    Ok(Some(Tree {
        graph_offset,
        root,
        group,
        sets,
        current: RecordEvidence(root)
            .token("CurrentFeature")
            .map(|reference| reference.entity_id),
        unstated: walk.unstated,
        unread_lists: walk.unread_lists,
        unlisted,
        tools,
    }))
}

struct Walk<'a, 'm> {
    containers: &'m HashMap<&'a str, &'a CatiaFeatureContainer>,
    main: Option<&'a str>,
    /// The first parent of each placed entity.
    placed: HashMap<u32, u32>,
    unstated: Vec<Unstated>,
    unread_lists: Vec<&'a str>,
}

impl<'a> Walk<'a, '_> {
    fn members(&mut self, container: &'a CatiaFeatureContainer) -> &'a [CatiaFeatureMember] {
        match RecordEvidence(container).members() {
            Evidence::Read(members) => members,
            Evidence::Absent => &[],
            Evidence::Unread => {
                self.unread_lists.push(&container.object_record);
                &[]
            }
        }
    }

    fn item(
        &mut self,
        parent: &CatiaFeatureContainer,
        index: usize,
        member: &'a CatiaFeatureMember,
    ) -> Option<Item<'a>> {
        let Entry::Vacant(placement) = self.placed.entry(member.entity_id) else {
            self.unstated.push(Unstated {
                record: parent.object_record.clone(),
                reason: format!(
                    "membership at position {} for entity {} repeats an earlier placement",
                    index + 1,
                    member.entity_id
                ),
            });
            return None;
        };
        placement.insert(parent.entity_id);
        let unread = |reason| Some(Item::Unread { member, reason });
        let Some(target) = member.target.as_ref() else {
            return unread("selects no record");
        };
        let record = target.object_record.as_str();
        let Some(container) = self.containers.get(record).copied() else {
            return match target.class.as_ref() {
                None => unread("has no compact class"),
                Some(class) => match classes::read(&class.name) {
                    Some(class) => Some(Item::Feature { member, class }),
                    None => unread("selects a blank catalog entry"),
                },
            };
        };
        Some(Item::Set(self.set(container)))
    }

    fn set(&mut self, container: &'a CatiaFeatureContainer) -> Set<'a> {
        let listed = self.members(container);
        let kind = if container.class.name == ORDERED_BODY_CLASS
            || (container.class.name == MAIN_BODY_CLASS
                && (self.main == Some(container.object_record.as_str())
                    || listed.iter().any(|member| {
                        member
                            .target
                            .as_ref()
                            .and_then(|target| target.class.as_ref())
                            .is_some_and(|class| {
                                classes::BODY_FEATURES.contains(&class.name.as_str())
                                    || classes::boolean_op(&class.name).is_some()
                            })
                    })))
        {
            SetKind::Body
        } else {
            SetKind::KindUnread
        };
        let members = listed
            .iter()
            .enumerate()
            .filter_map(|(index, member)| self.item(container, index, member))
            .collect();
        Set {
            container,
            kind,
            members,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Evidence<T> {
    Absent,
    Read(T),
    Unread,
}

impl<T> Evidence<T> {
    fn map<U>(self, project: impl FnOnce(T) -> U) -> Evidence<U> {
        match self {
            Self::Absent => Evidence::Absent,
            Self::Read(value) => Evidence::Read(project(value)),
            Self::Unread => Evidence::Unread,
        }
    }

    pub(super) fn value(self) -> Option<T> {
        match self {
            Self::Read(value) => Some(value),
            Self::Absent | Self::Unread => None,
        }
    }
}

struct RecordEvidence<'a>(&'a CatiaFeatureContainer);

impl<'a> RecordEvidence<'a> {
    fn field<T>(
        &self,
        name: &str,
        read: impl FnOnce(&'a CatiaSegmentValue) -> Option<T>,
    ) -> Evidence<T> {
        let mut matches = self
            .0
            .segments
            .iter()
            .filter(|segment| segment.name == name);
        match (matches.next(), matches.next()) {
            (Some(segment), None) => read(&segment.value).map_or(Evidence::Unread, Evidence::Read),
            (None, _) if self.0.unsegmented_payload_offset.is_none() => Evidence::Absent,
            _ => Evidence::Unread,
        }
    }

    fn list(&self, name: &str) -> Evidence<&'a [CatiaFeatureMember]> {
        self.field(name, |value| match value {
            CatiaSegmentValue::List(members) => Some(members.as_slice()),
            _ => None,
        })
    }

    fn members(&self) -> Evidence<&'a [CatiaFeatureMember]> {
        self.list("_FtrList")
    }

    fn token(&self, name: &str) -> Evidence<&'a CatiaFeatureReference> {
        self.field(name, |value| match value {
            CatiaSegmentValue::Token(reference) => Some(reference),
            _ => None,
        })
    }

    fn boolean_tool(
        &self,
        root_bodies: &HashMap<&str, &'a CatiaFeatureContainer>,
        owner: Option<u32>,
    ) -> Result<&'a CatiaFeatureContainer, &'static str> {
        let body = |member: &CatiaFeatureMember| {
            let target = member.target.as_ref()?;
            let body = *root_bodies.get(target.object_record.as_str())?;
            (Some(body.entity_id) != owner).then_some(body)
        };
        let feature_tool = self.members().value().and_then(|list| match list {
            [_, tool] => body(tool),
            _ => None,
        });
        let explicit_tool = self.list("ToolList").value().and_then(|list| match list {
            [tool] => body(tool),
            _ => None,
        });
        match (feature_tool, explicit_tool) {
            (Some(a), Some(b)) if a.entity_id != b.entity_id => {
                Err("_FtrList and ToolList select different bodies")
            }
            (Some(tool), _) | (_, Some(tool)) => Ok(tool),
            (None, None) => {
                Err("neither _FtrList nor ToolList selects an eligible ordered root body")
            }
        }
    }
}
