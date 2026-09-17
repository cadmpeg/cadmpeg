// SPDX-License-Identifier: Apache-2.0
//! Fillet radius groups, radius laws and the historical binding they carry.

use super::body_recipe::AsmHistoricalEntityKind;
use serde::Deserialize;
use serde::Serialize;

/// One radius assignment and its ordered edge group in a Fillet scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFilletRadiusGroup {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning Fillet scope record.
    pub scope_record_index: u32,
    /// Position among construction-operand groups in scope-reference order.
    pub group_ordinal: u32,
    /// Counted construction-operand group carrying the edges.
    pub group_record_index: u32,
    /// Ordered edge-operand records assigned this radius.
    pub edge_operand_record_indices: Vec<u32>,
    /// Radius law paired with this edge group.
    pub law: DesignFilletRadiusLaw,
    /// Tangency-weight parameter record paired with this edge group.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangency_weight_parameter_record_index"
    )]
    pub tangency_weight_parameter_record_index: Option<u32>,
}

/// Parameter records defining one Fillet group's radius law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignFilletRadiusLawWire",
    into = "DesignFilletRadiusLawWire"
)]
pub enum DesignFilletRadiusLaw {
    /// One radius applies along the complete edge group.
    Constant {
        /// Radius parameter record.
        radius_parameter_record_index: u32,
    },
    /// Constant transverse chord length across the fillet surface.
    Chordal {
        /// Chord-length parameter record.
        chord_length_parameter_record_index: u32,
    },
    /// Distinct support-face offsets along the complete edge group.
    Asymmetric {
        /// First support-face offset parameter record.
        offset_one_parameter_record_index: u32,
        /// Second support-face offset parameter record.
        offset_two_parameter_record_index: u32,
    },
    /// Explicit endpoint and optional midpoint radius controls.
    Variable {
        /// Radius at normalized parameter zero.
        start_radius_parameter_record_index: u32,
        /// Radius at normalized parameter one.
        end_radius_parameter_record_index: u32,
        /// Midpoint radius and normalized-parameter records in owner-local order.
        middle: Vec<DesignFilletMidpoint>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignFilletMidpoint {
    pub radius_parameter_record_index: u32,
    pub parameter_record_index: u32,
}

/// Parameter records defining one Fillet group's radius law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignFilletRadiusLawWire {
    /// One radius applies along the complete edge group.
    Constant {
        /// Radius parameter record.
        radius_parameter_record_index: u32,
    },
    /// Constant transverse chord length across the fillet surface.
    Chordal {
        /// Chord-length parameter record.
        chord_length_parameter_record_index: u32,
    },
    /// Distinct support-face offsets along the complete edge group.
    Asymmetric {
        /// First support-face offset parameter record.
        offset_one_parameter_record_index: u32,
        /// Second support-face offset parameter record.
        offset_two_parameter_record_index: u32,
    },
    /// Explicit endpoint and optional midpoint radius controls.
    Variable {
        /// Radius at normalized parameter zero.
        start_radius_parameter_record_index: u32,
        /// Radius at normalized parameter one.
        end_radius_parameter_record_index: u32,
        /// Midpoint radius records in owner-local order.
        middle_radius_parameter_record_indices: Vec<u32>,
        /// Midpoint normalized-parameter records parallel to the radii.
        middle_parameter_record_indices: Vec<u32>,
    },
}

impl TryFrom<DesignFilletRadiusLawWire> for DesignFilletRadiusLaw {
    type Error = String;
    fn try_from(wire: DesignFilletRadiusLawWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignFilletRadiusLawWire::Constant {
                radius_parameter_record_index,
            } => Self::Constant {
                radius_parameter_record_index,
            },
            DesignFilletRadiusLawWire::Chordal {
                chord_length_parameter_record_index,
            } => Self::Chordal {
                chord_length_parameter_record_index,
            },
            DesignFilletRadiusLawWire::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            } => Self::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            },
            DesignFilletRadiusLawWire::Variable {
                start_radius_parameter_record_index,
                end_radius_parameter_record_index,
                middle_radius_parameter_record_indices,
                middle_parameter_record_indices,
            } => {
                if middle_radius_parameter_record_indices.len()
                    != middle_parameter_record_indices.len()
                {
                    return Err("middle_radius_parameter_record_indices and middle_parameter_record_indices must have equal lengths".into());
                }
                Self::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle: middle_radius_parameter_record_indices
                        .into_iter()
                        .zip(middle_parameter_record_indices)
                        .map(|(radius_parameter_record_index, parameter_record_index)| {
                            DesignFilletMidpoint {
                                radius_parameter_record_index,
                                parameter_record_index,
                            }
                        })
                        .collect(),
                }
            }
        })
    }
}

impl From<DesignFilletRadiusLaw> for DesignFilletRadiusLawWire {
    fn from(law: DesignFilletRadiusLaw) -> Self {
        match law {
            DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index,
            } => Self::Constant {
                radius_parameter_record_index,
            },
            DesignFilletRadiusLaw::Chordal {
                chord_length_parameter_record_index,
            } => Self::Chordal {
                chord_length_parameter_record_index,
            },
            DesignFilletRadiusLaw::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            } => Self::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            },
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index,
                end_radius_parameter_record_index,
                middle,
            } => {
                let (middle_radius_parameter_record_indices, middle_parameter_record_indices) =
                    middle
                        .into_iter()
                        .map(|row| {
                            (
                                row.radius_parameter_record_index,
                                row.parameter_record_index,
                            )
                        })
                        .unzip();
                Self::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle_radius_parameter_record_indices,
                    middle_parameter_record_indices,
                }
            }
        }
    }
}

/// ASM history family, entity slot, and states for one selected identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalBinding {
    /// Stable ASM family containing the selected identity.
    #[serde(rename = "historical_entity_kind")]
    pub kind: AsmHistoricalEntityKind,
    /// Stable ASM entity slot after record-revision normalization.
    #[serde(rename = "historical_entity_ref")]
    pub entity_ref: i64,
    /// ASM history states containing the identity, in history arena order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(rename = "historical_state_ids")]
    pub state_ids: Vec<i64>,
}

#[derive(Deserialize)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
struct OptionalHistoricalBindingWire {
    #[serde(default, deserialize_with = "deserialize_historical_entity_kind")]
    historical_entity_kind: Option<AsmHistoricalEntityKind>,
    #[serde(default, deserialize_with = "deserialize_historical_entity_ref")]
    historical_entity_ref: Option<i64>,
    #[serde(default)]
    historical_state_ids: Vec<i64>,
}

pub(super) fn deserialize_historical_binding<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<HistoricalBinding>, D::Error> {
    let wire = OptionalHistoricalBindingWire::deserialize(deserializer)?;
    match (wire.historical_entity_kind, wire.historical_entity_ref) {
        (None, None) if wire.historical_state_ids.is_empty() => Ok(None),
        (Some(kind), Some(entity_ref)) => Ok(Some(HistoricalBinding { kind, entity_ref, state_ids: wire.historical_state_ids })),
        _ => Err(serde::de::Error::custom("historical_entity_kind and historical_entity_ref are required together for historical_state_ids")),
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_tangency_weight_parameter_record_index,
    u32,
    "tangency_weight_parameter_record_index"
);

cadmpeg_core::named_optional_field!(
    deserialize_historical_entity_kind,
    AsmHistoricalEntityKind,
    "historical_entity_kind"
);

cadmpeg_core::named_optional_field!(
    deserialize_historical_entity_ref,
    i64,
    "historical_entity_ref"
);

#[cfg(test)]
mod tests;
