// SPDX-License-Identifier: Apache-2.0
//! Construction operand groups, paths, transforms, and identities.

use super::extrude_roles::{assign_extrude_face_roles, assign_slotted_extrude_face_targets};
use super::extrude_selections::parse_extrude_identity_member;
use super::face_operand_frames::has_typed_edge_treatment_group;
use super::face_sources::indexed_operand_headers;
use crate::bytes::{lp_ascii_filtered_view, take_reference};
use crate::container::ContainerScan;
use crate::design::decode::scopes::extrude::{
    extrude_marked_reference_slots, is_class_296_two_sided_to_faces_scope,
};
use crate::design::decode::scopes::parameter_scope::payload_prologue;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::{
    indexed_record_header_at, next_indexed_record_offset_with_index, IndexedRecordOffsets,
};
use crate::design::decode::text::design_record_id;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::native_stream;
use crate::layout::{
    construction_operand_affine_transform_frame as trailing_affine,
    construction_operand_extended_path_prefix as extended_path,
    indexed_design_record_header as indexed_header,
    legacy_extrude_group_tail_marker as group_tail_marker,
    split_body_compact_group_prefix as split_group,
};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::references::LostEdgeReference;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandIdentity, DesignConstructionOperandRole,
    DesignConstructionPersistentIdentity, DesignConstructionTrackingPath,
};
use crate::records::topology::extrude_selection::{DesignExtrudeFaceEncoding, DesignOperandRole};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{index_from_u32, View};
use cadmpeg_core::CodecError;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

/// Decode counted construction-operand groups named by feature scopes.
///
/// A scope reference member whose record opens the group grammar but does not
/// close it is recorded on the owning scope, so a group the grammar cannot read
/// is distinguishable from a reference member that is not a group at all.
pub(crate) fn decode_construction_operand_groups(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandGroup>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut out = Vec::new();
    let mut record_offsets = HashMap::<&str, OnceCell<IndexedRecordOffsets>>::new();
    let mut split_metadata = HashMap::new();
    for scope in scopes.iter_mut().filter(|scope| {
        design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Coil)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Loft)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sweep)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Pipe)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::OffsetFaces)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Revolve)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Shell)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Thicken)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Move)
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Align
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::SurfacePatch)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::SurfaceRuled)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::BoundaryFill)
            || matches!(
                scope.payload(),
                crate::records::feature::scope::DesignScopePayload::SilhouetteSplit(Some(_))
            )
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Split)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Draft)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::ReplaceFace)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::SurfaceOffset)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::SurfaceTrim)
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::PathPattern
            || scope.mesh_face_groups().is_some()
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Scale)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::CircularPattern)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::RectangularPattern)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Mirror)
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::RemoveBody
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::ReverseNormal
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::MoveFace
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Modify
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::DeleteFace
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceUntrim
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::UnStitch
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal
            || (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Emboss
                && scope.emboss_operand_construction().is_some())
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Thread
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
            || matches!(
                scope.kind(),
                crate::records::feature::scope::DesignFeatureKind::BaseFlange
                    | crate::records::feature::scope::DesignFeatureKind::Bend
                    | crate::records::feature::scope::DesignFeatureKind::EdgeFlange
                    | crate::records::feature::scope::DesignFeatureKind::Hem
            )
            || has_typed_edge_treatment_group(&scope.kind())
    }) {
        let scope_group_start = out.len();
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let offsets = record_offsets.entry(stream).or_default();
        let metadata = if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Split) {
            split_metadata
                .entry(stream)
                .or_insert_with(|| {
                    crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)
                        .ok()
                        .flatten()
                })
                .as_deref()
        } else {
            None
        };
        let mut unclosed = Vec::new();
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SilhouetteSplit
                && ordinal != 2
            {
                continue;
            }
            if scope.emboss_operand_construction().is_some() && !matches!(ordinal, 0 | 2) {
                continue;
            }
            let (Ok(ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            match parse_construction_operand_group_in_stream(
                bytes,
                scope,
                ordinal,
                &RecordFrame::from(*header),
                Some(offsets),
                metadata,
            ) {
                ConstructionOperandGroupParse::Complete(group) => {
                    push_construction_operand_group(
                        &mut out,
                        group,
                        &entry.name,
                        header.byte_offset,
                    );
                }
                ConstructionOperandGroupParse::Unclosed => {
                    unclosed.push(record_index);
                }
                ConstructionOperandGroupParse::NotAGroup => {}
            }
        }
        scope.unclosed_construction_operand_groups = unclosed;
        if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude) {
            let slots = scope
                .extrude_prologue()
                .and_then(|prologue| extrude_marked_reference_slots(bytes, prologue));
            let groups = &mut out[scope_group_start..];
            assign_extrude_face_roles(scope, groups, slots.and_then(|slots| slots[6]));
            assign_slotted_extrude_face_targets(scope, groups, slots);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(super) fn push_construction_operand_group(
    out: &mut Vec<DesignConstructionOperandGroup>,
    mut group: Box<DesignConstructionOperandGroup>,
    stream: &str,
    offset: u64,
) {
    group.id = design_record_id(stream, ":design-construction-operand-group#", offset);
    out.push(*group);
}

/// Outcome of reading a scope reference member as a construction-operand group.
pub(crate) enum ConstructionOperandGroupParse {
    /// The record does not open a construction-operand group.
    NotAGroup,
    /// The record opens a group the grammar does not close.
    Unclosed,
    /// A complete group.
    Complete(Box<DesignConstructionOperandGroup>),
}

impl ConstructionOperandGroupParse {
    /// The group, where the record carried a complete one.
    #[cfg(test)]
    pub(super) fn complete(self) -> Option<DesignConstructionOperandGroup> {
        match self {
            Self::Complete(group) => Some(*group),
            Self::NotAGroup | Self::Unclosed => None,
        }
    }
}

/// Extrude operand role before the ordered face groups are separated into
/// start and termination uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingExtrudeRole {
    BodiesA,
    BodiesB,
    Profile,
    Faces(DesignExtrudeFaceEncoding),
}

