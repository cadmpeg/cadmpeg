// SPDX-License-Identifier: Apache-2.0
//! Shared helpers for design-owner unit tests.

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::parameters::design_parameter_discriminator;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::lp_utf16;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Run a test with a decode context that has the default resource policy.
pub(crate) fn with_test_decode_context<T>(f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    f(&ctx)
}

/// Build a record index under the default test decode policy.
pub(in crate::design) fn indexed_record_offsets_for_test(
    bytes: &[u8],
) -> crate::design::decode::sketch::IndexedRecordOffsets {
    crate::design::decode::sketch::IndexedRecordOffsets::build(bytes)
}

pub(crate) fn parameter_record(
    owner: Option<u32>,
    expression: &str,
    source_kind: &str,
    unit: Option<&str>,
    name: &str,
    evaluated_value: f64,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"305");
    out.extend_from_slice(&71u32.to_le_bytes());
    out.extend_from_slice(&[0; 11]);
    out.extend_from_slice(&design_parameter_discriminator(source_kind).to_le_bytes());
    out.push(0);
    out.extend_from_slice(&9u32.to_le_bytes());
    match owner {
        Some(owner) => {
            out.push(1);
            out.extend_from_slice(&owner.to_le_bytes());
            out.extend_from_slice(&[0; 6]);
        }
        None => out.push(0),
    }
    lp_utf16(&mut out, expression);
    out.extend_from_slice(if owner.is_some() {
        &[0; 9]
    } else {
        &[0, 0, 0, 0, 0, 0, 0, 0, 1]
    });
    lp_utf16(&mut out, source_kind);
    out.extend_from_slice(&0u32.to_le_bytes());
    if let Some(unit) = unit {
        lp_utf16(&mut out, unit);
    }
    lp_utf16(&mut out, name);
    out.extend_from_slice(&evaluated_value.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

pub(crate) fn parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 104];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"292");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40..48].copy_from_slice(&6.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..53].copy_from_slice(&45u32.to_le_bytes());
    frame[59..63].copy_from_slice(&9u32.to_le_bytes());
    frame[67] = 1;
    frame[68..72].copy_from_slice(&12u32.to_le_bytes());
    frame[78] = 1;
    frame[79] = 1;
    frame[81] = 1;
    frame[82..86].copy_from_slice(&46u32.to_le_bytes());
    frame[93] = 1;
    frame[94..98].copy_from_slice(&12u32.to_le_bytes());
    frame
}

pub(super) fn identity_matrix() -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub(super) fn design_type(
    class_tag: u32,
    type_guid: &str,
    base_type_guid: Option<&str>,
    version: u32,
    module: &str,
    entity_ids: Vec<u64>,
) -> crate::records::entity_header::SegmentTypeData {
    crate::records::entity_header::SegmentTypeData {
        class_tag: (class_tag)
            .to_string()
            .try_into()
            .expect("fixture class tag"),
        byte_offset: Some(0),
        type_guid: type_guid.to_owned().try_into().expect("type GUID"),
        type_guid_offset: Some(0),
        base_type_guid: base_type_guid.map_or(
            crate::records::entity_header::BaseTypeGuid::Absent,
            |value| crate::records::entity_header::BaseTypeGuid::Guid {
                value: value.to_owned().try_into().expect("base GUID"),
                offset: Some(0),
            },
        ),
        version,
        version_offset: Some(0),
        module: module.into(),
        entities: crate::records::identity::ReferenceRun::unlocated(entity_ids),
    }
}

pub(super) fn primary_record(
    entity_id: u64,
    bulk_offset: usize,
) -> crate::metastream::RecordIndexEntry {
    crate::metastream::RecordIndexEntry {
        entity_id,
        bulk_offset: u64_from_index(bulk_offset),
    }
}

pub(super) fn push_reference(out: &mut Vec<u8>, reference: u32) {
    out.push(1);
    out.extend_from_slice(&reference.to_le_bytes());
}

