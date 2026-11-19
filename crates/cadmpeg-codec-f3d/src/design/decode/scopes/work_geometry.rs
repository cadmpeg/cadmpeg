// SPDX-License-Identifier: Apache-2.0
//! Exact work-plane, work-axis and joint-origin frames.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::{marked_record_reference, rigid_transform_at};
use crate::bytes::f64s_at;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::joint_origin_legacy_class_337_266_frame as joint_origin_class_337_266;
use crate::layout::work_plane_direct_352_matrix_frame as work_plane_direct_352;
use crate::layout::work_plane_direct_363_matrix_frame as work_plane_direct_363;
use crate::layout::work_plane_direct_374_matrix_frame as work_plane_direct_374;
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
use crate::records::feature::work_geometry::joint_origin::{
    DesignJointOriginCarrierEvidence, DirectJointOriginCarrier,
};
use cadmpeg_core::decode::View;
use std::collections::HashMap;

#[cfg(test)]
mod tests;

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
) -> Option<(ScopePlacementFrame, [u8; 2])> {
    let mut candidate = None;
    for record_index in scope.reference_members().values() {
        for (start, paired) in records.frames(*record_index) {
            let frame_length = paired.checked_sub(start)?;
            let (matrix_at, reference, inferred_axes) = match frame_length {
                work_plane_legacy::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"400")
                        && bytes.get(paired + 4..paired + 7) == Some(b"262")
                        && bytes.get(start + 11..start + work_plane_legacy::MATRIX)
                            == Some(&[0u8; work_plane_legacy::MATRIX - 11][..]) =>
                {
                    (start + work_plane_legacy::MATRIX, None, [0, 0])
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
                    (start + work_plane_class_290::MATRIX, None, [1, 1])
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
                    (start + work_plane_325::MATRIX, None, [0, 0])
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
                    (
                        start + work_plane_class_256::MATRIX,
                        None,
                        View::u16_le_at(bytes, start + work_plane_class_256::OPAQUE_U16)?
                            .to_le_bytes(),
                    )
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
                    (
                        start + work_plane_class_337_325::MATRIX,
                        None,
                        View::u16_le_at(bytes, start + work_plane_class_337_325::OPAQUE_U16)?
                            .to_le_bytes(),
                    )
                }
                work_plane_class_322_332::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"322")
                        && bytes.get(paired + 4..paired + 7) == Some(b"261")
                        && bytes.get(start + 11..start + work_plane_class_322_332::MATRIX)
                            == Some(&[0u8; work_plane_class_322_332::MATRIX - 11][..]) =>
                {
                    (start + work_plane_class_322_332::MATRIX, None, [0, 0])
                }
                321 if bytes.get(start + 11..start + 49) == Some(&[0u8; 38][..]) => {
                    (start + 49, None, [0, 0])
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
                    (
                        start + work_plane_321_opaque::MATRIX,
                        None,
                        View::u16_le_at(bytes, start + work_plane_321_opaque::OPAQUE_U16)?
                            .to_le_bytes(),
                    )
                }
                321 if bytes.get(start + 4..start + 7) == Some(b"364")
                    && bytes.get(paired + 4..paired + 7) == Some(b"264")
                    && bytes.get(start + 11..start + 46) == Some(&[0u8; 35][..])
                    && bytes.get(start + 46..start + 49) == Some(&[1, 0, 0][..]) =>
                {
                    (start + 49, None, [0, 1])
                }
                321 if bytes.get(start + 4..start + 7) == Some(b"364")
                    && bytes.get(paired + 4..paired + 7) == Some(b"264")
                    && bytes.get(start + 11..start + 45) == Some(&[0u8; 34][..])
                    && bytes.get(start + 45..start + 49) == Some(&[0xcc, 0xcd, 0, 0][..]) =>
                {
                    (start + 49, None, [0xcc, 0xcd])
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
                    (start + 50, None, [0, 0])
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
                    (start + work_plane_337::MATRIX, None, [0, 0])
                }
                work_plane_direct_352::LEN
                | work_plane_direct_363::LEN
                | work_plane_direct_374::LEN
                    if bytes.get(start + work_plane_direct_352::MARKER)
                        == Some(&work_plane_direct_352::MARKER_VALUE)
                        && bytes.get(
                            start + work_plane_direct_352::ZERO_RUN
                                ..start + work_plane_direct_352::OPAQUE_U16,
                        ) == Some(
                            &[0u8; work_plane_direct_352::OPAQUE_U16
                                - work_plane_direct_352::ZERO_RUN][..],
                        )
                        && bytes.get(
                            start + work_plane_direct_352::ZERO_PAIR
                                ..start + work_plane_direct_352::MATRIX,
                        ) == Some(
                            &[0u8; work_plane_direct_352::MATRIX
                                - work_plane_direct_352::ZERO_PAIR][..],
                        ) =>
                {
                    (
                        start + work_plane_direct_352::MATRIX,
                        None,
                        View::u16_le_at(bytes, start + work_plane_direct_352::OPAQUE_U16)?
                            .to_le_bytes(),
                    )
                }
                362 | 373
                    if bytes.get(start + 55..start + 58) == Some(&[1, 0, 1][..])
                        && bytes.get(start + 62..start + 76) == Some(&[0u8; 14][..]) =>
                {
                    (
                        start + 76,
                        Some((
                            View::u32_le_at(bytes, start + 58)?,
                            u64_from_index(start + 58),
                        )),
                        [0, 0],
                    )
                }
                _ => continue,
            };
            let values = f64s_at::<16>(bytes, matrix_at)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            let Ok(transform) =
                crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
            else {
                continue;
            };
            if candidate
                .replace((
                    ScopePlacementFrame {
                        transform,
                        transform_offset: u64_from_index(matrix_at),
                        reference,
                    },
                    inferred_axes,
                ))
                .is_some()
            {
                return None;
            }
        }
    }
    candidate
}

