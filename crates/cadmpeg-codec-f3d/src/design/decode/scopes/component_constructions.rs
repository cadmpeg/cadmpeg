// SPDX-License-Identifier: Apache-2.0
//! Exact derived-instance, component-insert, copy-paste-component and component-pattern occurrence scopes.

use crate::bytes::{lp_utf16_bounded_charged, lp_utf16_bounded_scoped};
use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;

use crate::design::decode::text::relaxed_guid_end;
use crate::design::decode::text::{fixed_guid_end, fixed_utf16_ascii_eq};
use crate::ids::native_stream;
use crate::layout::component_insert_carrier_334_prefix as component_carrier_334;
use crate::layout::component_insert_identity_scope_compact as component_identity_scope;
use crate::layout::component_insert_identity_scope_shifted_prefix as component_identity_shifted;
use crate::layout::component_insert_matrix_scope_414_264_prefix as component_matrix_414;
use crate::layout::component_insert_relation_345_57 as component_insert_relation_345;
use crate::layout::component_insert_relation_child_393_58 as component_insert_relation_child_393;
use crate::layout::component_insert_scope_283_262_257 as component_scope_283_257;
use crate::layout::component_insert_scope_283_262_385 as component_scope_283_385;
use crate::layout::derived_instance_relation_310_57 as derived_instance_relation_310;
use crate::layout::derived_instance_scope_279_261 as derived_instance_279_261;
use crate::records::feature::assembly_features;
use crate::records::feature::assembly_features::DesignComponentInsertConstruction;
use crate::records::feature::assembly_features::DesignComponentOccurrence;
use crate::records::feature::assembly_features::DesignCopyPasteComponentOperation;
use crate::records::feature::assembly_features::DesignDerivedInstanceConstruction;
use crate::records::feature::patterns;
use crate::records::feature::patterns::DesignRectangularPatternInstances;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn exact_derived_instance_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<Option<DesignDerivedInstanceConstruction>, CodecError> {
    if !matches!(
        scope.kind(),
        scope::DesignFeatureKind::DerivedInstance | scope::DesignFeatureKind::CopyPaste
    ) {
        return Ok(None);
    }
    ctx.charge_work(
        u64_from_index(occurrences.len()),
        "scan F3D construction occurrences",
    )?;
    for occurrence in occurrences {
        let work = u64_from_index(occurrence.id.len())
            .checked_mul(4)
            .and_then(|value| value.checked_add(80))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("F3D occurrence scan work overflow", u64::MAX - 1, u64::MAX)
            })?;
        ctx.charge_work(work, "scan F3D construction occurrences")?;
    }
    let parsed = (|| {
        if scope.kind() != scope::DesignFeatureKind::DerivedInstance
            || scope.class_tag.as_str() != "279"
            || scope.paired_class_tag.as_str() != "261"
            || scope.frame_length() != u64_from_index(derived_instance_279_261::LEN)
            || scope.reference_members().len() != 1
        {
            return None;
        }
        let start = usize::try_from(scope.byte_offset()).ok()?;
        if bytes.get(
            start + derived_instance_279_261::REFERENCE_MARKER
                ..start + derived_instance_279_261::REFERENCE_RECORD_INDEX,
        )? != [derived_instance_279_261::REFERENCE_MARKER_VALUE]
            || bytes.get(
                start + derived_instance_279_261::REFERENCE_RECORD_INDEX + 4
                    ..start + derived_instance_279_261::REFERENCE_COUNT,
            )? != [0; 6]
            || View::u32_le_at(bytes, start + derived_instance_279_261::REFERENCE_COUNT)?
                != derived_instance_279_261::REFERENCE_COUNT_VALUE
            || marked_record_reference(bytes, start + derived_instance_279_261::RELATION_REFERENCE)?
                != *scope.reference_members().values().next()?
            || bytes.get(start + derived_instance_279_261::RELATION_REFERENCE + 11) != Some(&0)
        {
            return None;
        }
        let reference_record_index = View::u32_le_at(
            bytes,
            start + derived_instance_279_261::REFERENCE_RECORD_INDEX,
        )?;
        let transform_offset = start + derived_instance_279_261::TRANSFORM;
        let transform = rigid_transform_at(bytes, transform_offset)?;

        let relation_record_index = *scope.reference_members().values().next()?;
        let relation_at = records.first_offset(relation_record_index)?;
        let (relation_kind, _) =
            lp_ascii_filtered_view(bytes, relation_at, 3..=3, u8::is_ascii_graphic)?;
        if relation_at >= start
            || relation_kind != "310"
            || (match next_indexed_record_offset(ctx, bytes, relation_at + 1) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            }) != relation_at + derived_instance_relation_310::LEN
            || bytes.get(
                relation_at + derived_instance_relation_310::INDEXED_HEADER + 11
                    ..relation_at + derived_instance_relation_310::CARRIER_MARKER,
            )? != [0; 10]
            || bytes.get(relation_at + derived_instance_relation_310::CARRIER_MARKER)
                != Some(&derived_instance_relation_310::CARRIER_MARKER_VALUE)
            || bytes.get(
                relation_at + derived_instance_relation_310::CARRIER_RECORD_INDEX + 4
                    ..relation_at + derived_instance_relation_310::MIDDLE_MARKER,
            )? != [0; 8]
            || bytes.get(relation_at + derived_instance_relation_310::MIDDLE_MARKER)
                != Some(&derived_instance_relation_310::MIDDLE_MARKER_VALUE)
            || bytes.get(
                relation_at + derived_instance_relation_310::MIDDLE_RECORD_INDEX + 4
                    ..relation_at + derived_instance_relation_310::SCOPE_MARKER,
            )? != [0; 7]
            || bytes.get(relation_at + derived_instance_relation_310::SCOPE_MARKER)
                != Some(&derived_instance_relation_310::SCOPE_MARKER_VALUE)
            || View::u32_le_at(
                bytes,
                relation_at + derived_instance_relation_310::SCOPE_RECORD_INDEX,
            )? != scope.record_index
            || bytes.get(
                relation_at + derived_instance_relation_310::SCOPE_RECORD_INDEX + 4
                    ..relation_at + derived_instance_relation_310::LEN,
            )? != [0; 6]
        {
            return None;
        }
        let carrier_record_index = View::u32_le_at(
            bytes,
            relation_at + derived_instance_relation_310::CARRIER_RECORD_INDEX,
        )?;
        let stream = native_stream(&scope.id)?;
        let mut candidates = occurrences.iter().filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.class_tag.as_str() == "380"
                && occurrence.record_index == carrier_record_index
                && occurrence.byte_offset() < u64_from_index(relation_at)
                && occurrence.transform().map(|frame| frame.value) == Some(transform)
        });
        let carrier = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        let transform_offset = u64::try_from(transform_offset).ok()?;
        Some((|| {
            Ok(DesignDerivedInstanceConstruction {
                reference_record_index,
                relation_record_index,
                carrier_record_index,
                component_guid: carrier
                    .component_guid
                    .try_clone_for_decode(ctx, "retain F3D construction GUID")?,
                occurrence_guid: carrier
                    .occurrence_guid
                    .try_clone_for_decode(ctx, "retain F3D construction GUID")?,
                transform,
                transform_offset,
            })
        })())
    })();
    parsed.transpose()
}

