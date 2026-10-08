// SPDX-License-Identifier: Apache-2.0
//! Exact derived-instance, component-insert, copy-paste-component and component-pattern occurrence scopes.

use std::ops::RangeInclusive;

use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use super::shared_frames::unique_match;
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{lp_utf16_bounded_charged, lp_utf16_bounded_scoped};
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::relaxed_guid_end;
use crate::design::decode::text::{fixed_guid_end, fixed_utf16_ascii_eq};
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
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

/// The fixed fields of a class-279 derived-instance scope and its class-310
/// relation record.
struct DerivedInstanceFrame {
    reference_record_index: u32,
    relation_record_index: u32,
    relation_at: usize,
    carrier_record_index: u32,
    transform: SketchPlacementMatrix,
    transform_offset: usize,
}

fn derived_instance_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DerivedInstanceFrame> {
    use derived_instance_279_261 as layout;
    use derived_instance_relation_310 as relation;
    if scope.class_tag.as_str() != "279"
        || scope.paired_class_tag.as_str() != "261"
        || scope.frame_length() != u64_from_index(layout::LEN)
    {
        return None;
    }
    let [&relation_record_index] = scope.reference_members().values_array::<1>()?;
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + layout::REFERENCE_MARKER) != Some(&layout::REFERENCE_MARKER_VALUE)
        || !zeros_at::<{ layout::REFERENCE_COUNT - (layout::REFERENCE_RECORD_INDEX + 4) }>(
            bytes,
            start + layout::REFERENCE_RECORD_INDEX + 4,
        )
        || View::u32_le_at(bytes, start + layout::REFERENCE_COUNT)? != layout::REFERENCE_COUNT_VALUE
        || marked_record_reference(bytes, start + layout::RELATION_REFERENCE)?
            != relation_record_index
        || bytes.get(start + layout::RELATION_REFERENCE + 11) != Some(&0)
    {
        return None;
    }
    let reference_record_index = View::u32_le_at(bytes, start + layout::REFERENCE_RECORD_INDEX)?;
    let transform_offset = start + layout::TRANSFORM;
    let transform = rigid_transform_at(bytes, transform_offset)?;
    let relation_at = records.first_offset(relation_record_index)?;
    let (relation_kind, _) =
        lp_ascii_filtered_view(bytes, relation_at, 3..=3, u8::is_ascii_graphic)?;
    if relation_at >= start
        || relation_kind != "310"
        || !zeros_at::<{ relation::CARRIER_MARKER - (relation::INDEXED_HEADER + 11) }>(
            bytes,
            relation_at + relation::INDEXED_HEADER + 11,
        )
        || bytes.get(relation_at + relation::CARRIER_MARKER)
            != Some(&relation::CARRIER_MARKER_VALUE)
        || !zeros_at::<{ relation::MIDDLE_MARKER - (relation::CARRIER_RECORD_INDEX + 4) }>(
            bytes,
            relation_at + relation::CARRIER_RECORD_INDEX + 4,
        )
        || bytes.get(relation_at + relation::MIDDLE_MARKER) != Some(&relation::MIDDLE_MARKER_VALUE)
        || !zeros_at::<{ relation::SCOPE_MARKER - (relation::MIDDLE_RECORD_INDEX + 4) }>(
            bytes,
            relation_at + relation::MIDDLE_RECORD_INDEX + 4,
        )
        || bytes.get(relation_at + relation::SCOPE_MARKER) != Some(&relation::SCOPE_MARKER_VALUE)
        || View::u32_le_at(bytes, relation_at + relation::SCOPE_RECORD_INDEX)? != scope.record_index
        || !zeros_at::<{ relation::LEN - (relation::SCOPE_RECORD_INDEX + 4) }>(
            bytes,
            relation_at + relation::SCOPE_RECORD_INDEX + 4,
        )
    {
        return None;
    }
    Some(DerivedInstanceFrame {
        reference_record_index,
        relation_record_index,
        relation_at,
        carrier_record_index: View::u32_le_at(bytes, relation_at + relation::CARRIER_RECORD_INDEX)?,
        transform,
        transform_offset,
    })
}

pub(super) fn exact_derived_instance_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<Option<DesignDerivedInstanceConstruction>, CodecError> {
    if !matches!(
        scope.payload(),
        scope::DesignScopePayload::DerivedInstance(_)
    ) {
        return Ok(None);
    }
    let Some(frame) = derived_instance_frame(bytes, records, scope) else {
        return Ok(None);
    };
    if next_indexed_record_offset(ctx, bytes, frame.relation_at + 1)?
        != Some(frame.relation_at + derived_instance_relation_310::LEN)
    {
        return Ok(None);
    }
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let relation_offset = u64_from_index(frame.relation_at);
    let Some(carrier) = unique_match(
        ctx,
        occurrences,
        |occurrence| {
            Ok(occurrence.record_index == frame.carrier_record_index
                && occurrence.class_tag.as_str() == "380"
                && occurrence.byte_offset() < relation_offset
                && occurrence.transform().map(|transform| transform.value) == Some(frame.transform)
                && in_stream(ctx, &occurrence.id, stream)?)
        },
        "find F3D derived-instance carrier occurrence",
    )?
    .one() else {
        return Ok(None);
    };
    Ok(Some(DesignDerivedInstanceConstruction {
        reference_record_index: frame.reference_record_index,
        relation_record_index: frame.relation_record_index,
        carrier_record_index: frame.carrier_record_index,
        component_guid: carrier
            .component_guid
            .try_clone_for_decode(ctx, "retain F3D construction GUID")?,
        occurrence_guid: carrier
            .occurrence_guid
            .try_clone_for_decode(ctx, "retain F3D construction GUID")?,
        transform: frame.transform,
        transform_offset: u64_from_index(frame.transform_offset),
    }))
}

/// A component-insert scope prologue: the scope transform, its offset when
/// the scope stores one, and the occurrence identity.
type ComponentInsertPrologue = (SketchPlacementMatrix, Option<usize>, u64);

