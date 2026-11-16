// SPDX-License-Identifier: Apache-2.0
//! Exact hole constructions and hole face selections.

use super::point_data::{exact_point_data_construction, POINT_DATA_TYPE_GUID};
use super::point_direction::{point_direction_carrier_at, POINT_DIRECTION_TYPE_GUID};
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::lp_utf16_bounded;
use crate::design::decode::operands::parse_entity_selection_frame;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::hole;
use crate::records::feature::hole::DesignHoleConstruction;
use crate::records::feature::hole::DesignHoleFaceSelection;
use crate::records::feature::hole::DesignHoleLocation;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignWorkPointRuleForm;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Type GUID of the direct persistent face selection carried by a `Hole`.
const HOLE_FACE_SELECTION_TYPE_GUID: &str = "5A1BF548-241F-46FD-9FB5-E4B05126EB9D";

/// Accepted norm error for a serialized Hole drilling direction.
const EPS_HOLE_DIRECTION_NORM: f64 = 1.0e-12;

/// Longest standard sizing name, in UTF-16 code units, that a Hole extent
/// block admits.
const MAX_HOLE_STANDARD_NAME: usize = 256;

/// Decode the exact point-and-direction carrier owned by a `Hole` scope.
///
/// The carrier's versioned base level is distinct from the `WorkPoint` level:
/// it writes the position, direction, two construction parameters, `refType`,
/// and a counted input-reference run. Version four inserts tangent-point data
/// before that run; version one omits it and retains additional class members
/// before the paired header. The type GUID and version select the layout; the
/// dynamic class tag does not.
pub(in crate::design::decode) fn exact_hole_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
    required_scope_kind: &scope::DesignFeatureKind,
) -> Result<Option<DesignHoleConstruction>, CodecError> {
    (|| {
        if &scope.kind() != required_scope_kind {
            return None;
        }
        let face_selection = exact_hole_face_selection(bytes, records, scope, stream_types);
        let mut candidate = None;
        for record_index in scope.reference_members().values() {
            let Some((type_guid, version)) = stream_types.get(&u64::from(*record_index)) else {
                continue;
            };
            if *type_guid != POINT_DIRECTION_TYPE_GUID || !matches!(*version, 1 | 2 | 4) {
                continue;
            }
            for (start, paired_at) in records.frames(*record_index) {
                let Some(carrier) =
                    point_direction_carrier_at(bytes, start, paired_at, *record_index, *version)
                else {
                    continue;
                };
                let direction_norm = carrier
                    .direction
                    .iter()
                    .map(|component| component.get() * component.get())
                    .sum::<f64>();
                if (direction_norm - 1.0).abs() > EPS_HOLE_DIRECTION_NORM {
                    continue;
                }
                let next = DesignHoleConstruction {
                    point_record_index: *record_index,
                    point_record_byte_offset: u64::try_from(start).ok()?,
                    position: carrier.position,
                    position_offset: u64::try_from(carrier.position_at).ok()?,
                    direction: carrier.direction,
                    direction_offset: u64::try_from(carrier.direction_at).ok()?,
                    point_parameters: carrier.point_parameters,
                    point_parameter_offsets: [
                        u64::try_from(carrier.point_parameters_at).ok()?,
                        u64::try_from(carrier.point_parameters_at.checked_add(8)?).ok()?,
                    ],
                    reference_type: carrier.reference_type,
                    reference_type_offset: u64::try_from(carrier.reference_type_at).ok()?,
                    tangent_point_data: carrier.tangent_point_data,
                    input_records: carrier.input_records,
                    face_selection: None,
                    locations: Vec::new(),
                };
                if candidate.replace(next).is_some() {
                    return None;
                }
            }
        }
        candidate.map(|mut candidate| {
            candidate.face_selection = face_selection;
            if *required_scope_kind == scope::DesignFeatureKind::Hole {
                candidate.locations = hole_locations(bytes, records, scope, stream_types);
            }
            Ok(candidate)
        })
    })()
    .transpose()
}