pub(super) fn exact_component_insert_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignComponentInsertConstruction>, CodecError> {
    let parsed = (|| {
        macro_rules! admitted {
            ($value:expr) => {
                match $value {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                }
            };
        }

        let start = usize::try_from(scope.byte_offset()).ok()?;
        let relation_record_index = *scope.reference_members().values().next()?;
        if scope.kind() != scope::DesignFeatureKind::ComponentInsert
            || scope.reference_members().len() != 1
        {
            return None;
        }
        let (transform, transform_at, occurrence_identity) =
            match (scope.frame_length(), scope.paired_class_tag.as_str()) {
                (399, "259")
                    if bytes.get(start + 11..start + 20)? == [0; 9]
                        && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                        && bytes.get(start + 33..start + 37)? == [0; 4]
                        && bytes.get(start + 37) == Some(&1)
                        && View::u32_le_at(bytes, start + 38)? == relation_record_index
                        && bytes.get(start + 42..start + 50)? == [0, 0, 0, 0, 0, 0, 1, 0] =>
                {
                    let transform_at = start + 50;
                    (
                        rigid_transform_at(bytes, transform_at)?,
                        Some(transform_at),
                        View::u64_le_at(bytes, start + 25)?,
                    )
                }
                (381, "261")
                    if bytes.get(start + 11..start + 20)? == [0; 9]
                        && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                        && bytes.get(start + 33..start + 37)? == [0; 4]
                        && bytes.get(start + 37) == Some(&1)
                        && View::u32_le_at(bytes, start + 38)? == relation_record_index
                        && bytes.get(start + 42..start + 49)? == [0, 0, 0, 0, 0, 0, 1] =>
                {
                    let transform_at = start + 49;
                    (
                        rigid_transform_at(bytes, transform_at)?,
                        Some(transform_at),
                        View::u64_le_at(bytes, start + 25)?,
                    )
                }
                (395, "258")
                    if bytes.get(start + 11..start + 21)? == [0; 10]
                        && bytes.get(start + 29..start + 33)? == [0; 4]
                        && bytes.get(start + 33) == Some(&1)
                        && View::u32_le_at(bytes, start + 34)? == relation_record_index
                        && bytes.get(start + 38..start + 46)? == [0, 0, 0, 0, 0, 0, 1, 0] =>
                {
                    let transform_at = start + 46;
                    (
                        rigid_transform_at(bytes, transform_at)?,
                        Some(transform_at),
                        View::u64_le_at(bytes, start + 21)?,
                    )
                }
                (404, _)
                    if bytes.get(start + 11..start + 20)? == [0; 9]
                        && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                        && bytes.get(start + 25..start + 29)? == [0; 4]
                        && bytes.get(start + 37..start + 41)? == [0; 4]
                        && bytes.get(start + 41) == Some(&1)
                        && View::u32_le_at(bytes, start + 42)? == relation_record_index
                        && bytes.get(start + 46..start + 52)? == [0; 6]
                        && bytes.get(start + 52..start + 54)? == [1, 0] =>
                {
                    let transform_at = start + 54;
                    (
                        rigid_transform_at(bytes, transform_at)?,
                        Some(transform_at),
                        View::u64_le_at(bytes, start + 29)?,
                    )
                }
                (261, "263") if scope.class_tag.as_str() == "296" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    (match exact_component_insert_identity_scope(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }),
                ),
                (261, "261") if scope.class_tag.as_str() == "410" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    (match exact_component_insert_identity_scope(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }),
                ),
                (261, "258") if scope.class_tag.as_str() == "426" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    (match exact_component_insert_identity_scope(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }),
                ),
                (261, "266") if scope.class_tag.as_str() == "434" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    (match exact_component_insert_identity_scope(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }),
                ),
                (261, "264") if scope.class_tag.as_str() == "414" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    (match exact_component_insert_identity_scope(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }),
                ),
                (257 | 267, "264") if scope.class_tag.as_str() == "414" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    (match exact_component_insert_identity_scope_shifted(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }),
                ),
                (389, "264") if scope.class_tag.as_str() == "414" => {
                    match exact_component_insert_scope_414_264_389(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                }
                (257, "262") if scope.class_tag.as_str() == "283" => {
                    match exact_component_insert_scope_283_262_257(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                }
                (385, "262") if scope.class_tag.as_str() == "283" => {
                    match exact_component_insert_scope_283_262_385(
                        ctx,
                        bytes,
                        start,
                        relation_record_index,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    }
                }
                _ => return None,
            };
        let relation_at = records.first_offset(relation_record_index)?;
        let (carrier_record_index, placements) = if scope.frame_length() == 404 {
            if relation_at >= start
                || (match next_indexed_record_offset(ctx, bytes, relation_at + 1) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                }) != relation_at + 58
                || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
                || bytes.get(relation_at + 21) != Some(&1)
                || bytes.get(relation_at + 26..relation_at + 32)? != [0; 6]
                || bytes.get(relation_at + 32..relation_at + 35)? != [1, 0, 0]
                || bytes.get(relation_at + 35) != Some(&1)
                || bytes.get(relation_at + 40..relation_at + 47)? != [0; 7]
                || bytes.get(relation_at + 47) != Some(&1)
                || View::u32_le_at(bytes, relation_at + 48)? != scope.record_index
                || bytes.get(relation_at + 52..relation_at + 58)? != [0; 6]
            {
                return None;
            }
            let carrier_record_index = View::u32_le_at(bytes, relation_at + 22)?;
            let mut placements = Vec::new();
            let carrier_offsets = match ctx.admit_iter(
                records.offsets(carrier_record_index),
                "scan F3D component carrier offsets",
            ) {
                Ok(offsets) => offsets,
                Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
            };
            for &carrier_at in carrier_offsets.filter(|at| **at < relation_at) {
                let role_positions = match ctx.admit_iter(
                    bytes.get(carrier_at + 11..relation_at).unwrap_or(&[]),
                    "scan F3D component insert role positions",
                ) {
                    Ok(positions) => positions,
                    Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
                };
                for (relative, _) in role_positions.enumerate() {
                    let at = carrier_at + 11 + relative;
                    let after_role = match fixed_guid_end(ctx, bytes, at) {
                        Ok(Some(value)) => value,
                        Ok(None) => continue,
                        Err(error) => return Some(Err(error)),
                    };
                    if bytes.get(after_role..after_role + 12)
                        != Some(&[0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                    {
                        continue;
                    }
                    let transform_positions = match ctx.admit_iter(
                        bytes.get(carrier_at + 11..at).unwrap_or(&[]),
                        "scan F3D component insert transform positions",
                    ) {
                        Ok(positions) => positions,
                        Err(error) => {
                            return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error)))
                        }
                    };
                    for (relative, _) in transform_positions.enumerate() {
                        let transform_at = carrier_at + 11 + relative;
                        if let Err(error) =
                            ctx.charge_work(128, "match F3D component insert matrix")
                        {
                            return Some(Err(error));
                        }
                        if rigid_transform_at(bytes, transform_at) == Some(transform) {
                            if let Err(error) = ctx.reserve_capacity(
                                &mut placements,
                                1,
                                "f3d component insert placements",
                            ) {
                                return Some(Err(error));
                            }
                            let role = match lp_utf16_bounded_charged(
                                ctx,
                                bytes,
                                at,
                                36..=36,
                                "f3d Design UTF-16 text",
                            ) {
                                Ok(Some((role, _))) => role,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                            if let Err(error) = ctx.push_vec(
                                &mut placements,
                                (role, at + 4, Some(transform_at)),
                                "f3d component insert placements",
                            ) {
                                return Some(Err(error));
                            };
                        }
                    }
                }
            }
            (carrier_record_index, placements)
        } else if scope.class_tag.as_str() == "426" && scope.paired_class_tag.as_str() == "258" {
            let (carrier_record_index, role, role_offset) =
                match exact_component_insert_class_426_relation(
                    ctx,
                    bytes,
                    records,
                    relation_at,
                    start,
                    relation_record_index,
                    scope.record_index,
                ) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
            let role = admitted!(role.to_retained(ctx, "retain F3D UTF-16 string"));
            (
                carrier_record_index,
                admitted!(ctx.collect_vec(
                    [(role, role_offset, None)],
                    "f3d component insert placements"
                )),
            )
        } else {
            if relation_at >= start
                || (match next_indexed_record_offset(ctx, bytes, relation_at + 1) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                }) != relation_at + 57
                || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
                || bytes.get(relation_at + 21) != Some(&1)
                || bytes.get(relation_at + 26..relation_at + 34)? != [0; 8]
                || bytes.get(relation_at + 34) != Some(&1)
                || bytes.get(relation_at + 39..relation_at + 46)? != [0; 7]
                || bytes.get(relation_at + 46) != Some(&1)
                || View::u32_le_at(bytes, relation_at + 47)? != scope.record_index
                || bytes.get(relation_at + 51..relation_at + 57)? != [0; 6]
            {
                return None;
            }
            let carrier_record_index = View::u32_le_at(bytes, relation_at + 22)?;
            let carrier_at =
                unique_indexed_record_before(records, carrier_record_index, relation_at)?;
            if scope.class_tag.as_str() == "283" && scope.paired_class_tag.as_str() == "262" {
                let (role, role_offset) = match exact_component_insert_carrier_334(
                    ctx,
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                ) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                (
                    carrier_record_index,
                    admitted!(ctx.collect_vec(
                        [(role, role_offset, None)],
                        "f3d component insert placements"
                    )),
                )
            } else if scope.class_tag.as_str() == "296" && scope.paired_class_tag.as_str() == "263"
            {
                let (role, role_offset) = crate::xref::grouped_component_insert_identity(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                )?;
                let role = admitted!(role.to_retained(ctx, "retain F3D UTF-16 string"));
                (
                    carrier_record_index,
                    admitted!(ctx.collect_vec(
                        [(role, role_offset, None)],
                        "f3d component insert placements"
                    )),
                )
            } else if scope.class_tag.as_str() == "410" && scope.paired_class_tag.as_str() == "261"
            {
                let (role, role_offset) = crate::xref::grouped_component_insert_identity_class380(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                )?;
                let role = admitted!(role.to_retained(ctx, "retain F3D UTF-16 string"));
                (
                    carrier_record_index,
                    admitted!(ctx.collect_vec(
                        [(role, role_offset, None)],
                        "f3d component insert placements"
                    )),
                )
            } else if scope.class_tag.as_str() == "434" && scope.paired_class_tag.as_str() == "266"
            {
                let (role, role_offset) = crate::xref::grouped_component_insert_identity_class341(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                )?;
                let role = admitted!(role.to_retained(ctx, "retain F3D UTF-16 string"));
                (
                    carrier_record_index,
                    admitted!(ctx.collect_vec(
                        [(role, role_offset, None)],
                        "f3d component insert placements"
                    )),
                )
            } else if scope.class_tag.as_str() == "414" && scope.paired_class_tag.as_str() == "264"
            {
                let (role, role_offset, carrier_transform_offset) =
                    match crate::xref::repeated_target_component_insert(
                        ctx,
                        bytes,
                        carrier_at,
                        relation_at,
                        carrier_record_index,
                        transform.into(),
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                (
                    carrier_record_index,
                    admitted!(ctx.collect_vec(
                        [(role, role_offset, carrier_transform_offset)],
                        "f3d component insert placements"
                    )),
                )
            } else {
                let mut placements = Vec::new();
                let role_positions = match ctx.admit_iter(
                    bytes.get(carrier_at + 11..relation_at).unwrap_or(&[]),
                    "scan F3D component insert role positions",
                ) {
                    Ok(positions) => positions,
                    Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
                };
                for (relative, _) in role_positions.enumerate() {
                    let at = carrier_at + 11 + relative;
                    let after_role = match relaxed_guid_end(ctx, bytes, at) {
                        Ok(Some(value)) => value,
                        Ok(None) => continue,
                        Err(error) => return Some(Err(error)),
                    };
                    if bytes.get(after_role..after_role + 2) != Some(&[0, 0]) {
                        continue;
                    }
                    let transform_at = after_role.checked_add(2)?;
                    if rigid_transform_at(bytes, transform_at) == Some(transform) {
                        let role = match lp_utf16_bounded_charged(
                            ctx,
                            bytes,
                            at,
                            36..=38,
                            "f3d Design UTF-16 text",
                        ) {
                            Ok(Some((role, _))) => role,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };

                        if let Err(error) = ctx.reserve_capacity(
                            &mut placements,
                            1,
                            "f3d component insert placements",
                        ) {
                            return Some(Err(error));
                        }
                        if let Err(error) = ctx.push_vec(
                            &mut placements,
                            (role, at + 4, Some(transform_at)),
                            "f3d component insert placements",
                        ) {
                            return Some(Err(error));
                        };
                    }
                }
                if scope.frame_length() == 381 {
                    let legacy = match legacy_component_insert_placements(
                        ctx,
                        bytes,
                        carrier_at,
                        relation_at,
                        carrier_record_index,
                        transform,
                    ) {
                        Ok(value) => value,
                        Err(error) => return Some(Err(error)),
                    };

                    if let Err(error) = ctx.reserve_vec(
                        &mut placements,
                        legacy.len(),
                        "f3d component insert merged placements",
                    ) {
                        return Some(Err(error));
                    }
                    placements.extend(legacy);
                }
                (carrier_record_index, placements)
            }
        };
        if placements.len() != 1 {
            return None;
        }
        let (neutron_role, neutron_role_offset, carrier_transform_offset) =
            placements.into_iter().next()?;
        Some(Ok(DesignComponentInsertConstruction {
            relation_record_index,
            carrier_record_index,
            occurrence_identity: Some(occurrence_identity),
            neutron_role,
            neutron_role_offset: u64::try_from(neutron_role_offset).ok()?,
            placement: match (transform_at, carrier_transform_offset) {
                (Some(offset), carrier_offset) => {
                    Some(assembly_features::DesignComponentInsertMatrix {
                        scope: crate::records::identity::Located {
                            value: transform,
                            offset: u64::try_from(offset).ok()?,
                        },
                        carrier_offset: carrier_offset.map(u64::try_from).transpose().ok()?,
                    })
                }
                (None, None) => None,
                (None, Some(_)) => return None,
            },
        }))
    })();
    parsed.transpose()
}

