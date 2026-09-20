// SPDX-License-Identifier: Apache-2.0
//! Exact work-plane, work-axis and joint-origin frames.

use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::marked_record_reference;
use crate::bytes::f64s_at;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::joint_origin_legacy_class_337_266_frame as joint_origin_class_337_266;
use crate::layout::work_axis_direct_carrier_class_297 as work_axis_297;
use crate::layout::work_axis_direct_carrier_class_335 as work_axis_335;
use crate::layout::work_plane_legacy_321_opaque_matrix_frame as work_plane_321_opaque;
use crate::layout::work_plane_legacy_325_matrix_frame as work_plane_325;
use crate::layout::work_plane_legacy_337_matrix_frame as work_plane_337;
use crate::layout::work_plane_legacy_class_256_matrix_frame as work_plane_class_256;
use crate::layout::work_plane_legacy_class_290_matrix_frame as work_plane_class_290;
use crate::layout::work_plane_legacy_class_322_332_matrix_frame as work_plane_class_322_332;
use crate::layout::work_plane_legacy_class_337_325_matrix_frame as work_plane_class_337_325;
use crate::layout::work_plane_legacy_class_400_matrix_frame as work_plane_legacy;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignWorkAxisConstruction;
use crate::records::feature::work_geometry::DesignWorkAxisSource;
use cadmpeg_core::decode::View;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ScopePlacementFrame {
    pub(super) transform: crate::records::sketch_placement::SketchPlacementMatrix,
    pub(super) transform_offset: u64,
    pub(super) reference: Option<(u32, u64)>,
}

