// SPDX-License-Identifier: Apache-2.0
//! Hole constructions, their tangent points and face selections.

use crate::records::identity::{DesignSecondaryIdentity, Located, Sourced};
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{ser::SerializeSeq, Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_curve_secondary_identity,
    u64,
    "curve_secondary_identity"
);
cadmpeg_core::named_optional_field!(
    deserialize_face_selection,
    DesignHoleFaceSelection,
    "face_selection"
);
cadmpeg_core::named_optional_field!(deserialize_secondary_identity, u64, "secondary_identity");
cadmpeg_core::named_optional_field!(
    deserialize_tangent_point_data,
    [FiniteReal; 3],
    "tangent_point_data"
);
cadmpeg_core::named_optional_field!(
    deserialize_tangent_point_data_prefix,
    u8,
    "tangent_point_data_prefix"
);
cadmpeg_core::named_optional_field!(deserialize_extent_control, u8, "extent_control");
cadmpeg_core::named_optional_field!(
    deserialize_extent_leading_word,
    Sourced<u32>,
    "leading_word"
);
cadmpeg_core::named_optional_field!(
    deserialize_saved_non_distance_depth,
    Sourced<FiniteReal>,
    "saved_non_distance_depth"
);
cadmpeg_core::named_optional_field!(
    deserialize_direction_control,
    Located<u8>,
    "direction_control"
);

pub(crate) fn valid_hole_extent_control(control: u8) -> bool {
    matches!(control, 0 | 1)
}

pub(crate) fn valid_hole_direction_control(control: u8) -> bool {
    matches!(control, 0 | 1)
}

/// Carrier `refType` of a Hole placed by its sketch-point records.
const HOLE_CARRIER_SKETCH_POINTS: u32 = 13;
/// Carrier `refType` of a Hole placed at one point on its support face.
const HOLE_CARRIER_FACE_POINT: u32 = 19;
/// Point-data `refType` of a sketch-point record.
const POINT_DATA_SKETCH_POINT: u32 = 10;
/// Accepted difference, in source centimetres, between a carrier position and
/// the value that the placement form requires.
const EPS_HOLE_CARRIER_POSITION_CM: f64 = 1.0e-9;

fn hole_position_matches(left: &[FiniteReal; 3], right: &[f64; 3]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| (left.get() - right).abs() <= EPS_HOLE_CARRIER_POSITION_CM)
}

/// One point-data record that a `Hole` scope references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignHoleLocation {
    /// Point-data record referenced by the Hole scope.
    pub(crate) record_index: u32,
    /// Point coordinates in source model centimetres.
    pub(crate) position: [FiniteReal; 3],
    /// Byte offset of the first position coordinate.
    pub(crate) position_offset: u64,
    /// `refType` of the point-data record.
    pub(crate) reference_type: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignHoleExtentWire")]
pub(crate) struct DesignHoleExtent {
    /// Located u32 word before the extent controls in type versions 5 and 7.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) leading_word: Option<Sourced<u32>>,
    /// Extent control; type version 3 stores none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extent_control: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) saved_non_distance_depth: Option<Sourced<FiniteReal>>,
    /// Located control of type versions 5 and 7: `0` travels against the
    /// carrier normal and `1` travels along it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) direction_control: Option<Located<u8>>,
}

#[derive(Deserialize)]
struct DesignHoleExtentWire {
    #[serde(default, deserialize_with = "deserialize_extent_leading_word")]
    leading_word: Option<Sourced<u32>>,
    #[serde(default, deserialize_with = "deserialize_extent_control")]
    extent_control: Option<u8>,
    #[serde(default, deserialize_with = "deserialize_saved_non_distance_depth")]
    saved_non_distance_depth: Option<Sourced<FiniteReal>>,
    #[serde(default, deserialize_with = "deserialize_direction_control")]
    direction_control: Option<Located<u8>>,
}

impl TryFrom<DesignHoleExtentWire> for DesignHoleExtent {
    type Error = &'static str;

