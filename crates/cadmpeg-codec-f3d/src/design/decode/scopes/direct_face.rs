// SPDX-License-Identifier: Apache-2.0
//! Exact direct-face, move and scale operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::parameter_scope::parameter_scope_payload_length;
use super::point_data::exact_point_data_construction;
use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use super::thicken_shell::exact_shell_wall_slots;
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{take_reference, Reference};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::layout::modify_marker_one_prologue as modify_one;
use crate::layout::modify_marker_zero_prologue as modify_zero;
use crate::layout::move_face_marker_one_prologue as face_one;
use crate::layout::move_face_marker_zero_prologue as face_zero;
use crate::layout::move_marked_transform_prologue as marked_move;
use crate::layout::move_marker_one_transform_prologue as extended_move;
use crate::layout::move_transform_frame_253 as move_frame;
use crate::layout::move_transform_frame_254 as move_frame_254;
use crate::layout::move_transform_frame_273 as move_frame_273;
use crate::layout::move_transform_frame_274 as move_frame_274;
use crate::layout::move_transform_frame_283 as move_frame_283;
use crate::layout::move_transform_frame_284 as move_frame_284;
use crate::layout::move_transform_frame_293 as move_frame_293;
use crate::layout::move_transform_omitted_matrix_frame_125 as omitted_move_125;
use crate::layout::move_transform_omitted_matrix_frame_126 as omitted_move_126;
use crate::layout::rigid_transform_leading_prefix as leading_move;
use crate::layout::scale_legacy_uniform_tail as scale_tail;
use crate::layout::scale_matrix_marker_one_prologue as scale_matrix_one;
use crate::layout::scale_matrix_marker_zero_prologue as scale_matrix_zero;
use crate::records::entity_header::SegmentType;
use crate::records::feature::body_ops::{DesignScaleFactors, DesignScaleOperation};
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::direct_face::DesignModifyOperation;
use crate::records::feature::direct_face::DesignMoveOperation;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

const RIGID_TRANSFORM_TYPE_GUID: &str = "84E6245F-1CDC-4FB8-8048-163A85FAD3F0";
const RIGID_TRANSFORM_TYPE_VERSION: u32 = 2;

/// Primary-header offset of the marked distance reference of an `OffsetFaces`
/// scope, or `None` when its prologue is not a known form.
///
/// The 253- and 264-byte token-independent forms are identified by length.
/// The other forms are identified by their prologue and by the offset of the
/// ordered-reference count, because their scope tail can be 77 or 78 bytes.
/// A marker-one prologue stores `1` at offset 20 and a u32 zero at 21, so its
/// fields start four bytes later than those of a marker-zero prologue.
fn offset_faces_distance_marker(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<usize> {
    let references = scope.reference_members().len();
    if matches!(
        (parameter_scope_payload_length(scope), references),
        (Some(264), 4) | (Some(253), 3)
    ) && bytes.get(start + 25) == Some(&1)
    {
        return Some(25);
    }
    let count_offset = usize::try_from(scope.reference_count_offset())
        .ok()?
        .checked_sub(start)?;
    let distance_marker = match (bytes.get(start + 20)?, count_offset) {
        (1, 130) if View::u32_le_at(bytes, start + 21) == Some(0) => 25,
        (0, 126) => 21,
        _ => return None,
    };
    // A u32 one follows the marked distance reference, then the marked
    // reference to the selected-face group, the first ordered reference.
    let first_reference = scope.reference_members().values().next()?;
    (View::u32_le_at(bytes, start + distance_marker + 11) == Some(1)
        && marked_record_reference(bytes, start + distance_marker + 15) == Some(*first_reference))
    .then_some(distance_marker)
}

pub(crate) fn offset_faces_distance_reference_matches(
    scope: &DesignParameterScope,
    distance_record_index: u32,
) -> bool {
    scope.reference_members().len() >= 3
        && scope.reference_members().values().next_back() == Some(&distance_record_index)
}

pub(super) fn exact_direct_face_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    losses: &mut Vec<LossNote>,
) -> Option<DesignDirectFaceOperation> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    match design_feature_family(&scope.kind())? {
        DesignFeatureFamily::OffsetFaces => {
            let distance_marker = offset_faces_distance_marker(bytes, scope, start)?;
            let distance_record_index = marked_record_reference(bytes, start + distance_marker)?;
            if !offset_faces_distance_reference_matches(scope, distance_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, distance_record_index)?;
            Some(DesignDirectFaceOperation::OffsetFaces(
                direct_face::DesignOffsetFacesOperation {
                    distance: scalar.value,
                    distance_record_index,
                    distance_offset: scalar.value_offset,
                },
            ))
        }
        DesignFeatureFamily::Shell => exact_shell_wall_slots(bytes, records, scope, losses),
        _ => None,
    }
}

pub(super) fn exact_move_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    class_types: &HashMap<u32, &SegmentType>,
) -> Option<DesignMoveOperation> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Move) {
        return None;
    }
    marked_move_transform(bytes, records, scope).or_else(|| {
        unique_move_transform(
            bytes,
            records,
            scope.reference_members().values(),
            class_types,
        )
    })
}