fn exact_component_insert_class_426_relation<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    records: &IndexedRecordOffsets,
    relation_at: usize,
    scope_at: usize,
    relation_record_index: u32,
    scope_record_index: u32,
) -> Result<Option<(u32, crate::bytes::utf16::Utf16View<'a>, usize)>, CodecError> {
    (|| {
        let relation_end = relation_at + component_insert_relation_345::LEN;
        let (relation_class, relation_after_tag) =
            lp_ascii_filtered_view(bytes, relation_at, 3..=3, u8::is_ascii_digit)?;
        if relation_class != "345"
            || relation_after_tag != relation_at + 7
            || View::u32_le_at(bytes, relation_after_tag)? != relation_record_index
            || relation_at >= scope_at
            || (match next_indexed_record_offset(ctx, bytes, relation_at + 1) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            }) != relation_end
            || bytes.get(
                relation_at + component_insert_relation_345::INDEXED_HEADER + 11
                    ..relation_at + component_insert_relation_345::FIRST_MARKER,
            )? != [0; 10]
            || bytes.get(relation_at + component_insert_relation_345::FIRST_MARKER)
                != Some(&component_insert_relation_345::FIRST_MARKER_VALUE)
            || bytes.get(
                relation_at + component_insert_relation_345::FIRST_CARRIER_RECORD_INDEX + 4
                    ..relation_at + component_insert_relation_345::SECOND_MARKER,
            )? != [0; 8]
            || bytes.get(relation_at + component_insert_relation_345::SECOND_MARKER)
                != Some(&component_insert_relation_345::SECOND_MARKER_VALUE)
            || bytes.get(
                relation_at + component_insert_relation_345::SECOND_CHILD_RECORD_INDEX + 4
                    ..relation_at + component_insert_relation_345::SCOPE_MARKER,
            )? != [0; 7]
            || bytes.get(relation_at + component_insert_relation_345::SCOPE_MARKER)
                != Some(&component_insert_relation_345::SCOPE_MARKER_VALUE)
            || View::u32_le_at(
                bytes,
                relation_at + component_insert_relation_345::SCOPE_RECORD_INDEX,
            )? != scope_record_index
            || bytes.get(
                relation_at + component_insert_relation_345::SCOPE_RECORD_INDEX + 4..relation_end,
            )? != [0; 6]
        {
            return None;
        }

        let paired_at = relation_end;
        let (paired_class, paired_after_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 3..=3, u8::is_ascii_digit)?;
        if paired_class != "258"
            || paired_after_tag != paired_at + 7
            || View::u32_le_at(bytes, paired_after_tag)? != relation_record_index
        {
            return None;
        }

        let carrier_record_index = View::u32_le_at(
            bytes,
            relation_at + component_insert_relation_345::FIRST_CARRIER_RECORD_INDEX,
        )?;
        let child_record_index = View::u32_le_at(
            bytes,
            relation_at + component_insert_relation_345::SECOND_CHILD_RECORD_INDEX,
        )?;
        let child_at = match records.first_at_or_after(ctx, paired_at + 11, child_record_index) {
            Ok(child_at) => child_at?,
            Err(error) => return Some(Err(error)),
        };
        let child_end = child_at + component_insert_relation_child_393::LEN;
        let (child_class, child_after_tag) =
            lp_ascii_filtered_view(bytes, child_at, 3..=3, u8::is_ascii_digit)?;
        if child_class != "393"
            || child_after_tag != child_at + 7
            || View::u32_le_at(bytes, child_after_tag)? != child_record_index
            || (match next_indexed_record_offset(ctx, bytes, paired_at + 1) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            }) != child_at
            || (match next_indexed_record_offset(ctx, bytes, child_at + 1) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            }) != child_end
            || child_end != scope_at
            || bytes.get(
                child_at + 11..child_at + component_insert_relation_child_393::RELATION_MARKER,
            )? != [0; 20]
            || bytes.get(child_at + component_insert_relation_child_393::RELATION_MARKER)
                != Some(&component_insert_relation_child_393::RELATION_MARKER_VALUE)
            || View::u32_le_at(
                bytes,
                child_at + component_insert_relation_child_393::RELATION_RECORD_INDEX,
            )? != relation_record_index
            || bytes.get(
                child_at + component_insert_relation_child_393::RELATION_RECORD_INDEX + 4
                    ..child_at + component_insert_relation_child_393::OPAQUE_TOKEN,
            )? != [0; 6]
            || View::u64_le_at(
                bytes,
                child_at + component_insert_relation_child_393::OPAQUE_TOKEN,
            )
            .is_none()
            || bytes
                .get(child_at + component_insert_relation_child_393::OPAQUE_TOKEN + 8..child_end)?
                != [0; 8]
        {
            return None;
        }

        let carrier_at = unique_indexed_record_before(records, carrier_record_index, relation_at)?;
        let (role, role_offset) = crate::xref::grouped_component_insert_identity_class369(
            bytes,
            carrier_at,
            relation_at,
            carrier_record_index,
        )?;
        (Some((carrier_record_index, role, role_offset))).map(Ok)
    })()
    .transpose()
}