    fn try_from(wire: DesignHoleExtentWire) -> Result<Self, Self::Error> {
        if wire
            .extent_control
            .is_some_and(|control| !valid_hole_extent_control(control))
        {
            return Err("invalid Hole extent control");
        }
        if wire
            .direction_control
            .is_some_and(|control| !valid_hole_direction_control(control.value))
        {
            return Err("invalid Hole direction control");
        }
        Ok(Self {
            leading_word: wire.leading_word,
            extent_control: wire.extent_control,
            saved_non_distance_depth: wire.saved_non_distance_depth,
            direction_control: wire.direction_control,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DesignHoleScope {
    pub(crate) point: Option<DesignHoleConstruction>,
    pub(crate) extent: Option<DesignHoleExtent>,
}

/// Tangent-point payload of a version-four Hole point carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignHoleTangentPoint {
    pub(crate) prefix: u8,
    pub(crate) data: Sourced<[FiniteReal; 3]>,
}

/// Exact point-and-direction construction carried by a `Hole` scope.
///
/// The native point carrier stores the coordinates in source centimetres and
/// the direction as a unit model-space vector. The remaining fields preserve
/// the carrier's base-level evidence so later Hole forms can bind their input
/// records without reparsing the byte stream.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignHoleConstructionWire")]
pub(crate) struct DesignHoleConstruction {
    /// Point-data record selected by the Hole scope.
    pub(crate) point_record_index: u32,
    /// Byte offset of the point-data record header of a located scope.
    pub(crate) point_record_byte_offset: Option<u64>,
    /// Hole entry position in source model centimetres.
    pub(crate) position: [FiniteReal; 3],
    /// Byte offset of the first position coordinate of a located scope.
    pub(crate) position_offset: Option<u64>,
    /// Directed drilling vector in model space.
    pub(crate) direction: [FiniteReal; 3],
    /// Byte offset of the first direction component of a located scope.
    pub(crate) direction_offset: Option<u64>,
    /// Two point-construction parameters carried by the point-data base level.
    pub(crate) point_parameters: [FiniteReal; 2],
    /// Byte offsets of the two point-construction parameters of a located
    /// scope.
    pub(crate) point_parameter_offsets: Option<[u64; 2]>,
    /// `refType` construction rule carried by the point-data record.
    pub(crate) reference_type: u32,
    /// Byte offset of `reference_type` of a located scope.
    pub(crate) reference_type_offset: Option<u64>,
    /// Version-four tangent-point data with its prefix.
    pub(crate) tangent_point_data: Option<DesignHoleTangentPoint>,
    /// Targets of the counted input-reference run.
    pub(crate) input_records: Vec<Sourced<u32>>,
    /// Direct persistent face selection carried by the Hole scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) face_selection: Option<DesignHoleFaceSelection>,
    /// Point-data records of the Hole scope, in reference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) locations: Vec<DesignHoleLocation>,
}

impl DesignHoleConstruction {
    /// Entry point of each placed Hole, in source model centimetres. `None`
    /// when the carrier and its point-data records do not form an admitted
    /// placement.
    pub(crate) fn entry_points(&self) -> Option<Vec<[FiniteReal; 3]>> {
        if self.locations.is_empty() {
            return None;
        }
        match self.reference_type {
            HOLE_CARRIER_SKETCH_POINTS => (hole_position_matches(&self.position, &[0.0; 3])
                && self
                    .locations
                    .iter()
                    .all(|location| location.reference_type == POINT_DATA_SKETCH_POINT))
            .then(|| {
                self.locations
                    .iter()
                    .map(|location| location.position)
                    .collect()
            }),
            HOLE_CARRIER_FACE_POINT => self
                .locations
                .iter()
                .all(|location| {
                    hole_position_matches(&self.position, &location.position.map(FiniteReal::get))
                })
                .then(|| vec![self.position]),
            _ => None,
        }
    }

    /// Whether the carrier states that sketch-point records place the Hole.
    pub(crate) fn placed_by_sketch_points(&self) -> bool {
        self.reference_type == HOLE_CARRIER_SKETCH_POINTS
    }

    /// Drilling vector in model space under the scope's direction control.
    pub(crate) fn drilling_direction(&self, direction_control: u8) -> Option<[f64; 3]> {
        match direction_control {
            0 => Some(self.direction.map(|component| -component.get())),
            1 => Some(self.direction.map(FiniteReal::get)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHoleConstructionWire {
    /// Point-data record selected by the Hole scope.
    point_record_index: u32,
    /// Hole entry position in source model centimetres.
    position: [FiniteReal; 3],
    /// Directed drilling vector in model space.
    direction: [FiniteReal; 3],
    /// Two point-construction parameters carried by the point-data base level.
    point_parameters: [FiniteReal; 2],
    /// `refType` construction rule carried by the point-data record.
    reference_type: u32,
    /// Tangent-point data carried by the version-four point-data base level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangent_point_data"
    )]
    tangent_point_data: Option<[FiniteReal; 3]>,
    /// Serialized byte immediately before the version-four tangent-point data.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangent_point_data_prefix"
    )]
    tangent_point_data_prefix: Option<u8>,
    /// Record indices of the counted input-reference run.
    input_record_indices: Vec<u32>,
    /// Direct persistent face selection carried by the Hole scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_face_selection"
    )]
    face_selection: Option<DesignHoleFaceSelection>,
    /// Point-data records of the Hole scope, in reference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    locations: Vec<DesignHoleLocation>,
}

impl TryFrom<DesignHoleConstructionWire> for DesignHoleConstruction {
    type Error = String;
    fn try_from(wire: DesignHoleConstructionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            point_record_index: wire.point_record_index,
            point_record_byte_offset: None,
            position: wire.position,
            position_offset: None,
            direction: wire.direction,
            direction_offset: None,
            point_parameters: wire.point_parameters,
            point_parameter_offsets: None,
            reference_type: wire.reference_type,
            reference_type_offset: None,
            tangent_point_data: match (wire.tangent_point_data, wire.tangent_point_data_prefix) {
                (None, None) => None,
                (Some(value), Some(prefix)) => Some(DesignHoleTangentPoint {
                    prefix,
                    data: Sourced {
                        value,
                        offset: None,
                    },
                }),
                _ => {
                    return Err(
                        "tangent_point_data and tangent_point_data_prefix must occur together"
                            .into(),
                    )
                }
            },
            input_records: wire
                .input_record_indices
                .into_iter()
                .map(|value| Sourced {
                    value,
                    offset: None,
                })
                .collect(),
            face_selection: wire.face_selection,
            locations: wire.locations,
        })
    }
}

