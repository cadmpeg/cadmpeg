// SPDX-License-Identifier: Apache-2.0
//! Hole constructions, their tangent points and face selections.

use crate::records::identity::{DesignSecondaryIdentity, Located};
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_curve_secondary_identity,
    u64,
    "curve_secondary_identity"
);
cadmpeg_core::named_optional_field!(
    deserialize_curve_secondary_identity_offset,
    u64,
    "curve_secondary_identity_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_face_selection,
    DesignHoleFaceSelection,
    "face_selection"
);
cadmpeg_core::named_optional_field!(deserialize_secondary_identity, u64, "secondary_identity");
cadmpeg_core::named_optional_field!(
    deserialize_secondary_identity_offset,
    u64,
    "secondary_identity_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_tangent_point_data,
    [f64; 3],
    "tangent_point_data"
);
cadmpeg_core::named_optional_field!(
    deserialize_tangent_point_data_offset,
    u64,
    "tangent_point_data_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_tangent_point_data_prefix,
    u8,
    "tangent_point_data_prefix"
);
/// Tangent-point payload of a version-four Hole point carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignHoleTangentPoint {
    pub prefix: u8,
    pub data: Located<[f64; 3]>,
}

/// Exact point-and-direction construction carried by a `Hole` scope.
///
/// The native point carrier stores the coordinates in source centimetres and
/// the direction as a unit model-space vector. The remaining fields preserve
/// the carrier's base-level evidence so later Hole forms can bind their input
/// records without reparsing the byte stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignHoleConstructionWire",
    into = "DesignHoleConstructionWire"
)]
pub struct DesignHoleConstruction {
    /// Point-data record selected by the Hole scope.
    pub point_record_index: u32,
    /// Byte offset of the point-data record header.
    pub point_record_byte_offset: u64,
    /// Hole entry position in source model centimetres.
    pub position: [f64; 3],
    /// Byte offset of the first position coordinate.
    pub position_offset: u64,
    /// Directed drilling vector in model space.
    pub direction: [f64; 3],
    /// Byte offset of the first direction component.
    pub direction_offset: u64,
    /// Two point-construction parameters carried by the point-data base level.
    pub point_parameters: [f64; 2],
    /// Byte offsets of the two point-construction parameters.
    pub point_parameter_offsets: [u64; 2],
    /// `refType` construction rule carried by the point-data record.
    pub reference_type: u32,
    /// Byte offset of `reference_type`.
    pub reference_type_offset: u64,
    /// Version-four tangent-point data with its prefix and source location.
    pub tangent_point_data: Option<DesignHoleTangentPoint>,
    /// Located targets of the counted input-reference run.
    pub input_records: Vec<Located<u32>>,
    /// Direct persistent face selection carried by the Hole scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_selection: Option<DesignHoleFaceSelection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHoleConstructionWire {
    /// Point-data record selected by the Hole scope.
    point_record_index: u32,
    /// Byte offset of the point-data record header.
    point_record_byte_offset: u64,
    /// Hole entry position in source model centimetres.
    position: [f64; 3],
    /// Byte offset of the first position coordinate.
    position_offset: u64,
    /// Directed drilling vector in model space.
    direction: [f64; 3],
    /// Byte offset of the first direction component.
    direction_offset: u64,
    /// Two point-construction parameters carried by the point-data base level.
    point_parameters: [f64; 2],
    /// Byte offsets of the two point-construction parameters.
    point_parameter_offsets: [u64; 2],
    /// `refType` construction rule carried by the point-data record.
    reference_type: u32,
    /// Byte offset of `reference_type`.
    reference_type_offset: u64,
    /// Tangent-point data carried by the version-four point-data base level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangent_point_data"
    )]
    tangent_point_data: Option<[f64; 3]>,
    /// Serialized byte immediately before the version-four tangent-point data.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangent_point_data_prefix"
    )]
    tangent_point_data_prefix: Option<u8>,
    /// Byte offset of the first version-four tangent-point component.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangent_point_data_offset"
    )]
    tangent_point_data_offset: Option<u64>,
    /// Record indices of the counted input-reference run.
    input_record_indices: Vec<u32>,
    /// Byte offsets of the input-reference targets.
    input_record_offsets: Vec<u64>,
    /// Direct persistent face selection carried by the Hole scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_face_selection"
    )]
    face_selection: Option<DesignHoleFaceSelection>,
}