/// The prologue of a component-insert scope that stores its transform at a
/// fixed offset.
fn matrix_component_insert_prologue(
    bytes: &[u8],
    frame_length: u64,
    paired_class_tag: &str,
    start: usize,
    relation_record_index: u32,
) -> Option<ComponentInsertPrologue> {
    let names_relation =
        |at: usize| View::u32_le_at(bytes, start + at) == Some(relation_record_index);
    let (transform_at, identity_at) = match (frame_length, paired_class_tag) {
        (399, "259")
            if zeros_at::<9>(bytes, start + 11)
                && bytes_at::<5>(bytes, start + 20) == Some(&[1, 0, 0, 0, 0])
                && zeros_at::<4>(bytes, start + 33)
                && bytes.get(start + 37) == Some(&1)
                && names_relation(38)
                && bytes_at::<8>(bytes, start + 42) == Some(&[0, 0, 0, 0, 0, 0, 1, 0]) =>
        {
            (start + 50, start + 25)
        }
        (381, "261")
            if zeros_at::<9>(bytes, start + 11)
                && bytes_at::<5>(bytes, start + 20) == Some(&[1, 0, 0, 0, 0])
                && zeros_at::<4>(bytes, start + 33)
                && bytes.get(start + 37) == Some(&1)
                && names_relation(38)
                && bytes_at::<7>(bytes, start + 42) == Some(&[0, 0, 0, 0, 0, 0, 1]) =>
        {
            (start + 49, start + 25)
        }
        (395, "258")
            if zeros_at::<10>(bytes, start + 11)
                && zeros_at::<4>(bytes, start + 29)
                && bytes.get(start + 33) == Some(&1)
                && names_relation(34)
                && bytes_at::<8>(bytes, start + 38) == Some(&[0, 0, 0, 0, 0, 0, 1, 0]) =>
        {
            (start + 46, start + 21)
        }
        (404, _)
            if zeros_at::<9>(bytes, start + 11)
                && bytes_at::<5>(bytes, start + 20) == Some(&[1, 0, 0, 0, 0])
                && zeros_at::<4>(bytes, start + 25)
                && zeros_at::<4>(bytes, start + 37)
                && bytes.get(start + 41) == Some(&1)
                && names_relation(42)
                && zeros_at::<6>(bytes, start + 46)
                && bytes_at::<2>(bytes, start + 52) == Some(&[1, 0]) =>
        {
            (start + 54, start + 29)
        }
        _ => return None,
    };
    Some((
        rigid_transform_at(bytes, transform_at)?,
        Some(transform_at),
        View::u64_le_at(bytes, identity_at)?,
    ))
}

fn component_insert_prologue(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
    relation_record_index: u32,
) -> Option<ComponentInsertPrologue> {
    let identity = |occurrence_identity: Option<u64>| {
        occurrence_identity.map(|identity| (SketchPlacementMatrix::IDENTITY, None, identity))
    };
    let class_tag = scope.class_tag.as_str();
    let paired_class_tag = scope.paired_class_tag.as_str();
    match (scope.frame_length(), paired_class_tag) {
        (frame_length @ (399 | 381 | 395 | 404), _) => matrix_component_insert_prologue(
            bytes,
            frame_length,
            paired_class_tag,
            start,
            relation_record_index,
        ),
        (261, "263") if class_tag == "296" => identity(exact_component_insert_identity_scope(
            bytes,
            start,
            relation_record_index,
        )),
        (261, "261") if class_tag == "410" => identity(exact_component_insert_identity_scope(
            bytes,
            start,
            relation_record_index,
        )),
        (261, "258") if class_tag == "426" => identity(exact_component_insert_identity_scope(
            bytes,
            start,
            relation_record_index,
        )),
        (261, "266") if class_tag == "434" => identity(exact_component_insert_identity_scope(
            bytes,
            start,
            relation_record_index,
        )),
        (261, "264") if class_tag == "414" => identity(exact_component_insert_identity_scope(
            bytes,
            start,
            relation_record_index,
        )),
        (257 | 267, "264") if class_tag == "414" => identity(
            exact_component_insert_identity_scope_shifted(bytes, start, relation_record_index),
        ),
        (389, "264") if class_tag == "414" => {
            exact_component_insert_scope_414_264_389(bytes, start, relation_record_index)
        }
        (257, "262") if class_tag == "283" => {
            exact_component_insert_scope_283_262_257(bytes, start, relation_record_index)
        }
        (385, "262") if class_tag == "283" => {
            exact_component_insert_scope_283_262_385(bytes, start, relation_record_index)
        }
        _ => None,
    }
}

/// The carrier record a 57-byte component relation at `relation_at` names,
/// when the relation precedes the scope at `scope_at` and closes with a
/// back-reference to `scope_record_index`.
fn relation_57_carrier(
    bytes: &[u8],
    relation_at: usize,
    scope_at: usize,
    scope_record_index: u32,
) -> Option<u32> {
    if relation_at >= scope_at
        || !zeros_at::<10>(bytes, relation_at + 11)
        || bytes.get(relation_at + 21) != Some(&1)
        || !zeros_at::<8>(bytes, relation_at + 26)
        || bytes.get(relation_at + 34) != Some(&1)
        || !zeros_at::<7>(bytes, relation_at + 39)
        || bytes.get(relation_at + 46) != Some(&1)
        || View::u32_le_at(bytes, relation_at + 47)? != scope_record_index
        || !zeros_at::<6>(bytes, relation_at + 51)
    {
        return None;
    }
    View::u32_le_at(bytes, relation_at + 22)
}

/// The carrier record a 58-byte component relation at `relation_at` names.
fn relation_58_carrier(
    bytes: &[u8],
    relation_at: usize,
    scope_at: usize,
    scope_record_index: u32,
) -> Option<u32> {
    if relation_at >= scope_at
        || !zeros_at::<10>(bytes, relation_at + 11)
        || bytes.get(relation_at + 21) != Some(&1)
        || !zeros_at::<6>(bytes, relation_at + 26)
        || bytes_at::<3>(bytes, relation_at + 32) != Some(&[1, 0, 0])
        || bytes.get(relation_at + 35) != Some(&1)
        || !zeros_at::<7>(bytes, relation_at + 40)
        || bytes.get(relation_at + 47) != Some(&1)
        || View::u32_le_at(bytes, relation_at + 48)? != scope_record_index
        || !zeros_at::<6>(bytes, relation_at + 52)
    {
        return None;
    }
    View::u32_le_at(bytes, relation_at + 22)
}