pub(super) fn exact_align_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    envelope: &crate::records::feature::align::DesignAlignEnvelope,
    class_types: &HashMap<u32, &SegmentType>,
) -> Option<DesignMoveOperation> {
    let mut operation = move_face_transform(bytes, records, envelope.first_reference.value)?;
    let start = usize::try_from(operation.form_offset)
        .ok()?
        .checked_sub(operation.relative_form_offset()?)?;
    if operation.form != direct_face::DesignMoveForm::Form4
        || !rigid_transform_type_matches(bytes, start, class_types)
    {
        return None;
    }
    operation.transform_reference_offset = Some(envelope.first_reference.offset.checked_sub(1)?);
    Some(operation)
}

/// The transform record that a `Move` scope selects by the marked reference
/// in its prologue.
fn marked_move_transform(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignMoveOperation> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + indexed_header::LEN..start + extended_move::MARKER)? != [0; 9] {
        return None;
    }
    let marker = match bytes.get(start + extended_move::MARKER)? {
        0 => marked_move::TRANSFORM_REFERENCE,
        1 if View::u32_le_at(bytes, start + extended_move::ZERO_WORD) == Some(0) => {
            extended_move::TRANSFORM_REFERENCE
        }
        _ => return None,
    };
    let mut cursor = start.checked_add(marker)?;
    let (transform, _) = take_reference(bytes, &mut cursor)?.into_local()?;
    let transform_record_index = u32::try_from(transform).ok()?;
    if View::u32_le_at(bytes, cursor)? != 1 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let (group, _) = take_reference(bytes, &mut cursor)?.into_local()?;
    if u32::try_from(group).ok()? != *scope.reference_members().values().next()?
        || !scope
            .reference_members()
            .values()
            .any(|index| *index == transform_record_index)
    {
        return None;
    }
    let mut operation = move_face_transform(bytes, records, transform_record_index)?;
    operation.transform_reference_offset = Some(u64_from_index(start + marker));
    Some(operation)
}

