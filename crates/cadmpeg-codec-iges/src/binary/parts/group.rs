// SPDX-License-Identifier: Apache-2.0
//! Member slots of a Binary Group Parameter record whose target is given up.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;

use super::super::{
    integer_value, malformed, render_field, resolve_value, BinaryDirectory, BinaryParameter,
    BinaryValue, FieldRendering,
};
use super::dependencies::Dependencies;
use crate::directory::DirectoryFieldSlot;

pub(super) fn keep_failed_members(
    record: &mut BinaryParameter,
    owner: &BinaryDirectory,
    directory: &[BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
    dependencies: &Dependencies,
) -> Result<(), CodecError> {
    if record.entity_type != 402 {
        return Ok(());
    }
    let form = resolve_value(
        &owner.values[DirectoryFieldSlot::Form.slot()],
        directory,
        directory_by_offset,
    )?;
    let form = crate::directory::integer(render_field(&form, FieldRendering::Plain)?, "form")
        .map_err(|error| malformed(error.to_string()))?;
    if !matches!(form, 1 | 7 | 14 | 15) {
        return Ok(());
    }
    let Some(count) = record.values.first() else {
        return Ok(());
    };
    let count = resolve_value(count, directory, directory_by_offset)?;
    let Ok(count) = integer_value(&count, "Group member count").and_then(|count| {
        usize::try_from(count).map_err(|_| malformed("Binary Group member count is out of range"))
    }) else {
        return Ok(());
    };
    let Some(end) = count
        .checked_add(1)
        .filter(|end| *end <= record.values.len())
    else {
        return Ok(());
    };
    // The Group keeps its source count and one slot per source pointer. A
    // member whose Directory record is given up keeps the source sequence of
    // that record; only its target is given up (#222 Group count ruling,
    // rule 1).
    for value in record.values.get_mut(1..end).into_iter().flatten() {
        if let Some(sequence) = dependencies.failed_member_sequence(value) {
            *value = BinaryValue::Integer(i64::from(sequence));
        }
    }
    Ok(())
}