/// Interpret the role of a counted group owned by an Extrude scope.
///
/// The `0x12` face-group role is a legacy spelling of the one-sided-to-face
/// termination group. Class-296 two-sided-to-faces scopes use the same role
/// for their termination groups. It is also a valid Thicken role, so the
/// extent and exact-layout gates are part of this admission rule rather than
/// a global role alias.
pub(crate) fn extrude_has_selected_termination(scope: &DesignParameterScope) -> bool {
    matches!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::ReferenceAware {
            extent: DesignExtrudeExtent::OneSidedToFace | DesignExtrudeExtent::TwoSidedToFaces,
            start: DesignExtrudeStart::ProfilePlane,
            first_side_target_ordinal: None,
            ..
        })
    )
}

pub(super) fn extrude_operand_role(
    scope: &DesignParameterScope,
    role: DesignOperandRole,
) -> Option<PendingExtrudeRole> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Extrude) {
        return None;
    }
    match role {
        DesignOperandRole::BODIES_A => Some(PendingExtrudeRole::BodiesA),
        DesignOperandRole::BODIES_B => Some(PendingExtrudeRole::BodiesB),
        DesignOperandRole::PROFILE => Some(PendingExtrudeRole::Profile),
        DesignOperandRole::FACES => {
            Some(PendingExtrudeRole::Faces(DesignExtrudeFaceEncoding::Faces))
        }
        DesignOperandRole::ROLE_0X5
            if scope.extrude_prologue().map(DesignExtrudePrologue::start)
                == Some(DesignExtrudeStart::FromFace) =>
        {
            Some(PendingExtrudeRole::Faces(
                DesignExtrudeFaceEncoding::SelectedStart,
            ))
        }
        DesignOperandRole::ROLE_0X5 if extrude_has_selected_termination(scope) => Some(
            PendingExtrudeRole::Faces(DesignExtrudeFaceEncoding::SelectedStart),
        ),
        DesignOperandRole::ROLE_0X12
            if scope
                .extrude_prologue()
                .and_then(DesignExtrudePrologue::extent)
                .is_some_and(|extent| {
                    matches!(
                        extent,
                        DesignExtrudeExtent::OneSidedToFace
                            | DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero
                            | DesignExtrudeExtent::TwoSidedToTargetsOptionZero
                            | DesignExtrudeExtent::TwoSidedToTargetsOptionTwo
                            | DesignExtrudeExtent::TwoSidedThroughAllToFace
                    )
                }) =>
        {
            Some(PendingExtrudeRole::Faces(
                DesignExtrudeFaceEncoding::LegacyTermination,
            ))
        }
        DesignOperandRole::ROLE_0X12 if is_class_296_two_sided_to_faces_scope(scope) => Some(
            PendingExtrudeRole::Faces(DesignExtrudeFaceEncoding::LegacyTermination),
        ),
        _ => None,
    }
}

/// Indexed frame identity and stream position.
#[derive(Clone, Debug)]
pub(crate) struct RecordFrame {
    pub(crate) record_index: u32,
    pub(crate) class_tag: crate::records::references::DesignClassTag,
    pub(crate) byte_offset: u64,
}

impl From<&DesignRecordHeader> for RecordFrame {
    fn from(header: &DesignRecordHeader) -> Self {
        Self {
            record_index: header.record_index,
            class_tag: header.class_tag.clone(),
            byte_offset: header.byte_offset,
        }
    }
}

/// Read the construction-operand group at `header`.
///
/// The record's members are a leading-block presence byte, the property block
/// its presence byte gates, the counted member run, two optional references,
/// the counted trailing-reference run, a zero u32 and the u32 role, ten zero bytes, an
/// ordinal, a duration, and a repeat of the ordinal that one container
/// generation omits. The class-328 Move form has one null and one present
/// auxiliary reference, a zero trailing count, and a retained null trailing
/// slot before the role. The ordinary tail is a reference to record `N + 2`,
/// a flag block, a reference to record `N + 1`, a zero byte, the owning scope's
/// reference, and the same-index paired header. One container generation
/// prefixes the flag block with a further byte. A legacy Extrude tail can
/// reference an independent empty marker in place of record `N + 2`.
/// Exactly one ordinal and flag combination reaches the same-index paired
/// header with the required references.
pub(crate) fn parse_construction_operand_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &RecordFrame,
) -> ConstructionOperandGroupParse {
    parse_construction_operand_group_in_stream(
        bytes,
        scope,
        scope_reference_ordinal,
        header,
        None,
        None,
    )
}