pub(super) fn exact_work_plane_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<ScopePlacementFrame> {
    let mut candidates = Vec::new();
    for record_index in scope.reference_members().values() {
        for (start, paired) in records.frames(*record_index) {
            let frame_length = paired.checked_sub(start)?;
            let (matrix_at, reference) = match frame_length {
                work_plane_legacy::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"400")
                        && bytes.get(paired + 4..paired + 7) == Some(b"262")
                        && bytes.get(start + 11..start + work_plane_legacy::MATRIX)
                            == Some(&[0u8; work_plane_legacy::MATRIX - 11][..]) =>
                {
                    (start + work_plane_legacy::MATRIX, None)
                }
                work_plane_class_290::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"290")
                        && bytes.get(paired + 4..paired + 7) == Some(b"262")
                        && bytes.get(start + 11..start + work_plane_class_290::PREFIX_MARKER)
                            == Some(&[0u8; work_plane_class_290::PREFIX_MARKER - 11][..])
                        && bytes.get(
                            start + work_plane_class_290::PREFIX_MARKER
                                ..start + work_plane_class_290::MATRIX,
                        ) == Some(&[1, 1, 0, 0][..]) =>
                {
                    (start + work_plane_class_290::MATRIX, None)
                }
                work_plane_325::LEN
                    if matches!(
                        (
                            bytes.get(start + 4..start + 7),
                            bytes.get(paired + 4..paired + 7),
                        ),
                        (Some(b"320"), Some(b"258"))
                            | (Some(b"380"), Some(b"262"))
                            | (Some(b"308" | b"431"), Some(b"257"))
                            | (Some(b"364"), Some(b"263"))
                    ) && bytes.get(start + 11..start + work_plane_325::MATRIX)
                        == Some(&[0u8; work_plane_325::MATRIX - 11][..]) =>
                {
                    (start + work_plane_325::MATRIX, None)
                }
                work_plane_class_256::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"256")
                        && bytes.get(paired + 4..paired + 7) == Some(b"262")
                        && bytes.get(start + 11..start + work_plane_class_256::OPAQUE_U16)
                            == Some(&[0u8; work_plane_class_256::OPAQUE_U16 - 11][..])
                        && bytes.get(
                            start + work_plane_class_256::ZERO_PAIR
                                ..start + work_plane_class_256::MATRIX,
                        ) == Some(
                            &[0u8; work_plane_class_256::MATRIX - work_plane_class_256::ZERO_PAIR]
                                [..],
                        ) =>
                {
                    (start + work_plane_class_256::MATRIX, None)
                }
                work_plane_class_337_325::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"337")
                        && bytes.get(paired + 4..paired + 7) == Some(b"266")
                        && bytes.get(start + 11..start + work_plane_class_337_325::OPAQUE_U16)
                            == Some(&[0u8; work_plane_class_337_325::OPAQUE_U16 - 11][..])
                        && bytes.get(
                            start + work_plane_class_337_325::ZERO_PAIR
                                ..start + work_plane_class_337_325::MATRIX,
                        ) == Some(
                            &[0u8; work_plane_class_337_325::MATRIX
                                - work_plane_class_337_325::ZERO_PAIR][..],
                        ) =>
                {
                    (start + work_plane_class_337_325::MATRIX, None)
                }
                work_plane_class_322_332::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"322")
                        && bytes.get(paired + 4..paired + 7) == Some(b"261")
                        && bytes.get(start + 11..start + work_plane_class_322_332::MATRIX)
                            == Some(&[0u8; work_plane_class_322_332::MATRIX - 11][..]) =>
                {
                    (start + work_plane_class_322_332::MATRIX, None)
                }
                321 if bytes.get(start + 11..start + 49) == Some(&[0u8; 38][..]) => {
                    (start + 49, None)
                }
                work_plane_321_opaque::LEN
                    if matches!(
                        (
                            bytes.get(start + 4..start + 7),
                            bytes.get(paired + 4..paired + 7),
                        ),
                        (Some(b"341"), Some(b"261")) | (Some(b"346"), Some(b"262"))
                    ) && bytes.get(start + 11..start + work_plane_321_opaque::OPAQUE_U16)
                        == Some(&[0u8; work_plane_321_opaque::OPAQUE_U16 - 11][..])
                        && bytes.get(
                            start + work_plane_321_opaque::ZERO_PAIR
                                ..start + work_plane_321_opaque::MATRIX,
                        ) == Some(
                            &[0u8; work_plane_321_opaque::MATRIX
                                - work_plane_321_opaque::ZERO_PAIR][..],
                        ) =>
                {
                    (start + work_plane_321_opaque::MATRIX, None)
                }
                321 if bytes.get(start + 4..start + 7) == Some(b"364")
                    && bytes.get(paired + 4..paired + 7) == Some(b"264")
                    && bytes.get(start + 11..start + 46) == Some(&[0u8; 35][..])
                    && bytes.get(start + 46..start + 49) == Some(&[1, 0, 0][..]) =>
                {
                    (start + 49, None)
                }
                321 if bytes.get(start + 4..start + 7) == Some(b"364")
                    && bytes.get(paired + 4..paired + 7) == Some(b"264")
                    && bytes.get(start + 11..start + 45) == Some(&[0u8; 34][..])
                    && bytes.get(start + 45..start + 49) == Some(&[0xcc, 0xcd, 0, 0][..]) =>
                {
                    (start + 49, None)
                }
                326 if matches!(
                    (
                        bytes.get(start + 4..start + 7),
                        bytes.get(paired + 4..paired + 7),
                    ),
                    (Some(b"279"), Some(b"266"))
                        | (Some(b"409"), Some(b"258"))
                        | (Some(b"450"), Some(b"259"))
                ) && bytes.get(start + 11..start + 50) == Some(&[0u8; 39][..]) =>
                {
                    (start + 50, None)
                }
                work_plane_337::LEN
                    if matches!(
                        (
                            bytes.get(start + 4..start + 7),
                            bytes.get(paired + 4..paired + 7),
                        ),
                        (Some(b"350" | b"409"), Some(b"258"))
                    ) && bytes.get(start + 11..start + work_plane_337::MATRIX)
                        == Some(&[0u8; work_plane_337::MATRIX - 11][..]) =>
                {
                    (start + work_plane_337::MATRIX, None)
                }
                352 | 363 | 374
                    if bytes.get(start + 55) == Some(&1)
                        && bytes.get(start + 56..start + 66) == Some(&[0u8; 10][..]) =>
                {
                    (start + 66, None)
                }
                362 | 373
                    if bytes.get(start + 55..start + 58) == Some(&[1, 0, 1][..])
                        && bytes.get(start + 62..start + 76) == Some(&[0u8; 14][..]) =>
                {
                    (
                        start + 76,
                        Some((View::u32_le_at(bytes, start + 58)?, (start + 58) as u64)),
                    )
                }
                _ => continue,
            };
            let values = f64s_at(bytes, matrix_at, 16)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            let Ok(transform) =
                crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
            else {
                continue;
            };
            candidates.push(ScopePlacementFrame {
                transform,
                transform_offset: matrix_at as u64,
                reference,
            });
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(super) fn exact_work_axis_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignWorkAxisConstruction> {
    if scope.kind() != scope::DesignFeatureKind::WorkAxis {
        return None;
    }
    exact_two_point_work_axis_construction(bytes, records, scope)
        .or_else(|| exact_direct_work_axis_construction(bytes, records, scope))
}