impl TryFrom<DesignHoleConstructionWire> for DesignHoleConstruction {
    type Error = String;
    fn try_from(wire: DesignHoleConstructionWire) -> Result<Self, Self::Error> {
        if wire.input_record_indices.len() != wire.input_record_offsets.len() {
            return Err(
                "input_record_indices and input_record_offsets must have equal lengths".into(),
            );
        }
        Ok(Self {
            point_record_index: wire.point_record_index,
            point_record_byte_offset: wire.point_record_byte_offset,
            position: wire.position,
            position_offset: wire.position_offset,
            direction: wire.direction,
            direction_offset: wire.direction_offset,
            point_parameters: wire.point_parameters,
            point_parameter_offsets: wire.point_parameter_offsets,
            reference_type: wire.reference_type,
            reference_type_offset: wire.reference_type_offset,
            tangent_point_data: match (wire.tangent_point_data, wire.tangent_point_data_prefix, wire.tangent_point_data_offset) {
                (None, None, None) => None,
                (Some(value), Some(prefix), Some(offset)) => Some(DesignHoleTangentPoint { prefix, data: Located { value, offset } }),
                _ => return Err("tangent_point_data, tangent_point_data_prefix and tangent_point_data_offset must occur together".into()),
            },
            input_records: wire.input_record_indices.into_iter().zip(wire.input_record_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            face_selection: wire.face_selection,
        })
    }
}

impl From<DesignHoleConstruction> for DesignHoleConstructionWire {
    fn from(record: DesignHoleConstruction) -> Self {
        Self {
            point_record_index: record.point_record_index,
            point_record_byte_offset: record.point_record_byte_offset,
            position: record.position,
            position_offset: record.position_offset,
            direction: record.direction,
            direction_offset: record.direction_offset,
            point_parameters: record.point_parameters,
            point_parameter_offsets: record.point_parameter_offsets,
            reference_type: record.reference_type,
            reference_type_offset: record.reference_type_offset,
            tangent_point_data: record
                .tangent_point_data
                .as_ref()
                .map(|tangent| tangent.data.value),
            tangent_point_data_prefix: record
                .tangent_point_data
                .as_ref()
                .map(|tangent| tangent.prefix),
            tangent_point_data_offset: record
                .tangent_point_data
                .as_ref()
                .map(|tangent| tangent.data.offset),
            input_record_indices: record
                .input_records
                .iter()
                .map(|reference| reference.value)
                .collect(),
            input_record_offsets: record
                .input_records
                .iter()
                .map(|reference| reference.offset)
                .collect(),
            face_selection: record.face_selection,
        }
    }
}

/// Direct persistent face selection carried by a `Hole` scope.
///
/// Hole selections are scope references rather than construction-group
/// members. Their envelope is the same persistent entity-selection grammar
/// used by grouped operands, but the scope owns the selection directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignHoleFaceSelectionWire",
    into = "DesignHoleFaceSelectionWire"
)]
pub struct DesignHoleFaceSelection {
    /// Indexed record carrying the persistent selection envelope.
    pub record_index: u32,
    /// Byte offset of the selection envelope header.
    pub byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Primary persistent identity of the selected face.
    pub primary_identity: u64,
    /// Byte offset of the primary persistent identity.
    pub primary_identity_offset: u64,
    /// Secondary identity and any dependent curve identity, with their source locations.
    pub secondary: Option<DesignSecondaryIdentity<Located<u64>>>,
    /// History-qualified face proofs for the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Indexed record immediately following the selection envelope.
    pub next_record_index: u32,
    /// Byte offset of the following indexed record.
    pub next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHoleFaceSelectionWire {
    /// Indexed record carrying the persistent selection envelope.
    record_index: u32,
    /// Byte offset of the selection envelope header.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    identity_record_index: u32,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Primary persistent identity of the selected face.
    primary_identity: u64,
    /// Byte offset of the primary persistent identity.
    primary_identity_offset: u64,
    /// Optional secondary persistent identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_identity"
    )]
    secondary_identity: Option<u64>,
    /// Byte offset of the optional secondary persistent identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_identity_offset"
    )]
    secondary_identity_offset: Option<u64>,
    /// Optional secondary identity of a selected Sketch curve.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curve_secondary_identity"
    )]
    curve_secondary_identity: Option<u64>,
    /// Byte offset of the optional Sketch-curve secondary identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curve_secondary_identity_offset"
    )]
    curve_secondary_identity_offset: Option<u64>,
    /// History-qualified face proofs for the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Indexed record immediately following the selection envelope.
    next_record_index: u32,
    /// Byte offset of the following indexed record.
    next_byte_offset: u64,
}

impl TryFrom<DesignHoleFaceSelectionWire> for DesignHoleFaceSelection {
    type Error = String;
    fn try_from(wire: DesignHoleFaceSelectionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id,
            context_id_offset: wire.context_id_offset,
            identity_record_index: wire.identity_record_index,
            identity_record_offset: wire.identity_record_offset,
            primary_identity: wire.primary_identity,
            primary_identity_offset: wire.primary_identity_offset,
            secondary: DesignSecondaryIdentity::from_wire(
                Located::from_wire(
                    wire.secondary_identity,
                    wire.secondary_identity_offset,
                    "secondary_identity",
                )?,
                Located::from_wire(
                    wire.curve_secondary_identity,
                    wire.curve_secondary_identity_offset,
                    "curve_secondary_identity",
                )?,
            )?,
            historical_face_candidates: wire.historical_face_candidates,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignHoleFaceSelection> for DesignHoleFaceSelectionWire {
    fn from(record: DesignHoleFaceSelection) -> Self {
        Self {
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag.into(),
            asset_id: record.asset_id,
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id,
            context_id_offset: record.context_id_offset,
            identity_record_index: record.identity_record_index,
            identity_record_offset: record.identity_record_offset,
            primary_identity: record.primary_identity,
            primary_identity_offset: record.primary_identity_offset,
            secondary_identity: record.secondary.map(|secondary| secondary.identity.value),
            secondary_identity_offset: record.secondary.map(|secondary| secondary.identity.offset),
            curve_secondary_identity: record
                .secondary
                .and_then(|secondary| secondary.curve_identity)
                .map(|identity| identity.value),
            curve_secondary_identity_offset: record
                .secondary
                .and_then(|secondary| secondary.curve_identity)
                .map(|identity| identity.offset),
            historical_face_candidates: record.historical_face_candidates,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}