pub(super) fn parse_construction_operand_group_in_stream(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &RecordFrame,
    offsets: Option<&OnceCell<IndexedRecordOffsets>>,
    split_metadata: Option<&crate::metastream::MetaStream>,
) -> ConstructionOperandGroupParse {
    use ConstructionOperandGroupParse::{Complete, NotAGroup, Unclosed};

    let Ok(start) = usize::try_from(header.byte_offset) else {
        return NotAGroup;
    };
    // The indexed header is the three-digit class tag, the u64 entity id whose
    // low word is the record index, and the record's own empty name.
    if bytes.get(start + 11..start + 19) != Some(&[0; 8]) {
        return NotAGroup;
    }
    // The record ends at its paired header: the next indexed header with its
    // own index, or the end of the stream.
    let end = next_indexed_record_offset_with_index(bytes, start + 1, header.record_index)
        .unwrap_or(bytes.len());
    let Some(record_bytes) = bytes.get(..end) else {
        return NotAGroup;
    };
    let Some(mut cursor) = payload_prologue(record_bytes, start + 19, end) else {
        return NotAGroup;
    };
    let member_count_at = cursor;
    let Some(member_count) = View::u32_le_at(record_bytes, cursor) else {
        return NotAGroup;
    };
    cursor += 4;
    // A reference is at least one byte, so a count the rest of the record
    // cannot supply is corrupt and must not reach the allocator.
    if end
        .checked_sub(cursor)
        .is_none_or(|remaining| index_from_u32(member_count) > remaining)
    {
        return NotAGroup;
    }
    let members_at = cursor;
    for _ in 0..member_count {
        if take_record_reference(record_bytes, &mut cursor).is_none() {
            return NotAGroup;
        }
    }
    let mut auxiliary_slots = [None; 2];
    let mut auxiliary_reference_slots = [false; 2];
    for (present, slot) in auxiliary_reference_slots
        .iter_mut()
        .zip(&mut auxiliary_slots)
    {
        if record_bytes.get(cursor) == Some(&0) {
            cursor += 1;
            continue;
        }
        *present = true;
        let Some((record_index, offset)) = take_record_reference(record_bytes, &mut cursor) else {
            return NotAGroup;
        };

        *slot = Some(crate::records::identity::Located {
            value: record_index,
            offset,
        });
    }
    let Some(trailing_count) = View::u32_le_at(record_bytes, cursor) else {
        return NotAGroup;
    };
    cursor += 4;
    if end
        .checked_sub(cursor)
        .is_none_or(|remaining| index_from_u32(trailing_count) > remaining)
    {
        return NotAGroup;
    }
    let trailing_at = cursor;
    for _ in 0..trailing_count {
        if take_record_reference(record_bytes, &mut cursor).is_none() {
            return NotAGroup;
        }
    }
    let legacy_move_class_328 = scope.kind()
        == crate::records::feature::scope::DesignFeatureKind::Move
        && header.class_tag.as_str() == "328"
        && auxiliary_reference_slots == [false, true]
        && header.record_index.checked_add(13).is_some_and(|expected| {
            auxiliary_slots[1].is_some_and(|record| record.value == expected)
        })
        && trailing_count == 0;
    if legacy_move_class_328 {
        if record_bytes.get(cursor) != Some(&0) {
            return NotAGroup;
        }
        cursor += 1;
    }
    // The role occupies the high word of a u64 whose low word is zero.
    let role_at = cursor;
    let (Some(0), Some(role)) = (
        View::u32_le_at(record_bytes, role_at),
        View::u64_le_at(record_bytes, role_at),
    ) else {
        return NotAGroup;
    };
    let role = DesignOperandRole::from_raw(role);
    cursor += 8;
    if record_bytes.get(cursor..cursor + 10) != Some(&[0; 10]) {
        return NotAGroup;
    }
    cursor += 10;
    let opaque_index_at = cursor;
    let (Some(opaque_index), Some(opaque_scalar)) = (
        View::u32_le_at(record_bytes, cursor),
        View::f64_le_at(record_bytes, cursor + 4),
    ) else {
        return NotAGroup;
    };
    let Some(opaque_index) = std::num::NonZeroU32::new(opaque_index) else {
        return NotAGroup;
    };
    let Some(opaque_scalar) = cadmpeg_ir::scalar::FiniteReal::new(opaque_scalar) else {
        return NotAGroup;
    };
    cursor += 12;

    // Past this point the record has opened the group grammar, so a tail that
    // does not close is a group this reader cannot name.
    let Some(nested_index) = header.record_index.checked_add(1) else {
        return Unclosed;
    };
    let compact_split_group = scope.kind()
        == crate::records::feature::scope::DesignFeatureKind::Split
        && member_count != 0
        && member_count_at.checked_sub(start) == Some(split_group::MEMBER_COUNT)
        && auxiliary_reference_slots == [false, false]
        && trailing_count == 1
        && match role {
            DesignOperandRole::ROLE_0X21 => scope_reference_ordinal == 0,
            DesignOperandRole::BODIES_A => scope_reference_ordinal >= 2,
            _ => false,
        };
    let mut closed = None;
    for repeats_ordinal in [true, false] {
        let mut tail = cursor;
        if repeats_ordinal {
            if View::u32_le_at(record_bytes, tail) != Some(opaque_index.get()) {
                continue;
            }
            tail += 4;
        }
        let first_at = tail;
        let Some((first_tail_target, _)) = take_record_reference(record_bytes, &mut tail) else {
            continue;
        };
        let ordinary = Some(first_tail_target) == header.record_index.checked_add(2);
        for flag_bytes in [2usize, 3] {
            let Some(flags) = record_bytes.get(tail..tail + flag_bytes) else {
                continue;
            };
            if flags[flag_bytes - 2] > 1 || flags[flag_bytes - 1] > 1 {
                continue;
            }
            let variant = flags[flag_bytes - 2] != 0;
            let closing_flag = flags[flag_bytes - 1] != 0;
            let mut after = tail + flag_bytes;
            if take_record_reference(record_bytes, &mut after).map(|(index, _)| index)
                != Some(nested_index)
            {
                continue;
            }
            if record_bytes.get(after) != Some(&0) {
                continue;
            }
            after += 1;
            if take_record_reference(record_bytes, &mut after).map(|(index, _)| index)
                != Some(scope.record_index)
            {
                continue;
            }
            if after != end {
                continue;
            }
            let Some((paired_class_tag, after_tag)) =
                lp_ascii_filtered_view(bytes, end, 3..=3, u8::is_ascii_digit)
            else {
                continue;
            };
            if View::u32_le_at(bytes, after_tag) != Some(header.record_index) {
                continue;
            }
            let compact = !ordinary
                && compact_split_group
                && repeats_ordinal
                && flags == [0, 0]
                && marked_record_reference(record_bytes, first_at) == Some(first_tail_target)
                && split_metadata.is_some_and(|meta| {
                    super::split_groups::registered_companion(
                        bytes,
                        meta,
                        header,
                        first_tail_target,
                    )
                });
            if !ordinary && !compact {
                let has_independent_tail = if scope
                    .extrude_prologue()
                    .is_some_and(DesignExtrudePrologue::is_legacy_shifted_to_face)
                {
                    matches!(
                        role,
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::PROFILE
                            | DesignOperandRole::ROLE_0X12
                    )
                } else {
                    let ordinal = index_from_u32(scope_reference_ordinal);
                    let matching_role = if scope.is_legacy_class_330_blind_cut() {
                        paired_class_tag == "262"
                            && header.class_tag.as_str() == "291"
                            && matches!(
                                (ordinal, role),
                                (0, DesignOperandRole::BODIES_A) | (2, DesignOperandRole::PROFILE)
                            )
                    } else {
                        ordinal == 0
                            && role == DesignOperandRole::PROFILE
                            && scope.is_legacy_class_330_symmetric_distance()
                    };
                    matching_role
                        && member_count == 1
                        && marked_record_reference(record_bytes, members_at).is_some_and(|member| {
                            scope
                                .reference_members()
                                .values()
                                .skip(ordinal)
                                .take(2)
                                .copied()
                                .eq([header.record_index, member])
                        })
                };
                if !(has_independent_tail
                    && repeats_ordinal
                    && flags == [0, 0]
                    && first_tail_target > header.record_index
                    && offsets.is_some_and(|offsets| {
                        is_following_empty_extrude_record(
                            bytes,
                            offsets,
                            first_tail_target,
                            after_tag + size_of::<u32>(),
                        )
                    }))
                {
                    continue;
                }
            }
            if closed
                .replace((
                    variant,
                    closing_flag,
                    after,
                    paired_class_tag.to_owned(),
                    compact,
                ))
                .is_some()
            {
                return Unclosed;
            }
        }
    }
    let Some((variant, closing_flag, paired_at, paired_class_tag, compact)) = closed else {
        return Unclosed;
    };

    // Face groups take their start/termination use from their ordered position
    // in the scope, which `assign_extrude_face_roles` resolves once every group
    // of the scope is decoded.
    let operand_role = match extrude_operand_role(scope, role) {
        Some(PendingExtrudeRole::BodiesA) => DesignConstructionOperandRole::ExtrudeBodiesA,
        Some(PendingExtrudeRole::BodiesB) => DesignConstructionOperandRole::ExtrudeBodiesB,
        Some(PendingExtrudeRole::Profile) => DesignConstructionOperandRole::ExtrudeProfile,
        Some(PendingExtrudeRole::Faces(_)) | None => DesignConstructionOperandRole::Other(role),
    };
    let (Ok(member_count_offset), Ok(role_offset), Ok(opaque_index_offset), Ok(paired_byte_offset)) = (
        u64::try_from(member_count_at),
        u64::try_from(role_at),
        u64::try_from(opaque_index_at),
        u64::try_from(paired_at),
    ) else {
        return Unclosed;
    };
    let Ok(paired_class_tag) =
        crate::records::references::DesignClassTag::try_from(paired_class_tag)
    else {
        return Unclosed;
    };
    let Ok(opaque_scalar) = cadmpeg_ir::scalar::NonNegativeReal::try_from(opaque_scalar) else {
        return Unclosed;
    };
    // Candidate records share this prologue. Allocate the counted references
    // only after the role and paired frame establish the complete group grammar.
    if trailing_count > 1 {
        return Unclosed;
    }
    let mut members = Vec::new();
    let mut cursor = members_at;
    for _ in 0..member_count {
        let Some((value, offset)) = take_record_reference(record_bytes, &mut cursor) else {
            return NotAGroup;
        };
        members.push(crate::records::identity::Located { value, offset });
    }
    if compact
        && !scope
            .reference_members()
            .values()
            .skip(index_from_u32(scope_reference_ordinal))
            .take(members.len() + 1)
            .copied()
            .eq(std::iter::once(header.record_index)
                .chain(members.iter().map(|member| member.value)))
    {
        return Unclosed;
    }
    let auxiliary_records = auxiliary_slots.into_iter().flatten().collect::<Vec<_>>();
    let mut trailing_records = Vec::new();
    let mut cursor = trailing_at;
    for _ in 0..trailing_count {
        let Some((value, offset)) = take_record_reference(record_bytes, &mut cursor) else {
            return NotAGroup;
        };
        trailing_records.push(crate::records::identity::Located { value, offset });
    }
    let Ok(frame) = DesignConstructionOperandGroupFrame::from_parts(
        crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
            member_count_offset,
            auxiliary_records,
            auxiliary_paths: Vec::new(),
            trailing_records,
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index,
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset: opaque_index_offset + 4,
            variant,
            closing_flag,
        },
    ) else {
        return Unclosed;
    };
    let Ok(group) = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: String::new(),
            scope_record_index: scope.record_index,
            scope_reference_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            members,
            lost_edge_references: Vec::new(),
            frame,
            operand_role,
            role_offset,
            paired_class_tag,
            paired_byte_offset,
        },
    ) else {
        return Unclosed;
    };
    Complete(Box::new(group))
}