fn exact_two_point_work_axis_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignWorkAxisConstruction> {
    let [axis_record_index, _, first_point_record_index, _, second_point_record_index] =
        scope.reference_members().values_array()?;
    let axis_frames = records.frames(*axis_record_index).collect::<Vec<_>>();
    let [(axis_start, axis_paired)] = axis_frames.as_slice() else {
        return None;
    };
    if axis_paired.checked_sub(*axis_start)? != 232
        || bytes.get(axis_start + 11..axis_start + 21) != Some(&[0; 10])
        || View::u32_le_at(bytes, axis_start + 21)? != 8
        || View::u32_le_at(bytes, axis_start + 118)? != 2
    {
        return None;
    }
    let values = f64s_at(bytes, axis_start + 25, 8)?;
    if values.iter().any(|value| !value.is_finite()) || values[6..] != [0.0, 0.0] {
        return None;
    }
    let origin: [f64; 3] = values[..3].try_into().ok()?;
    let displacement: [f64; 3] = values[3..6].try_into().ok()?;
    let displacement_length = displacement[0]
        .hypot(displacement[1])
        .hypot(displacement[2]);
    if displacement_length <= f64::EPSILON {
        return None;
    }
    let point_record_indices = [*first_point_record_index, *second_point_record_index];
    for (ordinal, expected) in point_record_indices.iter().enumerate() {
        let reference_at = axis_start + 122 + ordinal * 11;
        if bytes.get(reference_at) != Some(&1)
            || View::u32_le_at(bytes, reference_at + 1)? != *expected
            || bytes.get(reference_at + 5..reference_at + 11) != Some(&[0; 6])
        {
            return None;
        }
    }
    let mut points = [[0.0; 3]; 2];
    let mut point_offsets = [0; 2];
    for (ordinal, record_index) in point_record_indices.iter().enumerate() {
        let point_frames = records.frames(*record_index).collect::<Vec<_>>();
        let [(start, paired)] = point_frames.as_slice() else {
            return None;
        };
        if paired.checked_sub(*start)? != 197 || bytes.get(start + 11..start + 42) != Some(&[0; 31])
        {
            return None;
        }
        let point = f64s_at(bytes, start + 42, 3)?;
        if point.iter().any(|value| !value.is_finite()) {
            return None;
        }
        points[ordinal] = point.try_into().ok()?;
        point_offsets[ordinal] = u64::try_from(start + 42).ok()?;
    }
    let endpoint = std::array::from_fn(|axis| origin[axis] + displacement[axis]);
    if points != [origin, endpoint] {
        return None;
    }
    Some(DesignWorkAxisConstruction {
        origin,
        displacement,
        origin_offset: u64::try_from(axis_start + 25).ok()?,
        displacement_offset: u64::try_from(axis_start + 49).ok()?,
        source: Some(DesignWorkAxisSource::TwoPoint {
            point_record_indices,
            point_offsets,
        }),
    })
}