fn exact_component_insert_carrier_334(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Result<Option<(String, usize)>, CodecError> {
    let parsed = (|| {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
        if class_tag != "334"
            || after_tag != carrier_at + 7
            || View::u32_le_at(bytes, after_tag)? != carrier_record_index
        {
            return None;
        }
        (match fixed_guid_end(
            ctx,
            bytes,
            carrier_at + component_carrier_334::COMPONENT_IDENTITY,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        });

        let role_start = carrier_at + component_carrier_334::NEUTRON_ROLE;
        Some(Ok(role_start))
    })();
    let Some(role_start) = parsed.transpose()? else {
        return Ok(None);
    };
    let Some((role, role_end)) = direct_utf16_role_until_tail(ctx, bytes, role_start, relation_at)?
    else {
        return Ok(None);
    };
    (|| {
        if !crate::bytes::is_guid_prefix(&role)
            || role.as_bytes().get(36) != Some(&b'_')
            || !role.get(37..)?.starts_with("urn:")
            || bytes.get(role_end)? != &0
            || bytes.get(role_end + 1)? == &0
            || bytes.get(role_end + 2..role_end + 6)? != [0; 4]
            || View::u32_le_at(bytes, role_end + 6)? == 0
        {
            return None;
        }
        (match fixed_guid_end(ctx, bytes, role_end + COMPONENT_CARRIER_ROLE_TAIL_BYTES) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        });
        Some(Ok((role, role_start)))
    })()
    .transpose()
}