pub(super) fn is_following_empty_extrude_record(
    bytes: &[u8],
    offsets: &OnceCell<IndexedRecordOffsets>,
    record_index: u32,
    after: usize,
) -> bool {
    let records = offsets.get_or_init(|| IndexedRecordOffsets::build(bytes));
    let &[target] = records.offsets(record_index) else {
        return false;
    };
    let Some(end) = target.checked_add(group_tail_marker::LEN) else {
        return false;
    };
    target >= after
        && bytes
            .get(target + group_tail_marker::ZERO_PAYLOAD..end)
            .is_some_and(|payload| payload.iter().all(|&byte| byte == 0))
        && indexed_record_header_at(bytes, end).is_some()
}

pub(super) fn construction_tracking_registry<'cache>(
    scan: &ContainerScan,
    bulk_name: &str,
    registries: &'cache mut HashMap<String, Option<super::tracking_registration::Registry>>,
) -> Result<Option<&'cache super::tracking_registration::Registry>, CodecError> {
    let registry = match registries.entry(bulk_name.to_owned()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(slot) => slot.insert(
            crate::design::decode::meta::metadata_for_bulk_stream(scan, bulk_name)?
                .map(super::tracking_registration::Registry::new),
        ),
    };
    Ok(registry.as_ref())
}