fn move_face_operand_markers(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<(usize, usize)> {
    let count_offset = usize::try_from(scope.reference_count_offset())
        .ok()?
        .checked_sub(start)?;
    let bytes = bytes.get(start..)?;
    let (flag, transform, reference_flag, group) =
        match (*bytes.get(face_zero::MARKER)?, count_offset) {
            (face_one::MARKER_VALUE, face_one::REFERENCE_COUNT)
                if View::u32_le_at(bytes, face_one::ZERO_WORD) == Some(0) =>
            {
                (
                    face_one::TRANSFORM_FLAG,
                    face_one::TRANSFORM_REFERENCE,
                    face_one::REFERENCE_FLAG,
                    face_one::GROUP_REFERENCE,
                )
            }
            (face_zero::MARKER_VALUE, face_zero::REFERENCE_COUNT) => (
                face_zero::TRANSFORM_FLAG,
                face_zero::TRANSFORM_REFERENCE,
                face_zero::REFERENCE_FLAG,
                face_zero::GROUP_REFERENCE,
            ),
            _ => return None,
        };
    (bytes.get(flag) == Some(&1) && View::u32_le_at(bytes, reference_flag) == Some(1))
        .then_some((transform, group))
}

pub(super) fn exact_move_face_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    class_types: &HashMap<u32, &SegmentType>,
    losses: &mut Vec<LossNote>,
) -> Option<DesignMoveOperation> {
    if scope.kind() != scope::DesignFeatureKind::MoveFace {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let (transform_marker, group_marker) = move_face_operand_markers(bytes, scope, start)?;
    let transform_at = start.checked_add(transform_marker)?;
    let transform = marked_record_reference(bytes, transform_at)?;
    let mut typed = None;
    let mut unregistered = None;
    let mut typed_ambiguous = false;
    let mut unregistered_ambiguous = false;
    let mut conflicting_registration = None;
    for (frame_start, paired) in records.frames(transform) {
        let Some((tag, after_tag)) =
            lp_ascii_filtered_view(bytes, frame_start, 3..=3, u8::is_ascii_digit)
        else {
            continue;
        };
        if View::u32_le_at(bytes, after_tag) != Some(transform) {
            continue;
        }
        let Ok(tag) = tag.parse::<u32>() else {
            continue;
        };
        let registration = class_types.get(&tag);
        if let Some(row) = registration.filter(|row| {
            row.version != RIGID_TRANSFORM_TYPE_VERSION
                || !row
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(RIGID_TRANSFORM_TYPE_GUID)
        }) {
            conflicting_registration = Some((tag, *row));
            continue;
        }
        let Some(next) = move_transform_at(bytes, frame_start, paired, transform) else {
            continue;
        };
        if registration.is_some() {
            typed_ambiguous |= typed.replace(next).is_some();
        } else {
            unregistered_ambiguous |= unregistered.replace(next).is_some();
        }
    }
    let mut operation = if let Some(operation) = typed {
        if typed_ambiguous {
            return None;
        }
        operation
    } else {
        if unregistered_ambiguous {
            return None;
        }
        let Some(operation) = unregistered else {
            if let Some((tag, row)) = conflicting_registration {
                losses.push(crate::loss::F3dLossCode::FeatureMoveFaceTypeConflict.note(format!(
                    "MoveFace scope {} retains transform record {transform} natively after conflicting class registration {tag}: {} version {} (DR-401).",
                    scope.id, row.type_guid.as_str(), row.version
                )));
            }
            return None;
        };
        losses.push(crate::loss::F3dLossCode::FeatureMoveFaceTypeAssumed.note(format!(
            "MoveFace scope {} reads marked transform record {transform} by its frame with unavailable type registration (DR-401).",
            scope.id
        )));
        operation
    };
    operation.transform_reference_offset = Some(u64_from_index(transform_at));
    operation.face_group = start.checked_add(group_marker).and_then(|at| {
        Some(crate::records::identity::Located {
            value: marked_record_reference(bytes, at)?,
            offset: u64_from_index(at.checked_add(1)?),
        })
    });
    let prefix_fields = bytes
        .get(start.checked_add(face_zero::PREFIX_FIELDS)?..start.checked_add(face_zero::MARKER)?)?;
    if prefix_fields != [0; 9] {
        losses.push(crate::loss::F3dLossCode::FeatureMoveFacePrologueAssumed.note(format!(
            "MoveFace scope {} keeps its marked transform while prefix fields {:?} are assumed to leave motion unchanged (DR-401).",
            scope.id, prefix_fields
        )));
    }
    Some(operation)
}

fn move_face_transform(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<DesignMoveOperation> {
    let mut candidate = None;
    for (start, paired) in records.frames(record_index) {
        let (_, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if View::u32_le_at(bytes, after_tag) != Some(record_index) {
            continue;
        }
        let Some(next) = move_transform_at(bytes, start, paired, record_index) else {
            continue;
        };
        if candidate.replace(next).is_some() {
            return None;
        }
    }
    candidate
}

fn move_transform_at(
    bytes: &[u8],
    start: usize,
    paired: usize,
    record_index: u32,
) -> Option<DesignMoveOperation> {
    use direct_face::{DesignMoveLeading, DesignMovePrefix, DesignMoveReference};

    let frame = View::over_retained(bytes).child(start, paired)?;
    let bytes = frame.window();
    if bytes.get(indexed_header::LEN..leading_move::SCALAR) != Some(&[0; 10]) {
        return None;
    }
    let scalar =
        cadmpeg_ir::scalar::FiniteReal::new(View::f64_le_at(bytes, leading_move::SCALAR)?)?;
    let mut cursor = leading_move::LEN;
    let mut references = std::array::from_fn::<_, 6, _>(|_| DesignMoveReference::Null);
    let mut codes = [0; 2];
    for (ordinal, reference) in references.iter_mut().enumerate() {
        if ordinal == 5 {
            codes[0] = View::u32_le_at(bytes, cursor)?;
            cursor = cursor.checked_add(4)?;
        }
        *reference = match take_reference(bytes, &mut cursor)? {
            Reference::Null => DesignMoveReference::Null,
            Reference::Local {
                target,
                inline_type_guid,
            } => {
                let record_index = u32::try_from(target).ok()?;
                match inline_type_guid {
                    None => DesignMoveReference::Local { record_index },
                    Some(type_guid) => DesignMoveReference::Typed {
                        record_index,
                        type_guid: type_guid.to_owned(),
                    },
                }
            }
            _ => return None,
        };
    }
    codes[1] = View::u32_le_at(bytes, cursor)?;
    cursor = cursor.checked_add(4)?;
    let form_at = cursor;
    let form = direct_face::DesignMoveForm::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
    let [first, second, third, fourth, fifth, sixth] = references;
    let leading = (form == direct_face::DesignMoveForm::Form4
        || scalar.get() != 0.0
        || [&first, &second]
            .iter()
            .any(|reference| !matches!(reference, DesignMoveReference::Null)))
    .then_some(DesignMoveLeading {
        scalar,
        references: [first, second],
    });
    let mut prefix = DesignMovePrefix {
        references: [third, fourth, fifth, sixth],
        codes,
        matrix_omitted: false,
        leading,
    };
    if form == direct_face::DesignMoveForm::Form2 && !prefix.full_references() {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    prefix.matrix_omitted = match bytes.get(cursor)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let transform_at = cursor.checked_add(1)?;
    if prefix.matrix_omitted
        && (!matches!(
            form,
            direct_face::DesignMoveForm::Form4 | direct_face::DesignMoveForm::Form5
        ) || form == direct_face::DesignMoveForm::Form5
            && (prefix.codes != [0; 2]
                || prefix.leading.is_some()
                || prefix
                    .references
                    .iter()
                    .any(|reference| !matches!(reference, DesignMoveReference::Null)))
            || !matches!(
                frame
                    .window()
                    .len()
                    .checked_sub(form_at.checked_sub(move_frame::FORM)?)?,
                omitted_move_125::LEN | omitted_move_126::LEN
            ))
    {
        return None;
    }
    let mut view = frame;
    view.seek(start.checked_add(transform_at)?)?;
    let transform = if prefix.matrix_omitted {
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
    } else {
        let mut transform = [[0.0; 4]; 4];
        for row in &mut transform {
            for value in row {
                *value = view.f64_le()?;
            }
        }
        crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform).ok()?
    };
    let prefix = (prefix
        .references
        .iter()
        .any(|reference| !matches!(reference, DesignMoveReference::Null))
        || prefix.codes != [0; 2]
        || form == direct_face::DesignMoveForm::Form2
        || prefix.matrix_omitted
        || prefix.leading.is_some())
    .then_some(prefix);
    Some(DesignMoveOperation {
        transform,
        transform_offset: u64_from_index(start + transform_at),
        transform_record_index: record_index,
        form,
        form_offset: u64_from_index(start + form_at),
        transform_reference_offset: None,
        prefix,
        ..DesignMoveOperation::default()
    })
}

pub(super) fn move_transform_references_loss(
    scope: &DesignParameterScope,
    operation: &DesignMoveOperation,
) -> Option<LossNote> {
    (operation.form == direct_face::DesignMoveForm::Form2
        && operation.prefix.as_ref().is_none_or(direct_face::DesignMovePrefix::full_references)).then(|| {
        crate::loss::F3dLossCode::FeatureMoveTransformReferencesAssumed.note(format!(
            "{} scope {} takes the rigid matrix of its form-2 transform record {}; the four references of the record are assumed to leave that motion unchanged (DR-375).",
            scope.kind().as_str(),
            scope.id,
            operation.transform_record_index
        ))
    })
}

pub(super) fn move_transform_prefix_losses(
    scope: &DesignParameterScope,
    operation: &DesignMoveOperation,
) -> Vec<LossNote> {
    let prefix_assumed = operation.prefix.as_ref().is_some_and(|prefix| {
        prefix.leading.as_ref().is_some_and(|leading| {
            leading.scalar.get() != 0.0
                || leading
                    .references
                    .iter()
                    .any(|reference| !matches!(reference, direct_face::DesignMoveReference::Null))
        }) || prefix.codes != [0; 2]
            || prefix
                .references
                .iter()
                .any(|reference| !matches!(reference, direct_face::DesignMoveReference::Null))
                && (!prefix.full_references()
                    || operation.form != direct_face::DesignMoveForm::Form2)
    });
    [
        operation.prefix.as_ref().is_some_and(|prefix| prefix.matrix_omitted).then_some(crate::loss::F3dLossCode::FeatureMoveTransformIdentityAssumed),
        matches!(operation.form, direct_face::DesignMoveForm::Form4 | direct_face::DesignMoveForm::Form6).then_some(crate::loss::F3dLossCode::FeatureMoveTransformModeAssumed),
        prefix_assumed.then_some(crate::loss::F3dLossCode::FeatureMoveTransformPrefixAssumed),
    ].into_iter().flatten().map(|code| {
        if code == crate::loss::F3dLossCode::FeatureMoveTransformIdentityAssumed {
            return code.note(format!("{} scope {} takes identity motion from the omitted-matrix marker of transform record {} (DR-306).", scope.kind().as_str(), scope.id, operation.transform_record_index));
        }
        code.note(format!(
        "{} scope {} applies the rigid matrix of form {} in transform record {}; its prefix references and codes {:?} are assumed to leave that motion unchanged (DR-306).",
        scope.kind().as_str(), scope.id, u32::from(operation.form), operation.transform_record_index, operation.prefix
    )) }).collect()
}

/// The one admitted rigid-transform frame among `record_indices`.
fn unique_move_transform<'a>(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_indices: impl IntoIterator<Item = &'a u32>,
    class_types: &HashMap<u32, &SegmentType>,
) -> Option<DesignMoveOperation> {
    let mut candidate = None;
    for record_index in record_indices {
        for (start, paired) in records.frames(*record_index) {
            let Some((_, after_tag)) =
                lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)
            else {
                continue;
            };
            if View::u32_le_at(bytes, after_tag) != Some(*record_index) {
                continue;
            }
            let Some(next) = move_transform_at(bytes, start, paired, *record_index) else {
                continue;
            };
            if !rigid_transform_rule_breaks(bytes, start, paired, next.form, class_types).is_empty()
            {
                continue;
            }
            if candidate.replace(next).is_some() {
                return None;
            }
        }
    }
    candidate
}

fn rigid_transform_type_matches(
    bytes: &[u8],
    start: usize,
    class_types: &HashMap<u32, &SegmentType>,
) -> bool {
    lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)
        .and_then(|(class_tag, _)| class_tag.parse::<u32>().ok())
        .and_then(|class_tag| class_types.get(&class_tag))
        .is_some_and(|row| {
            row.version == RIGID_TRANSFORM_TYPE_VERSION
                && row
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(RIGID_TRANSFORM_TYPE_GUID)
        })
}

