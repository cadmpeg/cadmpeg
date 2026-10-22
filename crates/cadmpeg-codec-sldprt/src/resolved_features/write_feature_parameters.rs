// SPDX-License-Identifier: Apache-2.0
//! Author operation scalar lanes from the current canonical parameter caches.

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::ParameterValue;
use cadmpeg_ir::CadIr;

use crate::native::SldprtNative;

/// Native scalar cells duplicate only the temporary output projection. The
/// document keeps the sole authored control in its parameter or literal slot.
pub(crate) fn synthesize(ir: &CadIr, native: &mut SldprtNative) -> Result<(), CodecError> {
    const LANE: &str = "sldprt:generated:feature-class-input#0";
    if !ir.scope.design {
        return Ok(());
    }
    native.feature_input_lanes.retain(|lane| lane.id != LANE);
    let mut next = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(crate::records::Feature::source_value)
        .max()
        .unwrap_or(0);
    let mut payload = Vec::new();
    for feature in native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
    {
        let Some(class) = feature.input_class.as_deref() else {
            continue;
        };
        let object = feature.source_value().ok_or_else(|| {
            CodecError::malformed("authored feature has no native object identity")
        })?;
        // (O-capacity): the class declaration stores a u16 byte length.
        let length = u16::try_from(class.len())
            .map_err(|_| CodecError::malformed("SLDPRT native class name exceeds u16"))?;
        payload.extend_from_slice(super::CLASS_MARKER);
        payload.extend_from_slice(&length.to_le_bytes());
        payload.extend_from_slice(class.as_bytes());
        super::write_generate::append_generated_object_name(&mut payload, &feature.name, object)?;
        let neutral = ir.model.features.iter().find(|neutral| {
            neutral.native_ref.as_deref() == Some(feature.id.as_str())
                || (neutral.native_ref.is_none()
                    && feature.id
                        == crate::history::write::features::generated_feature_record_id(
                            &neutral.id,
                        ))
        });
        for name in feature.parameters.keys() {
            let bound = neutral
                .map(|owner| {
                    crate::history::encode::bindings::role_parameter(
                        owner,
                        &feature.parameters,
                        name.as_str(),
                    )
                })
                .transpose()?
                .flatten();
            let parameter = bound
                .as_ref()
                .and_then(|id| {
                    ir.model
                        .parameters
                        .iter()
                        .find(|parameter| &parameter.id == id)
                })
                .or_else(|| {
                    neutral.and_then(|owner| {
                        ir.model.parameters.iter().find(|parameter| {
                            parameter.owner.as_ref() == Some(&owner.id)
                                && parameter.name == name.as_str()
                        })
                    })
                });
            let value = match parameter {
                Some(parameter) => parameter.value.clone(),
                None => {
                    let mut literal = feature.clone();
                    literal.dimension_properties.clear();
                    crate::history::parameters::source_parameter_value(&literal, name.as_str())
                }
            };
            let Some((class, value)) = value
                .as_ref()
                .and_then(|value| scalar(value, class, name.as_str()))
            else {
                continue;
            };
            // (O-capacity): every scalar cell carries a distinct u32 object id.
            next = next.checked_add(1).ok_or_else(|| {
                CodecError::malformed("SLDPRT scalar object identity space is exhausted")
            })?;
            super::write_generate::append_generated_scalar(
                &mut payload,
                class,
                name.as_str(),
                value,
                next,
                &[],
            )?;
        }
    }
    if !payload.is_empty() {
        native.feature_input_lanes.push(
            super::write_prepare::generated_native_lane(
                &native.feature_histories,
                LANE,
                None,
                payload,
            )?
            .0,
        );
    }
    Ok(())
}

fn scalar(value: &ParameterValue, owner_class: &str, name: &str) -> Option<(&'static str, f64)> {
    match value {
        ParameterValue::Length(value) => Some(("moLengthParameter_c", value.get() / 1000.0)),
        ParameterValue::Angle(value) => Some(("moAngleParameter_c", value.get())),
        ParameterValue::Integer(value)
            if matches!(owner_class, "moLPattern_c" | "moCirPattern_c")
                && matches!(name, "Count" | "Count2" | "D1") =>
        {
            crate::history::parameters::eval::exact_integer_f64(*value)
                .map(|value| ("moNumberDim_c", value))
        }
        // Other unitless controls remain in current Keywords expressions. A
        // dimension catalogue name does not establish its scalar grammar.
        ParameterValue::Real(_)
        | ParameterValue::Integer(_)
        | ParameterValue::Boolean(_)
        | ParameterValue::String(_) => None,
    }
}