/// Bind exact typed records selected by construction-group trailing runs.
pub(crate) fn bind_construction_operand_trailing_records(
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut registries = HashMap::new();
    for group in groups {
        group
            .frame
            .try_set_trailing_transforms(Vec::new())
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_dual_transforms(Vec::new())
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_flags(Vec::new())
            .map_err(CodecError::Malformed)?;
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut trailing_transforms = Vec::new();
        let mut trailing_dual_transforms = Vec::new();
        let mut trailing_flags = Vec::new();
        for record in group.frame.trailing_records() {
            let Some(header) = headers.get(&(stream, record.value)) else {
                continue;
            };
            if let Some(transform) = parse_construction_operand_transform(bytes, header) {
                if transform.record_index().checked_add(1)
                    != Some(transform.following_record_index())
                    && !construction_tracking_registry(scan, &entry.name, &mut registries)?
                        .is_some_and(|registry| registry.supports_transform(&transform))
                {
                    continue;
                }
                trailing_transforms.push(transform);
            } else if let Some(transform) = parse_construction_operand_dual_transform(bytes, header)
            {
                trailing_dual_transforms.push(transform);
            } else if let Some(flag) = parse_construction_operand_flag(bytes, header) {
                trailing_flags.push(flag);
            }
        }
        group
            .frame
            .try_set_trailing_transforms(trailing_transforms)
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_dual_transforms(trailing_dual_transforms)
            .map_err(CodecError::Malformed)?;
        group
            .frame
            .try_set_trailing_flags(trailing_flags)
            .map_err(CodecError::Malformed)?;
    }
    Ok(())
}

pub(super) fn parse_construction_operand_flag(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::topology::construction::DesignConstructionOperandFlag> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || bytes.get(start + 23) != Some(&0)
    {
        return None;
    }
    let value = match bytes.get(start + 22)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(
        crate::records::topology::construction::DesignConstructionOperandFlag {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            value,
            value_offset: u64::try_from(start + 22).ok()?,
        },
    )
}

/// Bind exact persistent-entity path records selected by construction groups.
pub(crate) fn bind_construction_operand_paths(
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = indexed_operand_headers(headers);
    for group in groups {
        group
            .frame
            .try_set_auxiliary_paths(Vec::new())
            .map_err(CodecError::Malformed)?;
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut auxiliary_paths = Vec::new();
        for record in &group.frame.auxiliary_records {
            let Some(header) = headers.get(&(stream, record.value)) else {
                continue;
            };
            if let Some(path) =
                parse_construction_operand_path(bytes, group.scope_record_index, header)
            {
                auxiliary_paths.push(path);
            }
        }
        group
            .frame
            .try_set_auxiliary_paths(auxiliary_paths)
            .map_err(CodecError::Malformed)?;
    }
    Ok(())
}

pub(super) fn parse_construction_operand_path(
    bytes: &[u8],
    expected_scope_record_index: u32,
    header: &DesignRecordHeader,
) -> Option<crate::records::topology::construction::DesignConstructionOperandPath> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let entity_at =
        if bytes.get(start + 11..start + 21)? == [0; 10] && bytes.get(start + 21) == Some(&1) {
            start + 22
        } else if extended_construction_path_prefix(bytes, start)? {
            start + extended_path::ENTITY_REFERENCE
        } else {
            return None;
        };
    let entity_ref = View::u64_le_at(bytes, entity_at)?;
    let entity_ref_offset = u64::try_from(entity_at).ok()?;
    let placement_at = entity_at + 8;
    let (placement, mut cursor) = if bytes.get(placement_at..placement_at + 3)? == [0; 3] {
        let transform = rigid_transform_at(bytes, placement_at + 3)?;
        if bytes.get(placement_at + 131) != Some(&0) {
            return None;
        }
        (
            crate::records::topology::construction::DesignConstructionPathPlacement::Transform(
                transform,
            ),
            placement_at + 132,
        )
    } else {
        let variant = match bytes.get(placement_at..placement_at + 4)? {
            [0, 0, variant @ (0 | 1), 0] => *variant != 0,
            _ => return None,
        };
        (
            crate::records::topology::construction::DesignConstructionPathPlacement::Compact(
                variant,
            ),
            placement_at + 4,
        )
    };
    let (scope_record_index, scope_record_index_offset) =
        take_record_reference(bytes, &mut cursor)?;
    if scope_record_index != expected_scope_record_index {
        return None;
    }
    let (nested_record_index, nested_record_index_offset) =
        take_record_reference(bytes, &mut cursor)?;
    if nested_record_index != header.record_index.checked_add(2)?
        || bytes.get(cursor..cursor + 6)? != [0; 6]
    {
        return None;
    }
    let following_at = cursor.checked_add(6)?;
    let (following_class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_tag)?;
    if following_record_index != header.record_index.checked_add(1)? {
        return None;
    }
    crate::records::topology::construction::DesignConstructionOperandPath::try_new(
        crate::records::topology::construction::DesignConstructionOperandPathDraft {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            entity_ref,
            entity_ref_offset,
            placement,
            scope_record_index,
            scope_record_index_offset,
            nested_record_index,
            nested_record_index_offset,
            following_record_index,
            following_byte_offset: u64::try_from(following_at).ok()?,
            following_class_tag: crate::design::decode::text::class_tag_from_view(
                following_class_tag,
            )
            .ok()?,
        },
    )
    .ok()
}