fn rigid_transform_rule_breaks(
    bytes: &[u8],
    start: usize,
    paired: usize,
    form: direct_face::DesignMoveForm,
    class_types: &HashMap<u32, &SegmentType>,
) -> Vec<String> {
    let mut breaks = Vec::new();
    if !rigid_transform_type_matches(bytes, start, class_types) {
        breaks.push(format!(
            "a class type other than {RIGID_TRANSFORM_TYPE_GUID} version {RIGID_TRANSFORM_TYPE_VERSION}"
        ));
    }
    let Some(frame_length) = paired.checked_sub(start) else {
        breaks.push("a paired header before its primary header".to_owned());
        return breaks;
    };
    let length_form = match frame_length {
        move_frame::LEN | move_frame_254::LEN => direct_face::DesignMoveForm::Form5,
        move_frame_273::LEN | move_frame_274::LEN => direct_face::DesignMoveForm::Form1,
        move_frame_283::LEN | move_frame_284::LEN => direct_face::DesignMoveForm::Form0,
        move_frame_293::LEN => direct_face::DesignMoveForm::Form2,
        _ => {
            breaks.push(format!("a {frame_length}-byte frame"));
            return breaks;
        }
    };
    if form != length_form {
        breaks.push(format!(
            "form word {} in a {frame_length}-byte frame",
            u32::from(form)
        ));
    }
    breaks
}

