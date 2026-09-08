// SPDX-License-Identifier: Apache-2.0
//! Topology families that carry Parasolid attribute lists.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum TopologyAttributeKind {
    Shell,
    Face,
    Loop,
    Edge,
    Fin,
    Vertex,
}

impl TopologyAttributeKind {
    pub(crate) const ALL: [Self; 6] = [
        Self::Shell,
        Self::Face,
        Self::Loop,
        Self::Edge,
        Self::Fin,
        Self::Vertex,
    ];

    pub(crate) fn node_kind(self) -> crate::framing::node_kind::NodeKind {
        match self {
            Self::Shell => crate::framing::node_kind::NodeKind::Shell,
            Self::Face => crate::framing::node_kind::NodeKind::Face,
            Self::Loop => crate::framing::node_kind::NodeKind::Loop,
            Self::Edge => crate::framing::node_kind::NodeKind::Edge,
            Self::Fin => crate::framing::node_kind::NodeKind::Fin,
            Self::Vertex => crate::framing::node_kind::NodeKind::Vertex,
        }
    }

    pub(crate) fn code(self) -> u8 {
        self.node_kind().code()
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Face => "face",
            Self::Loop => "loop",
            Self::Edge => "edge",
            Self::Fin => "fin",
            Self::Vertex => "vertex",
        }
    }
}

impl From<TopologyAttributeKind> for u8 {
    fn from(kind: TopologyAttributeKind) -> Self {
        kind.node_kind().code()
    }
}

impl TryFrom<u8> for TopologyAttributeKind {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            13 => Ok(Self::Shell),
            14 => Ok(Self::Face),
            15 => Ok(Self::Loop),
            16 => Ok(Self::Edge),
            17 => Ok(Self::Fin),
            18 => Ok(Self::Vertex),
            _ => Err("topology_type must identify shell, face, loop, edge, fin, or vertex"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TopologyAttributeKind;

    #[test]
    fn topology_attribute_kind_preserves_codes_and_rejects_other_kinds() {
        for (kind, code) in TopologyAttributeKind::ALL.into_iter().zip(13..=18) {
            let wire = code.to_string();
            assert_eq!(serde_json::to_string(&kind).unwrap(), wire);
            assert_eq!(
                serde_json::from_str::<TopologyAttributeKind>(&wire).unwrap(),
                kind
            );
        }
        for code in [0, 12, 19, 255] {
            assert!(serde_json::from_str::<TopologyAttributeKind>(&code.to_string()).is_err());
        }
    }
}