pub(super) fn extended_construction_path_prefix(bytes: &[u8], start: usize) -> Option<bool> {
    let zero_run = start + extended_path::ZERO_RUN;
    let trailing_zero_run = start + extended_path::TRAILING_ZERO_RUN;
    Some(
        bytes
            .get(zero_run..start + extended_path::PREFIX_VALUE)?
            .iter()
            .all(|byte| *byte == 0)
            && matches!(
                View::u32_le_at(bytes, start + extended_path::PREFIX_VALUE)?,
                crate::layout::assembly_locator_prefix_two::WORD_VALUE
                    | extended_path::PREFIX_VALUE_VALUE
            )
            && bytes
                .get(trailing_zero_run..start + extended_path::ENTITY_MARKER)?
                .iter()
                .all(|byte| *byte == 0)
            && *bytes.get(start + extended_path::ENTITY_MARKER)?
                == extended_path::ENTITY_MARKER_VALUE,
    )
}

pub(super) fn parse_construction_operand_transform(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::topology::construction::DesignConstructionOperandTransform> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + indexed_header::LEN..start + trailing_affine::MATRIX)?
        != [0; trailing_affine::MATRIX - indexed_header::LEN]
        || bytes.get(start + trailing_affine::TRAILER..start + trailing_affine::LEN)? != [1, 0]
    {
        return None;
    }
    let transform_at = start.checked_add(trailing_affine::MATRIX)?;
    let transform = rigid_transform_at(bytes, transform_at)?;
    let following_at = start.checked_add(trailing_affine::LEN)?;
    let (following_class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_tag)?;
    if following_record_index <= header.record_index {
        return None;
    }
    crate::records::topology::construction::DesignConstructionOperandTransform::try_new(
        crate::records::topology::construction::DesignConstructionOperandTransformDraft {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            transform,
            transform_offset: u64::try_from(transform_at).ok()?,
            following_record_index,
            following_byte_offset: u64::try_from(following_at).ok()?,
            following_class_tag: crate::design::decode::text::class_tag_from_view(
                following_class_tag,
            )
            .ok()?,
        },
    )
    .ok()
}

pub(super) fn parse_construction_operand_dual_transform(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::topology::construction::DesignConstructionOperandDualTransform> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10] || bytes.get(start + 277) != Some(&0) {
        return None;
    }
    let first_at = start.checked_add(21)?;
    let second_at = start.checked_add(149)?;
    Some(
        crate::records::topology::construction::DesignConstructionOperandDualTransform {
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            first_transform: rigid_transform_at(bytes, first_at)?,
            first_transform_offset: u64::try_from(first_at).ok()?,
            second_transform: rigid_transform_at(bytes, second_at)?,
            second_transform_offset: u64::try_from(second_at).ok()?,
        },
    )
}

pub(super) fn rigid_transform_at(
    bytes: &[u8],
    at: usize,
) -> Option<crate::records::sketch_placement::SketchPlacementMatrix> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut transform = [[0.0; 4]; 4];
    for row in &mut transform {
        for cell in row {
            *cell = view.f64_le()?;
        }
    }
    transform.try_into().ok()
}

/// Take one reference naming a record of the same segment, advancing `at` past
/// every byte it owns. Returns the record index and the byte offset of the low
/// word of the target entity id.
pub(super) fn take_record_reference(bytes: &[u8], at: &mut usize) -> Option<(u32, u64)> {
    let target_at = at.checked_add(1)?;
    let reference = take_reference(bytes, at)?;
    let (target, _) = reference.local()?;
    Some((u32::try_from(target).ok()?, u64::try_from(target_at).ok()?))
}

/// Decode the persistent identity frame named by each construction-operand group.
pub(crate) fn decode_construction_operand_identities(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(Vec<DesignConstructionOperandIdentity>, usize), CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut registries = HashMap::new();
    let mut untyped = 0;
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let Some(trailing_record_index) = group
            .frame
            .trailing_records()
            .first()
            .map(|record| &record.value)
        else {
            continue;
        };
        let Some(wrapper_header) = headers.get(&(stream, *trailing_record_index)) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let registry = construction_tracking_registry(scan, &entry.name, &mut registries)?;
        if let Some(mut identity) =
            parse_construction_operand_identity(bytes, group, wrapper_header, registry)
        {
            if identity
                .tracking_path()
                .is_some_and(|path| !registry.is_some_and(|registry| registry.supports(path)))
            {
                untyped += 1;
                continue;
            }
            identity.id = design_record_id(
                &entry.name,
                ":design-construction-operand-identity#",
                wrapper_header.byte_offset,
            );

            out.push(identity);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|left, right| left.id == right.id);
    Ok((out, untyped))
}