struct InputRecordIndices<'a>(&'a [Sourced<u32>]);

impl Serialize for InputRecordIndices<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for record in self.0 {
            seq.serialize_element(&record.value)?;
        }
        seq.end()
    }
}

#[derive(Serialize)]
struct HoleConstructionView<'a> {
    point_record_index: u32,
    position: &'a [FiniteReal; 3],
    direction: &'a [FiniteReal; 3],
    point_parameters: &'a [FiniteReal; 2],
    reference_type: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tangent_point_data: Option<&'a [FiniteReal; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tangent_point_data_prefix: Option<u8>,
    input_record_indices: InputRecordIndices<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    face_selection: Option<&'a DesignHoleFaceSelection>,
    #[serde(skip_serializing_if = "<[DesignHoleLocation]>::is_empty")]
    locations: &'a [DesignHoleLocation],
}

impl Serialize for DesignHoleConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let tangent = self.tangent_point_data.as_ref();
        HoleConstructionView {
            point_record_index: self.point_record_index,
            position: &self.position,
            direction: &self.direction,
            point_parameters: &self.point_parameters,
            reference_type: self.reference_type,
            tangent_point_data: tangent.map(|value| &value.data.value),
            tangent_point_data_prefix: tangent.map(|value| value.prefix),
            input_record_indices: InputRecordIndices(&self.input_records),
            face_selection: self.face_selection.as_ref(),
            locations: &self.locations,
        }
        .serialize(serializer)
    }
}