/// Read every point-data record that `scope` references, in reference order.
/// A record that does not read leaves no location, so a partial run cannot
/// stand for the Hole's placements.
fn hole_locations(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Vec<DesignHoleLocation> {
    scope
        .reference_members()
        .values()
        .filter(|record_index| {
            stream_types
                .get(&u64::from(**record_index))
                .is_some_and(|(type_guid, _)| *type_guid == POINT_DATA_TYPE_GUID)
        })
        .map(|record_index| {
            let point =
                exact_point_data_construction(bytes, records, [record_index], stream_types)?;
            Some(DesignHoleLocation {
                record_index: point.point_record_index,
                position: point.position,
                position_offset: point.position_offset,
                reference_type: point.rule.reference_type(),
                vertex_input: match point.rule.form() {
                    DesignWorkPointRuleForm::Vertex { input } => Some(input.clone()),
                    _ => None,
                },
            })
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

/// Read the Hole extent controls of `scope`. `type_version` is the Hole type
/// version that the scope class row states.
pub(super) fn hole_extent(
    bytes: &[u8],
    scope: &DesignParameterScope,
    type_version: Option<u32>,
) -> Option<hole::DesignHoleExtent> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.paired_byte_offset()).ok()?;
    let bytes = bytes.get(..end)?;
    let (_, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    let after_index = after_tag.checked_add(4)?;
    let prologue = after_index.checked_add(8)?;
    if bytes.get(after_index..prologue)? != [0; 8] {
        return None;
    }
    hole_extent_payload(bytes, prologue, extent_layout(type_version?)?)
}

/// The extent block of a Hole type version: whether a u32 word precedes the
/// `01 01` marker, and whether an extent control byte follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExtentLayout {
    leading_word: bool,
    control: bool,
}

fn extent_layout(type_version: u32) -> Option<ExtentLayout> {
    let (leading_word, control) = match type_version {
        3 => (false, false),
        4 => (false, true),
        5 | 7 => (true, true),
        _ => return None,
    };
    Some(ExtentLayout {
        leading_word,
        control,
    })
}

fn hole_extent_payload(
    bytes: &[u8],
    at: usize,
    layout: ExtentLayout,
) -> Option<hole::DesignHoleExtent> {
    if bytes.get(at) != Some(&0) {
        return None;
    }
    let prologue = super::parameter_scope::read_payload_prologue(bytes, at, bytes.len())?;
    if prologue.property_count != u32::from(prologue.saved_hole_depth.is_some()) {
        return None;
    }
    let mut cursor = prologue.end;
    let leading_word = if layout.leading_word {
        let value = View::u32_le_at(bytes, cursor)?;
        let offset = u64::try_from(cursor).ok()?;
        cursor = cursor.checked_add(4)?;
        Some(crate::records::identity::Located { value, offset })
    } else {
        None
    };
    let mut direction_control = None;
    let mut standard = None;
    let mut hole_type = None;
    let extent_control = if layout.control {
        let [1, 1, control, flag, ..] = bytes.get(cursor..)? else {
            return None;
        };
        if !hole::valid_hole_extent_control(*control) {
            return None;
        }
        match (layout.leading_word, *flag) {
            (false, 0) => {}
            (true, 0 | 1) => {
                if let Some(block) = hole_standard_block(bytes, cursor, *flag) {
                    direction_control = block.direction_control;
                    standard = block.standard;
                    hole_type = Some(block.hole_type);
                }
            }
            _ => return None,
        }
        Some(*control)
    } else {
        let [1, 1, ..] = bytes.get(cursor..)? else {
            return None;
        };
        if prologue.saved_hole_depth.is_some() {
            return None;
        }
        None
    };
    Some(hole::DesignHoleExtent {
        leading_word,
        extent_control,
        saved_non_distance_depth: prologue.saved_hole_depth,
        direction_control,
        standard,
        hole_type,
    })
}

/// The fields that type versions 5 and 7 store after their extent controls.
struct HoleStandardBlock {
    standard: Option<hole::DesignHoleStandard>,
    direction_control: Option<crate::records::identity::Located<u8>>,
    hole_type: crate::records::identity::Located<u32>,
}

/// Read the block after the extent controls at `controls`: u32 `1`, four
/// counted UTF-16 sizing names, a u32 hole type, and the direction control.
/// Flag `0` stores four empty names; flag `1` stores the names of a standard
/// hole.
fn hole_standard_block(bytes: &[u8], controls: usize, flag: u8) -> Option<HoleStandardBlock> {
    let mut at = controls.checked_add(4)?;
    if View::u32_le_at(bytes, at)? != 1 {
        return None;
    }
    at = at.checked_add(4)?;
    let names_at = at;
    let mut names = Vec::new();
    for _ in 0..4 {
        let (name, end) = lp_utf16_bounded(bytes, at, 0..=MAX_HOLE_STANDARD_NAME)?;
        names.push(name);
        at = end;
    }
    let hole_type = crate::records::identity::Located {
        value: View::u32_le_at(bytes, at)?,
        offset: u64::try_from(at).ok()?,
    };
    if !hole::valid_hole_type(hole_type.value) {
        return None;
    }
    at = at.checked_add(4)?;
    let value = *bytes.get(at)?;
    let direction_control =
        hole::valid_hole_direction_control(value).then_some(crate::records::identity::Located {
            value,
            offset: u64::try_from(at).ok()?,
        });
    let standard = match (flag, names.as_slice()) {
        (0, [standard, fit, size, fastener])
            if standard.is_empty() && fit.is_empty() && size.is_empty() && fastener.is_empty() =>
        {
            None
        }
        (1, [standard, fit, size, fastener]) if !standard.is_empty() => {
            Some(hole::DesignHoleStandard {
                offset: u64::try_from(names_at).ok()?,
                standard: standard.clone(),
                fit: fit.clone(),
                size: size.clone(),
                fastener: fastener.clone(),
            })
        }
        _ => return None,
    };
    Some(HoleStandardBlock {
        standard,
        direction_control,
        hole_type,
    })
}

fn exact_hole_face_selection(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignHoleFaceSelection> {
    let mut candidate = None;
    for record_index in scope.reference_members().values() {
        if stream_types.get(&u64::from(*record_index)) != Some(&(HOLE_FACE_SELECTION_TYPE_GUID, 1))
        {
            continue;
        }
        for (start, _paired_at) in records.frames(*record_index) {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)
            else {
                continue;
            };
            let Ok(class_tag) = crate::design::decode::text::class_tag_from_view(class_tag) else {
                continue;
            };
            if after_tag != start + 7 || View::u32_le_at(bytes, after_tag) != Some(*record_index) {
                continue;
            }
            let Ok(start_offset) = u64::try_from(start) else {
                return None;
            };
            let Some(frame) = parse_entity_selection_frame(
                bytes,
                *record_index,
                start_offset,
                class_tag.as_str(),
            ) else {
                continue;
            };
            let Ok(asset_id) =
                crate::records::mesh::DesignRelaxedGuidText::try_from(frame.asset_id)
            else {
                continue;
            };
            let Ok(context_id) =
                crate::records::mesh::DesignRelaxedGuidText::try_from(frame.context_id)
            else {
                continue;
            };
            let next = DesignHoleFaceSelection {
                record_index: frame.record_index,
                byte_offset: frame.byte_offset,
                class_tag,
                asset_id,
                asset_id_offset: frame.asset_id_offset,
                context_id,
                context_id_offset: frame.context_id_offset,
                serialization_form: frame.serialization_form,
                identity_record_index: frame.identity_record_index,
                identity_record_offset: frame.identity_record_offset,
                primary_identity: frame.primary_identity,
                primary_identity_offset: frame.primary_identity_offset,
                secondary: frame.secondary,
                historical_face_candidates: Vec::new(),
                next_record_index: frame.next_record_index,
                next_byte_offset: frame.next_byte_offset,
            };
            if candidate.replace(next).is_some() {
                return None;
            }
        }
    }
    candidate
}

#[cfg(test)]
mod tests;