/// Collect one Design stream's lost-edge records in source byte order.
pub(super) fn collect_stream_lost_edges<'a>(
    lost_edges: &'a [LostEdgeReference],
    stream: &str,
) -> Vec<&'a LostEdgeReference> {
    let mut edges = lost_edges
        .iter()
        .filter(|edge| native_stream(&edge.id) == Some(stream))
        .collect::<Vec<_>>();
    edges.sort_by_key(|edge| edge.record_byte_offset());
    edges
}

/// Bind a contiguous unresolved-edge run to the construction group whose
/// first identity wrapper terminates that run.
///
/// A group binds only when it has one identity chain, one terminating run,
/// and a run cardinality equal to its operand count. Any other group keeps no
/// lost-edge references; its unresolved records stay unrepaired and are
/// reported as lost edge references.
pub(crate) fn bind_lost_edge_groups(
    groups: &mut [DesignConstructionOperandGroup],
    identities: &[DesignConstructionOperandIdentity],
    lost_edges: &[LostEdgeReference],
) {
    for group in groups {
        group.lost_edge_references.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut identity_matches = identities.iter().filter(|identity| {
            native_stream(&identity.id) == Some(stream)
                && identity.group_record_index == group.record_index
        });
        let Some(identity) = identity_matches.next() else {
            continue;
        };
        if identity_matches.next().is_some() {
            continue;
        }
        let Some(wrapper) = identity.wrappers().first() else {
            continue;
        };
        let wrapper_record_index = wrapper.record_index;
        let wrapper_byte_offset = wrapper.byte_offset;
        let wrapper_class_tag = wrapper.class_tag.as_str();
        let stream_edges = collect_stream_lost_edges(lost_edges, stream);
        let mut terminal = None;
        let mut multiple_terminals = false;
        for (ordinal, edge) in stream_edges.iter().enumerate() {
            if edge.next_record_index == wrapper_record_index
                && edge.next_byte_offset() == wrapper_byte_offset
                && edge.next_class_tag.as_str() == wrapper_class_tag
                && terminal.replace(ordinal).is_some()
            {
                multiple_terminals = true;
            }
        }
        if multiple_terminals {
            continue;
        }
        let Some(terminal) = terminal else {
            continue;
        };
        let mut start = terminal;
        while start > 0 {
            let previous = stream_edges[start - 1];
            let current = stream_edges[start];
            if previous.next_byte_offset() != current.record_byte_offset()
                || previous.next_record_index != current.record_index
                || previous.next_class_tag != current.class_tag
            {
                break;
            }
            start -= 1;
        }
        let run = &stream_edges[start..=terminal];
        if run.len() != group.members().len() {
            continue;
        }
        group.lost_edge_references = run.iter().map(|edge| edge.id.clone()).collect();
    }
}

pub(super) fn parse_construction_operand_identity(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    wrapper_header: &DesignRecordHeader,
    registry: Option<&super::tracking_registration::Registry>,
) -> Option<DesignConstructionOperandIdentity> {
    let mut current_at = usize::try_from(wrapper_header.byte_offset).ok()?;
    let mut current_record_index = wrapper_header.record_index;
    let mut current_class_tag = wrapper_header.class_tag.clone();
    let mut chain_started = false;
    if let Some(transform) = parse_construction_operand_transform(bytes, wrapper_header) {
        if transform.record_index().checked_add(1) != Some(transform.following_record_index())
            && !registry.is_some_and(|registry| registry.supports_transform(&transform))
        {
            return None;
        }
        current_at = usize::try_from(transform.following_byte_offset()).ok()?;
        current_record_index = transform.following_record_index();
        current_class_tag = transform.following_class_tag;
        chain_started = true;
    }
    let mut wrappers = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if parse_construction_tracking_path(
            bytes,
            current_at,
            current_record_index,
            &current_class_tag,
        )
        .is_some()
            || bytes.get(current_at + 11..current_at + 21)? != [0; 10]
            || bytes.get(current_at + 21..current_at + 24)? != [1, 1, 0]
        {
            break;
        }
        if seen.contains(&(current_record_index, current_at)) {
            return None;
        }

        seen.insert((current_record_index, current_at));

        wrappers.push(
            crate::records::topology::construction::DesignIdentityWrapper {
                record_index: current_record_index,
                byte_offset: u64::try_from(current_at).ok()?,
                class_tag: current_class_tag,
            },
        );
        current_at = current_at.checked_add(24)?;
        let (next_class_tag, after_next_tag) =
            lp_ascii_filtered_view(bytes, current_at, 0..=2000, u8::is_ascii_graphic)?;
        if next_class_tag.len() != 3 {
            return None;
        }
        current_record_index = View::u32_le_at(bytes, after_next_tag)?;
        current_class_tag =
            crate::design::decode::text::class_tag_from_view(next_class_tag).ok()?;
        chain_started = true;
    }
    let tracking_path = parse_construction_tracking_path(
        bytes,
        current_at,
        current_record_index,
        &current_class_tag,
    );
    if let Some(path) = &tracking_path {
        current_at = usize::try_from(path.following_byte_offset()).ok()?;
        current_record_index = path.following_record_index();
        current_class_tag.clone_from(&path.following_class_tag);
        chain_started = true;
    }
    if !chain_started {
        return None;
    }
    let persistent_identity = match parse_extrude_identity_member(bytes, current_at, None) {
        Some(member) => DesignConstructionPersistentIdentity::try_new(
            crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
                local_id: member.local_id,
                local_id_offset: member.local_id_offset,
                asset_id: member.asset_id.try_into().ok()?,
                asset_id_offset: member.asset_id_offset,
                context_id: member.context_id.try_into().ok()?,
                context_id_offset: member.context_id_offset,
                tail_slot_present: member.tail_slot_present,
                tail_slot_offset: member.tail_slot_offset,
                next_record_index: member.next_record_index,
                next_byte_offset: member.next_byte_offset,
            },
        )
        .ok(),
        None => None,
    };
    DesignConstructionOperandIdentity::try_new(
        crate::records::topology::construction::DesignConstructionOperandIdentityDraft {
            id: String::new(),
            group_record_index: group.record_index,
            wrappers,
            following_record_index: current_record_index,
            following_byte_offset: u64::try_from(current_at).ok()?,
            following_class_tag: current_class_tag,
            tracking_path,
            persistent_identity,
        },
    )
    .ok()
}

