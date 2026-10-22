// SPDX-License-Identifier: Apache-2.0
//! Recovery of list entries owned by a Binary Group Parameter record.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;

use super::super::{
    integer_value, malformed, render_field, resolve_value, BinaryDirectory, BinaryParameter,
    BinaryValue, FieldRendering,
};
use super::dependencies::Dependencies;
use crate::directory::DirectoryFieldSlot;

pub(super) fn prune_failed_members(
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
    let mut index = 0;
    let mut remaining = count;
    record.values.retain(|value| {
        let member = index > 0 && index < end;
        index += 1;
        if member && dependencies.failed_member(value) {
            remaining -= 1;
            false
        } else {
            true
        }
    });
    if remaining != count {
        record.values[0] = BinaryValue::Integer(
            i64::try_from(remaining)
                .map_err(|_| malformed("Binary Group member count exceeds signed range"))?,
        );
    }
    Ok(())
}