const COMPONENT_CARRIER_ROLE_TAIL_BYTES: usize = 10;

fn direct_utf16_role_until_tail(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    limit: usize,
) -> Result<Option<(String, usize)>, CodecError> {
    let end = (|| {
        let mut at = start;
        while at.checked_add(COMPONENT_CARRIER_ROLE_TAIL_BYTES)? <= limit {
            if bytes.get(at)? == &0
                && bytes.get(at + 2..at + 6)? == [0; 4]
                && View::u32_le_at(bytes, at + 6).is_some_and(|value| value != 0)
            {
                return Some(at);
            }
            let code_unit = View::u16_le_at(bytes, at)?;
            let byte = u8::try_from(code_unit).ok()?;
            if !byte.is_ascii_graphic() {
                return None;
            }
            at = at.checked_add(2)?;
        }
        None
    })();
    let Some(end) = end else {
        return Ok(None);
    };
    let count = (end - start) / 2;

    let mut role = ctx.retained_string(count, "f3d component carrier role text")?;
    let Some(role_bytes) = bytes.get(start..end) else {
        return Ok(None);
    };
    for (relative, _) in ctx
        .admit_iter(role_bytes, "scan F3D component carrier role code units")?
        .enumerate()
        .step_by(2)
    {
        let at = start + relative;
        let Some(unit) = View::u16_le_at(bytes, at) else {
            return Ok(None);
        };
        let Some(byte) = u8::try_from(unit).ok() else {
            return Ok(None);
        };
        role.push(char::from(byte));
    }
    Ok(Some((role, end)))
}

