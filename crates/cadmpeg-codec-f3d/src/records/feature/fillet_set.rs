// SPDX-License-Identifier: Apache-2.0
//! Fillet set records that a parameter-owned `Fillet` scope names in its prefix.

use super::fixed_parameters::DesignFixedFilletParameters;
use serde::{Deserialize, Serialize};

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
    /// Fillet sets in prefix order.
    pub(crate) sets: Vec<DesignFilletSet>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignFilletSetListWire {
    form: DesignFilletSetListForm,
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
            sets: wire.sets,
        })
    }
}

/// Byte run between the set list and the edge-group list of a `Fillet` scope prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignFilletSetListForm {
    /// `01 00 01` and nine zero bytes.
    Current,
    /// `01 01` and nine zero bytes.
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
    /// Boolean byte at offset 26 of a 97-byte frame or offset 24 of a 93-byte frame.
    pub(crate) flag: bool,
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
