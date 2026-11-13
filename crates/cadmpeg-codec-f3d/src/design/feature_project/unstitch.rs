// SPDX-License-Identifier: Apache-2.0
//! `UnStitch` scope projection.

use super::{face_scope_frame, unique_feature_match};
use crate::design::face_resolve::resolved_historical_face_group;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
    face::DesignFaceOperand,
};
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

const IDENTITY_COUNT_OFFSETS: [usize; 3] = [
    crate::layout::unstitch_prologue_compact::IDENTITY_COUNT,
    crate::layout::unstitch_prologue_marked::IDENTITY_COUNT,
    crate::layout::unstitch_prologue_extended::IDENTITY_COUNT,
];
const RUN_COUNTS_AND_NULL_GUID_BYTES: u64 = 91;
const MARKED_REFERENCE_BYTES: u64 = 11;

fn known_unstitch_frame(scope: &DesignParameterScope) -> bool {
    let Some((count_offset, tail_length)) = face_scope_frame(scope) else {
        return false;
    };
    let Ok(references) = u64::try_from(scope.reference_members().len()) else {
        return false;
    };
    let label_bytes = scope
        .label
        .as_ref()
        .map_or(0, |label| label.value.encode_utf16().count());
    let fixed_tail = u64::try_from(label_bytes)
        .ok()
        .and_then(|units| units.checked_mul(2))
        .and_then(|bytes| tail_length.checked_sub(bytes));
    fixed_tail.is_some_and(|tail| matches!(tail, 76..=78))
        && IDENTITY_COUNT_OFFSETS.iter().any(|start| {
            references
                .checked_mul(MARKED_REFERENCE_BYTES)
                .and_then(|bytes| bytes.checked_add(RUN_COUNTS_AND_NULL_GUID_BYTES))
                .and_then(|bytes| bytes.checked_add(u64::try_from(*start).ok()?))
                .and_then(|minimum| count_offset.checked_sub(minimum))
                .is_some_and(|extra| extra % MARKED_REFERENCE_BYTES == 0)
        })
}

pub(super) fn project_unstitch(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    losses: &mut Vec<LossNote>,
) -> FeatureDefinition {
    if !known_unstitch_frame(scope) {
        losses.push(F3dLossCode::UnstitchFrameAssumed.note(format!(
            "UnStitch {} assumes its frame preserves separation of the selected faces.",
            scope.id
        )));
    }
    let stream = native_stream(&scope.id);
    let group = unique_feature_match(construction_groups.iter().filter(|group| {
        stream.is_some()
            && native_stream(&group.id) == stream
            && group.scope_record_index == scope.record_index
    }))
    .filter(|group| {
        group.scope_reference_ordinal == 0
            && scope.reference_members().values().next() == Some(&group.record_index)
            && group.role() == DesignOperandRole::ROLE_0X10
            && !group.members().is_empty()
            && group.members().iter().map(|member| member.value).eq(scope
                .reference_members()
                .values()
                .skip(1)
                .copied())
    });
    let faces = match group {
        Some(group) => resolved_historical_face_group(
            scope,
            scope.previous_history_state_id(),
            group,
            face_operands,
        )
        .unwrap_or_else(|| FaceSelection::Native(group.id.clone())),
        None => {
            losses.push(F3dLossCode::UnstitchGroupSelectionAssumed.note(format!(
                "UnStitch {} retains its ordered references as the face selection because their role-0x10 group definition is missing or inconsistent.",
                scope.id
            )));
            FaceSelection::Native(scope.id.clone())
        }
    };
    losses.push(F3dLossCode::UnstitchPrologueControlsAssumed.note(format!(
        "UnStitch {} assumes its prologue flags and identity runs separate exactly the faces of its role-0x10 group.",
        scope.id
    )));
    FeatureDefinition::Operation(FeatureOperation::UnstitchFaces { faces })
}
