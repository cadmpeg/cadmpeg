// SPDX-License-Identifier: Apache-2.0
//! Fillet set records that a parameter-owned `Fillet` scope names in its prefix.

use super::fixed_parameters::DesignFixedFilletParameters;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_curvature_continuity,
    bool,
    "curvature_continuity"
);
cadmpeg_core::named_optional_field!(deserialize_tangent_chain, bool, "tangent_chain");
cadmpeg_core::named_optional_field!(
    deserialize_weight_owner,
    u32,
    "tangency_weight_owner_record_index"
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

impl DesignFilletSetList {
    pub(crate) fn projects_short_sets(&self, groups: impl IntoIterator<Item = u32>) -> bool {
        self.sets.iter().any(DesignFilletSet::is_short_constant)
            && self.supports_set_law_projection()
            && groups
                .into_iter()
                .all(|group| self.sets.iter().any(|set| set.group_record_index == group))
    }

    pub(crate) fn supports_set_law_projection(&self) -> bool {
        match self.sets.as_slice() {
            [] => false,
            [set] => set.law.has_implicit_ordinal() || set.is_short_constant(),
            sets => sets.iter().all(|set| {
                matches!(set.law, DesignFilletSetLaw::Chordal { .. }) || set.is_short_constant()
            }),
        }
    }
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
        if !wire.sets.iter().all(DesignFilletSet::options_match_law)
            || (wire.sets.len() != 1 && wire.sets.iter().any(|set| set.law.has_implicit_ordinal()))
        {
            return Err("a fillet set list has incompatible law options");
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
    /// `01 01` and nine padding bytes.
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_weight_owner"
    )]
    pub(crate) tangency_weight_owner_record_index: Option<u32>,
    /// Edge-set tangent-chain flag.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangent_chain"
    )]
    pub(crate) tangent_chain: Option<bool>,
    /// Curvature-continuity flag of the 97-byte constant form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curvature_continuity"
    )]
    pub(crate) curvature_continuity: Option<bool>,
}

impl DesignFilletSet {
    pub(crate) fn is_short_constant(&self) -> bool {
        usize::try_from(self.frame_length) == Ok(crate::layout::fillet_set_short_constant::LEN)
            && matches!(self.law, DesignFilletSetLaw::Constant { .. })
    }

    pub(crate) fn options_match_law(&self) -> bool {
        if self.is_short_constant() {
            return self.tangency_weight_owner_record_index.is_none()
                && self.tangent_chain.is_none()
                && self.curvature_continuity.is_none();
        }
        let asymmetric = matches!(self.law, DesignFilletSetLaw::Asymmetric { .. });
        self.tangency_weight_owner_record_index.is_some() != asymmetric
            && self.tangent_chain.is_some() != self.law.has_implicit_ordinal()
    }
}

/// Parameter owner that carries a fillet set's radius law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
    /// Two support-face offsets with a separate owner for each side.
    Asymmetric {
        /// First offset owner record.
        offset_one_owner_record_index: u32,
        /// Second offset owner record.
        offset_two_owner_record_index: u32,
    },
    /// Endpoint and midpoint owners in native order.
    Variable {
        /// Start radius owner record.
        start_radius_owner_record_index: u32,
        /// End radius owner record.
        end_radius_owner_record_index: u32,
        /// Paired midpoint owners in set-list order.
        middle: [DesignFilletMidpointOwners; 2],
    },
}

/// Owners at one common ordinal in the midpoint radius and position lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignFilletMidpointOwners {
    pub(crate) radius_owner_record_index: u32,
    pub(crate) position_owner_record_index: u32,
}

impl DesignFilletSetLaw {
    pub(crate) const fn has_implicit_ordinal(self) -> bool {
        matches!(self, Self::Asymmetric { .. } | Self::Variable { .. })
    }

    pub(crate) fn owner_roles(self) -> impl Iterator<Item = (u32, &'static str)> {
        let mut roles = [None; 6];
        match self {
            Self::Constant {
                radius_owner_record_index,
            } => {
                roles[0] = Some((radius_owner_record_index, "Radius"));
            }
            Self::Chordal {
                chord_length_owner_record_index,
            } => {
                roles[0] = Some((chord_length_owner_record_index, "ChordLen"));
            }
            Self::Asymmetric {
                offset_one_owner_record_index,
                offset_two_owner_record_index,
            } => {
                roles[0] = Some((offset_one_owner_record_index, "EdgeOffset1"));
                roles[1] = Some((offset_two_owner_record_index, "EdgeOffset2"));
            }
            Self::Variable {
                start_radius_owner_record_index,
                end_radius_owner_record_index,
                middle,
            } => {
                roles = [
                    Some((start_radius_owner_record_index, "StartRadius")),
                    Some((end_radius_owner_record_index, "EndRadius")),
                    Some((middle[0].radius_owner_record_index, "MidRadius")),
                    Some((middle[0].position_owner_record_index, "MidParams")),
                    Some((middle[1].radius_owner_record_index, "MidRadius")),
                    Some((middle[1].position_owner_record_index, "MidParams")),
                ];
            }
        }
        roles.into_iter().flatten()
    }
}

mod identity_rewrite;
