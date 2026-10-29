// SPDX-License-Identifier: Apache-2.0
//! Source projections with existing native consumers.

use super::group::GroupReferenceStatus;
use super::region::RegionFields;
use crate::iter_wire::IterWire;
use crate::records::group::GroupSelector;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RecordFamily {
    Region {
        fields: RegionFields,
    },
    Group {
        references: [u32; 5],
        node_id: u32,
        selector: GroupSelector,
        linked_reference_status: GroupReferenceStatus,
    },
}

pub(crate) struct RecordFamilyReferences<'a>(pub(crate) &'a RecordFamily);

impl Serialize for RecordFamilyReferences<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            RecordFamily::Region { fields } => IterWire(fields.references()).serialize(serializer),
            RecordFamily::Group { references, .. } => references.serialize(serializer),
        }
    }
}

impl RecordFamily {
    pub(crate) const fn kind(&self) -> u16 {
        match self {
            Self::Region { .. } => 19,
            Self::Group { .. } => 90,
        }
    }
    pub(crate) const fn family_name(&self) -> &'static str {
        match self {
            Self::Region { .. } => "REGION",
            Self::Group { .. } => "GROUP",
        }
    }
    pub(crate) const fn node_id(&self) -> u32 {
        match self {
            Self::Region { fields } => fields.node_id,
            Self::Group { node_id, .. } => *node_id,
        }
    }
}
