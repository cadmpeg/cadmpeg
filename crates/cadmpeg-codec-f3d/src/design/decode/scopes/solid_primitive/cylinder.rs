use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_ir::transform::Transform;

use super::{
    extrude_operation_at, fixed_guid_end, marked_record_reference, TYPE_GUID_OWNER,
    TYPE_GUID_PLACEMENT, TYPE_GUID_SUPPORT_FACE,
};
use crate::bytes::{typed_local_reference, TYPED_LOCAL_REFERENCE_LEN};
use crate::layout::counted_cylinder_reference as counted;
use crate::layout::prefixed_cylinder_header as prefixed;
use crate::layout::shifted_cylinder_primitive_352_frame as compact;
use crate::layout::shifted_cylinder_primitive_502_frame as expanded;
use crate::layout::typed_cylinder_prefix as typed;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::primitives::DesignCylinderConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use crate::records::sketch_placement::SketchPlacementMatrix;

const TYPE_GUID_CYLINDER_RESULT: &str = "92112554-B316-46D1-8AFD-C49D91252558";
const TYPE_GUID_CYLINDER_TAIL: &str = "D26351F0-5940-4D23-AA20-2C35475A6D9E";
const TYPED_ROLES: [&str; 5] = [
    TYPE_GUID_SUPPORT_FACE,
    TYPE_GUID_PLACEMENT,
    TYPE_GUID_OWNER,
    TYPE_GUID_OWNER,
    TYPE_GUID_CYLINDER_RESULT,
];

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(super) struct ExtendedCylinderPrologue {
    pub(super) operation: DesignExtrudeOperation,
    pub(super) operation_offset: usize,
    pub(super) construction: DesignCylinderConstruction,
}

