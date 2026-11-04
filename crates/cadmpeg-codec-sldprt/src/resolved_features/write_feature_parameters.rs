// SPDX-License-Identifier: Apache-2.0
//! Author operation scalar lanes from the current canonical parameter caches.

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation, ParameterValue};
use cadmpeg_ir::CadIr;

use crate::layout::extrusion_sparse_operation_trailer as extrusion_trailer;
use crate::native::SldprtNative;

/// Native scalar cells duplicate only the temporary output projection. The
/// document keeps the sole authored control in its parameter or literal slot.
pub(crate) fn synthesize(
    ir: &CadIr,
    native: &mut SldprtNative,
) -> Result<(), crate::writer::write_error::WriteError> {
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
        let length = crate::writer::field_u16(
            &crate::writer::CAPACITY_FIELDS[28],
            cadmpeg_core::decode::u64_from_index(class.len()),
        )?;
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
        if matches!(class, "moExtrusion_c" | "moICE_c" | "moCut_c") {
            let operation = neutral.and_then(|feature| match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) => Some(*op),
                _ => None,
            });
            let family = match operation {
                Some(BooleanOp::Join) => Some(0x0140_u16),
                Some(BooleanOp::Cut) => Some(0x01ca_u16),
                _ => None,
            };
            if let Some(family) = family {
                let trailer = payload.len() - 12;
                let family = family.to_le_bytes();
                payload
                    [trailer + extrusion_trailer::FAMILY..trailer + extrusion_trailer::OBJECT_ID]
                    .copy_from_slice(&[family[0], family[1], 0, 0xc0]);
                payload.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xfe, 0xff]);
            }
        }
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
        native
            .feature_input_lanes
            .push(super::write_prepare::generated_native_lane(
                &native.feature_histories,
                LANE,
                None,
                payload,
            )?);
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation};

    #[test]
    fn generated_extrusion_lane_carries_boolean_operation() {
        let mut source = crate::test_support::container::sldprt_with_body(
            &crate::test_support::parasolid::triangle_body(),
        );
        source.extend(crate::test_support::container::make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Extrusion Name="Boss" id="9"><Dimension Name="D1">20</Dimension></Extrusion></Keywords>"#,
        ));
        source.extend(crate::test_support::container::make_block(
            0x42,
            "Contents/Config-0-ResolvedFeatures",
            &crate::test_support::history::resolved_feature_classes_with_ids(&[(
                "moExtrusion_c",
                "Boss",
                9,
            )]),
        ));
        let decoded = crate::SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        for expected in [BooleanOp::Join, BooleanOp::Cut] {
            let mut ir = decoded.ir().clone();
            ir.model.features[0].evaluation.edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition
                else {
                    panic!("fixture has an extrusion");
                };
                *op = expected;
            });
            let mut native = crate::test_support::native::sldprt_native(decoded.ir());
            super::synthesize(&ir, &mut native).unwrap();
            let lane = native
                .feature_input_lanes
                .iter()
                .find(|lane| lane.id == "sldprt:generated:feature-class-input#0")
                .unwrap();
            let name = lane.names.iter().find(|name| name.value == "Boss").unwrap();
            let family = match expected {
                BooleanOp::Join => 0x0140,
                BooleanOp::Cut => 0x01ca,
                _ => unreachable!(),
            };
            assert_eq!(
                super::super::operations::feature_inline_operation_fields(lane, name),
                Some((family, 0))
            );
            let trailer =
                usize::try_from(name.offset).unwrap() + 6 + name.value.encode_utf16().count() * 2;
            let family_bytes = family.to_le_bytes();
            assert_eq!(
                &lane.native_payload[trailer + super::extrusion_trailer::FAMILY
                    ..trailer + super::extrusion_trailer::OBJECT_ID],
                &[family_bytes[0], family_bytes[1], 0, 0xc0]
            );
            ir.model.features[0].evaluation.edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition
                else {
                    unreachable!()
                };
                *op = BooleanOp::Unresolved;
            });
            super::super::operations::bind_extrusion_operations(
                &mut ir.model.features,
                &native.feature_histories,
                std::slice::from_ref(lane),
                None,
            );
            assert!(matches!(ir.model.features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) if *op == expected
            ));
        }
    }
}