pub(super) fn push_genesis_block(out: &mut Vec<u8>, genesis: u64) {
    out.push(1);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&13u32.to_le_bytes());
    out.extend_from_slice(b"EntityGenesis");
    out.extend_from_slice(&23u32.to_le_bytes());
    out.extend_from_slice(b"IntrinsicMetaTypeuint64");
    out.extend_from_slice(&genesis.to_le_bytes());
}

pub(super) fn assembly_operand_frame_fixture(scope_record_index: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 648];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"273");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[20] = 1;
    bytes[25] = 1;
    for (reference_at, transform_at, reference, translation) in [
        (28, 40, 70_u32, [1.0_f64, 2.0, 3.0]),
        (168, 180, 80_u32, [4.0, 5.0, 6.0]),
    ] {
        bytes[reference_at] = 1;
        bytes[reference_at + 1..reference_at + 5].copy_from_slice(&reference.to_le_bytes());
        for (ordinal, value) in [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
            0.0,
            0.0,
            0.0,
            1.0,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[transform_at + ordinal * 8..transform_at + ordinal * 8 + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[637..641].copy_from_slice(&3_u32.to_le_bytes());
    bytes[641..644].copy_from_slice(b"259");
    bytes[644..648].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes
}

/// Build exact timeline frames for fixture scopes in their supplied order.
pub(crate) fn synthetic_feature_timelines(
    scopes: &[crate::records::feature::scope::DesignParameterScope],
) -> Vec<crate::records::entity_header::DesignFeatureTimeline> {
    let mut streams = Vec::<(&str, Vec<crate::records::identity::Located<u64>>)>::new();
    for scope in scopes {
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let item = crate::records::identity::Located {
            value: u64::from(scope.record_index),
            offset: 0,
        };
        if let Some((_, items)) = streams
            .iter_mut()
            .find(|(candidate, _)| *candidate == stream)
        {
            items.push(item);
        } else {
            streams.push((stream, vec![item]));
        }
    }
    streams
        .into_iter()
        .map(|(stream, items)| {
            crate::records::entity_header::DesignFeatureTimeline::try_new(
                crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
                crate::records::entity_header::DesignTimelineFrame::test_items(0, items),
                crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
                std::num::NonZeroU64::new(1).unwrap(),
                0,
                std::num::NonZeroU64::new(1).unwrap(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>()
}

/// Lengthen a synthetic scope's frame to `frame_length`, keeping its
/// reference table, kind, and ordinal at their distance from the frame end.
pub(crate) fn lengthen_scope_frame(
    draft: &mut crate::records::feature::scope::DesignParameterScopeDraft,
    frame_length: u64,
) {
    let shift = frame_length - draft.frame_length;
    draft.reference_count_offset += shift;
    if let Some(rows) = draft.reference_members.located_rows() {
        draft.reference_members = crate::records::identity::ReferenceRun::located(
            rows.iter()
                .map(|member| crate::records::identity::Located {
                    value: member.value,
                    offset: member.offset + shift,
                })
                .collect(),
        );
    }
    draft.kind_offset += shift;
    draft.feature_ordinal_offset += shift;
    draft.frame_length = frame_length;
    draft.paired_byte_offset += shift;
}

/// Append `records` to the ordered reference table of `scope`, as a scope
/// names the parameter owners that belong to it.
pub(crate) fn name_scope_references(
    scope: &mut crate::records::feature::scope::DesignParameterScope,
    records: impl IntoIterator<Item = u32>,
) {
    let mut members = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    members.extend(records);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(members);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .expect("the scope fixture admits its extended reference table");
}

pub(crate) fn set_extrude_operation(scope: &mut DesignParameterScope, operation: DesignExtrudeOperation) {
    let Some(
        DesignExtrudePrologue::LegacyDistance {
            operation: value, ..
        }
        | DesignExtrudePrologue::ReferenceAware {
            operation: value, ..
        }
        | DesignExtrudePrologue::ShiftedReferenceAware {
            operation: value, ..
        }
        | DesignExtrudePrologue::LegacyShifted {
            operation: value, ..
        },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *value = operation;
}

pub(crate) fn set_extrude_extent(scope: &mut DesignParameterScope, extent: DesignExtrudeExtent) {
    let Some(prologue) = scope.extrude_prologue_mut() else {
        panic!("test scope must carry an Extrude prologue");
    };
    match prologue {
        DesignExtrudePrologue::LegacyDistance { .. } => {
            assert_eq!(extent, DesignExtrudeExtent::OneSidedDistance);
        }
        DesignExtrudePrologue::ReferenceAware {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            first_side_target_ordinal,
            ..
        } => {
            *value = extent;
            (direction_face_extend_values[0], *side_extent_discriminators) = match extent {
                DesignExtrudeExtent::OneSidedDistance => (1, [1, 0]),
                DesignExtrudeExtent::OneSidedToFace => (1, [2, 0]),
                DesignExtrudeExtent::OneSidedThroughNext => (1, [3, 0]),
                DesignExtrudeExtent::OneSidedThroughAll => (1, [4, 0]),
                DesignExtrudeExtent::TwoSidedToFaces => (2, [2, 0]),
                DesignExtrudeExtent::TwoSidedDistance => (2, [1, 1]),
                DesignExtrudeExtent::TwoSidedDistanceToFace => (2, [1, 2]),
                DesignExtrudeExtent::SymmetricDistance => (3, [1, 0]),
                DesignExtrudeExtent::SymmetricThroughAll => (3, [4, 4]),
            };
            if extent != DesignExtrudeExtent::OneSidedToFace {
                *first_side_target_ordinal = None;
            }
        }
        DesignExtrudePrologue::LegacyShifted {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            ..
        } => {
            *value = Some(extent);
            (*direction_face_extend_values, *side_extent_discriminators) = match extent {
                DesignExtrudeExtent::OneSidedDistance => ([1, 0], [1, 0]),
                DesignExtrudeExtent::OneSidedToFace => ([1, 0], [2, 0]),
                DesignExtrudeExtent::OneSidedThroughNext => ([1, 0], [3, 0]),
                DesignExtrudeExtent::OneSidedThroughAll => ([1, 0], [4, 0]),
                DesignExtrudeExtent::TwoSidedToFaces => ([2, 0], [2, 0]),
                DesignExtrudeExtent::TwoSidedDistance => ([2, 0], [1, 1]),
                DesignExtrudeExtent::TwoSidedDistanceToFace => ([2, 0], [1, 2]),
                DesignExtrudeExtent::SymmetricDistance => ([3, 0], [1, 0]),
                DesignExtrudeExtent::SymmetricThroughAll => ([3, 0], [4, 4]),
            };
        }
        DesignExtrudePrologue::ShiftedReferenceAware {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            ..
        } => {
            assert_eq!(extent, DesignExtrudeExtent::TwoSidedToFaces);
            *value = extent;
            (*direction_face_extend_values, *side_extent_discriminators) = ([2, 1], [2, 0]);
        }
    }
}

pub(crate) fn set_extrude_direction_reversed(scope: &mut DesignParameterScope, reversed: bool) {
    let Some(
        DesignExtrudePrologue::LegacyDistance {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::ReferenceAware {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::ShiftedReferenceAware {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::LegacyShifted {
            direction_reversed, ..
        },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *direction_reversed = reversed;
}

pub(crate) fn set_extrude_start(scope: &mut DesignParameterScope, start: DesignExtrudeStart) {
    let Some(
        DesignExtrudePrologue::ReferenceAware { start: value, .. }
        | DesignExtrudePrologue::ShiftedReferenceAware { start: value, .. }
        | DesignExtrudePrologue::LegacyShifted { start: value, .. },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *value = start;
}