pub(super) fn marked_move_transform_loss(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    operation: &DesignMoveOperation,
    class_types: &HashMap<u32, &SegmentType>,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    operation.transform_reference_offset?;
    let start = usize::try_from(operation.form_offset)
        .ok()?
        .checked_sub(operation.relative_form_offset()?)?;
    let (_, paired) = records
        .frames(operation.transform_record_index)
        .find(|(at, _)| *at == start)?;
    let breaks = rigid_transform_rule_breaks(bytes, start, paired, operation.form, class_types);
    (!breaks.is_empty()).then(|| {
        crate::loss::F3dLossCode::FeatureMoveTransformAssumed.note(format!(
            "{} scope {} keeps the rigid matrix of its marked transform record {}, which has {} (DR-306).",
            scope.kind().as_str(),
            scope.id,
            operation.transform_record_index,
            breaks.join(" and ")
        ))
    })
}

pub(super) fn exact_modify_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignModifyOperation> {
    if scope.kind() != scope::DesignFeatureKind::Modify {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + modify_zero::ZERO_RUN_9..start + modify_zero::MARKER)? != [0; 9] {
        return None;
    }
    let (transform_at, count_at, groups_at) = match *bytes.get(start + modify_zero::MARKER)? {
        modify_zero::MARKER_VALUE => (
            modify_zero::TRANSFORM_REFERENCE,
            modify_zero::BODY_COUNT,
            modify_zero::FIRST_BODY_GROUP,
        ),
        modify_one::MARKER_VALUE if View::u32_le_at(bytes, start + modify_one::ZERO_WORD)? == 0 => {
            (
                modify_one::TRANSFORM_REFERENCE,
                modify_one::BODY_COUNT,
                modify_one::FIRST_BODY_GROUP,
            )
        }
        _ => return None,
    };
    let transform_record_index = marked_record_reference(bytes, start + transform_at)?;
    let references = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    let count = usize::try_from(View::u32_le_at(bytes, start + count_at)?).ok()?;
    if count == 0 || references.len() < 1 + 4 * count {
        return None;
    }
    let mut body_group_record_indices = Vec::with_capacity(count);
    for ordinal in 0..count {
        let group = marked_record_reference(bytes, start + groups_at + 11 * ordinal)?;
        if references.get(1 + 2 * count + 2 * ordinal) != Some(&group) {
            return None;
        }
        body_group_record_indices.push(group);
    }
    if references.first() != Some(&transform_record_index) {
        return None;
    }
    let mut motion = move_face_transform(bytes, records, transform_record_index)?;
    motion.transform_reference_offset = Some(u64_from_index(start + transform_at));
    Some(DesignModifyOperation {
        motion,
        body_group_record_indices,
        body_count_offset: u64_from_index(start + count_at),
    })
}