/// Whether the next indexed header after the one at `at` opens at `at + length`.
fn record_ends_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    length: usize,
) -> Result<bool, CodecError> {
    Ok(next_indexed_record_offset(ctx, bytes, at + 1)? == Some(at + length))
}

/// A neutron-role field found by a carrier scan: its counted UTF-16 field,
/// the code-unit bounds it is read with, and the carrier transform after it.
struct ScannedRole {
    role_at: usize,
    role_units: RangeInclusive<usize>,
    transform_at: usize,
}

/// A decoded component-insert placement.
struct ComponentInsertPlacement {
    carrier_record_index: u32,
    role: String,
    role_offset: usize,
    carrier_transform_at: Option<usize>,
}

impl ComponentInsertPlacement {
    /// A placement whose carrier names its role directly, copied into
    /// retained text.
    fn grouped(
        ctx: &DecodeContext<'_>,
        carrier_record_index: u32,
        found: Option<(crate::bytes::utf16::Utf16View<'_>, usize)>,
    ) -> Result<Option<Self>, CodecError> {
        let Some((role, role_offset)) = found else {
            return Ok(None);
        };
        Ok(Some(Self {
            carrier_record_index,
            role: role.to_retained(ctx, "retain F3D UTF-16 string")?,
            role_offset,
            carrier_transform_at: None,
        }))
    }

    fn scanned(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        carrier_record_index: u32,
        scanned: Option<ScannedRole>,
    ) -> Result<Option<Self>, CodecError> {
        let Some(scanned) = scanned else {
            return Ok(None);
        };
        let Some((role, _)) = lp_utf16_bounded_charged(
            ctx,
            bytes,
            scanned.role_at,
            scanned.role_units,
            "f3d Design UTF-16 text",
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            carrier_record_index,
            role,
            role_offset: scanned.role_at + 4,
            carrier_transform_at: Some(scanned.transform_at),
        }))
    }
}

pub(super) fn exact_component_insert_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignComponentInsertConstruction>, CodecError> {
    if !matches!(
        scope.payload(),
        scope::DesignScopePayload::ComponentInsert(_)
    ) {
        return Ok(None);
    }
    let Some([&relation_record_index]) = scope.reference_members().values_array::<1>() else {
        return Ok(None);
    };
    let Some(start) = usize::try_from(scope.byte_offset()).ok() else {
        return Ok(None);
    };
    let Some((transform, transform_at, occurrence_identity)) =
        component_insert_prologue(bytes, scope, start, relation_record_index)
    else {
        return Ok(None);
    };
    let Some(relation_at) = records.first_offset(relation_record_index) else {
        return Ok(None);
    };
    let Some(placement) = component_insert_placement(
        ctx,
        bytes,
        records,
        scope,
        relation_at,
        relation_record_index,
        transform,
    )?
    else {
        return Ok(None);
    };
    let placement_matrix = match (transform_at, placement.carrier_transform_at) {
        (Some(offset), carrier_offset) => Some(assembly_features::DesignComponentInsertMatrix {
            scope: crate::records::identity::Located {
                value: transform,
                offset: u64_from_index(offset),
            },
            carrier_offset: carrier_offset.map(u64_from_index),
        }),
        (None, None) => None,
        (None, Some(_)) => return Ok(None),
    };
    Ok(Some(DesignComponentInsertConstruction {
        relation_record_index,
        carrier_record_index: placement.carrier_record_index,
        occurrence_identity: Some(occurrence_identity),
        neutron_role: placement.role,
        neutron_role_offset: u64_from_index(placement.role_offset),
        placement: placement_matrix,
    }))
}

/// The one placement a component-insert scope's relation and carrier records
/// state. A carrier scan that finds a second placement leaves none.
fn component_insert_placement(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    relation_at: usize,
    relation_record_index: u32,
    transform: SketchPlacementMatrix,
) -> Result<Option<ComponentInsertPlacement>, CodecError> {
    let Some(start) = usize::try_from(scope.byte_offset()).ok() else {
        return Ok(None);
    };
    let class_tags = (scope.class_tag.as_str(), scope.paired_class_tag.as_str());
    if scope.frame_length() == 404 {
        let Some(carrier_record_index) =
            relation_58_carrier(bytes, relation_at, start, scope.record_index)
        else {
            return Ok(None);
        };
        if !record_ends_at(ctx, bytes, relation_at, 58)? {
            return Ok(None);
        }
        let scanned = expanded_carrier_role(
            ctx,
            bytes,
            records,
            carrier_record_index,
            relation_at,
            transform,
        )?;
        return ComponentInsertPlacement::scanned(ctx, bytes, carrier_record_index, scanned);
    }
    if class_tags == ("426", "258") {
        let Some((carrier_record_index, role, role_offset)) =
            exact_component_insert_class_426_relation(
                ctx,
                bytes,
                records,
                relation_at,
                start,
                relation_record_index,
                scope.record_index,
            )?
        else {
            return Ok(None);
        };
        return ComponentInsertPlacement::grouped(
            ctx,
            carrier_record_index,
            Some((role, role_offset)),
        );
    }
    let Some(carrier_record_index) =
        relation_57_carrier(bytes, relation_at, start, scope.record_index)
    else {
        return Ok(None);
    };
    if !record_ends_at(ctx, bytes, relation_at, 57)? {
        return Ok(None);
    }
    let Some(carrier_at) =
        unique_indexed_record_before(ctx, records, carrier_record_index, relation_at)?
    else {
        return Ok(None);
    };
    let grouped = |found| ComponentInsertPlacement::grouped(ctx, carrier_record_index, found);
    match class_tags {
        ("283", "262") => {
            return Ok(exact_component_insert_carrier_334(
                ctx,
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
            )?
            .map(|(role, role_offset)| ComponentInsertPlacement {
                carrier_record_index,
                role,
                role_offset,
                carrier_transform_at: None,
            }));
        }
        ("414", "264") => {
            return Ok(crate::xref::repeated_target_component_insert(
                ctx,
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
                transform.into(),
            )?
            .map(|(role, role_offset, carrier_transform_at)| {
                ComponentInsertPlacement {
                    carrier_record_index,
                    role,
                    role_offset,
                    carrier_transform_at,
                }
            }));
        }
        ("296", "263") => {
            return grouped(crate::xref::grouped_component_insert_identity(
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
            ));
        }
        ("410", "261") => {
            return grouped(crate::xref::grouped_component_insert_identity_class380(
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
            ));
        }
        ("434", "266") => {
            return grouped(crate::xref::grouped_component_insert_identity_class341(
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
            ));
        }
        _ => {}
    }
    let mut scanned = None;
    if scanned_carrier_role(ctx, bytes, carrier_at, relation_at, transform, &mut scanned)? {
        return Ok(None);
    }
    if scope.frame_length() == 381
        && legacy_component_insert_role(
            ctx,
            bytes,
            carrier_at,
            relation_at,
            carrier_record_index,
            transform,
            &mut scanned,
        )?
    {
        return Ok(None);
    }
    ComponentInsertPlacement::scanned(ctx, bytes, carrier_record_index, scanned)
}

