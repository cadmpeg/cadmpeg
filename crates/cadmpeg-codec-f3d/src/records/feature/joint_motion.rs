// SPDX-License-Identifier: Apache-2.0
//! The motion of an assembly joint: its freedoms and its limits.

use crate::records::identity::ReferenceRun;
use serde::{Deserialize, Serialize};

/// The freedoms and limits of one assembly joint, read from its motion record
/// and its joint record. A joint without freedom entries has no motion record
/// here: it is rigid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignJointMotion {
    /// Globally unique deterministic identifier for this native motion.
    pub(crate) id: String,
    /// Record index of the joint record.
    pub(crate) joint_record_index: u32,
    /// Byte offset of the joint record's primary header.
    #[serde(skip)]
    pub(crate) joint_byte_offset: Option<u64>,
    /// Record index of the motion record holding the freedom entries.
    pub(crate) motion_record_index: u32,
    /// Byte offset of the motion record's primary header.
    #[serde(skip)]
    pub(crate) motion_byte_offset: Option<u64>,
    /// Freedom codes in record order, each at the offset of its code.
    pub(crate) freedoms: ReferenceRun<u32>,
    /// The joint record's limit lanes, when its self-reference is present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_limits"
    )]
    pub(crate) limits: Option<DesignJointLimitLanes>,
}

/// The two limit flags and the maximum and minimum after a joint record's
/// self-reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignJointLimitLanes {
    /// Byte offset of the first of the two flag bytes.
    #[serde(skip)]
    pub(crate) flags_offset: Option<u64>,
    /// The first flag byte, `0` or `1`.
    pub(crate) first_flag: u8,
    /// The second flag byte, `0` or `1`.
    pub(crate) second_flag: u8,
    /// Maximum, in the freedom's internal unit.
    pub(crate) maximum: f64,
    /// Minimum, in the freedom's internal unit.
    pub(crate) minimum: f64,
    /// Byte offset of the maximum; the minimum follows it.
    #[serde(skip)]
    pub(crate) values_offset: Option<u64>,
}

cadmpeg_core::named_optional_field!(deserialize_limits, DesignJointLimitLanes, "limits");

impl DesignJointMotion {
    pub(crate) fn retain_source(&self, fidelity: &mut cadmpeg_ir::SourceFidelity) {
        for (field, offset) in [
            ("joint_byte_offset", self.joint_byte_offset),
            ("motion_byte_offset", self.motion_byte_offset),
            (
                "limits.flags_offset",
                self.limits.as_ref().and_then(|limits| limits.flags_offset),
            ),
            (
                "limits.values_offset",
                self.limits.as_ref().and_then(|limits| limits.values_offset),
            ),
        ] {
            if let Some(offset) = offset {
                fidelity
                    .metadata
                    .insert(format!("{}.{field}", self.id), offset.to_string());
            }
        }
        if let Some(codes) = self.freedoms.located_rows() {
            for (ordinal, code) in codes.iter().enumerate() {
                fidelity.metadata.insert(
                    format!("{}.freedom_offsets.{ordinal}", self.id),
                    code.offset.to_string(),
                );
            }
        }
    }
}