pub(crate) fn move_operation_links_hold(
    scope: &DesignParameterScope,
    header: Option<&crate::records::decal::DesignRecordHeader>,
) -> bool {
    scope.move_operation().is_none_or(|operation| {
        let face = scope.kind() == scope::DesignFeatureKind::MoveFace;
        let face_offsets = [
            (face_zero::REFERENCE_COUNT, face_zero::TRANSFORM_REFERENCE, face_zero::GROUP_REFERENCE),
            (face_one::REFERENCE_COUNT, face_one::TRANSFORM_REFERENCE, face_one::GROUP_REFERENCE),
        ].into_iter().find_map(|(count, transform, group)| {
            if scope.byte_offset().checked_add(u64_from_index(count)) != Some(scope.reference_count_offset()) {
                return None;
            }
            Some((
                scope.byte_offset().checked_add(u64_from_index(transform))?,
                scope.byte_offset().checked_add(u64_from_index(group))?.checked_add(1)?,
            ))
        });
        let transform_offset_valid = operation.transform_reference_offset.is_none_or(|offset| {
            if face {
                face_offsets.is_some_and(|(expected, _)| offset == expected)
            } else {
                [marked_move::TRANSFORM_REFERENCE, extended_move::TRANSFORM_REFERENCE]
                    .into_iter()
                    .any(|at| scope.byte_offset().checked_add(u64_from_index(at)) == Some(offset))
            }
        });
        let marked_face_link = face && operation.transform_reference_offset.is_some() && transform_offset_valid;
        header.is_some_and(|header| {
            transform_offset_valid
                && operation.face_group.as_ref().is_none_or(|group| {
                    face && face_offsets.is_some_and(|(_, expected)| group.offset == expected)
                })
                && (scope.reference_members().values().any(|index| *index == operation.transform_record_index)
                    || marked_face_link)
                && header.record_index == operation.transform_record_index
                && (operation.form != direct_face::DesignMoveForm::Form4
                    || operation.prefix.as_ref().is_some_and(|prefix| prefix.leading.is_some()))
                && operation.prefix.as_ref().is_none_or(|prefix| {
                    (operation.form != direct_face::DesignMoveForm::Form2
                        || prefix.full_references())
                        && (!prefix.matrix_omitted
                            || (operation.form == direct_face::DesignMoveForm::Form4
                                || operation.form == direct_face::DesignMoveForm::Form5
                                    && prefix.codes == [0; 2]
                                    && prefix.leading.is_none()
                                    && prefix.references.iter().all(|reference| {
                                        matches!(reference, direct_face::DesignMoveReference::Null)
                                    }))
                                && operation.transform
                                    == crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY)
                })
                && operation.relative_form_offset().is_some_and(|offset| {
                    header.byte_offset.checked_add(u64_from_index(offset))
                        == Some(operation.form_offset)
                        && operation.form_offset.checked_add(5) == Some(operation.transform_offset)
                })
        })
    })
}

const SCALE_MATRIX_CENTER_TOLERANCE: f64 = 1e-9;

fn group_member_indices(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<Vec<u32>> {
    let (start, paired) = records.frames(record_index).next()?;
    let count = usize::try_from(View::u32_le_at(bytes, start + 21)?).ok()?;
    if count == 0 || count > paired.checked_sub(start.checked_add(25)?)? / 11 {
        return None;
    }
    (0..count)
        .map(|ordinal| marked_record_reference(bytes, start + 25 + 11 * ordinal))
        .collect()
}

fn long_modern_scale(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignScaleOperation> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Scale) {
        return None;
    }
    let references = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    let [factor_record_index, body_group_record_index, ..] = references.as_slice() else {
        return None;
    };
    let center_record_index = *references.last()?;
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if references.len() <= 5
        || View::u32_le_at(bytes, start + 20)? != 1
        || bytes.get(start + 24) != Some(&0)
        || marked_record_reference(bytes, start + 33)? != center_record_index
        || marked_record_reference(bytes, start + 44)? != *factor_record_index
        || View::u32_le_at(bytes, start + 55)? != 1
        || bytes.get(start + 59) != Some(&0)
        || View::u32_le_at(bytes, start + 60)? != 1
        || View::u32_le_at(bytes, start + 64)? != 1
        || marked_record_reference(bytes, start + 68)? != *body_group_record_index
    {
        return None;
    }
    let members = group_member_indices(bytes, records, *body_group_record_index)?;
    if references.len() != members.len() + 4
        || references.get(2..2 + members.len())? != members.as_slice()
    {
        return None;
    }
    let center = exact_point_data_construction(
        bytes,
        records,
        std::slice::from_ref(&center_record_index),
        stream_types,
    )
    .map(|point| crate::records::identity::Located {
        value: point.position,
        offset: point.position_offset,
    });
    Some(DesignScaleOperation {
        body_group_record_index: *body_group_record_index,
        further_body_group_record_indices: Vec::new(),
        center_record_index,
        center_position: center,
        factors: DesignScaleFactors::Uniform {
            factor: cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, start + 25)?)?,
            offset: u64_from_index(start + 25),
        },
        matrix_offset: None,
    })
}