fn exact_component_insert_scope_283_262_257(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Result<
    Option<(
        crate::records::sketch_placement::SketchPlacementMatrix,
        Option<usize>,
        u64,
    )>,
    CodecError,
> {
    (|| {
        if bytes.get(start + 11..start + 21)? != [0; 10]
            || bytes.get(
                start + component_scope_283_257::RELATION_MARKER
                    ..start + component_scope_283_257::RELATION_MARKER + 1,
            )? != [1]
            || View::u32_le_at(
                bytes,
                start + component_scope_283_257::RELATION_RECORD_INDEX,
            )? != relation_record_index
            || bytes.get(start + 38..start + 44)? != [0; 6]
            || bytes.get(start + 44..start + 46)? != [1, 1]
            || View::u32_le_at(
                bytes,
                start + component_scope_283_257::NULL_GUID_CODE_UNIT_COUNT,
            )? != 36
        {
            return None;
        }
        let after_null_guid = match fixed_utf16_ascii_eq(
            ctx,
            bytes,
            start + component_scope_283_257::NULL_GUID_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if after_null_guid != start + component_scope_283_257::REFERENCE_COUNT - 3
            || View::u32_le_at(bytes, start + component_scope_283_257::REFERENCE_COUNT)? != 1
            || bytes.get(start + component_scope_283_257::REFERENCE_MARKER) != Some(&1)
            || View::u32_le_at(
                bytes,
                start + component_scope_283_257::REFERENCE_RECORD_INDEX,
            )? != relation_record_index
            || bytes.get(start + 134..start + 140)? != [0; 6]
            || View::u32_le_at(
                bytes,
                start + component_scope_283_257::PREVIOUS_HISTORY_STATE_ID,
            )? != u32::MAX
        {
            return None;
        }
        (Some((
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            None,
            View::u64_le_at(bytes, start + component_scope_283_257::OCCURRENCE_IDENTITY)?,
        )))
        .map(Ok)
    })()
    .transpose()
}

fn exact_component_insert_scope_283_262_385(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Result<
    Option<(
        crate::records::sketch_placement::SketchPlacementMatrix,
        Option<usize>,
        u64,
    )>,
    CodecError,
> {
    (|| {
        if bytes.get(start + 11..start + 21)? != [0; 10]
            || bytes.get(start + 44..start + 52)? != [1, 0, 0, 0, 0, 0, 0, 0]
            || bytes.get(start + 38..start + 44)? != [0; 6]
            || bytes.get(
                start + component_scope_283_385::RELATION_MARKER
                    ..start + component_scope_283_385::RELATION_MARKER + 1,
            )? != [1]
            || View::u32_le_at(
                bytes,
                start + component_scope_283_385::RELATION_RECORD_INDEX,
            )? != relation_record_index
        {
            return None;
        }
        let transform_at = start + component_scope_283_385::TRANSFORM;
        let transform = rigid_transform_at(bytes, transform_at)?;
        let after_null_guid = match fixed_utf16_ascii_eq(
            ctx,
            bytes,
            start + component_scope_283_385::NULL_GUID_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if after_null_guid != start + component_scope_283_385::REFERENCE_COUNT - 3
            || View::u32_le_at(bytes, start + component_scope_283_385::REFERENCE_COUNT)? != 1
            || bytes.get(start + component_scope_283_385::REFERENCE_MARKER) != Some(&1)
            || View::u32_le_at(
                bytes,
                start + component_scope_283_385::REFERENCE_RECORD_INDEX,
            )? != relation_record_index
            || bytes.get(start + 262..start + 268)? != [0; 6]
            || View::u32_le_at(
                bytes,
                start + component_scope_283_385::PREVIOUS_HISTORY_STATE_ID,
            )? != u32::MAX
        {
            return None;
        }
        (Some((
            transform,
            Some(transform_at),
            View::u64_le_at(bytes, start + component_scope_283_385::OCCURRENCE_IDENTITY)?,
        )))
        .map(Ok)
    })()
    .transpose()
}

const NULL_COMPONENT_INSERT_GUID: &str = "00000000-0000-0000-0000-000000000000";

fn exact_component_insert_identity_scope(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Result<Option<u64>, CodecError> {
    (|| {
        const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
        if bytes.get(start + 11..start + 20)? != [0; 9]
            || bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
            || bytes.get(start + 33..start + 37)? != [0; 4]
            || bytes.get(start + 37) != Some(&1)
            || View::u32_le_at(
                bytes,
                start + component_identity_scope::RELATION_RECORD_INDEX,
            )? != relation_record_index
            || bytes.get(start + 42..start + 48)? != [0; 6]
            || bytes.get(
                start + component_identity_scope::IDENTITY_MARKERS
                    ..start + component_identity_scope::IDENTITY_MARKERS + 2,
            )? != [1, 1]
            || View::u32_le_at(
                bytes,
                start + component_identity_scope::OPAQUE_CODE_UNIT_COUNT,
            )? != 36
        {
            return None;
        }
        let after_opaque_guid = match fixed_utf16_ascii_eq(
            ctx,
            bytes,
            start + component_identity_scope::OPAQUE_CODE_UNIT_COUNT,
            NULL_GUID,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if after_opaque_guid != start + component_identity_scope::OPAQUE_UTF16_PAYLOAD + 72 {
            return None;
        }
        (View::u64_le_at(bytes, start + component_identity_scope::OCCURRENCE_IDENTITY)).map(Ok)
    })()
    .transpose()
}

fn exact_component_insert_identity_scope_shifted(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Result<Option<u64>, CodecError> {
    (|| {
        if bytes.get(start + 11..start + 21)? != [0; 10]
            || bytes.get(start + 29..start + 33)? != [0; 4]
            || bytes.get(start + component_identity_shifted::RELATION_MARKER) != Some(&1)
            || View::u32_le_at(
                bytes,
                start + component_identity_shifted::RELATION_RECORD_INDEX,
            )? != relation_record_index
            || bytes.get(start + 38..start + 44)? != [0; 6]
            || bytes.get(
                start + component_identity_shifted::IDENTITY_MARKERS
                    ..start + component_identity_shifted::IDENTITY_MARKERS + 2,
            )? != [1, 1]
        {
            return None;
        }
        let after_null_guid = match fixed_utf16_ascii_eq(
            ctx,
            bytes,
            start + component_identity_shifted::NULL_GUID_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if after_null_guid != start + component_identity_shifted::LEN {
            return None;
        }
        (View::u64_le_at(
            bytes,
            start + component_identity_shifted::OCCURRENCE_IDENTITY,
        ))
        .map(Ok)
    })()
    .transpose()
}

fn exact_component_insert_scope_414_264_389(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Result<
    Option<(
        crate::records::sketch_placement::SketchPlacementMatrix,
        Option<usize>,
        u64,
    )>,
    CodecError,
> {
    (|| {
        if bytes.get(start + 11..start + 20)? != [0; 9]
            || bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
            || bytes.get(start + 33..start + 37)? != [0; 4]
            || bytes.get(start + component_matrix_414::RELATION_MARKER) != Some(&1)
            || View::u32_le_at(bytes, start + component_matrix_414::RELATION_RECORD_INDEX)?
                != relation_record_index
            || bytes.get(start + 42..start + 48)? != [0; 6]
            || bytes.get(
                start + component_matrix_414::MATRIX_MARKERS
                    ..start + component_matrix_414::MATRIX_MARKERS + 2,
            )? != [1, 0]
        {
            return None;
        }
        let transform_at = start + component_matrix_414::TRANSFORM;
        let transform = rigid_transform_at(bytes, transform_at)?;
        let after_null_guid = match fixed_utf16_ascii_eq(
            ctx,
            bytes,
            start + component_matrix_414::NULL_GUID_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if after_null_guid != start + component_matrix_414::LEN {
            return None;
        }
        (Some((
            transform,
            Some(transform_at),
            View::u64_le_at(bytes, start + component_matrix_414::OCCURRENCE_IDENTITY)?,
        )))
        .map(Ok)
    })()
    .transpose()
}

fn legacy_component_insert_placements(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    transform: crate::records::sketch_placement::SketchPlacementMatrix,
) -> Result<Vec<(String, usize, Option<usize>)>, CodecError> {
    let Some((class_tag, after_tag)) =
        lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)
    else {
        return Ok(Vec::new());
    };
    if class_tag != "288"
        || after_tag != carrier_at + 7
        || View::u32_le_at(bytes, after_tag) != Some(carrier_record_index)
    {
        return Ok(Vec::new());
    }
    let mut placements = Vec::new();
    let candidate_start = carrier_at.checked_add(11).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "F3D legacy component insert candidate offset overflow",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    let candidate_starts = bytes.get(candidate_start..relation_at).unwrap_or(&[]);
    for (relative, _) in ctx
        .admit_iter(
            candidate_starts,
            "scan F3D legacy component insert candidate starts",
        )?
        .enumerate()
    {
        let first_at = candidate_start.checked_add(relative).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "F3D legacy component insert candidate offset overflow",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
        let role_at = match fixed_guid_end(ctx, bytes, first_at) {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err(error) => return Err(error),
        };
        let after_role = match fixed_guid_end(ctx, bytes, role_at) {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err(error) => return Err(error),
        };
        if bytes.get(after_role..after_role + 14)
            != Some(&[1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0])
        {
            continue;
        }
        let after_asset_guid = match fixed_guid_end(ctx, bytes, after_role + 14) {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err(error) => return Err(error),
        };
        let Some((asset_identity, after_asset_identity, _asset_reservation)) =
            lp_utf16_bounded_scoped(
                ctx,
                bytes,
                after_asset_guid + 1,
                37..=256,
                "f3d Design temporary UTF-16 text",
            )?
        else {
            continue;
        };
        if !asset_identity
            .split_once('_')
            .is_some_and(|(guid, locator)| {
                crate::bytes::is_guid_relaxed(guid) && locator.starts_with("urn:")
            })
            || bytes.get(after_asset_guid) != Some(&0)
            || bytes.get(after_asset_identity) != Some(&0)
        {
            continue;
        }
        let carrier_transform_at = after_asset_identity + 1;
        let after_transform = carrier_transform_at + 16 * 8;
        let Some((repeated_identity, after_repeated_identity, _repeated_reservation)) =
            lp_utf16_bounded_scoped(
                ctx,
                bytes,
                after_transform + 4,
                37..=256,
                "f3d Design temporary UTF-16 text",
            )?
        else {
            continue;
        };
        if rigid_transform_at(bytes, carrier_transform_at) == Some(transform)
            && repeated_identity == asset_identity
            && bytes.get(after_transform..after_transform + 4) == Some(&[0; 4])
            && bytes.get(after_repeated_identity..relation_at)
                == Some(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        {
            ctx.reserve_vec(&mut placements, 1, "f3d legacy component insert placements")?;
            let Some((role, _)) =
                lp_utf16_bounded_charged(ctx, bytes, role_at, 36..=36, "f3d Design UTF-16 text")?
            else {
                continue;
            };
            placements.push((role, role_at + 4, Some(carrier_transform_at)));
        }
    }
    Ok(placements)
}

pub(super) fn exact_copy_paste_component_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<Option<DesignCopyPasteComponentOperation>, CodecError> {
    if !matches!(
        scope.kind(),
        scope::DesignFeatureKind::DerivedInstance | scope::DesignFeatureKind::CopyPaste
    ) {
        return Ok(None);
    }
    ctx.charge_work(
        u64_from_index(occurrences.len()),
        "scan F3D construction occurrences",
    )?;
    for occurrence in occurrences {
        let work = u64_from_index(occurrence.id.len())
            .checked_mul(4)
            .and_then(|value| value.checked_add(80))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("F3D occurrence scan work overflow", u64::MAX - 1, u64::MAX)
            })?;
        ctx.charge_work(work, "scan F3D construction occurrences")?;
    }
    let parsed = (|| {
        let stream = native_stream(&scope.id)?;
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let relation_record_index = *scope.reference_members().values().next()?;
        // The compact frame omits one four-byte prologue field, so both placements
        // and every marked reference before them move four bytes earlier.
        let source_at = match (scope.kind_name(), scope.frame_length()) {
            ("CopyPaste", 529) => 38,
            ("CopyPaste", 525) => 34,
            _ => return None,
        };
        if scope.reference_members().len() != 1 {
            return None;
        }
        let source_transform_offset = start.checked_add(source_at)?;
        let copied_transform_offset = source_transform_offset.checked_add(156)?;
        let source_transform = rigid_transform_at(bytes, source_transform_offset)?;
        let copied_transform = rigid_transform_at(bytes, copied_transform_offset)?;
        let relation_at = records.first_offset(relation_record_index)?;
        if relation_at >= start
            || (match next_indexed_record_offset(ctx, bytes, relation_at + 1) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            }) != relation_at + 57
            || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
            || bytes.get(relation_at + 21) != Some(&1)
            || bytes.get(relation_at + 26..relation_at + 34)? != [0; 8]
            || bytes.get(relation_at + 34) != Some(&1)
            || bytes.get(relation_at + 39..relation_at + 46)? != [0; 7]
            || bytes.get(relation_at + 46) != Some(&1)
            || View::u32_le_at(bytes, relation_at + 47)? != scope.record_index
            || bytes.get(relation_at + 51..relation_at + 57)? != [0; 6]
        {
            return None;
        }
        let copied_occurrence_record_index = View::u32_le_at(bytes, relation_at + 22)?;
        let mut copied_candidates = occurrences.iter().filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.record_index == copied_occurrence_record_index
                && occurrence.byte_offset() < u64_from_index(relation_at)
                && occurrence.transform().map(|frame| frame.value) == Some(copied_transform)
        });
        let copied = copied_candidates.next()?;
        if copied_candidates.next().is_some() {
            return None;
        }
        let mut source_candidates = occurrences.iter().filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.byte_offset() < copied.byte_offset()
                && occurrence
                    .component_guid
                    .as_str()
                    .eq_ignore_ascii_case(copied.component_guid.as_str())
                && occurrence.transform().is_none()
        });
        let source = source_candidates.next()?;
        if source_candidates.next().is_some() {
            return None;
        }
        Some((|| {
            Ok(DesignCopyPasteComponentOperation {
                relation_record_index,
                source_occurrence_record_index: source.record_index,
                copied_occurrence_record_index,
                component_guid: copied
                    .component_guid
                    .try_clone_for_decode(ctx, "retain F3D construction GUID")?,
                source_occurrence_guid: source
                    .occurrence_guid
                    .try_clone_for_decode(ctx, "retain F3D construction GUID")?,
                copied_occurrence_guid: copied
                    .occurrence_guid
                    .try_clone_for_decode(ctx, "retain F3D construction GUID")?,
                source_transform,
                source_transform_offset: u64_from_index(source_transform_offset),
                copied_transform,
                copied_transform_offset: u64_from_index(copied_transform_offset),
            })
        })())
    })();
    parsed.transpose()
}

pub(super) fn bind_component_pattern_occurrences(
    ctx: &DecodeContext<'_>,
    scope: &mut DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<(), CodecError> {
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(());
    };
    let byte_offset = scope.byte_offset();
    let Some(instances) = scope
        .rectangular_pattern_construction()
        .and_then(|construction| construction.instances())
    else {
        return Ok(());
    };
    let mut component_guid = None;
    let mut validate_frame =
        |ordinal: usize, frame: &patterns::DesignPatternInstance| -> Result<bool, CodecError> {
            let expected_ordinal = u32::try_from(ordinal)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "F3D pattern occurrence ordinal overflow",
                        u64::from(u32::MAX),
                        u64_from_index(ordinal),
                    )
                })?;
            let Some(candidate) =
                unique_pattern_occurrence(ctx, stream, frame, expected_ordinal, occurrences)?
            else {
                return Ok(false);
            };
            if let Some(first_guid) = component_guid {
                if !candidate
                    .component_guid
                    .as_str()
                    .eq_ignore_ascii_case(first_guid)
                {
                    return Ok(false);
                }
            } else {
                component_guid = Some(candidate.component_guid.as_str());
            }
            Ok(true)
        };
    let mut valid_frames = true;
    match instances {
        DesignRectangularPatternInstances::Bodies(frames) => {
            for (ordinal, frame) in ctx
                .admit_iter(frames, "scan F3D body pattern frames")?
                .enumerate()
                .skip(1)
            {
                if !validate_frame(ordinal, frame)? {
                    valid_frames = false;
                    break;
                }
            }
        }
        DesignRectangularPatternInstances::Components {
            seed, generated, ..
        } => {
            for (ordinal, frame) in ctx
                .admit_iter(
                    std::slice::from_ref(&seed.instance),
                    "scan F3D component pattern seed frame",
                )?
                .enumerate()
                .skip(1)
            {
                if !validate_frame(ordinal, frame)? {
                    valid_frames = false;
                    break;
                }
            }
            if valid_frames {
                for (generated_ordinal, instance) in ctx
                    .admit_iter(generated, "scan F3D generated component pattern frames")?
                    .enumerate()
                {
                    let ordinal = generated_ordinal.checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "F3D pattern occurrence ordinal overflow",
                            u64::from(u32::MAX),
                            u64_from_index(generated_ordinal),
                        )
                    })?;
                    if !validate_frame(ordinal, &instance.instance)? {
                        valid_frames = false;
                        break;
                    }
                }
            }
        }
    }
    if !valid_frames {
        return Ok(());
    }
    let Some(component_guid) = component_guid else {
        return Ok(());
    };
    ctx.charge_work(
        u64_from_index(occurrences.len()),
        "scan F3D pattern seed occurrences",
    )?;
    for occurrence in occurrences {
        let work = u64_from_index(occurrence.id.len())
            .checked_add(80)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("F3D pattern seed work overflow", u64::MAX - 1, u64::MAX)
            })?;
        ctx.charge_work(work, "scan F3D pattern seed occurrences")?;
    }
    let mut seed_candidates = occurrences.iter().filter(|occurrence| {
        native_stream(&occurrence.id) == Some(stream)
            && occurrence.byte_offset() < byte_offset
            && occurrence
                .component_guid
                .as_str()
                .eq_ignore_ascii_case(component_guid)
            && matches!(
                occurrence.placement(),
                assembly_features::DesignComponentOccurrencePlacement::Base
            )
    });
    let Some(seed) = seed_candidates.next() else {
        return Ok(());
    };
    if seed_candidates.next().is_some() {
        return Ok(());
    }
    let Some(seed_frame) = instances.frames().next().copied() else {
        return Ok(());
    };
    let mut generated = Vec::new();
    let mut append_generated =
        |ordinal: usize, frame: &patterns::DesignPatternInstance| -> Result<(), CodecError> {
            let expected_ordinal = u32::try_from(ordinal)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "F3D pattern occurrence ordinal overflow",
                        u64::from(u32::MAX),
                        u64_from_index(ordinal),
                    )
                })?;
            let candidate =
                unique_pattern_occurrence(ctx, stream, frame, expected_ordinal, occurrences)?
                    .ok_or_else(|| {
                        CodecError::malformed("F3D pattern occurrence changed during binding")
                    })?;
            ctx.reserve_vec(
                &mut generated,
                1,
                "f3d component pattern generated instances",
            )?;
            generated.push(patterns::DesignPatternComponentInstance {
                instance: *frame,
                occurrence_guid: candidate
                    .occurrence_guid
                    .try_clone_for_decode(ctx, "retain F3D pattern GUID")?,
            });
            Ok(())
        };
    match instances {
        DesignRectangularPatternInstances::Bodies(frames) => {
            for (ordinal, frame) in ctx
                .admit_iter(frames, "collect F3D body pattern instances")?
                .enumerate()
                .skip(1)
            {
                append_generated(ordinal, frame)?;
            }
        }
        DesignRectangularPatternInstances::Components {
            seed,
            generated: rows,
            ..
        } => {
            for (ordinal, frame) in ctx
                .admit_iter(
                    std::slice::from_ref(&seed.instance),
                    "skip F3D component pattern seed frame",
                )?
                .enumerate()
                .skip(1)
            {
                append_generated(ordinal, frame)?;
            }
            for (generated_ordinal, instance) in ctx
                .admit_iter(rows, "collect F3D generated component pattern instances")?
                .enumerate()
            {
                let ordinal = generated_ordinal.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "F3D pattern occurrence ordinal overflow",
                        u64::from(u32::MAX),
                        u64_from_index(generated_ordinal),
                    )
                })?;
                append_generated(ordinal, &instance.instance)?;
            }
        }
    }
    let bound = DesignRectangularPatternInstances::Components {
        component_guid: seed
            .component_guid
            .try_clone_for_decode(ctx, "retain F3D pattern GUID")?,
        seed: patterns::DesignPatternComponentInstance {
            instance: seed_frame,
            occurrence_guid: seed
                .occurrence_guid
                .try_clone_for_decode(ctx, "retain F3D pattern GUID")?,
        },
        generated,
    };
    if let Some(construction) = scope.rectangular_pattern_construction_mut() {
        construction
            .try_set_instances(Some(bound))
            .map_err(CodecError::malformed)?;
    }
    Ok(())
}