pub(super) fn parse_construction_tracking_path(
    bytes: &[u8],
    wrapper_at: usize,
    wrapper_record_index: u32,
    wrapper_class_tag: &crate::records::references::DesignClassTag,
) -> Option<DesignConstructionTrackingPath> {
    if bytes.get(wrapper_at + 11..wrapper_at + 21)? != [0; 10]
        || bytes.get(wrapper_at + 21) != Some(&1)
        || View::u64_le_at(bytes, wrapper_at + 22)?
            != u64::from(wrapper_record_index.checked_add(1)?)
        || bytes.get(wrapper_at + 30..wrapper_at + 33)? != [0; 3]
    {
        return None;
    }
    let carrier_at = wrapper_at.checked_add(33)?;
    let (carrier_class_tag, after_carrier_tag) =
        lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    let carrier_record_index = View::u32_le_at(bytes, after_carrier_tag)?;
    if carrier_record_index != wrapper_record_index.checked_add(1)?
        || bytes.get(carrier_at + 11..carrier_at + 21)? != [0; 10]
        || View::u32_le_at(bytes, carrier_at + 21)? != 1
        || View::u32_le_at(bytes, carrier_at + 25)? != 0
        || View::u32_le_at(bytes, carrier_at + 29)? != 1
        || !matches!(View::u32_le_at(bytes, carrier_at + 33)?, 2 | 3)
        || View::u64_le_at(bytes, carrier_at + 45)? != 0
        || View::u32_le_at(bytes, carrier_at + 53)? != 1
        || View::u64_le_at(bytes, carrier_at + 65)? != 0
    {
        return None;
    }
    let primary_identity = View::u64_le_at(bytes, carrier_at + 37)?;
    let selector = View::i32_le_at(bytes, carrier_at + 57)?;
    let kind = View::u32_le_at(bytes, carrier_at + 61)?;
    let mut cursor = carrier_at.checked_add(73)?;
    let first_related_identity = match take_optional_tracking_identity(bytes, &mut cursor) {
        TrackingIdentityField::Invalid => return None,
        TrackingIdentityField::Absent => None,
        TrackingIdentityField::Present(value) => Some(value),
    };
    let second_related_identity = match take_optional_tracking_identity(bytes, &mut cursor) {
        TrackingIdentityField::Invalid => return None,
        TrackingIdentityField::Absent => None,
        TrackingIdentityField::Present(value) => Some(value),
    };
    let following_at = cursor;
    let (following_class_tag, after_following_tag) =
        lp_ascii_filtered_view(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_following_tag)?;
    if following_record_index != carrier_record_index.checked_add(1)? {
        return None;
    }
    DesignConstructionTrackingPath::try_new(
        crate::records::topology::construction::DesignConstructionTrackingPathDraft {
            wrapper_record_index,
            wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
            wrapper_class_tag: wrapper_class_tag.clone(),
            carrier_record_index,
            carrier_byte_offset: u64::try_from(carrier_at).ok()?,
            carrier_class_tag: crate::design::decode::text::class_tag_from_view(carrier_class_tag)
                .ok()?,
            primary_identity,
            primary_identity_offset: u64::try_from(carrier_at + 37).ok()?,
            selector,
            selector_offset: u64::try_from(carrier_at + 57).ok()?,
            kind,
            kind_offset: u64::try_from(carrier_at + 61).ok()?,
            first_related_identity,
            second_related_identity,
            following_record_index,
            following_byte_offset: u64::try_from(following_at).ok()?,
            following_class_tag: crate::design::decode::text::class_tag_from_view(
                following_class_tag,
            )
            .ok()?,
        },
    )
    .ok()
}

pub(super) enum TrackingIdentityField {
    Invalid,
    Absent,
    Present(crate::records::identity::Located<u64>),
}

pub(super) fn take_optional_tracking_identity(
    bytes: &[u8],
    cursor: &mut usize,
) -> TrackingIdentityField {
    match View::u32_le_at(bytes, *cursor) {
        Some(0) => {
            let Some(next) = (*cursor).checked_add(4) else {
                return TrackingIdentityField::Invalid;
            };
            *cursor = next;
            TrackingIdentityField::Absent
        }
        Some(1) => {
            let Some(value_at) = (*cursor).checked_add(4) else {
                return TrackingIdentityField::Invalid;
            };
            let Some(value) = View::u64_le_at(bytes, value_at) else {
                return TrackingIdentityField::Invalid;
            };
            let Some(next) = value_at.checked_add(8) else {
                return TrackingIdentityField::Invalid;
            };
            *cursor = next;
            let Ok(offset) = u64::try_from(value_at) else {
                return TrackingIdentityField::Invalid;
            };
            TrackingIdentityField::Present(crate::records::identity::Located { value, offset })
        }
        _ => TrackingIdentityField::Invalid,
    }
}

pub(crate) fn construction_closing_flag_losses(
    groups: &[DesignConstructionOperandGroup],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    groups
        .iter()
        .filter(|group| group.frame.closing_flag)
        .map(|group| {
            crate::loss::F3dLossCode::ConstructionClosingFlagAssumed.note(format!(
                "construction group {} has closing flag 1; its interpretation assumes the flag does not change operation semantics",
                group.id
            ))
        })
        .collect()
}
