// SPDX-License-Identifier: Apache-2.0
//! Entity selection operands and work point sketch point frames.

use super::face_sources::indexed_operand_headers;
use crate::bytes::{is_guid_relaxed, lp_ascii_filtered_view, lp_utf16_bounded};
use crate::container::ContainerScan;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::layout::{
    coil_compact_face_selection_prefix as coil_face_sel,
    coil_compact_persistent_selection_prefix as coil_persist_sel,
    coil_modern_selection_prefix as coil_modern_sel,
    indexed_design_record_header as indexed_header, marked_sketch_curve_identity as marked_curve,
    work_point_sketch_point_compact_identity as compact_identity,
    work_point_sketch_point_identity as sketch_point_identity,
};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignWorkPointSketchPointIdentityForm;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandIdentity,
};
use crate::records::topology::entity_selection::{
    DesignEntitySelectionOperand, SelectionSerializationForm,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

/// The wrapper, carrier, and following records of every decoded tracking
/// path. A tracking path can follow identity wrappers whose record indices are
/// unrelated to its own, so its headers are named by the path itself.
pub(crate) fn tracking_path_record_indices(
    identities: &[DesignConstructionOperandIdentity],
) -> impl Iterator<Item = (&str, u32)> {
    identities.iter().flat_map(|identity| {
        native_stream(&identity.id)
            .zip(identity.tracking_path())
            .into_iter()
            .flat_map(|(stream, path)| {
                [
                    path.wrapper_record_index(),
                    path.carrier_record_index(),
                    path.following_record_index(),
                ]
                .map(move |record_index| (stream, record_index))
            })
    })
}

/// Decode nested persistent-entity frames named by construction groups.
pub(crate) fn decode_entity_selection_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEntitySelectionOperand>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let loft_point_group = is_loft_point_group(group, scopes);
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut operand) =
                parse_entity_selection_operand_with(bytes, group, ordinal, header, loft_point_group)
            {
                operand.id = design_record_id(
                    &entry.name,
                    ":design-entity-selection-operand#",
                    header.byte_offset,
                );

                out.push(operand);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Whether `group` is a role-`0x21` group whose one owning scope is a Loft, so
/// that its members can carry the compact sketch-point identity.
pub(crate) fn is_loft_point_group(
    group: &DesignConstructionOperandGroup,
    scopes: &[DesignParameterScope],
) -> bool {
    let mut owners = scopes.iter().filter(|scope| {
        scope.record_index == group.scope_record_index
            && native_stream(&scope.id) == native_stream(&group.id)
    });
    group.role() == DesignOperandRole::ROLE_0X21
        && owners.next().is_some_and(|scope| {
            matches!(
                crate::design::design_feature_family(&scope.kind()),
                Some(crate::design::DesignFeatureFamily::Loft)
            )
        })
        && owners.next().is_none()
}

pub(in crate::design::decode) fn parse_entity_selection_operand(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignEntitySelectionOperand> {
    parse_entity_selection_operand_with(bytes, group, group_member_ordinal, header, false)
}

/// `parse_entity_selection_operand`; `sketch_point` also admits the compact
/// sketch-point identity of a Loft point member.
pub(super) fn parse_entity_selection_operand_with(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    sketch_point: bool,
) -> Option<DesignEntitySelectionOperand> {
    let frame = parse_entity_selection_frame_with(
        bytes,
        header.record_index,
        header.byte_offset,
        header.class_tag.as_str(),
        sketch_point,
    )?;
    DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: String::new(),
            scope_record_index: group.scope_record_index,
            group_record_index: group.record_index,
            group_member_ordinal,
            record_index: frame.record_index,
            byte_offset: frame.byte_offset,
            class_tag: frame.class_tag,
            asset_id: frame.asset_id.try_into().ok()?,
            asset_id_offset: frame.asset_id_offset,
            context_id: frame.context_id.try_into().ok()?,
            context_id_offset: frame.context_id_offset,
            serialization_form: frame.serialization_form,
            identity_record_index: frame.identity_record_index,
            identity_record_offset: frame.identity_record_offset,
            primary_identity: frame.primary_identity,
            primary_identity_offset: frame.primary_identity_offset,
            secondary: frame.secondary,
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: frame.next_record_index,
            next_byte_offset: frame.next_byte_offset,
        },
    )
    .ok()
}

