// SPDX-License-Identifier: Apache-2.0
//! Shared access to inherited `REPRESENTATION` attributes.

use super::ValueExt;
use crate::parse::{RawRecord, Value};

pub(super) fn parameters<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(
            &record.partials[..],
            "STEP representation parameter partial traversal",
        )?
        .find_map(|partial| {
            (is_representation_name(&partial.name) && !partial.parameters.is_empty())
                .then_some(partial.parameters.as_slice())
        }))
}

pub(super) fn items<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<impl DoubleEndedIterator<Item = u64> + 'a>, cadmpeg_core::CodecError> {
    let Some(items) = item_values(ctx, record)? else {
        return Ok(None);
    };
    Ok(Some(
        ctx.admit_iter(items, "STEP representation item traversal")?
            .filter_map(ValueExt::reference),
    ))
}

pub(super) fn item_values<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(
            &record.partials[..],
            "STEP representation item partial traversal",
        )?
        .find_map(|partial| {
            if !is_representation_name(&partial.name) {
                return None;
            }
            partial.parameters.get(1).and_then(ValueExt::list)
        }))
}

pub(super) fn is_representation_name(name: &str) -> bool {
    name == "REPRESENTATION"
        || name.ends_with("_REPRESENTATION")
        || name == "SHAPE_REPRESENTATION_WITH_PARAMETERS"
        || name == "TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS"
}

#[cfg(test)]
mod tests {
    use super::{items, parameters};
    use crate::parse::{PartialRecord, RawRecord, Value};

    #[test]
    fn shape_representation_with_parameters_uses_inherited_attributes() {
        let record = RawRecord {
            partials: crate::parse::partials::RecordPartials::single(PartialRecord {
                name: "SHAPE_REPRESENTATION_WITH_PARAMETERS".into(),
                parameters: vec![
                    Value::String(b"datum target".to_vec()),
                    Value::List(vec![Value::Reference(2), Value::Reference(3)]),
                    Value::Reference(4),
                ],
            }),
            span: 0..1,
        };

        crate::test_support::with_service_context(&[], |_, ctx| {
            assert_eq!(
                parameters(ctx, &record).expect("representation parameters"),
                Some(
                    [
                        Value::String(b"datum target".to_vec()),
                        Value::List(vec![Value::Reference(2), Value::Reference(3)]),
                        Value::Reference(4),
                    ]
                    .as_slice()
                )
            );
            assert_eq!(
                items(ctx, &record)
                    .expect("representation items")
                    .map(Iterator::collect::<Vec<_>>),
                Some(vec![2, 3])
            );
        });
    }
}