/// Direct persistent face selection carried by a `Hole` scope.
///
/// Hole selections are scope references rather than construction-group
/// members. Their envelope is the same persistent entity-selection grammar
/// used by grouped operands, but the scope owns the selection directly.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignHoleFaceSelectionWire")]
pub(crate) struct DesignHoleFaceSelection {
    /// Indexed record carrying the persistent selection envelope.
    pub(crate) record_index: u32,
    /// Byte offset of the selection envelope header of a located scope.
    pub(crate) byte_offset: Option<u64>,
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units of a
    /// located scope.
    pub(crate) asset_id_offset: Option<u64>,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units of a located
    /// scope.
    pub(crate) context_id_offset: Option<u64>,
    /// Nested indexed record carrying the persistent identity.
    pub(crate) identity_record_index: u32,
    /// Byte offset of the nested identity record of a located scope.
    pub(crate) identity_record_offset: Option<u64>,
    /// Primary persistent identity of the selected face.
    pub(crate) primary_identity: u64,
    /// Byte offset of the primary persistent identity of a located scope.
    pub(crate) primary_identity_offset: Option<u64>,
    /// Secondary identity and any dependent curve identity.
    pub(crate) secondary: Option<DesignSecondaryIdentity<Sourced<u64>>>,
    /// History-qualified face proofs for the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Indexed record immediately following the selection envelope.
    pub(crate) next_record_index: u32,
    /// Byte offset of the following indexed record of a located scope.
    pub(crate) next_byte_offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHoleFaceSelectionWire {
    /// Indexed record carrying the persistent selection envelope.
    record_index: u32,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    asset_id: DesignRelaxedGuidText,
    /// UUID of the selection context.
    context_id: DesignRelaxedGuidText,
    /// Nested indexed record carrying the persistent identity.
    identity_record_index: u32,
    /// Primary persistent identity of the selected face.
    primary_identity: u64,
    /// Optional secondary persistent identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_identity"
    )]
    secondary_identity: Option<u64>,
    /// Optional secondary identity of a selected Sketch curve.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curve_secondary_identity"
    )]
    curve_secondary_identity: Option<u64>,
    /// History-qualified face proofs for the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Indexed record immediately following the selection envelope.
    next_record_index: u32,
}

impl TryFrom<DesignHoleFaceSelectionWire> for DesignHoleFaceSelection {
    type Error = String;
    fn try_from(wire: DesignHoleFaceSelectionWire) -> Result<Self, Self::Error> {
        let unlocated = |value| Sourced {
            value,
            offset: None,
        };
        Ok(Self {
            record_index: wire.record_index,
            byte_offset: None,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id,
            asset_id_offset: None,
            context_id: wire.context_id,
            context_id_offset: None,
            identity_record_index: wire.identity_record_index,
            identity_record_offset: None,
            primary_identity: wire.primary_identity,
            primary_identity_offset: None,
            secondary: DesignSecondaryIdentity::from_wire(
                wire.secondary_identity.map(unlocated),
                wire.curve_secondary_identity.map(unlocated),
            )?,
            historical_face_candidates: wire.historical_face_candidates,
            next_record_index: wire.next_record_index,
            next_byte_offset: None,
        })
    }
}

#[derive(Serialize)]
struct HoleFaceSelectionView<'a> {
    record_index: u32,
    class_tag: &'a str,
    asset_id: &'a DesignRelaxedGuidText,
    context_id: &'a DesignRelaxedGuidText,
    identity_record_index: u32,
    primary_identity: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    secondary_identity: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    curve_secondary_identity: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    historical_face_candidates: &'a Vec<DesignEntitySelectionFaceCandidate>,
    next_record_index: u32,
}