fn matrix_scale(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<(DesignScaleOperation, u32)> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Scale) {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + scale_matrix_zero::ZERO_RUN_9..start + scale_matrix_zero::MARKER)?
        != [0; 9]
    {
        return None;
    }
    let (word_at, count_at, groups_at) = match *bytes.get(start + scale_matrix_zero::MARKER)? {
        scale_matrix_zero::MARKER_VALUE => (
            scale_matrix_zero::WORD,
            scale_matrix_zero::BODY_COUNT,
            scale_matrix_zero::FIRST_BODY_GROUP,
        ),
        scale_matrix_one::MARKER_VALUE
            if View::u32_le_at(bytes, start + scale_matrix_one::ZERO_WORD)? == 0 =>
        {
            (
                scale_matrix_one::WORD,
                scale_matrix_one::BODY_COUNT,
                scale_matrix_one::FIRST_BODY_GROUP,
            )
        }
        _ => return None,
    };
    let word = View::u32_le_at(bytes, start + word_at)?;
    let references = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    let count = usize::try_from(View::u32_le_at(bytes, start + count_at)?).ok()?;
    if count == 0 || count > references.len() {
        return None;
    }
    let mut at = start + groups_at;
    let mut groups = Vec::with_capacity(count);
    for _ in 0..count {
        groups.push(marked_record_reference(bytes, at)?);
        at += 11;
    }
    let center_record_index = marked_record_reference(bytes, at)?;
    at += 11;
    let mut slots = [None; 4];
    for slot in &mut slots {
        match *bytes.get(at)? {
            0 => at += 1,
            1 => {
                *slot = Some(marked_record_reference(bytes, at)?);
                at += 11;
            }
            _ => return None,
        }
    }
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let named = groups
        .iter()
        .chain([&center_record_index])
        .chain(slots.iter().flatten());
    if named.into_iter().any(|index| !references.contains(index)) {
        return None;
    }
    if u64_from_index(at.checked_add(204)?) > scope.paired_byte_offset() {
        return None;
    }
    let mut view = View::over_retained(bytes.get(at..)?);
    let mut matrix = [0.0; 16];
    for value in &mut matrix {
        *value = view.f64_le()?;
    }
    let (guid, _) = crate::bytes::lp_utf16_bounded_view(bytes, at + 128, 36..=36)?;
    if !guid.eq_str("00000000-0000-0000-0000-000000000000")
        || matrix.iter().any(|value| !value.is_finite())
        || matrix[12..] != [0.0, 0.0, 0.0, 1.0]
        || [1, 2, 4, 6, 8, 9]
            .into_iter()
            .any(|cell| matrix[cell] != 0.0)
    {
        return None;
    }
    let diagonal = [matrix[0], matrix[5], matrix[10]];
    let translation = [matrix[3], matrix[7], matrix[11]];
    let positive = diagonal.map(cadmpeg_ir::scalar::PositiveReal::new);
    let [Some(x), Some(y), Some(z)] = positive else {
        return None;
    };
    let factors = match slots {
        [Some(_), None, None, None] if diagonal[0] == diagonal[1] && diagonal[1] == diagonal[2] => {
            DesignScaleFactors::Uniform {
                factor: x,
                offset: u64_from_index(at),
            }
        }
        [None, Some(_), Some(_), Some(_)] => DesignScaleFactors::PerAxis {
            factors: [x, y, z],
            offsets: [
                u64_from_index(at),
                u64_from_index(at + 40),
                u64_from_index(at + 80),
            ],
        },
        _ => return None,
    };
    let center = exact_point_data_construction(
        bytes,
        records,
        std::slice::from_ref(&center_record_index),
        stream_types,
    );
    if let Some(point) = &center {
        let holds = (0..3).all(|axis| {
            let expected = (1.0 - diagonal[axis]) * point.position[axis].get();
            expected.is_finite()
                && (translation[axis] - expected).abs()
                    <= SCALE_MATRIX_CENTER_TOLERANCE
                        * translation[axis].abs().max(expected.abs()).max(1.0)
        });
        if !holds {
            return None;
        }
    }
    Some((
        DesignScaleOperation {
            body_group_record_index: groups[0],
            further_body_group_record_indices: groups[1..].to_vec(),
            center_record_index,
            center_position: center.map(|point| crate::records::identity::Located {
                value: point.position,
                offset: point.position_offset,
            }),
            factors,
            matrix_offset: Some(u64_from_index(at)),
        },
        word,
    ))
}

