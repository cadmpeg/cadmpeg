// SPDX-License-Identifier: Apache-2.0
//! `DeleteFace` and `SurfaceDeleteFace` scope projection.

use super::{face_scope_frame, unique_feature_match};
use crate::design::face_resolve::{resolved_face_group, resolved_historical_face_group};
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
    face::DesignFaceOperand,
};
use cadmpeg_core::CodecError;

/// The frame of a `DeleteFace` scope whose references are typed: 51 bytes per
/// reference-table entry, with the count at offset 163 or 167 and the typed
/// scope tail. `None` for any other frame.
fn typed_delete_face_frame(scope: &DesignParameterScope) -> Option<()> {
    if scope.kind_name() != "DeleteFace"
        || !matches!(
            scope
                .reference_count_offset()
                .checked_sub(scope.byte_offset())?,
            163 | 167
        )
    {
        return None;
    }
    let reference_bytes = u64::try_from(scope.reference_members().len())
        .ok()?
        .checked_mul(u64::try_from(crate::bytes::TYPED_LOCAL_REFERENCE_LEN).ok()?)?;
    let kind_offset = scope
        .reference_count_offset()
        .checked_add(12)?
        .checked_add(reference_bytes)?;
    let kind_bytes = u64::try_from(scope.source_kind_name().encode_utf16().count())
        .ok()?
        .checked_mul(2)?;
    (scope.kind_offset() == kind_offset
        && scope.paired_byte_offset()
            == kind_offset.checked_add(kind_bytes)?.checked_add(
                u64::try_from(crate::layout::typed_reference_parameter_scope_tail::LEN).ok()?,
            )?)
    .then_some(())
}

fn known_delete_face_frame(scope: &DesignParameterScope) -> bool {
    if typed_delete_face_frame(scope).is_some() {
        return true;
    }
    let Some((count_offset, tail_length)) = face_scope_frame(scope) else {
        return false;
    };
    let known_count = match scope.kind_name() {
        "DeleteFace" => matches!(count_offset, 123 | 127 | 131),
        "SurfaceDeleteFace" => matches!(count_offset, 123 | 127 | 128 | 131 | 134 | 138 | 142),
        _ => false,
    };
    known_count
        && u64::try_from(
            scope
                .label
                .as_ref()
                .map_or(0, |label| label.value.encode_utf16().count()),
        )
        .ok()
        .and_then(|units| units.checked_mul(2))
        .and_then(|bytes| tail_length.checked_sub(bytes))
        .is_some_and(|tail| matches!(tail, 76 | 77 | 78))
}

pub(super) fn project_delete_face(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

    let heal = match scope.kind_name() {
        "DeleteFace" => true,
        "SurfaceDeleteFace" => false,
        _ => return Ok(None),
    };
    if !known_delete_face_frame(scope) {
        losses.push(crate::loss::F3dLossCode::DeleteFaceFrameControlsAssumed.note(format!(
            "{} {} assumes its frame controls preserve removal of the selected faces with healing set to {heal}.", scope.kind_name(), scope.id
        )));
    }
    let faces = match selected_delete_faces(scope, construction_groups, face_operands, heal)? {
        Some(faces) => faces,
        None => {
            losses.push(crate::loss::F3dLossCode::DeleteFaceGroupSelectionAssumed.note(format!(
                "{} {} retains its ordered references as a face selection because their role-0x10 group definition is missing or inconsistent.", scope.kind_name(), scope.id
            )));
            FaceSelection::Native(scope.id.clone())
        }
    };
    if !heal
        && scope
            .reference_count_offset()
            .checked_sub(scope.byte_offset())
            == u64::try_from(crate::layout::surface_delete_face_wide_prologue::REFERENCE_COUNT).ok()
    {
        losses.push(crate::loss::F3dLossCode::SurfaceDeleteFaceWidePrologueAssumed.note(format!(
            "SurfaceDeleteFace {} treats the wider prologue as preserving unhealed deletion of its role-0x10 face group.", scope.id
        )));
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DeleteFace { faces, heal },
    )))
}

fn selected_delete_faces(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    heal: bool,
) -> Result<Option<cadmpeg_ir::features::FaceSelection>, CodecError> {
    use cadmpeg_ir::features::FaceSelection;
    let reference_count = scope.reference_members().len();
    if reference_count < 2 {
        return Ok(None);
    }
    let stream = or_none!(native_stream(&scope.id));
    let matching = construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    });
    let Some(group) = unique_feature_match(matching) else {
        return Ok(None);
    };
    if group.scope_reference_ordinal != 0
        || group.record_index != *or_none!(scope.reference_members().values().next())
        || group.role() != DesignOperandRole::ROLE_0X10
        || !group.members().iter().map(|member| member.value).eq(scope
            .reference_members()
            .values()
            .skip(1)
            .copied())
    {
        return Ok(None);
    }
    let faces = resolved_historical_face_group(
        scope,
        scope.previous_history_state_id(),
        group,
        face_operands,
    )
    .map_or_else(
        || {
            if heal {
                Ok(None)
            } else {
                resolved_face_group(group, face_operands)
            }
        },
        |face| Ok(Some(face)),
    )?;
    let faces = match faces {
        Some(faces) => faces,
        None => FaceSelection::Native(group.id.clone()),
    };
    Ok(Some(faces))
}
