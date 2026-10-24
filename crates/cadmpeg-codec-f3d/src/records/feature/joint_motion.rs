// SPDX-License-Identifier: Apache-2.0
//! The motion of an assembly joint: its freedoms and its limits.

use crate::records::identity::Located;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_limits, DesignJointLimitLanes, "limits");
cadmpeg_core::named_optional_field!(deserialize_entry_value, f64, "value");

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
    pub(crate) joint_byte_offset: u64,
    /// Record index of the motion record holding the freedom entries.
    pub(crate) motion_record_index: u32,
    /// Byte offset of the motion record's primary header.
    pub(crate) motion_byte_offset: u64,
    /// Freedom codes in record order, each at the offset of its code.
    pub(crate) freedoms: Vec<Located<u32>>,
    /// The word before each freedom code, with the f64 a word-`1` entry
    /// stores after its code, one per freedom in the same order. Empty when
    /// every entry stores word `3`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) entry_words: Vec<DesignJointFreedomWord>,
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
    pub(crate) flags_offset: u64,
    /// The first flag byte, `0` or `1`.
    pub(crate) first_flag: u8,
    /// The second flag byte, `0` or `1`.
    pub(crate) second_flag: u8,
    /// Maximum, in the freedom's internal unit.
    pub(crate) maximum: f64,
    /// Minimum, in the freedom's internal unit.
    pub(crate) minimum: f64,
    /// Byte offset of the maximum; the minimum follows it.
    pub(crate) values_offset: u64,
}

/// The u32 word before a freedom entry's code, and the f64 a word-`1` entry
/// stores after its code.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignJointFreedomWord {
    /// The u32 word, `1` or `3`.
    pub(crate) word: u32,
    /// The f64 after the code of a word-`1` entry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_entry_value"
    )]
    pub(crate) value: Option<f64>,
}
