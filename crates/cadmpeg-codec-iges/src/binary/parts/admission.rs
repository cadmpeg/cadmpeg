// SPDX-License-Identifier: Apache-2.0
//! Field-role boundary for the source-owned Binary semantic handoff.

use super::super::{BinaryDirectory, BinaryParameter, BinaryValue};
use super::dependencies::Dependencies;
use crate::directory::DirectoryFieldSlot;

pub(super) fn allows_handoff(
    parameter: &BinaryParameter,
    owner: &BinaryDirectory,
    dependencies: &Dependencies,
) -> bool {
    match parameter.entity_type {
        110 | 306 => parameter
            .values
            .iter()
            .all(|value| !matches!(value, BinaryValue::Pointer(_))),
        116 if owner.has_point_parameters() => parameter
            .values
            .iter()
            .enumerate()
            .all(|(index, value)| index == 3 || !matches!(value, BinaryValue::Pointer(_))),
        402 if matches!(
            owner.values[DirectoryFieldSlot::Form.slot()],
            BinaryValue::Integer(1 | 7 | 14 | 15)
        ) =>
        {
            let Some(BinaryValue::Integer(count)) = parameter.values.first() else {
                return false;
            };
            let Some(end) = usize::try_from(*count)
                .ok()
                .and_then(|count| count.checked_add(1))
                .filter(|end| *end <= parameter.values.len())
            else {
                return false;
            };
            parameter
                .values
                .iter()
                .enumerate()
                .all(|(index, value)| match value {
                    BinaryValue::Pointer(pointer) if index > 0 && index < end => {
                        dependencies.member_resolves_or_is_dropped(*pointer)
                    }
                    BinaryValue::Pointer(_) => false,
                    _ => true,
                })
        }
        _ => false,
    }
}