fn unique_pattern_occurrence<'a>(
    ctx: &DecodeContext<'_>,
    stream: &str,
    frame: &patterns::DesignPatternInstance,
    ordinal: u32,
    occurrences: &'a [DesignComponentOccurrence],
) -> Result<Option<&'a DesignComponentOccurrence>, CodecError> {
    ctx.charge_work(
        u64_from_index(occurrences.len()),
        "scan F3D pattern occurrences",
    )?;
    for occurrence in occurrences {
        ctx.charge_work(
            u64_from_index(occurrence.id.len()),
            "scan F3D pattern occurrences",
        )?;
    }
    let mut candidates = occurrences.iter().filter(|occurrence| {
        native_stream(&occurrence.id) == Some(stream)
            && occurrence.transform().map(|frame| frame.offset) == Some(frame.transform.offset)
            && occurrence.occurrence_ordinal() == ordinal
    });
    let candidate = candidates.next();
    Ok(if candidates.next().is_some() {
        None
    } else {
        candidate
    })
}

fn unique_indexed_record_before(
    records: &IndexedRecordOffsets,
    record_index: u32,
    end: usize,
) -> Option<usize> {
    let offsets = records.offsets(record_index);
    let [at] = &offsets[..offsets.partition_point(|offset| *offset < end)] else {
        return None;
    };
    Some(*at)
}

#[cfg(test)]
mod tests;
