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

pub(super) fn is_representation_name(name: &str) -> bool {
    name == "REPRESENTATION"
        || name.ends_with("_REPRESENTATION")
        || name == "SHAPE_REPRESENTATION_WITH_PARAMETERS"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{PartialRecord, RawRecord};

    #[test]
    fn shape_representation_with_parameters_uses_inherited_attributes() {
        let record = RawRecord {
            partials: crate::parse::RecordPartials::single(PartialRecord {
                name: "SHAPE_REPRESENTATION_WITH_PARAMETERS".into(),
                parameters: vec![
                    Value::String(b"datum target".to_vec()),
                    Value::List(vec![Value::Reference(2), Value::Reference(3)]),
                    Value::Reference(4),
                ],
            }),
            span: 0..1,
        };

        assert_eq!(
            parameters(&record),
            Some(
                [
                    Value::String(b"datum target".to_vec()),
                    Value::List(vec![Value::Reference(2), Value::Reference(3)]),
                    Value::Reference(4),
                ]
                .as_slice()
            )
        );
        assert_eq!(items(&record), Some(vec![2, 3]));
    }
}
