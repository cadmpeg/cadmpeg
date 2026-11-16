// SPDX-License-Identifier: Apache-2.0
//! Source projections with existing native consumers.

use super::group::GroupReferenceStatus;
use super::region::RegionFields;
use crate::records::group::GroupSelector;
use cadmpeg_core::iter_wire::IterWire;
use serde::Serialize;

/// A Deltas node projected onto its record family.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordFamily {
    /// A type-19 REGION node.
    Region {
        /// REGION fields.
        fields: RegionFields,
    },
    /// A type-90 GROUP node.
    Group {
        /// Pointer fields in field order.
        references: [u32; 5],
        /// GROUP node ID.
        node_id: u32,
        /// Entity class the GROUP collects.
        selector: GroupSelector,
        /// Linked-reference status byte.
        linked_reference_status: GroupReferenceStatus,
    },
}

/// Serializes the pointer fields of a record family.
pub struct RecordFamilyReferences<'a>(pub &'a RecordFamily);

impl Serialize for RecordFamilyReferences<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            RecordFamily::Region { fields } => IterWire(fields.references()).serialize(serializer),
            RecordFamily::Group { references, .. } => references.serialize(serializer),
        }
    }
}

impl RecordFamily {
    /// Node type of the family.
    pub const fn kind(&self) -> u16 {
        match self {
            Self::Region { .. } => 19,
            Self::Group { .. } => 90,
        }
    }
    /// Parasolid node-type name of the family.
    pub const fn family_name(&self) -> &'static str {
        match self {
            Self::Region { .. } => "REGION",
            Self::Group { .. } => "GROUP",
        }
    }
    /// Node ID of the record.
    pub const fn node_id(&self) -> u32 {
        match self {
            Self::Region { fields } => fields.node_id,
            Self::Group { node_id, .. } => *node_id,
        }
    }
}