impl Serialize for DesignHoleFaceSelection {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let secondary = self.secondary;
        let curve = secondary.and_then(|value| value.curve_identity);
        HoleFaceSelectionView {
            record_index: self.record_index,
            class_tag: self.class_tag.as_str(),
            asset_id: &self.asset_id,
            context_id: &self.context_id,
            identity_record_index: self.identity_record_index,
            primary_identity: self.primary_identity,
            secondary_identity: secondary.map(|value| value.identity.value),
            curve_secondary_identity: curve.map(|value| value.value),
            historical_face_candidates: &self.historical_face_candidates,
            next_record_index: self.next_record_index,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::{DesignHoleConstruction, DesignHoleFaceSelection};
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn hole_extent_round_trip_checks_control() {
        for control in [0, 1] {
            let value = serde_json::json!({"extent_control": control, "saved_non_distance_depth": {"value": 1.0}});
            let extent: super::DesignHoleExtent = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(extent).unwrap(), value);
        }
        let worded_without_offset =
            serde_json::json!({"leading_word": {"value": 2}, "extent_control": 0});
        let extent: super::DesignHoleExtent =
            serde_json::from_value(worded_without_offset.clone()).unwrap();
        assert_eq!(serde_json::to_value(extent).unwrap(), worded_without_offset);
        let directed = serde_json::json!({
            "extent_control": 0, "direction_control": {"value": 1, "offset": 7}
        });
        let extent: super::DesignHoleExtent = serde_json::from_value(directed.clone()).unwrap();
        assert_eq!(serde_json::to_value(extent).unwrap(), directed);
        assert!(serde_json::from_value::<super::DesignHoleExtent>(
            serde_json::json!({"direction_control": {"value": 2, "offset": 7}})
        )
        .is_err());
        assert!(serde_json::from_value::<super::DesignHoleExtent>(
            serde_json::json!({"direction_control": null})
        )
        .is_err());
        let worded =
            serde_json::json!({"leading_word": {"value": 2}, "extent_control": 0});
        let extent: super::DesignHoleExtent = serde_json::from_value(worded.clone()).unwrap();
        assert_eq!(serde_json::to_value(extent).unwrap(), worded);
        assert!(serde_json::from_value::<super::DesignHoleExtent>(
            serde_json::json!({"leading_word": null, "extent_control": 0})
        )
        .is_err());
        let bare = serde_json::json!({});
        let extent: super::DesignHoleExtent = serde_json::from_value(bare.clone()).unwrap();
        assert_eq!(extent.extent_control, None);
        assert_eq!(serde_json::to_value(extent).unwrap(), bare);
        assert!(serde_json::from_value::<super::DesignHoleExtent>(
            serde_json::json!({"extent_control": null})
        )
        .is_err());
        assert!(serde_json::from_value::<super::DesignHoleExtent>(
            serde_json::json!({"extent_control": 2})
        )
        .is_err());
        assert!(serde_json::from_value::<super::DesignHoleExtent>(
            serde_json::json!({"extent_control": 1, "saved_non_distance_depth": null})
        )
        .is_err());
    }