/// Persistent identity payload shared by entity-selection consumers that do
/// not belong to a construction-operand group.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::design::decode) struct EntitySelectionFrame {
    pub(in crate::design::decode) record_index: u32,
    pub(in crate::design::decode) byte_offset: u64,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(in crate::design::decode) asset_id: String,
    pub(in crate::design::decode) asset_id_offset: u64,
    pub(in crate::design::decode) context_id: String,
    pub(in crate::design::decode) context_id_offset: u64,
    pub(in crate::design::decode) serialization_form: SelectionSerializationForm,
    pub(in crate::design::decode) identity_record_index: u32,
    pub(in crate::design::decode) identity_record_offset: u64,
    pub(in crate::design::decode) primary_identity: u64,
    pub(in crate::design::decode) primary_identity_offset: u64,
    pub(in crate::design::decode) secondary: Option<
        crate::records::identity::DesignSecondaryIdentity<crate::records::identity::Located<u64>>,
    >,
    pub(in crate::design::decode) next_record_index: u32,
    pub(in crate::design::decode) next_byte_offset: u64,
}

pub(in crate::design::decode) struct EntitySelectionPrefix {
    pub(in crate::design::decode) asset_id: String,
    pub(in crate::design::decode) asset_id_offset: u64,
    pub(in crate::design::decode) context_id: String,
    pub(in crate::design::decode) context_id_offset: u64,
    pub(in crate::design::decode) serialization_form: SelectionSerializationForm,
    pub(super) after_context_id: usize,
}

