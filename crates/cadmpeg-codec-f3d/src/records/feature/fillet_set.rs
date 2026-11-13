// SPDX-License-Identifier: Apache-2.0
//! Fillet set records that a parameter-owned `Fillet` scope names in its prefix.

use super::fixed_parameters::DesignFixedFilletParameters;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_curvature_continuity,
    bool,
    "curvature_continuity"
);

/// Operation data of a `Fillet` scope: fixed scalar lanes or the fillet sets of its prefix.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DesignFilletScope {
    pub(crate) fixed: Option<DesignFixedFilletParameters>,
    pub(crate) sets: Option<DesignFilletSetList>,
}

impl DesignFilletScope {
    /// The payload of one decoded `Fillet` scope; a set list takes precedence
    /// over fixed scalar lanes.
    pub(crate) fn admit(
        fixed: Option<DesignFixedFilletParameters>,
        sets: Option<DesignFilletSetList>,
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

/// Ordered fillet sets named by the prefix of a `Fillet` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignFilletSetListWire")]
pub(crate) struct DesignFilletSetList {
    /// Byte run between the set list and the edge-group list.
    pub(crate) form: DesignFilletSetListForm,
    /// Rolling-ball corner flag, the first byte of the flag run.
    pub(crate) rolling_ball_corner: bool,
    /// Fillet sets in prefix order.
    pub(crate) sets: Vec<DesignFilletSet>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignFilletSetListWire {
    form: DesignFilletSetListForm,
    rolling_ball_corner: bool,
    sets: Vec<DesignFilletSet>,
}

impl TryFrom<DesignFilletSetListWire> for DesignFilletSetList {
    type Error = &'static str;

    fn try_from(wire: DesignFilletSetListWire) -> Result<Self, Self::Error> {
        if wire.sets.is_empty() {
            return Err("a fillet set list names at least one set");
        }
        Ok(Self {
            form: wire.form,
            rolling_ball_corner: wire.rolling_ball_corner,
            sets: wire.sets,
        })
    }
}

/// Byte run between the set list and the edge-group list of a `Fillet` scope prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignFilletSetListForm {
    /// The rolling-ball byte, `00 01`, and nine zero bytes.
    Current,
    /// The rolling-ball byte, `01`, and nine zero bytes.
    Compact,
}

/// One fillet set record: an edge group, its radius law owner, and its tangency-weight owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignFilletSet {
    /// Indexed record of the set.
    pub(crate) record_index: u32,
    /// Byte offset of the set's indexed header.
    pub(crate) byte_offset: u64,
    /// Frame length up to the next indexed header.
    pub(crate) frame_length: u64,
    /// Construction-operand group of the scope that this set treats.
    pub(crate) group_record_index: u32,
    /// Parameter owner of the set's radius law.
    pub(crate) law: DesignFilletSetLaw,
    /// Parameter owner of the set's `TangencyWeight`.
    pub(crate) tangency_weight_owner_record_index: u32,
    /// Edge-set tangent-chain flag.
    pub(crate) tangent_chain: bool,
    /// Curvature-continuity flag of the 97-byte constant form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curvature_continuity"
    )]
    pub(crate) curvature_continuity: Option<bool>,
}

/// Parameter owner that carries a fillet set's radius law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DesignFilletSetLaw {
    /// Owner of a `Radius` parameter.
    Constant {
        /// Radius parameter owner record.
        radius_owner_record_index: u32,
    },
    /// Owner of a `ChordLen` parameter.
    Chordal {
        /// Chord-length parameter owner record.
        chord_length_owner_record_index: u32,
    },
}

mod identity_rewrite;
