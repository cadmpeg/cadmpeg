use super::assembly_operand_paths::{exact_position_operand_path, StreamTypes};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::{
    captured_position_matrix as matrix, captured_position_prefix as prefix,
    captured_position_row as row, captured_position_suffix as suffix,
};
use crate::records::feature::position::{DesignCapturedPosition, DesignCapturedPositions};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

pub(crate) const TYPE_GUID: &str = "8EE00B00-76BB-49AB-8C25-E837FEC5BDA5";

pub(super) fn registered(scope: &DesignParameterScope, types: &StreamTypes<'_>) -> bool {
    types
        .get(&u64::from(scope.record_index))
        .is_some_and(|(guid, version)| *version == 6 && guid.eq_ignore_ascii_case(TYPE_GUID))
}

pub(in crate::design) fn decode(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &StreamTypes<'_>,
) -> Result<Option<DesignCapturedPositions>, CodecError> {
    if !registered(scope, types) {
        return Ok(None);
    }
    let Some((mut view, count)) = frame(bytes, scope) else {
        return Ok(None);
    };
    let empty = DesignCapturedPosition {
        target: Located {
            value: 0,
            offset: 0,
        },
        matrix: None,
        path_index: None,
    };
    let retained_per_row = std::mem::size_of::<DesignCapturedPosition>()
        + 4 * std::mem::size_of::<(u32, Option<usize>)>();
    let retained = u64_from_index(count)
        .checked_mul(u64_from_index(retained_per_row))
        .ok_or_else(|| CodecError::malformed("F3D captured-position storage size overflows"))?;
    ctx.retain_bytes(retained, "retain F3D captured positions")?;
    let mut rows = ctx.alloc_filled(count, empty, "decode F3D captured positions")?;
    let mut paths = Vec::new();
    let mut by_target = std::collections::BTreeMap::new();
    for position in &mut rows {
        let Some(parsed) = read_row(&mut view) else {
            return Ok(None);
        };
        *position = parsed;
        position.path_index = match by_target.entry(position.target.value) {
            std::collections::btree_map::Entry::Occupied(entry) => *entry.get(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let path = exact_position_operand_path(
                    ctx,
                    bytes,
                    records,
                    scope,
                    types,
                    &position.target,
                )?;
                let index = path.map(|path| {
                    let index = paths.len();
                    paths.push(path);
                    index
                });
                entry.insert(index);
                index
            }
        };
    }
    let positions = DesignCapturedPositions { rows, paths };
    Ok((view.is_empty() && positions.valid_for_scope(scope)).then_some(positions))
}

fn frame<'a>(bytes: &'a [u8], scope: &DesignParameterScope) -> Option<(View<'a>, usize)> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.reference_count_offset()).ok()?;
    let rows_end = end.checked_sub(suffix::LEN)?;
    let mut head = View::over_retained(bytes).child(start, rows_end)?;
    head.seek(start.checked_add(prefix::ZERO_RUN)?)?;
    if head.take(prefix::ZERO_RUN_VALUE.len())? != prefix::ZERO_RUN_VALUE {
        return None;
    }
    let count = head.u32_le()?;
    let rows = head.child(head.position(), rows_end)?;
    let count = rows.counted(u64::from(count), row::LEN)?.get();
    let mut tail = View::over_retained(bytes).child(rows_end, end)?;
    if tail.u32_le()? != suffix::ZERO_WORD_VALUE || tail.u32_le()? != suffix::IDENTITY_LENGTH_VALUE
    {
        return None;
    }
    for expected in "00000000-0000-0000-0000-000000000000".encode_utf16() {
        if tail.u16_le()? != expected {
            return None;
        }
    }
    if tail.take(suffix::ZERO_TAIL_VALUE.len())? != suffix::ZERO_TAIL_VALUE || !tail.is_empty() {
        return None;
    }
    Some((rows, count))
}

fn read_row(view: &mut View<'_>) -> Option<DesignCapturedPosition> {
    if view.u8()? != row::ZERO_VALUE || view.u8()? != row::REFERENCE_MARKER_VALUE {
        return None;
    }
    let target = Located {
        offset: u64_from_index(view.position()),
        value: u32::try_from(view.u64_le()?).ok()?,
    };
    if view.u16_le()? != row::REFERENCE_PADDING_VALUE {
        return None;
    }
    let matrix = match view.u8()? {
        0 => {
            let offset = u64_from_index(view.position());
            let mut values = [FiniteReal::ZERO; 16];
            let mut coefficients =
                view.child(view.position(), view.position().checked_add(matrix::LEN)?)?;
            for value in &mut values {
                *value = FiniteReal::new(coefficients.f64_le()?)?;
            }
            view.seek(coefficients.position())?;
            Some(Located {
                value: values,
                offset,
            })
        }
        1 => None,
        _ => return None,
    };
    Some(DesignCapturedPosition {
        target,
        matrix,
        path_index: None,
    })
}

#[cfg(test)]
mod tests;