pub(super) fn read_extended_prologue(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<ExtendedCylinderPrologue> {
    let frame = View::over_retained(bytes)
        .child(start, usize::try_from(scope.paired_byte_offset()).ok()?)?;
    if scope.reference_count_offset() == u64_from_index(start.checked_add(typed::REFERENCE_COUNT)?)
    {
        return read_typed_prologue(frame, scope);
    }
    let prefix_word_width = prefixed::LEN - expanded::OPERATION;
    // `construction_shift` moves the counted construction reference, which
    // otherwise follows the matrix directly; `tail_shift` moves the GUID and
    // the reference run.
    let (compact_form, prefix_shift, construction_shift, tail_shift) =
        match (scope.frame_length(), scope.reference_members().len()) {
            (356, 5) => (
                true,
                prefix_word_width,
                prefix_word_width,
                prefix_word_width,
            ),
            (479, 5) | (501 | 502, 7) => (false, 0, 0, 0),
            (506, 7) => (
                false,
                prefix_word_width,
                prefix_word_width,
                prefix_word_width,
            ),
            (507, 7) => (false, 0, prefix_word_width, prefix_word_width),
            // The prefixed five-reference form keeps the construction reference
            // at its unprefixed offset, after four zero bytes, and stores eight
            // zero bytes before its GUID.
            (489, 5) => (false, prefix_word_width, 0, 2 * prefix_word_width),
            _ => return None,
        };
    let window = frame.window();
    if prefix_shift == 0 {
        shifted_header(window)?;
    } else if window.get(prefixed::ZERO_PREFIX..prefixed::PREFIX_WORD)? != [0; 9]
        || View::u32_le_at(window, prefixed::PREFIX_WORD)? != 1
        || window.get(prefixed::ZERO_FLAG) != Some(&0)
        || window.get(prefixed::FORM_MARKER) != Some(&1)
    {
        return None;
    }

    let operation = expanded::OPERATION + prefix_shift;
    let first = expanded::FIRST_REFERENCE + prefix_shift;
    let mut members = scope.reference_members().values().rev().copied();
    if window.get(expanded::REFERENCE_GAP + prefix_shift) != Some(&0)
        || window.get(first..first + 2)? != [1, 1]
        || View::u32_le_at(window, first + 2)? != members.next()?
        || window.get(first + 6..first + 11)? != [0; 5]
    {
        return None;
    }
    let mut placement_record_index = 0;
    for offset in [
        expanded::SECOND_REFERENCE,
        expanded::THIRD_REFERENCE,
        expanded::FOURTH_REFERENCE,
    ] {
        placement_record_index = marked_record_reference(window, offset + prefix_shift)?;
        if placement_record_index != members.next()? || placement_record_index == 0 {
            return None;
        }
    }

    let (count, guid, reference_count, local_frame) = if compact_form {
        if window.get(compact::COMPACT_TAIL_MARKER + prefix_shift) != Some(&1) {
            return None;
        }
        (
            compact::COMPACT_TAIL_COUNT + prefix_shift,
            compact::GUID_CODE_UNIT_COUNT + prefix_shift,
            compact::REFERENCE_COUNT + prefix_shift,
            None,
        )
    } else {
        let matrix = expanded::MATRIX + prefix_shift;
        let matrix_end = expanded::ZERO_RUN_8_AFTER_MATRIX + prefix_shift;
        if window.get(expanded::ZERO_BEFORE_MATRIX + prefix_shift) != Some(&0) {
            return None;
        }
        let count = if construction_shift == 0 && tail_shift == 0 {
            matrix_end
        } else {
            let count = expanded::CONSTRUCTION_REFERENCE + construction_shift;
            if !window.get(matrix_end..count)?.iter().all(|byte| *byte == 0) {
                return None;
            }
            count
        };
        (
            count,
            expanded::GUID_CODE_UNIT_COUNT + tail_shift,
            expanded::REFERENCE_COUNT + tail_shift,
            Some(read_rigid_frame(frame, matrix, matrix_end)?),
        )
    };
    if View::u32_le_at(window, count + counted::COUNT)? != 1
        || !window
            .get(count + counted::LEN..guid)?
            .iter()
            .all(|byte| *byte == 0)
        || u64_from_index(start + reference_count) != scope.reference_count_offset()
        || View::u32_le_at(window, reference_count)?
            != u32::try_from(scope.reference_members().len()).ok()?
        || window.get(fixed_guid_end(window, guid)?..reference_count)? != [0; 3]
    {
        return None;
    }
    let reference_offset = count + counted::REFERENCE;
    let reference = marked_record_reference(window, reference_offset)?;
    if reference == 0 {
        return None;
    }
    Some(ExtendedCylinderPrologue {
        operation: extrude_operation_at(window, operation)?,
        operation_offset: start + operation,
        construction: DesignCylinderConstruction {
            placement_record_index,
            reference: Located {
                value: reference,
                offset: u64_from_index(start + reference_offset),
            },
            local_frame,
        },
    })
}

fn shifted_header(window: &[u8]) -> Option<()> {
    (window.get(expanded::ZERO_RUN_10..expanded::FORM_MARKER)? == [0; 10]
        && window.get(expanded::FORM_MARKER) == Some(&1))
    .then_some(())
}

fn read_rigid_frame(
    frame: View<'_>,
    matrix: usize,
    matrix_end: usize,
) -> Option<Located<SketchPlacementMatrix>> {
    let offset = frame.start().checked_add(matrix)?;
    let mut matrix_view = frame.child(offset, frame.start().checked_add(matrix_end)?)?;
    let mut rows = [[0.0; 4]; 4];
    for value in rows.iter_mut().flatten() {
        *value = matrix_view.f64_le()?;
    }
    let [row_x, row_y, row_z, _] = rows;
    if !Transform::affine([row_x, row_y, row_z])?.is_proper_rigid() {
        return None;
    }
    Some(Located {
        value: rows.try_into().ok()?,
        offset: u64_from_index(offset),
    })
}

fn read_typed_prologue(
    frame: View<'_>,
    scope: &DesignParameterScope,
) -> Option<ExtendedCylinderPrologue> {
    let window = frame.window();
    shifted_header(window)?;
    let mut members = scope.reference_members().values().copied();
    if members.len() != TYPED_ROLES.len()
        || window.get(typed::NESTED_MARKER) != Some(&1)
        || window.get(typed::MATRIX_FLAG) != Some(&0)
        || View::u32_le_at(window, typed::ONE_COUNT)? != 1
        || window.get(typed::ZERO_RUN_8..typed::GUID_CODE_UNIT_COUNT)? != [0; 8]
        || fixed_guid_end(window, typed::GUID_CODE_UNIT_COUNT)? != typed::ZERO_RUN_3
        || window.get(typed::ZERO_RUN_3..typed::REFERENCE_COUNT)? != [0; 3]
        || View::u32_le_at(window, typed::REFERENCE_COUNT)?
            != u32::try_from(TYPED_ROLES.len()).ok()?
    {
        return None;
    }
    let roles = members.clone().zip(TYPED_ROLES);
    for (ordinal, pair) in roles.clone().enumerate() {
        if pair.0 == 0
            || typed_local_reference(window, typed::LEN + ordinal * TYPED_LOCAL_REFERENCE_LEN)
                != Some(pair)
        {
            return None;
        }
    }
    for (ordinal, pair) in roles.rev().take(4).enumerate() {
        if typed_local_reference(
            window,
            typed::TYPED_REFERENCES + ordinal * TYPED_LOCAL_REFERENCE_LEN,
        ) != Some(pair)
        {
            return None;
        }
    }
    let (reference, guid) = typed_local_reference(window, typed::TAIL_REFERENCE)?;
    if reference == 0 || guid != TYPE_GUID_CYLINDER_TAIL {
        return None;
    }
    Some(ExtendedCylinderPrologue {
        operation: extrude_operation_at(window, expanded::OPERATION)?,
        operation_offset: frame.start() + expanded::OPERATION,
        construction: DesignCylinderConstruction {
            placement_record_index: members.nth(1)?,
            reference: Located {
                value: reference,
                offset: u64_from_index(frame.start() + typed::TAIL_REFERENCE),
            },
            local_frame: Some(read_rigid_frame(frame, typed::MATRIX, typed::ONE_COUNT)?),
        },
    })
}
