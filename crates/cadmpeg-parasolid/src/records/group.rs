// SPDX-License-Identifier: Apache-2.0
//! GROUP and `MEMBER_OF_FEATURE` membership records of a primary node sequence.

use crate::xmt::NonNullXmt;
use serde::{Deserialize, Serialize};

/// One membership-relevant node of a primary node sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MembershipRecord {
    /// Stream-local node index.
    pub xmt: NonNullXmt,
    /// Membership role of the node.
    pub kind: MembershipKind,
}

/// Membership role of one node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipKind {
    /// A type-90 GROUP node.
    Group {
        /// GROUP node ID.
        node_id: u32,
        /// Entity class the GROUP collects.
        selector: GroupSelector,
        /// First `MEMBER_OF_FEATURE` pointer.
        first_member: u32,
    },
    /// A type-91 `MEMBER_OF_FEATURE` node.
    Member {
        /// Owning GROUP pointer.
        owning_group: u32,
        /// Member entity pointer.
        owner: u32,
        /// Next member pointer.
        next_member: u32,
        /// Previous member pointer.
        previous_member: u32,
    },
    /// A node that a member can name.
    Node {
        /// Node type.
        kind: u16,
        /// Node ID.
        node_id: u32,
    },
    /// A node outside the membership roles.
    Other,
}

/// Entity class code `1..=10` of a GROUP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum GroupSelector {
    /// 1: instances.
    Instance,
    /// 2: faces.
    Face,
    /// 3: loops.
    Loop,
    /// 4: edges.
    Edge,
    /// 5: vertices.
    Vertex,
    /// 6: surfaces.
    Surface,
    /// 7: curves.
    Curve,
    /// 8: points.
    Point,
    /// 9: mixed entity classes.
    Mixed,
    /// 10: regions.
    Region,
}

impl TryFrom<u8> for GroupSelector {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::Instance,
            2 => Self::Face,
            3 => Self::Loop,
            4 => Self::Edge,
            5 => Self::Vertex,
            6 => Self::Surface,
            7 => Self::Curve,
            8 => Self::Point,
            9 => Self::Mixed,
            10 => Self::Region,
            _ => return Err("selector: must be in 1..=10"),
        })
    }
}

impl From<GroupSelector> for u8 {
    fn from(value: GroupSelector) -> Self {
        match value {
            GroupSelector::Instance => 1,
            GroupSelector::Face => 2,
            GroupSelector::Loop => 3,
            GroupSelector::Edge => 4,
            GroupSelector::Vertex => 5,
            GroupSelector::Surface => 6,
            GroupSelector::Curve => 7,
            GroupSelector::Point => 8,
            GroupSelector::Mixed => 9,
            GroupSelector::Region => 10,
        }
    }
}

/// Whether a GROUP node's pointer fields are all non-null and its second names a stored node.
pub fn valid_group_references(references: &[u32; 5]) -> bool {
    references.iter().all(|reference| *reference != 0) && references[1] > 1
}

#[cfg(test)]
mod tests {
    use super::GroupSelector;

    #[test]
    fn group_selector_wire_covers_the_ten_declared_families() {
        for value in 1..=10u8 {
            let selector = GroupSelector::try_from(value).unwrap();
            assert_eq!(u8::from(selector), value);
            assert_eq!(
                serde_json::to_value(selector).unwrap(),
                serde_json::json!(value)
            );
            assert_eq!(
                serde_json::from_value::<GroupSelector>(serde_json::json!(value)).unwrap(),
                selector
            );
        }
        for value in [0u8, 11, 255] {
            assert!(serde_json::from_value::<GroupSelector>(serde_json::json!(value)).is_err());
        }
    }
}
