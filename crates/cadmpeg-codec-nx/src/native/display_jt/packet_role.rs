// SPDX-License-Identifier: Apache-2.0
//! Semantic roles of compressed JT topology packets.

use std::fmt;

use serde::{Deserialize, Serialize};

/// One of the eight topology-coder contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TopologyContext {
    C0,
    C1,
    C2,
    C3,
    C4,
    C5,
    C6,
    C7,
}

impl TopologyContext {
    pub const ALL: [Self; 8] = [
        Self::C0,
        Self::C1,
        Self::C2,
        Self::C3,
        Self::C4,
        Self::C5,
        Self::C6,
        Self::C7,
    ];

    fn parse(value: &str) -> Option<Self> {
        match value {
            "0" => Some(Self::C0),
            "1" => Some(Self::C1),
            "2" => Some(Self::C2),
            "3" => Some(Self::C3),
            "4" => Some(Self::C4),
            "5" => Some(Self::C5),
            "6" => Some(Self::C6),
            "7" => Some(Self::C7),
            _ => None,
        }
    }
}

/// Semantic lane carried by one compressed topology packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum TopologyPacketRole {
    FaceDegrees(TopologyContext),
    VertexValences,
    VertexGroups,
    VertexFlags,
    FaceAttributeMasks(TopologyContext),
    FaceAttributeMasks7Next30,
    FaceAttributeMasks7Upper4,
    HighDegreeFaceAttributeMasks(usize),
    SplitFaceSymbols,
    SplitFacePositions,
}

impl fmt::Display for TopologyPacketRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FaceDegrees(context) => write!(formatter, "face_degrees_{}", *context as u8),
            Self::VertexValences => formatter.write_str("vertex_valences"),
            Self::VertexGroups => formatter.write_str("vertex_groups"),
            Self::VertexFlags => formatter.write_str("vertex_flags"),
            Self::FaceAttributeMasks(context) => {
                write!(formatter, "face_attribute_masks_{}", *context as u8)
            }
            Self::FaceAttributeMasks7Next30 => {
                formatter.write_str("face_attribute_masks_7_next_30")
            }
            Self::FaceAttributeMasks7Upper4 => {
                formatter.write_str("face_attribute_masks_7_upper_4")
            }
            Self::HighDegreeFaceAttributeMasks(ordinal) => {
                write!(formatter, "high_degree_face_attribute_masks_{ordinal}")
            }
            Self::SplitFaceSymbols => formatter.write_str("split_face_symbols"),
            Self::SplitFacePositions => formatter.write_str("split_face_positions"),
        }
    }
}

impl From<TopologyPacketRole> for String {
    fn from(role: TopologyPacketRole) -> Self {
        role.to_string()
    }
}

impl TryFrom<String> for TopologyPacketRole {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "vertex_valences" => return Ok(Self::VertexValences),
            "vertex_groups" => return Ok(Self::VertexGroups),
            "vertex_flags" => return Ok(Self::VertexFlags),
            "face_attribute_masks_7_next_30" => return Ok(Self::FaceAttributeMasks7Next30),
            "face_attribute_masks_7_upper_4" => return Ok(Self::FaceAttributeMasks7Upper4),
            "split_face_symbols" => return Ok(Self::SplitFaceSymbols),
            "split_face_positions" => return Ok(Self::SplitFacePositions),
            _ => {}
        }
        if let Some(context) = value
            .strip_prefix("face_degrees_")
            .and_then(TopologyContext::parse)
        {
            return Ok(Self::FaceDegrees(context));
        }
        if let Some(context) = value
            .strip_prefix("face_attribute_masks_")
            .and_then(TopologyContext::parse)
        {
            return Ok(Self::FaceAttributeMasks(context));
        }
        if let Some(ordinal) = value
            .strip_prefix("high_degree_face_attribute_masks_")
            .and_then(|suffix| {
                let ordinal = suffix.parse::<usize>().ok()?;
                (suffix == ordinal.to_string()).then_some(ordinal)
            })
        {
            return Ok(Self::HighDegreeFaceAttributeMasks(ordinal));
        }
        Err("unknown JT topology packet role")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_role_wire_preserves_indexed_and_fixed_labels() {
        for label in [
            "face_degrees_0",
            "face_degrees_7",
            "vertex_valences",
            "vertex_groups",
            "vertex_flags",
            "face_attribute_masks_0",
            "face_attribute_masks_7",
            "face_attribute_masks_7_next_30",
            "face_attribute_masks_7_upper_4",
            "high_degree_face_attribute_masks_0",
            "high_degree_face_attribute_masks_12",
            "split_face_symbols",
            "split_face_positions",
        ] {
            let json = format!("\"{label}\"");
            let role: TopologyPacketRole = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&role).unwrap(), json);
        }
    }

    #[test]
    fn packet_role_wire_rejects_unknown_contexts_and_noncanonical_ordinals() {
        for label in [
            "face_degrees_8",
            "face_attribute_masks_8",
            "face_degrees_00",
            "high_degree_face_attribute_masks_01",
            "high_degree_face_attribute_masks_-1",
            "high_degree_face_attribute_masks_",
            "face_attribute_masks_7_next_31",
        ] {
            assert!(
                serde_json::from_value::<TopologyPacketRole>(serde_json::json!(label)).is_err()
            );
        }
    }
}