/// Record `found` in `slot`. Returns whether `slot` already held a role, which
/// makes the carrier ambiguous.
fn second_role(slot: &mut Option<ScannedRole>, found: ScannedRole) -> bool {
    slot.replace(found).is_some()
}

/// The one placement among the carriers of `carrier_record_index` before the
/// relation. The scans stop at a second placement.
fn expanded_carrier_role(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    carrier_record_index: u32,
    relation_at: usize,
    transform: SketchPlacementMatrix,
) -> Result<Option<ScannedRole>, CodecError> {
    let carriers = records.offsets(carrier_record_index);
    let before = ctx.partition_point(
        carriers,
        |carrier_at| Ok(*carrier_at < relation_at),
        "find F3D component carrier offsets",
    )?;
    let mut scanned = None;
    let ambiguous = ctx.position_by(
        carriers.get(..before).unwrap_or(&[]),
        |&carrier_at| {
            expanded_carrier_roles(
                ctx,
                bytes,
                carrier_at + 11,
                relation_at,
                transform,
                &mut scanned,
            )
        },
        "scan F3D component carrier offsets",
    )?;
    Ok(if ambiguous.is_some() { None } else { scanned })
}

/// Scan one expanded carrier whose fields run from `fields_at` to the
/// relation. A placement pairs a role GUID followed by the `00 01 06` tail
/// with a scope transform somewhere before the role. A role after the second
/// transform position pairs with both, so only the first two transform
/// positions are found. Each placement goes into `scanned`; returns whether a
/// second placement makes the scope ambiguous, where the scan stops.
fn expanded_carrier_roles(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    fields_at: usize,
    relation_at: usize,
    transform: SketchPlacementMatrix,
    scanned: &mut Option<ScannedRole>,
) -> Result<bool, CodecError> {
    let mut transforms = [None; 2];
    let mut found = 0;
    let mut transform_at = fields_at;
    ctx.position_by(
        bytes.get(fields_at..relation_at).unwrap_or(&[]),
        |_| {
            let candidate = transform_at;
            transform_at += 1;
            if rigid_transform_at(bytes, candidate) == Some(transform) {
                transforms[found] = Some(candidate);
                found += 1;
            }
            Ok(found == transforms.len())
        },
        "scan F3D component insert transform positions",
    )?;
    let [Some(first_transform), second_transform] = transforms else {
        return Ok(false);
    };
    let roles_at = first_transform + 1;
    let mut role_at = roles_at;
    Ok(ctx
        .position_by(
            bytes.get(roles_at..relation_at).unwrap_or(&[]),
            |_| {
                let at = role_at;
                role_at += 1;
                let Some(after_role) = fixed_guid_end(bytes, at) else {
                    return Ok(false);
                };
                if bytes_at::<12>(bytes, after_role) != Some(&[0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                {
                    return Ok(false);
                }
                Ok(second_transform.is_some_and(|second| second < at)
                    || second_role(
                        scanned,
                        ScannedRole {
                            role_at: at,
                            role_units: 36..=36,
                            transform_at: first_transform,
                        },
                    ))
            },
            "scan F3D component insert role positions",
        )?
        .is_some())
}

/// Scan the carrier at `carrier_at` for a relaxed-GUID role followed by two
/// zero bytes and the scope transform, recording it in `scanned`. Returns
/// whether a second role makes the carrier ambiguous; the scan stops there.
fn scanned_carrier_role(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    transform: SketchPlacementMatrix,
    scanned: &mut Option<ScannedRole>,
) -> Result<bool, CodecError> {
    let window_start = carrier_at + 11;
    let mut role_at = window_start;
    Ok(ctx
        .position_by(
            bytes.get(window_start..relation_at).unwrap_or(&[]),
            |_| {
                let at = role_at;
                role_at += 1;
                let Some(after_role) = relaxed_guid_end(bytes, at) else {
                    return Ok(false);
                };
                if bytes_at::<2>(bytes, after_role) != Some(&[0, 0]) {
                    return Ok(false);
                }
                let transform_at = after_role + 2;
                Ok(rigid_transform_at(bytes, transform_at) == Some(transform)
                    && second_role(
                        scanned,
                        ScannedRole {
                            role_at: at,
                            role_units: 36..=38,
                            transform_at,
                        },
                    ))
            },
            "scan F3D component insert role positions",
        )?
        .is_some())
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
    use component_insert_relation_345 as relation;
    use component_insert_relation_child_393 as child;
    let relation_end = relation_at + relation::LEN;
    let paired_at = relation_end;
    let relation_fields = || {
        Some(
            exact_indexed_header_at(bytes, relation_at, relation_record_index) == Some(b"345")
                && relation_at < scope_at
                && zeros_at::<{ relation::FIRST_MARKER - (relation::INDEXED_HEADER + 11) }>(
                    bytes,
                    relation_at + relation::INDEXED_HEADER + 11,
                )
                && bytes.get(relation_at + relation::FIRST_MARKER)
                    == Some(&relation::FIRST_MARKER_VALUE)
                && zeros_at::<
                    { relation::SECOND_MARKER - (relation::FIRST_CARRIER_RECORD_INDEX + 4) },
                >(
                    bytes,
                    relation_at + relation::FIRST_CARRIER_RECORD_INDEX + 4,
                )
                && bytes.get(relation_at + relation::SECOND_MARKER)
                    == Some(&relation::SECOND_MARKER_VALUE)
                && zeros_at::<{ relation::SCOPE_MARKER - (relation::SECOND_CHILD_RECORD_INDEX + 4) }>(
                    bytes,
                    relation_at + relation::SECOND_CHILD_RECORD_INDEX + 4,
                )
                && bytes.get(relation_at + relation::SCOPE_MARKER)
                    == Some(&relation::SCOPE_MARKER_VALUE)
                && View::u32_le_at(bytes, relation_at + relation::SCOPE_RECORD_INDEX)?
                    == scope_record_index
                && zeros_at::<{ relation::LEN - (relation::SCOPE_RECORD_INDEX + 4) }>(
                    bytes,
                    relation_at + relation::SCOPE_RECORD_INDEX + 4,
                )
                && exact_indexed_header_at(bytes, paired_at, relation_record_index) == Some(b"258"),
        )
    };
    if relation_fields() != Some(true) {
        return Ok(None);
    }
    let (Some(carrier_record_index), Some(child_record_index)) = (
        View::u32_le_at(bytes, relation_at + relation::FIRST_CARRIER_RECORD_INDEX),
        View::u32_le_at(bytes, relation_at + relation::SECOND_CHILD_RECORD_INDEX),
    ) else {
        return Ok(None);
    };
    let Some(child_at) = records.first_at_or_after(ctx, paired_at + 11, child_record_index)? else {
        return Ok(None);
    };
    let child_end = child_at + child::LEN;
    let child_fields = || {
        Some(
            exact_indexed_header_at(bytes, child_at, child_record_index) == Some(b"393")
                && child_end == scope_at
                && zeros_at::<{ child::RELATION_MARKER - 11 }>(bytes, child_at + 11)
                && bytes.get(child_at + child::RELATION_MARKER)
                    == Some(&child::RELATION_MARKER_VALUE)
                && View::u32_le_at(bytes, child_at + child::RELATION_RECORD_INDEX)?
                    == relation_record_index
                && zeros_at::<{ child::OPAQUE_TOKEN - (child::RELATION_RECORD_INDEX + 4) }>(
                    bytes,
                    child_at + child::RELATION_RECORD_INDEX + 4,
                )
                && View::u64_le_at(bytes, child_at + child::OPAQUE_TOKEN).is_some()
                && zeros_at::<{ child::LEN - (child::OPAQUE_TOKEN + 8) }>(
                    bytes,
                    child_at + child::OPAQUE_TOKEN + 8,
                ),
        )
    };
    if child_fields() != Some(true)
        || !record_ends_at(ctx, bytes, relation_at, relation::LEN)?
        || next_indexed_record_offset(ctx, bytes, paired_at + 1)? != Some(child_at)
        || !record_ends_at(ctx, bytes, child_at, child::LEN)?
    {
        return Ok(None);
    }
    let Some(carrier_at) =
        unique_indexed_record_before(ctx, records, carrier_record_index, relation_at)?
    else {
        return Ok(None);
    };
    Ok(crate::xref::grouped_component_insert_identity_class369(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
    )
    .map(|(role, role_offset)| (carrier_record_index, role, role_offset)))
}

fn exact_component_insert_carrier_334(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Result<Option<(String, usize)>, CodecError> {
    if exact_indexed_header_at(bytes, carrier_at, carrier_record_index) != Some(b"334")
        || fixed_guid_end(
            bytes,
            carrier_at + component_carrier_334::COMPONENT_IDENTITY,
        )
        .is_none()
    {
        return Ok(None);
    }
    let role_start = carrier_at + component_carrier_334::NEUTRON_ROLE;
    let Some(role_end) = direct_utf16_role_end(ctx, bytes, role_start, relation_at)? else {
        return Ok(None);
    };
    if !is_guid_urn_role(bytes, role_start, role_end)
        || !matches!(bytes.get(role_end + 1), Some(1..))
        || fixed_guid_end(bytes, role_end + COMPONENT_CARRIER_ROLE_TAIL_BYTES).is_none()
    {
        return Ok(None);
    }
    let role = retain_direct_utf16_role(ctx, bytes, role_start, role_end)?;
    Ok(Some((role, role_start)))
}

/// Bytes from a direct role's end to the counted GUID after it: a zero byte,
/// a nonzero byte, four zero bytes and a nonzero u32.
const COMPONENT_CARRIER_ROLE_TAIL_BYTES: usize = 10;

/// Whether the direct role whose code units run from `start` to `end` opens
/// with a hyphenated GUID, `_` and `urn:`. Only the first 41 code units are
/// read; `direct_utf16_role_end` validated each as one ASCII graphic byte.
fn is_guid_urn_role(bytes: &[u8], start: usize, end: usize) -> bool {
    let Some(units) = bytes.get(start..end).and_then(|units| units.get(..82)) else {
        return false;
    };
    let prefix: [u8; 41] = std::array::from_fn(|index| units[index * 2]);
    std::str::from_utf8(&prefix).is_ok_and(crate::bytes::is_guid_prefix)
        && prefix[36] == b'_'
        && prefix[37..] == *b"urn:"
}

/// The end of the direct UTF-16 role that starts at `start`: the first code
/// unit at least the tail length before `limit` that opens the role tail.
/// Every code unit before it is one ASCII graphic byte and a zero byte. Each
/// code unit the scan visits is admitted.
fn direct_utf16_role_end(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    limit: usize,
) -> Result<Option<usize>, CodecError> {
    // A code unit at `at` is visited while `at + 10 <= limit`, so the scanned
    // window ends eight bytes before `limit`.
    let window = limit
        .checked_sub(COMPONENT_CARRIER_ROLE_TAIL_BYTES - 2)
        .and_then(|end| bytes.get(start..end))
        .unwrap_or(&[]);
    let (units, _) = window.as_chunks::<2>();
    let mut at = start;
    let mut end = None;
    ctx.position_by(
        units,
        |unit| {
            let unit_at = at;
            at += 2;
            if unit[0] == 0
                && zeros_at::<4>(bytes, unit_at + 2)
                && View::u32_le_at(bytes, unit_at + 6).is_some_and(|value| value != 0)
            {
                end = Some(unit_at);
                return Ok(true);
            }
            Ok(unit[1] != 0 || !unit[0].is_ascii_graphic())
        },
        "scan F3D component carrier role code units",
    )?;
    Ok(end)
}

/// Copy the validated direct role from `start` to `end` into retained text.
fn retain_direct_utf16_role(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<String, CodecError> {
    let units = bytes.get(start..end).unwrap_or(&[]);
    ctx.utf16le_text(
        units,
        units.len() / 2,
        false,
        "f3d component carrier role text",
    )
}

fn exact_component_insert_scope_283_262_257(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<ComponentInsertPrologue> {
    use component_scope_283_257 as layout;
    let fields = || {
        Some(
            zeros_at::<10>(bytes, start + 11)
                && bytes.get(start + layout::RELATION_MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + layout::RELATION_RECORD_INDEX)?
                    == relation_record_index
                && zeros_at::<6>(bytes, start + 38)
                && bytes_at::<2>(bytes, start + 44) == Some(&[1, 1])
                && View::u32_le_at(bytes, start + layout::NULL_GUID_CODE_UNIT_COUNT)? == 36
                && View::u32_le_at(bytes, start + layout::REFERENCE_COUNT)? == 1
                && bytes.get(start + layout::REFERENCE_MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + layout::REFERENCE_RECORD_INDEX)?
                    == relation_record_index
                && zeros_at::<6>(bytes, start + 134)
                && View::u32_le_at(bytes, start + layout::PREVIOUS_HISTORY_STATE_ID)? == u32::MAX,
        )
    };
    let occurrence_identity = View::u64_le_at(bytes, start + layout::OCCURRENCE_IDENTITY)?;
    if fields() != Some(true)
        || fixed_utf16_ascii_eq(
            bytes,
            start + layout::NULL_GUID_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) != Some(start + layout::REFERENCE_COUNT - 3)
    {
        return None;
    }
    Some((SketchPlacementMatrix::IDENTITY, None, occurrence_identity))
}

fn exact_component_insert_scope_283_262_385(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<ComponentInsertPrologue> {
    use component_scope_283_385 as layout;
    let fields = || {
        Some(
            zeros_at::<10>(bytes, start + 11)
                && bytes_at::<8>(bytes, start + 44) == Some(&[1, 0, 0, 0, 0, 0, 0, 0])
                && zeros_at::<6>(bytes, start + 38)
                && bytes.get(start + layout::RELATION_MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + layout::RELATION_RECORD_INDEX)?
                    == relation_record_index
                && View::u32_le_at(bytes, start + layout::REFERENCE_COUNT)? == 1
                && bytes.get(start + layout::REFERENCE_MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + layout::REFERENCE_RECORD_INDEX)?
                    == relation_record_index
                && zeros_at::<6>(bytes, start + 262)
                && View::u32_le_at(bytes, start + layout::PREVIOUS_HISTORY_STATE_ID)? == u32::MAX,
        )
    };
    let transform_at = start + layout::TRANSFORM;
    let (Some(transform), Some(occurrence_identity)) = (
        rigid_transform_at(bytes, transform_at),
        View::u64_le_at(bytes, start + layout::OCCURRENCE_IDENTITY),
    ) else {
        return None;
    };
    if fields() != Some(true)
        || fixed_utf16_ascii_eq(
            bytes,
            start + layout::NULL_GUID_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) != Some(start + layout::REFERENCE_COUNT - 3)
    {
        return None;
    }
    Some((transform, Some(transform_at), occurrence_identity))
}

const NULL_COMPONENT_INSERT_GUID: &str = "00000000-0000-0000-0000-000000000000";

fn exact_component_insert_identity_scope(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<u64> {
    use component_identity_scope as layout;
    let fields = || {
        Some(
            zeros_at::<9>(bytes, start + 11)
                && bytes_at::<5>(bytes, start + 20) == Some(&[1, 0, 0, 0, 0])
                && zeros_at::<4>(bytes, start + 33)
                && bytes.get(start + 37) == Some(&1)
                && View::u32_le_at(bytes, start + layout::RELATION_RECORD_INDEX)?
                    == relation_record_index
                && zeros_at::<6>(bytes, start + 42)
                && bytes_at::<2>(bytes, start + layout::IDENTITY_MARKERS) == Some(&[1, 1])
                && View::u32_le_at(bytes, start + layout::OPAQUE_CODE_UNIT_COUNT)? == 36,
        )
    };
    let occurrence_identity = View::u64_le_at(bytes, start + layout::OCCURRENCE_IDENTITY)?;
    if fields() != Some(true)
        || fixed_utf16_ascii_eq(
            bytes,
            start + layout::OPAQUE_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) != Some(start + layout::OPAQUE_UTF16_PAYLOAD + 72)
    {
        return None;
    }
    Some(occurrence_identity)
}

fn exact_component_insert_identity_scope_shifted(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<u64> {
    use component_identity_shifted as layout;
    let fields = || {
        Some(
            zeros_at::<10>(bytes, start + 11)
                && zeros_at::<4>(bytes, start + 29)
                && bytes.get(start + layout::RELATION_MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + layout::RELATION_RECORD_INDEX)?
                    == relation_record_index
                && zeros_at::<6>(bytes, start + 38)
                && bytes_at::<2>(bytes, start + layout::IDENTITY_MARKERS) == Some(&[1, 1]),
        )
    };
    let occurrence_identity = View::u64_le_at(bytes, start + layout::OCCURRENCE_IDENTITY)?;
    if fields() != Some(true)
        || fixed_utf16_ascii_eq(
            bytes,
            start + layout::NULL_GUID_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) != Some(start + layout::LEN)
    {
        return None;
    }
    Some(occurrence_identity)
}

fn exact_component_insert_scope_414_264_389(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<ComponentInsertPrologue> {
    use component_matrix_414 as layout;
    let fields = || {
        Some(
            zeros_at::<9>(bytes, start + 11)
                && bytes_at::<5>(bytes, start + 20) == Some(&[1, 0, 0, 0, 0])
                && zeros_at::<4>(bytes, start + 33)
                && bytes.get(start + layout::RELATION_MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + layout::RELATION_RECORD_INDEX)?
                    == relation_record_index
                && zeros_at::<6>(bytes, start + 42)
                && bytes_at::<2>(bytes, start + layout::MATRIX_MARKERS) == Some(&[1, 0]),
        )
    };
    let transform_at = start + layout::TRANSFORM;
    let (Some(transform), Some(occurrence_identity)) = (
        rigid_transform_at(bytes, transform_at),
        View::u64_le_at(bytes, start + layout::OCCURRENCE_IDENTITY),
    ) else {
        return None;
    };
    if fields() != Some(true)
        || fixed_utf16_ascii_eq(
            bytes,
            start + layout::NULL_GUID_CODE_UNIT_COUNT,
            NULL_COMPONENT_INSERT_GUID,
        ) != Some(start + layout::LEN)
    {
        return None;
    }
    Some((transform, Some(transform_at), occurrence_identity))
}

/// Whether `identity` is a relaxed GUID, `_` and a `urn:` locator.
fn is_guid_urn_identity(ctx: &DecodeContext<'_>, identity: &str) -> Result<bool, CodecError> {
    let Some(at) = ctx.find_bytes(
        identity.as_bytes(),
        b"_",
        "split F3D legacy component insert identity",
    )?
    else {
        return Ok(false);
    };
    // `_` is ASCII, so both halves start on character boundaries.
    let (Some(guid), Some(locator)) = (identity.get(..at), identity.get(at + 1..)) else {
        return Ok(false);
    };
    Ok(crate::bytes::is_guid_relaxed(guid) && locator.as_bytes().starts_with(b"urn:"))
}

/// Scan a legacy class-288 carrier for its role: a GUID, the role GUID, a
/// fixed marker, an asset GUID, a `GUID_urn:` asset identity, the scope
/// transform and the repeated asset identity that closes the carrier. Each
/// role goes into `scanned`; returns whether a second role makes the carrier
/// ambiguous, where the scan stops.
fn legacy_component_insert_role(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    transform: SketchPlacementMatrix,
    scanned: &mut Option<ScannedRole>,
) -> Result<bool, CodecError> {
    const TEMPORARY_TEXT: &str = "f3d Design temporary UTF-16 text";
    if exact_indexed_header_at(bytes, carrier_at, carrier_record_index) != Some(b"288") {
        return Ok(false);
    }
    let candidate_start = carrier_at + 11;
    let mut first_at = candidate_start;
    Ok(ctx
        .position_by(
            bytes.get(candidate_start..relation_at).unwrap_or(&[]),
            |_| {
                let at = first_at;
                first_at += 1;
                let Some(role_at) = fixed_guid_end(bytes, at) else {
                    return Ok(false);
                };
                let Some(after_role) = fixed_guid_end(bytes, role_at) else {
                    return Ok(false);
                };
                if bytes_at::<14>(bytes, after_role)
                    != Some(&[1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0])
                {
                    return Ok(false);
                }
                let Some(after_asset_guid) = fixed_guid_end(bytes, after_role + 14) else {
                    return Ok(false);
                };
                if bytes.get(after_asset_guid) != Some(&0) {
                    return Ok(false);
                }
                let Some((asset_identity, after_asset_identity, _asset_storage)) =
                    lp_utf16_bounded_scoped(
                        ctx,
                        bytes,
                        after_asset_guid + 1,
                        37..=256,
                        TEMPORARY_TEXT,
                    )?
                else {
                    return Ok(false);
                };
                let carrier_transform_at = after_asset_identity + 1;
                let after_transform = carrier_transform_at + 16 * 8;
                if bytes.get(after_asset_identity) != Some(&0)
                    || !is_guid_urn_identity(ctx, &asset_identity)?
                    || rigid_transform_at(bytes, carrier_transform_at) != Some(transform)
                    || !zeros_at::<4>(bytes, after_transform)
                {
                    return Ok(false);
                }
                let Some((repeated_identity, after_repeated_identity, _repeated_storage)) =
                    lp_utf16_bounded_scoped(
                        ctx,
                        bytes,
                        after_transform + 4,
                        37..=256,
                        TEMPORARY_TEXT,
                    )?
                else {
                    return Ok(false);
                };
                Ok(after_repeated_identity + 12 == relation_at
                    && bytes_at::<12>(bytes, after_repeated_identity)
                        == Some(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                    && ctx.equal_bytes(
                        repeated_identity.as_bytes(),
                        asset_identity.as_bytes(),
                        "match F3D legacy component insert identities",
                    )?
                    && second_role(
                        scanned,
                        ScannedRole {
                            role_at,
                            role_units: 36..=36,
                            transform_at: carrier_transform_at,
                        },
                    ))
            },
            "scan F3D legacy component insert candidate starts",
        )?
        .is_some())
}

pub(super) fn exact_copy_paste_component_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<Option<DesignCopyPasteComponentOperation>, CodecError> {
    if !matches!(scope.payload(), scope::DesignScopePayload::CopyPaste(_)) {
        return Ok(None);
    }
    // The compact frame omits one four-byte prologue field, so both placements
    // and every marked reference before them move four bytes earlier.
    let source_at = match scope.frame_length() {
        529 => 38,
        525 => 34,
        _ => return Ok(None),
    };
    let Some([&relation_record_index]) = scope.reference_members().values_array::<1>() else {
        return Ok(None);
    };
    let Some(start) = usize::try_from(scope.byte_offset()).ok() else {
        return Ok(None);
    };
    let source_transform_offset = start + source_at;
    let copied_transform_offset = source_transform_offset + 156;
    let (Some(source_transform), Some(copied_transform), Some(relation_at)) = (
        rigid_transform_at(bytes, source_transform_offset),
        rigid_transform_at(bytes, copied_transform_offset),
        records.first_offset(relation_record_index),
    ) else {
        return Ok(None);
    };
    let Some(copied_occurrence_record_index) =
        relation_57_carrier(bytes, relation_at, start, scope.record_index)
    else {
        return Ok(None);
    };
    if !record_ends_at(ctx, bytes, relation_at, 57)? {
        return Ok(None);
    }
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let relation_offset = u64_from_index(relation_at);
    let Some(copied) = unique_match(
        ctx,
        occurrences,
        |occurrence| {
            Ok(occurrence.record_index == copied_occurrence_record_index
                && occurrence.byte_offset() < relation_offset
                && occurrence.transform().map(|frame| frame.value) == Some(copied_transform)
                && in_stream(ctx, &occurrence.id, stream)?)
        },
        "find F3D copied component occurrence",
    )?
    .one() else {
        return Ok(None);
    };
    let Some(source) = unique_match(
        ctx,
        occurrences,
        |occurrence| {
            Ok(occurrence.byte_offset() < copied.byte_offset()
                && occurrence.transform().is_none()
                && ctx.eq_ignore_ascii_case(
                    occurrence.component_guid.as_str(),
                    copied.component_guid.as_str(),
                    "match F3D copied component GUID",
                )?
                && in_stream(ctx, &occurrence.id, stream)?)
        },
        "find F3D copy source occurrence",
    )?
    .one() else {
        return Ok(None);
    };
    Ok(Some(DesignCopyPasteComponentOperation {
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
    }))
}

pub(super) fn bind_component_pattern_occurrences(
    ctx: &DecodeContext<'_>,
    scope: &mut DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<(), CodecError> {
    let byte_offset = scope.byte_offset();
    let Some(instances) = scope
        .rectangular_pattern_construction()
        .and_then(|construction| construction.instances())
    else {
        return Ok(());
    };
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(());
    };
    // Each generated frame, from ordinal 1, with its unique occurrence. Every
    // occurrence must name the same component.
    let mut match_storage = ctx.reserve_scoped(0, "f3d component pattern occurrences")?;
    let mut matched = Vec::new();
    let mut component_guid = None;
    let mut ordinal = 0_usize;
    let mut match_frame = |frame: &patterns::DesignPatternInstance| -> Result<bool, CodecError> {
        // The seed is occurrence 1, so the frame at position `ordinal` is
        // occurrence `ordinal + 1`.
        ordinal += 1;
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
        match component_guid {
            Some(first_guid) => {
                if !ctx.eq_ignore_ascii_case(
                    candidate.component_guid.as_str(),
                    first_guid,
                    "match F3D pattern component GUID",
                )? {
                    return Ok(false);
                }
            }
            None => component_guid = Some(candidate.component_guid.as_str()),
        }
        ctx.push_scoped_vec(
            &mut match_storage,
            &mut matched,
            (*frame, candidate),
            "f3d component pattern occurrences",
        )?;
        Ok(true)
    };
    let valid_frames = match instances {
        DesignRectangularPatternInstances::Bodies(frames) => ctx.all_by(
            frames.get(1..).unwrap_or(&[]),
            &mut match_frame,
            "scan F3D body pattern frames",
        )?,
        DesignRectangularPatternInstances::Components { generated, .. } => ctx.all_by(
            generated,
            |row| match_frame(&row.instance),
            "scan F3D generated component pattern frames",
        )?,
    };
    let Some(component_guid) = component_guid.filter(|_| valid_frames) else {
        return Ok(());
    };
    let Some(seed) = unique_match(
        ctx,
        occurrences,
        |occurrence| {
            Ok(occurrence.byte_offset() < byte_offset
                && matches!(
                    occurrence.placement(),
                    assembly_features::DesignComponentOccurrencePlacement::Base
                )
                && ctx.eq_ignore_ascii_case(
                    occurrence.component_guid.as_str(),
                    component_guid,
                    "match F3D pattern seed component GUID",
                )?
                && in_stream(ctx, &occurrence.id, stream)?)
        },
        "find F3D pattern seed occurrence",
    )?
    .one() else {
        return Ok(());
    };
    let Some(seed_frame) = instances.frames().next().copied() else {
        return Ok(());
    };
    let mut generated = Vec::new();
    for (frame, candidate) in ctx.admit_iter(&matched, "collect F3D pattern instances")? {
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

/// The only occurrence in `stream` placed at the frame's transform offset with
/// the one-based `ordinal`.
fn unique_pattern_occurrence<'a>(
    ctx: &DecodeContext<'_>,
    stream: &str,
    frame: &patterns::DesignPatternInstance,
    ordinal: u32,
    occurrences: &'a [DesignComponentOccurrence],
) -> Result<Option<&'a DesignComponentOccurrence>, CodecError> {
    Ok(unique_match(
        ctx,
        occurrences,
        |occurrence| {
            Ok(occurrence.transform().map(|transform| transform.offset)
                == Some(frame.transform.offset)
                && occurrence.occurrence_ordinal() == ordinal
                && in_stream(ctx, &occurrence.id, stream)?)
        },
        "find F3D pattern occurrence",
    )?
    .one())
}

/// The only header of `record_index` before `end`. The ascending offsets are
/// bisected.
fn unique_indexed_record_before(
    ctx: &DecodeContext<'_>,
    records: &IndexedRecordOffsets,
    record_index: u32,
    end: usize,
) -> Result<Option<usize>, CodecError> {
    let offsets = records.offsets(record_index);
    let before = ctx.partition_point(
        offsets,
        |offset| Ok(*offset < end),
        "find F3D component carrier record",
    )?;
    Ok(match offsets.get(..before) {
        Some([at]) => Some(*at),
        _ => None,
    })
}

#[cfg(test)]
mod tests;