fn exact_direct_work_axis_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignWorkAxisConstruction> {
    let [carrier_record_index, support_record_index] = scope.reference_members().values_array()?;
    let (
        carrier_class,
        carrier_paired_class,
        carrier_length,
        support_class,
        support_paired_class,
        value_count_offset,
        axis_values_offset,
        reference_count_offset,
        reference_preamble_offset,
    ) = match (
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length(),
    ) {
        ("302", "262", 268) => (
            "297",
            "262",
            work_axis_297::LEN,
            "306",
            "262",
            work_axis_297::VALUE_COUNT,
            work_axis_297::AXIS_VALUES,
            work_axis_297::REFERENCE_COUNT,
            work_axis_297::REFERENCE_PREAMBLE,
        ),
        ("361", "258", 254) => (
            "335",
            "258",
            work_axis_335::LEN,
            "349",
            "258",
            work_axis_335::VALUE_COUNT,
            work_axis_335::AXIS_VALUES,
            work_axis_335::REFERENCE_COUNT,
            work_axis_335::REFERENCE_PREAMBLE,
        ),
        _ => return None,
    };
    let carrier_frames = records.frames(*carrier_record_index).collect::<Vec<_>>();
    let [(carrier_start, carrier_paired)] = carrier_frames.as_slice() else {
        return None;
    };
    let carrier_primary_class =
        exact_indexed_header_at(bytes, *carrier_start, *carrier_record_index)?;
    let carrier_paired_class_tag =
        exact_indexed_header_at(bytes, *carrier_paired, *carrier_record_index)?;
    if carrier_paired.checked_sub(*carrier_start)? != carrier_length
        || carrier_primary_class != carrier_class
        || carrier_paired_class_tag != carrier_paired_class
    {
        return None;
    }
    let support_frames = records.frames(*support_record_index).collect::<Vec<_>>();
    let [(support_start, support_paired)] = support_frames.as_slice() else {
        return None;
    };
    let support_primary_class =
        exact_indexed_header_at(bytes, *support_start, *support_record_index)?;
    let support_paired_class_tag =
        exact_indexed_header_at(bytes, *support_paired, *support_record_index)?;
    if support_paired.checked_sub(*support_start)? != 293
        || support_primary_class != support_class
        || support_paired_class_tag != support_paired_class
    {
        return None;
    }
    if bytes.get(*carrier_start + 11..*carrier_start + 21) != Some(&[0; 10])
        || View::u32_le_at(bytes, *carrier_start + value_count_offset)? != 8
        || View::u32_le_at(bytes, *carrier_start + reference_count_offset)? != 6
        || View::u32_le_at(bytes, *carrier_start + reference_preamble_offset)? != 1
    {
        return None;
    }
    let values = f64s_at(bytes, (*carrier_start).checked_add(axis_values_offset)?, 8)?;
    if values.iter().any(|value| !value.is_finite()) || values[6..] != [0.0, 0.0] {
        return None;
    }
    let origin: [f64; 3] = values[..3].try_into().ok()?;
    let displacement: [f64; 3] = values[3..6].try_into().ok()?;
    let displacement_length = displacement[0]
        .hypot(displacement[1])
        .hypot(displacement[2]);
    if displacement_length <= f64::EPSILON {
        return None;
    }
    Some(DesignWorkAxisConstruction {
        origin,
        displacement,
        origin_offset: u64::try_from((*carrier_start).checked_add(axis_values_offset)?).ok()?,
        displacement_offset: u64::try_from(
            (*carrier_start).checked_add(axis_values_offset + 3 * 8)?,
        )
        .ok()?,
        source: Some(DesignWorkAxisSource::DirectCarrier {
            carrier_record_index: *carrier_record_index,
            support_record_index: *support_record_index,
        }),
    })
}

pub(super) fn exact_joint_origin_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<ScopePlacementFrame> {
    if scope.kind() != scope::DesignFeatureKind::JointOrigin
        || matches!(scope.frame_length(), 300 | 322 | 344)
    {
        return None;
    }
    let mut candidates = Vec::new();
    for record_index in scope.reference_members().values() {
        for (start, paired) in records.frames(*record_index) {
            if paired.checked_sub(start)? == joint_origin_class_337_266::LEN
                && bytes.get(start + 4..start + 7) == Some(b"337")
                && bytes.get(paired + 4..paired + 7) == Some(b"266")
                && bytes.get(start + 11..start + joint_origin_class_337_266::MATRIX_PREFIX)
                    == Some(&[0; joint_origin_class_337_266::MATRIX_PREFIX - 11][..])
                && bytes.get(
                    start + joint_origin_class_337_266::MATRIX_PREFIX
                        ..start + joint_origin_class_337_266::MATRIX,
                ) == Some(&joint_origin_class_337_266::MATRIX_PREFIX_VALUE)
            {
                let values = f64s_at(bytes, start + joint_origin_class_337_266::MATRIX, 16)?;
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.into_iter().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if let Ok(transform) =
                    crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
                {
                    candidates.push(ScopePlacementFrame {
                        transform,
                        transform_offset: (start + joint_origin_class_337_266::MATRIX) as u64,
                        reference: None,
                    });
                }
                continue;
            }
            if paired.checked_sub(start)? == 385
                && bytes.get(start + 4..start + 7) == Some(b"364")
                && bytes.get(paired + 4..paired + 7) == Some(b"264")
                && bytes.get(start + 11..start + 45) == Some(&[0; 34])
                && bytes.get(start + 45..start + 49) == Some(&[1, 1, 0, 0])
            {
                let values = f64s_at(bytes, start + 49, 16)?;
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.into_iter().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if let Ok(transform) =
                    crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
                {
                    candidates.push(ScopePlacementFrame {
                        transform,
                        transform_offset: (start + 49) as u64,
                        reference: None,
                    });
                }
                continue;
            }
            if !matches!(paired.checked_sub(start)?, 336 | 347)
                || bytes.get(start + 11..start + 45)? != [0; 34]
                || bytes.get(start + 50..start + 60)? != [0; 10]
            {
                continue;
            }
            let reference = marked_record_reference(bytes, start + 45)?;
            let values = f64s_at(bytes, start + 60, 16)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            let Ok(transform) =
                crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
            else {
                continue;
            };
            candidates.push(ScopePlacementFrame {
                transform,
                transform_offset: (start + 60) as u64,
                reference: Some((reference, (start + 46) as u64)),
            });
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}