pub(super) fn exact_scale_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<DesignScaleOperation>, cadmpeg_core::CodecError> {
    let reference_count = scope.reference_members().len();
    let payload_form = matches!(
        (parameter_scope_payload_length(scope), reference_count),
        (Some(297 | 299), 5) | (Some(308), 6)
    );
    let english_frame_form = matches!(
        (scope.frame_length(), reference_count),
        (307 | 309, 5) | (318, 6)
    );
    let mut read_by_english_frame = false;
    let operation = (|| -> Option<Result<DesignScaleOperation, cadmpeg_core::CodecError>> {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Scale) {
            return None;
        }
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let (body_group_record_index, center_record_index, uniform_factor_offset, center) =
            if parameter_scope_payload_length(scope) == Some(303)
                && scope.reference_members().len() == 5
            {
                let [factor_record_index, body_group_record_index, _, _, center_record_index] =
                    scope.reference_members().values_array()?;
                if View::u32_le_at(bytes, start + 20)? != 1
                    || bytes.get(start + 24) != Some(&0)
                    || marked_record_reference(bytes, start + 33)? != *center_record_index
                    || marked_record_reference(bytes, start + 44)? != *factor_record_index
                    || View::u32_le_at(bytes, start + 55)? != 1
                    || bytes.get(start + 59) != Some(&0)
                    || View::u32_le_at(bytes, start + 60)? != 1
                    || View::u32_le_at(bytes, start + 64)? != 1
                    || marked_record_reference(bytes, start + 68)? != *body_group_record_index
                {
                    return None;
                }
                let center = exact_point_data_construction(
                    bytes,
                    records,
                    std::slice::from_ref(center_record_index),
                    stream_types,
                )
                .map(|point| (point.position, point.position_offset));
                (
                    *body_group_record_index,
                    *center_record_index,
                    start + 25,
                    center,
                )
            } else if scope.kind() == scope::DesignFeatureKind::Scale
                && (payload_form || english_frame_form)
            {
                read_by_english_frame = !payload_form;
                if read_by_english_frame && scope.frame_length() == 309 {
                    let (guid, _) = crate::bytes::lp_utf16_bounded_view(
                        bytes,
                        start.checked_add(75)?,
                        36..=36,
                    )?;
                    if !guid.eq_str("00000000-0000-0000-0000-000000000000")
                        || View::u24_le_at(bytes, start.checked_add(151)?)? != 0
                        || scope.reference_count_offset() != scope.byte_offset().checked_add(154)?
                        || scope.kind_offset() != scope.byte_offset().checked_add(221)?
                        || scope.feature_ordinal_offset() != scope.byte_offset().checked_add(231)?
                        || scope.previous_history_state_id_offset()
                            != scope.byte_offset().checked_add(262)
                    {
                        return None;
                    }
                }
                if parameter_scope_payload_length(scope) == Some(299) {
                    let (guid, _) = crate::bytes::lp_utf16_bounded_view(
                        bytes,
                        start.checked_add(75)?,
                        36..=36,
                    )?;
                    if !guid.eq_str("00000000-0000-0000-0000-000000000000")
                        || View::u24_le_at(bytes, start.checked_add(151)?)? != 0
                        || scope.reference_count_offset() != scope.byte_offset().checked_add(154)?
                        || scope.kind_offset() != scope.byte_offset().checked_add(221)?
                        || scope.feature_ordinal_offset()
                            != scope
                                .paired_byte_offset()
                                .checked_sub(u64::try_from(scale_tail::LEN).ok()?)?
                        || scope.previous_history_state_id_offset()
                            != scope.feature_ordinal_offset().checked_add(
                                u64::try_from(scale_tail::PREVIOUS_HISTORY_STATE_ID).ok()?,
                            )
                    {
                        return None;
                    }
                }
                let mut references = scope.reference_members().values();
                let factor_record_index = references.next()?;
                let body_group_record_index = references.next()?;
                let center_record_index = references.next_back()?;
                if bytes.get(start + 16..start + 21)? != [0; 5]
                    || marked_record_reference(bytes, start + 29)? != *center_record_index
                    || marked_record_reference(bytes, start + 40)? != *factor_record_index
                    || View::u32_le_at(bytes, start + 51)? != 1
                    || bytes.get(start + 55) != Some(&0)
                    || View::u32_le_at(bytes, start + 56)? != 1
                    || View::u32_le_at(bytes, start + 60)? != 1
                    || marked_record_reference(bytes, start + 64)? != *body_group_record_index
                {
                    return None;
                }
                let point = exact_point_data_construction(
                    bytes,
                    records,
                    std::slice::from_ref(center_record_index),
                    stream_types,
                )?;
                (
                    *body_group_record_index,
                    *center_record_index,
                    start + 21,
                    Some((point.position, point.position_offset)),
                )
            } else {
                return None;
            };
        let uniform_factor =
            cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, uniform_factor_offset)?)?;
        Some(Ok(DesignScaleOperation {
            body_group_record_index,
            further_body_group_record_indices: Vec::new(),
            center_record_index,
            center_position: center
                .map(|(value, offset)| crate::records::identity::Located { value, offset }),
            factors: DesignScaleFactors::Uniform {
                factor: uniform_factor,
                offset: u64_from_index(uniform_factor_offset),
            },
            matrix_offset: None,
        }))
    })()
    .transpose()?;
    let operation = match operation {
        Some(operation) => Some(operation),
        None => long_modern_scale(bytes, records, scope, stream_types).or_else(|| {
            let (operation, word) = matrix_scale(bytes, records, scope, stream_types)?;
            if word != 0 {
                losses.push(crate::loss::F3dLossCode::FeatureScaleMatrixWordAssumed.note(
                    format!(
                        "{} stores word {word:#010x} in its matrix Scale prologue; the Scale assumes that the word leaves the stored matrix unchanged (DR-465).",
                        scope.id
                    ),
                ));
            }
            Some(operation)
        }),
    };
    if operation.is_some() && read_by_english_frame {
        losses.push(
            crate::loss::F3dLossCode::FeatureScaleFrameLengthFormAssumed.note(format!(
                "{} reads the legacy Scale form of its {}-byte frame; its {}-byte payload matches no form.",
                scope.id,
                scope.frame_length(),
                parameter_scope_payload_length(scope).unwrap_or_default()
            )),
        );
    }
    Ok(operation)
}

#[cfg(test)]
mod tests;