pub(in crate::design::decode) fn parse_entity_selection_prefix(
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Option<EntitySelectionPrefix> {
    // Persistent selections use a ten-byte prelude and a u32 presence value.
    // Face-recipe selections add two prelude bytes, encode presence as one
    // byte, and add three zero bytes before the first UTF-16 length.
    let asset_start = if bytes.get(start + 11..start + coil_persist_sel::NESTED_SELECTION_MARKER)?
        == [0; 10]
        && bytes.get(start + coil_persist_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_persist_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(start + 26..start + coil_persist_sel::ASSET_PRESENCE)? == [0; 6]
        && View::u32_le_at(bytes, start + coil_persist_sel::ASSET_PRESENCE)? == 1
    {
        start.checked_add(coil_persist_sel::ASSET_UUID_LENGTH)?
    } else if let Some(asset_start) = typed_nested_selection_asset_start(bytes, start, record_index)
    {
        asset_start
    } else if bytes.get(start + 11..start + coil_modern_sel::NESTED_SELECTION_MARKER)? == [0; 11]
        && bytes.get(start + coil_modern_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_modern_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(
            start + coil_modern_sel::NESTED_RECORD_INDEX + 4
                ..start + coil_modern_sel::ASSET_PRESENCE,
        )? == [0; 6]
        && View::u32_le_at(bytes, start + coil_modern_sel::ASSET_PRESENCE)? == 1
    {
        start.checked_add(coil_modern_sel::ASSET_UUID_LENGTH)?
    } else if bytes.get(start + 11..start + coil_face_sel::NESTED_SELECTION_MARKER)? == [0; 12]
        && bytes.get(start + coil_face_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_face_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(start + 28..start + coil_face_sel::ASSET_PRESENCE)? == [0; 6]
        && bytes.get(start + coil_face_sel::ASSET_PRESENCE) == Some(&1)
    {
        start.checked_add(coil_face_sel::ASSET_UUID_LENGTH)?
    } else {
        return None;
    };
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, asset_start, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let serialization_form = SelectionSerializationForm::try_from(View::u32_le_at(
        bytes,
        after_context_id.checked_add(4)?,
    )?)
    .ok()?;
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
    {
        return None;
    }
    Some(EntitySelectionPrefix {
        asset_id,
        asset_id_offset: u64::try_from(asset_start.checked_add(4)?).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id.checked_add(4)?).ok()?,
        serialization_form,
        after_context_id,
    })
}

/// The byte after the selection's inline-typed nested reference, where the
/// asset UUID length starts. A container generation whose references carry
/// inline type GUIDs writes the reference to record `N+3` as a typed
/// reference: the persistent-prefix reference with its LP-ASCII target type
/// GUID after the u64 target entity ID.
fn typed_nested_selection_asset_start(
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Option<usize> {
    if bytes.get(start + 11..start + coil_persist_sel::NESTED_SELECTION_MARKER)? != [0; 10]
        || bytes.get(start + coil_persist_sel::NESTED_SELECTION_MARKER) != Some(&1)
        || View::u32_le_at(bytes, start + coil_persist_sel::NESTED_RECORD_INDEX)?
            != record_index.checked_add(3)?
    {
        return None;
    }
    let type_guid_at = start.checked_add(coil_persist_sel::NESTED_RECORD_INDEX + 8)?;
    if bytes.get(start + coil_persist_sel::NESTED_RECORD_INDEX + 4..type_guid_at)? != [0; 4] {
        return None;
    }
    let (type_guid, after_type_guid) =
        lp_ascii_filtered_view(bytes, type_guid_at, 36..=37, u8::is_ascii_graphic)?;
    if !is_guid_relaxed(type_guid)
        || bytes.get(after_type_guid..after_type_guid.checked_add(2)?)? != [0; 2]
        || View::u32_le_at(bytes, after_type_guid.checked_add(2)?)? != 1
    {
        return None;
    }
    after_type_guid.checked_add(6)
}

/// Match the selected curve identity carried by a nested entity-selection frame.
pub(in crate::design) fn entity_selection_matches_curve(
    operand: &DesignEntitySelectionOperand,
    curve: &SketchCurveIdentity,
) -> bool {
    operand.sketch_point().is_none()
        && operand.secondary().is_some_and(|secondary| {
            curve.primary_id.get() == secondary.identity.value
                && secondary
                    .curve_identity
                    .is_none_or(|identity| curve.secondary_id == identity.value)
        })
}

/// Direct sketch-point identity carried by a `WorkPoint` input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorkPointSketchPointFrame {
    pub(super) asset_id: String,
    pub(super) asset_id_offset: u64,
    pub(super) context_id: String,
    pub(super) context_id_offset: u64,
    pub(super) serialization_form: SelectionSerializationForm,
    pub(super) identity_record_index: u32,
    pub(super) identity_record_offset: u64,
    pub(super) identity_form: DesignWorkPointSketchPointIdentityForm,
    pub(super) sketch_record_index: u32,
    pub(super) sketch_record_index_offset: u64,
    pub(super) point_persistent_id: u64,
    pub(super) point_persistent_id_offset: u64,
    pub(super) next_record_index: u32,
    pub(super) next_byte_offset: u64,
}

/// Parse the direct sketch-point identity variant of a `WorkPoint` input.
///
/// The outer prefix is shared with persistent entity selections. Its identity
/// record is a separate four-record envelope. The marked identity stores nine
/// zero bytes, a one-byte presence marker, and two marked `u32` slots
/// separated by zero `u32` values; the compact identity stores ten zero bytes
/// and two `u64` slots. The following record is the point-data record.
pub(super) fn parse_work_point_sketch_point_frame(
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
) -> Option<WorkPointSketchPointFrame> {
    let start = usize::try_from(byte_offset).ok()?;
    let prefix = parse_entity_selection_prefix(bytes, start, record_index)?;
    let paired_at = next_indexed_record_offset(bytes, prefix.after_context_id + 8)?;
    let nested_one_at = next_indexed_record_offset(bytes, paired_at + indexed_header::LEN)?;
    let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + indexed_header::LEN)?;
    let identity_at = next_indexed_record_offset(bytes, nested_two_at + indexed_header::LEN)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + indexed_header::LEN)?;
    let expected = [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ];
    for (offset, expected) in [paired_at, nested_one_at, nested_two_at, identity_at]
        .into_iter()
        .zip(expected)
    {
        let (_, after_tag) = lp_ascii_filtered_view(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_tag)? != expected {
            return None;
        }
    }
    let (_, after_next_tag) =
        lp_ascii_filtered_view(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
    let next_record_index = View::u32_le_at(bytes, after_next_tag)?;
    if next_record_index != record_index.checked_add(4)? {
        return None;
    }
    let (identity_form, sketch_record_index, point_persistent_id) =
        if identity_at.checked_add(sketch_point_identity::LEN)? == next_at {
            if bytes.get(
                identity_at + indexed_header::LEN..identity_at + sketch_point_identity::PRESENCE,
            )? != [0; 9]
                || bytes.get(identity_at + sketch_point_identity::PRESENCE) != Some(&1)
                || View::u32_le_at(bytes, identity_at + sketch_point_identity::PRESENCE + 1)? != 0
                || View::u32_le_at(
                    bytes,
                    identity_at + sketch_point_identity::SKETCH_RECORD_INDEX + 4,
                )? != 0
            {
                return None;
            }
            (
                DesignWorkPointSketchPointIdentityForm::Marked,
                View::u32_le_at(
                    bytes,
                    identity_at + sketch_point_identity::SKETCH_RECORD_INDEX,
                )?,
                u64::from(View::u32_le_at(
                    bytes,
                    identity_at + sketch_point_identity::POINT_PERSISTENT_ID,
                )?),
            )
        } else if identity_at.checked_add(compact_identity::LEN)? == next_at {
            if bytes
                .get(
                    identity_at + indexed_header::LEN
                        ..identity_at + compact_identity::SKETCH_RECORD_INDEX,
                )?
                .iter()
                .any(|byte| *byte != 0)
            {
                return None;
            }
            (
                DesignWorkPointSketchPointIdentityForm::Compact,
                u32::try_from(View::u64_le_at(
                    bytes,
                    identity_at + compact_identity::SKETCH_RECORD_INDEX,
                )?)
                .ok()?,
                View::u64_le_at(bytes, identity_at + compact_identity::POINT_PERSISTENT_ID)?,
            )
        } else {
            return None;
        };
    Some(WorkPointSketchPointFrame {
        asset_id: prefix.asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id: prefix.context_id,
        context_id_offset: prefix.context_id_offset,
        serialization_form: prefix.serialization_form,
        identity_record_index: record_index.checked_add(3)?,
        identity_record_offset: u64::try_from(identity_at).ok()?,
        identity_form,
        sketch_record_index,
        sketch_record_index_offset: u64::try_from(
            identity_at.checked_add(identity_form.sketch_record_index_offset())?,
        )
        .ok()?,
        point_persistent_id,
        point_persistent_id_offset: u64::try_from(
            identity_at.checked_add(identity_form.point_persistent_id_offset())?,
        )
        .ok()?,
        next_record_index,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

/// Parse the nested persistent-entity frame without assigning group ownership.
pub(in crate::design::decode) fn parse_entity_selection_frame(
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
    class_tag: &str,
) -> Option<EntitySelectionFrame> {
    parse_entity_selection_frame_with(bytes, record_index, byte_offset, class_tag, false)
}

/// `parse_entity_selection_frame`; `sketch_point` also admits the compact
/// sketch-point identity of a Loft point member.
pub(in crate::design::decode) fn parse_entity_selection_frame_with(
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
    class_tag: &str,
    sketch_point: bool,
) -> Option<EntitySelectionFrame> {
    let start = usize::try_from(byte_offset).ok()?;
    let prefix = parse_entity_selection_prefix(bytes, start, record_index)?;
    let paired_at = next_indexed_record_offset(bytes, prefix.after_context_id + 8)?;
    let nested_one_at = next_indexed_record_offset(bytes, paired_at + 11)?;
    let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + 11)?;
    let identity_at = next_indexed_record_offset(bytes, nested_two_at + 11)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + 11)?;
    let expected = [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ];
    for (offset, expected) in [paired_at, nested_one_at, nested_two_at, identity_at]
        .into_iter()
        .zip(expected)
    {
        let (_, after_tag) = lp_ascii_filtered_view(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_tag)? != expected {
            return None;
        }
    }
    let (_, after_next_tag) =
        lp_ascii_filtered_view(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
    let next_record_index = View::u32_le_at(bytes, after_next_tag)?;
    let (primary_identity_offset, primary_identity, secondary) = if bytes
        .get(identity_at + marked_curve::ZERO_PREFIX..identity_at + marked_curve::PRESENCE)?
        == [0; 9]
        && bytes.get(identity_at + marked_curve::PRESENCE) == Some(&1)
        && bytes.get(
            identity_at + marked_curve::PRESENCE + 1
                ..identity_at + marked_curve::OWNER_RECORD_INDEX,
        )? == [0; 12]
        && View::u32_le_at(bytes, identity_at + marked_curve::OWNER_HIGH_ZERO)? == 0
        && View::u32_le_at(bytes, identity_at + marked_curve::CURVE_HIGH_ZERO)? == 0
        && identity_at.checked_add(marked_curve::LEN)? == next_at
        && next_record_index == record_index.checked_add(4)?
    {
        let primary_identity = u64::from(View::u32_le_at(
            bytes,
            identity_at + marked_curve::OWNER_RECORD_INDEX,
        )?);
        let secondary_identity = u64::from(View::u32_le_at(
            bytes,
            identity_at + marked_curve::CURVE_PERSISTENT_ID,
        )?);
        (
            identity_at.checked_add(marked_curve::OWNER_RECORD_INDEX)?,
            primary_identity,
            Some(crate::records::identity::DesignSecondaryIdentity {
                identity: crate::records::identity::Located {
                    value: secondary_identity,
                    offset: u64::try_from(
                        identity_at.checked_add(marked_curve::CURVE_PERSISTENT_ID)?,
                    )
                    .ok()?,
                },
                curve_identity: None,
            }),
        )
    } else if bytes.get(identity_at + 11..identity_at + 21)? == [0; 10]
        && identity_at.checked_add(45)? == next_at
    {
        let curve_secondary_identity_offset = identity_at.checked_add(21)?;
        let primary_identity_offset = identity_at.checked_add(29)?;
        let secondary_identity_offset = identity_at.checked_add(37)?;
        (
            primary_identity_offset,
            View::u64_le_at(bytes, primary_identity_offset)?,
            Some(crate::records::identity::DesignSecondaryIdentity {
                identity: crate::records::identity::Located {
                    value: View::u64_le_at(bytes, secondary_identity_offset)?,
                    offset: u64::try_from(secondary_identity_offset).ok()?,
                },
                curve_identity: Some(crate::records::identity::Located {
                    value: View::u64_le_at(bytes, curve_secondary_identity_offset)?,
                    offset: u64::try_from(curve_secondary_identity_offset).ok()?,
                })
                .filter(|identity| identity.value != 0),
            }),
        )
    } else if sketch_point
        && bytes.get(identity_at + 11..identity_at + compact_identity::SKETCH_RECORD_INDEX)?
            == [0; 10]
        && identity_at.checked_add(compact_identity::LEN)? == next_at
    {
        let primary_identity_offset =
            identity_at.checked_add(compact_identity::SKETCH_RECORD_INDEX)?;
        let secondary_identity_offset =
            identity_at.checked_add(compact_identity::POINT_PERSISTENT_ID)?;
        (
            primary_identity_offset,
            View::u64_le_at(bytes, primary_identity_offset)?,
            Some(crate::records::identity::DesignSecondaryIdentity {
                identity: crate::records::identity::Located {
                    value: View::u64_le_at(bytes, secondary_identity_offset)?,
                    offset: u64::try_from(secondary_identity_offset).ok()?,
                },
                curve_identity: None,
            }),
        )
    } else if bytes.get(identity_at + 11..identity_at + 21)? == [0; 10]
        && identity_at.checked_add(29)? == next_at
    {
        let primary_identity_offset = identity_at.checked_add(21)?;
        (
            primary_identity_offset,
            View::u64_le_at(bytes, primary_identity_offset)?,
            None,
        )
    } else {
        return None;
    };
    Some(EntitySelectionFrame {
        record_index,
        byte_offset,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        asset_id: prefix.asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id: prefix.context_id,
        context_id_offset: prefix.context_id_offset,
        serialization_form: prefix.serialization_form,
        identity_record_index: record_index.checked_add(3)?,
        identity_record_offset: u64::try_from(identity_at).ok()?,
        primary_identity,
        primary_identity_offset: u64::try_from(primary_identity_offset).ok()?,
        secondary,
        next_record_index,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}
