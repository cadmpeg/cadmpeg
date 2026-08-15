// SPDX-License-Identifier: Apache-2.0
//! Shared access to inherited `REPRESENTATION` attributes.

use crate::parse::{RawRecord, Value};

pub(super) fn parameters(record: &RawRecord) -> Option<&[Value]> {
    record.partials.iter().find_map(|partial| {
        (is_representation_name(&partial.name) && !partial.parameters.is_empty())
            .then_some(partial.parameters.as_slice())
    })
}

pub(super) fn items(record: &RawRecord) -> Option<Vec<u64>> {
    record.partials.iter().find_map(|partial| {
        if !is_representation_name(&partial.name) {
            return None;
        }
        partial
            .parameters
            .get(1)
            .and_then(value_list)
            .map(|items| items.iter().filter_map(value_reference).collect::<Vec<_>>())
    })
}

pub(super) fn context(record: &RawRecord) -> Option<u64> {
    record.partials.iter().find_map(|partial| {
        if !is_representation_name(&partial.name) {
            return None;
        }
        partial.parameters.get(2).and_then(value_reference)
    })
}

fn is_representation_name(name: &str) -> bool {
    name == "REPRESENTATION"
        || name.ends_with("_REPRESENTATION")
        || name == "TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS"
}

fn value_list(value: &Value) -> Option<&[Value]> {
    match value {
        Value::List(values) => Some(values),
        _ => None,
    }
}

fn value_reference(value: &Value) -> Option<u64> {
    match value {
        Value::Reference(id) => Some(*id),
        _ => None,
    }
}