    fn carrier(
        reference_type: u32,
        position: [f64; 3],
        locations: &[(u32, [f64; 3])],
    ) -> DesignHoleConstruction {
        let locations: Vec<_> = locations
            .iter()
            .enumerate()
            .map(|(index, (reference_type, position))| {
                serde_json::json!({
                    "record_index": index, "position": position,
                    "position_offset": index, "reference_type": reference_type
                })
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "point_record_index": 1, "point_record_byte_offset": 2,
            "position": position, "position_offset": 3,
            "direction": [0.0, 0.0, 1.0], "direction_offset": 4,
            "point_parameters": [0.0, 0.0], "point_parameter_offsets": [5, 6],
            "reference_type": reference_type, "reference_type_offset": 7,
            "input_record_indices": [8], "input_record_offsets": [9],
            "locations": locations
        }))
        .unwrap()
    }

    fn entry_points(construction: &DesignHoleConstruction) -> Option<Vec<[f64; 3]>> {
        construction.entry_points().map(|points| {
            points
                .into_iter()
                .map(|point| point.map(cadmpeg_ir::scalar::FiniteReal::get))
                .collect()
        })
    }

    const A: [f64; 3] = [1.0, 1.5, 2.0];
    const B: [f64; 3] = [3.0, 1.5, 2.0];
    const ZERO: [f64; 3] = [0.0, 0.0, 0.0];

    #[test]
    fn sketch_point_carrier_places_one_hole_per_point_record() {
        let two = carrier(13, ZERO, &[(10, A), (10, B)]);
        assert_eq!(entry_points(&two), Some(vec![A, B]));
        let one = carrier(13, ZERO, &[(10, B)]);
        assert_eq!(entry_points(&one), Some(vec![B]));
    }

    #[test]
    fn sketch_point_carrier_needs_every_record_to_be_a_sketch_point() {
        assert_eq!(entry_points(&carrier(13, ZERO, &[(10, A), (17, B)])), None);
        assert_eq!(entry_points(&carrier(13, ZERO, &[(17, A), (10, B)])), None);
        assert_eq!(entry_points(&carrier(13, ZERO, &[(17, A), (17, B)])), None);
    }

    #[test]
    fn sketch_point_carrier_with_a_stored_position_defines_no_placement() {
        assert_eq!(entry_points(&carrier(13, A, &[(10, A)])), None);
        assert_eq!(
            entry_points(&carrier(13, [0.0, 0.0, 1.0], &[(10, A)])),
            None
        );
    }

    #[test]
    fn face_point_carrier_needs_every_record_at_its_position() {
        assert_eq!(entry_points(&carrier(19, A, &[(17, A)])), Some(vec![A]));
        assert_eq!(
            entry_points(&carrier(19, A, &[(6, A), (5, A)])),
            Some(vec![A])
        );
        assert_eq!(entry_points(&carrier(19, A, &[(6, A), (5, B)])), None);
        assert_eq!(entry_points(&carrier(19, A, &[(6, B), (5, A)])), None);
        assert_eq!(entry_points(&carrier(19, A, &[(6, B), (5, B)])), None);
    }

    #[test]
    fn hole_without_point_records_or_with_another_form_has_no_entry_point() {
        assert_eq!(entry_points(&carrier(13, ZERO, &[])), None);
        assert_eq!(entry_points(&carrier(19, A, &[])), None);
        for other in [0, 1, 10, 12, 14, 18, 20] {
            assert_eq!(entry_points(&carrier(other, A, &[(other, A)])), None);
        }
    }

    #[test]
    fn direction_control_states_the_travel_from_the_carrier_normal() {
        let hole = carrier(19, A, &[(17, A)]);
        assert_eq!(hole.drilling_direction(0), Some([-0.0, -0.0, -1.0]));
        assert_eq!(hole.drilling_direction(1), Some([0.0, 0.0, 1.0]));
        assert_eq!(hole.drilling_direction(2), None);
    }

    #[test]
    fn hole_locations_round_trip_and_stay_absent_when_empty() {
        let located = carrier(19, A, &[(17, A)]);
        let value = serde_json::to_value(&located).unwrap();
        assert_eq!(
            value["locations"],
            serde_json::json!([{
                "record_index": 0, "position": A, "position_offset": 0, "reference_type": 17
            }])
        );
        let decoded: DesignHoleConstruction = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, located);
        let bare = serde_json::to_value(carrier(19, A, &[])).unwrap();
        assert!(bare.get("locations").is_none());
    }

    #[test]
    fn hole_construction_input_columns_stream_once_with_retained_limit() {
        #[derive(serde::Serialize)]
        struct Row<'a> {
            id: &'static str,
            construction: &'a DesignHoleConstruction,
        }

        let construction = serde_json::json!({
            "point_record_index": 1,
            "position": [0.0, 1.0, 2.0],
            "direction": [0.0, 0.0, 1.0],
            "point_parameters": [0.0, 1.0],
            "reference_type": 0,
            "input_record_indices": [8]
        });
        let admitted: DesignHoleConstruction =
            serde_json::from_value(construction.clone()).expect("valid fixture");
        let row = Row {
            id: "f3d:design:hole#1",
            construction: &admitted,
        };
        assert_native_limit(
            &row,
            serde_json::json!({"id": row.id, "construction": construction}),
        );
    }

    #[test]
    fn hole_face_selection_streams_once_with_retained_limit() {
        #[derive(serde::Serialize)]
        struct Row<'a> {
            id: &'static str,
            selection: &'a DesignHoleFaceSelection,
        }

        let selection = serde_json::json!({
            "record_index": 1, "class_tag": "451",
            "asset_id": "11111111-1111-1111-1111-111111111111",
            "context_id": "22222222-2222-2222-2222-222222222222",
            "identity_record_index": 5,
            "primary_identity": 7,
            "next_record_index": 9
        });
        let admitted: DesignHoleFaceSelection =
            serde_json::from_value(selection.clone()).expect("valid fixture");
        let row = Row {
            id: "f3d:design:hole-selection#1",
            selection: &admitted,
        };
        assert_native_limit(
            &row,
            serde_json::json!({"id": row.id, "selection": selection}),
        );
    }
}

mod identity_rewrite;
