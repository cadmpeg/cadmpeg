// SPDX-License-Identifier: Apache-2.0
//! Chamfer set records that a parameter-owned `Chamfer` scope names in its prefix.

use super::fixed_parameters::DesignFixedChamferParameters;
use serde::{Deserialize, Serialize};

/// Operation data of a `Chamfer` scope: fixed scalar lanes or the chamfer sets of its prefix.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DesignChamferScope {
    pub(crate) fixed: Option<DesignFixedChamferParameters>,
    pub(crate) sets: Option<DesignChamferSetList>,
}

impl DesignChamferScope {
    /// The payload of one decoded `Chamfer` scope; a set list takes precedence
    /// over fixed scalar lanes.
    pub(crate) fn admit(
        fixed: Option<DesignFixedChamferParameters>,
        sets: Option<DesignChamferSetList>,
    ) -> Self {
        match sets {
            Some(sets) => Self {
                fixed: None,
                sets: Some(sets),
            },
            None => Self { fixed, sets: None },
        }
    }
}

/// Ordered chamfer sets named by the prefix of a `Chamfer` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignChamferSetListWire")]
pub(crate) struct DesignChamferSetList {
    /// Byte run before the entry run.
    pub(crate) form: DesignChamferSetListForm,
    /// Corner treatment where edges of the list meet.
    pub(crate) corner_type: DesignChamferCornerType,
    /// Number of entries in the prefix entry run.
    pub(crate) entry_count: u32,
    /// Chamfer sets in prefix order.
    pub(crate) sets: Vec<DesignChamferSet>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignChamferSetListWire {
    form: DesignChamferSetListForm,
    corner_type: DesignChamferCornerType,
    entry_count: u32,
    sets: Vec<DesignChamferSet>,
}

impl TryFrom<DesignChamferSetListWire> for DesignChamferSetList {
    type Error = &'static str;

    fn try_from(wire: DesignChamferSetListWire) -> Result<Self, Self::Error> {
        if wire.sets.is_empty() || wire.entry_count == 0 {
            return Err("a chamfer set list names at least one set and one entry");
        }
        Ok(Self {
            form: wire.form,
            corner_type: wire.corner_type,
            entry_count: wire.entry_count,
            sets: wire.sets,
        })
    }
}

/// Corner treatment of a `Chamfer` scope, stored as `u32` 1, 2, or 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignChamferCornerType {
    /// Value 1.
    Chamfer,
    /// Value 2.
    Miter,
    /// Value 3.
    Blend,
}

/// Byte run before the entry run of a `Chamfer` scope prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignChamferSetListForm {
    /// `00 01`.
    Current,
    /// `01`.
    Compact,
}

/// One chamfer set record: an edge group and the owners of its dimensional specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignChamferSet {
    /// Indexed record of the set.
    pub(crate) record_index: u32,
    /// Byte offset of the set's primary indexed header.
    pub(crate) byte_offset: u64,
    /// Frame length up to the set's paired indexed header.
    pub(crate) frame_length: u64,
    /// Construction-operand group of the scope that this set treats.
    pub(crate) group_record_index: u32,
    /// Parameter owners of the set's dimensional specification.
    pub(crate) spec: DesignChamferSetSpec,
    /// Indexed record referenced after the fixed words.
    pub(crate) auxiliary_record_index: u32,
    /// Number of prefix entries that name this set.
    pub(crate) entry_count: u32,
    /// Edge-set tangent-chain flag.
    pub(crate) tangent_chain: bool,
}

/// Parameter owners of one chamfer set's dimensional specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DesignChamferSetSpec {
    /// Owner of the one equal distance.
    EqualDistance {
        /// Distance parameter owner record.
        distance_owner_record_index: u32,
    },
    /// Owners of the first-face and second-face distances.
    TwoDistances {
        /// First distance parameter owner record.
        first_owner_record_index: u32,
        /// Second distance parameter owner record.
        second_owner_record_index: u32,
    },
    /// Owners of the distance and the angle.
    DistanceAngle {
        /// Distance parameter owner record.
        distance_owner_record_index: u32,
        /// Angle parameter owner record.
        angle_owner_record_index: u32,
    },
}

mod identity_rewrite;
