// SPDX-License-Identifier: Apache-2.0
//! The seven current numeric fields of a registered mesh material property.

use super::{
    encoding::{Encoded, Omission},
    *,
};
use cadmpeg_ir::attributes::AttributeNumericList;

pub(super) fn matches(values: &[AttributeValue]) -> bool {
    matches!(
        values,
        [
            AttributeValue::Integer(_),
            AttributeValue::NumericList(AttributeNumericList::Color(_)),
            AttributeValue::NumericList(AttributeNumericList::Color(_)),
            AttributeValue::NumericList(AttributeNumericList::Color(_)),
            AttributeValue::NumericList(AttributeNumericList::Color(_)),
            AttributeValue::NumericList(AttributeNumericList::Float(_)),
            AttributeValue::NumericList(AttributeNumericList::Float(_))
        ]
    )
}

pub(super) fn encode(
    values: &[AttributeValue],
    identity: &str,
    losses: &mut Vec<LossNote>,
) -> Result<Result<Encoded, Omission>, CodecError> {
    let Some(AttributeValue::Integer(binding)) = values.first() else {
        return Err(CodecError::malformed("mesh material tuple has no binding"));
    };
    let binding = match u32::try_from(*binding).ok().filter(|binding| *binding <= 2) {
        Some(binding) => binding,
        None => {
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "mesh material attribute {identity} binding is outside the registered 0..=2 domain"
            )));
            return Ok(Err(Omission::OutOfRange));
        }
    };
    let mut bytes = binding.to_le_bytes().to_vec();
    let mut rounded = false;
    for value in &values[1..] {
        match value {
            AttributeValue::NumericList(AttributeNumericList::Color(Some(values)))
                if values.iter().all(Option::is_some) =>
            {
                bytes.extend_from_slice(
                    &u32::try_from(values.len())
                        .map_err(CodecError::malformed)?
                        .to_le_bytes(),
                );
                for value in values.iter().flatten() {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            AttributeValue::NumericList(AttributeNumericList::Float(Some(values)))
                if values.iter().all(Option::is_some) =>
            {
                bytes.extend_from_slice(
                    &u32::try_from(values.len())
                        .map_err(CodecError::malformed)?
                        .to_le_bytes(),
                );
                for value in values.iter().flatten() {
                    let Some(value32) = cadmpeg_core::convert::f32_from_f64(value.get()) else {
                        losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!("mesh material attribute {identity} has a coefficient outside its registered float32 range")));
                        return Ok(Err(Omission::OutOfRange));
                    };
                    rounded |= f64::from(value32) != value.get();
                    bytes.extend_from_slice(&value32.to_le_bytes());
                }
            }
            _ => return Ok(Err(Omission::Missing)),
        }
    }
    if rounded {
        losses.push(FreecadLossCode::ExportCoordinateQuantized.note(format!("mesh material attribute {identity} coefficients are rounded to their registered float32 representation")));
    }
    Ok(Ok(Encoded {
        runtime: "Mesh::PropertyMaterial".into(),
        tag: "Material",
        bytes,
        inline: None,
    }))
}