pub(super) fn direct_joint_origin_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &HashMap<u64, (&str, u32)>,
) -> Option<(ScopePlacementFrame, DesignJointOriginCarrierEvidence)> {
    use crate::layout::joint_origin_direct_carrier_369 as common;

    let (carrier, start) = DirectJointOriginCarrier::for_scope(scope)?
        .into_iter()
        .filter(|carrier| carrier.has_registered_types(scope, types))
        .find_map(|carrier| {
            let start = usize::try_from(carrier.start).ok()?;
            let end = start.checked_add(carrier.length)?;
            records
                .frames(carrier.record_index)
                .any(|pair| pair == (start, end))
                .then_some((carrier, start))
        })?;
    let mut view = View::over_retained(bytes);
    view.seek(start)?;
    let frame = view.take(carrier.length)?;
    let prefix = &frame[common::ZERO_PREFIX..carrier.matrix];
    let scope_backlink = frame[carrier.scope_backlink..].try_into().ok()?;
    let reference = carrier.construction_index(prefix, scope_backlink, scope.record_index)?;
    if records.offsets(reference).is_empty() {
        return None;
    }
    Some((
        ScopePlacementFrame {
            transform: rigid_transform_at(frame, carrier.matrix)?,
            transform_offset: u64_from_index(start + carrier.matrix),
            reference: Some((
                reference,
                u64_from_index(start + carrier.construction_reference + 1),
            )),
        },
        DesignJointOriginCarrierEvidence {
            frame_length: u64_from_index(carrier.length),
            prefix: prefix.to_vec().into(),
            scope_backlink: (*scope_backlink).into(),
        },
    ))
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
    let mut candidate = None;
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
                let values = f64s_at::<16>(bytes, start + joint_origin_class_337_266::MATRIX)?;
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.into_iter().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if let Ok(transform) =
                    crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
                {
                    if candidate
                        .replace(ScopePlacementFrame {
                            transform,
                            transform_offset: u64_from_index(
                                start + joint_origin_class_337_266::MATRIX,
                            ),
                            reference: None,
                        })
                        .is_some()
                    {
                        return None;
                    }
                }
                continue;
            }
            if paired.checked_sub(start)? == 385
                && bytes.get(start + 4..start + 7) == Some(b"364")
                && bytes.get(paired + 4..paired + 7) == Some(b"264")
                && bytes.get(start + 11..start + 45) == Some(&[0; 34])
                && bytes.get(start + 45..start + 49) == Some(&[1, 1, 0, 0])
            {
                let values = f64s_at::<16>(bytes, start + 49)?;
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.into_iter().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if let Ok(transform) =
                    crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
                {
                    if candidate
                        .replace(ScopePlacementFrame {
                            transform,
                            transform_offset: u64_from_index(start + 49),
                            reference: None,
                        })
                        .is_some()
                    {
                        return None;
                    }
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
            let values = f64s_at::<16>(bytes, start + 60)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            let Ok(transform) =
                crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
            else {
                continue;
            };
            if candidate
                .replace(ScopePlacementFrame {
                    transform,
                    transform_offset: u64_from_index(start + 60),
                    reference: Some((reference, u64_from_index(start + 46))),
                })
                .is_some()
            {
                return None;
            }
        }
    }
    candidate
}
