// SPDX-License-Identifier: Apache-2.0
//! Author operation scalar lanes from the current canonical parameter caches.

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation, ParameterValue};
use cadmpeg_ir::CadIr;
use std::collections::HashSet;

use crate::layout::extrusion_sparse_operation_trailer as extrusion_trailer;
use crate::native::SldprtNative;

pub(crate) const SCALAR_IDENTITY_CAPACITY: cadmpeg_ir::codec::write::CapacityField =
    cadmpeg_ir::codec::write::CapacityField {
        field: "feature_input.scalar_identity",
        limit: 0xffff_ffff_u64,
        source: "docs/formats/sldprt.md §2: scalar object identifier u32 LE",
        helper: "ScalarIdentities::take",
    };

pub(crate) struct ScalarIdentities {
    used: HashSet<u32>,
    next: Option<u32>,
}

impl ScalarIdentities {
    pub(crate) fn new(native: &SldprtNative, configuration: Option<&str>) -> Self {
        let mut used = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(crate::records::Feature::source_value)
            .collect::<HashSet<_>>();
        for lane in native
            .feature_input_lanes
            .iter()
            .filter(|lane| lane.configuration.as_deref() == configuration)
        {
            used.extend(lane.scalars.iter().map(|scalar| scalar.object_id));
            used.extend(
                lane.names
                    .iter()
                    .filter_map(|name| name.object_id.and_then(crate::records::ObjectId::value)),
            );
        }
        Self {
            used,
            next: Some(1),
        }
    }

    pub(crate) fn take(&mut self) -> Result<u32, crate::writer::write_error::WriteError> {
        loop {
            let candidate = self
                .next
                .ok_or(crate::writer::write_error::WriteError::Capacity {
                    field: &SCALAR_IDENTITY_CAPACITY,
                    value: u64::from(u32::MAX) + 1,
                })?;
            self.next = candidate.checked_add(1);
            if self.used.insert(candidate) {
                return Ok(candidate);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ConfigurationOperation {
    Extrusion(u16),
    Revolution(u32),
}

pub(crate) fn synthesize_configuration_operations(
    ir: &CadIr,
    native: &mut SldprtNative,
) -> Result<
    HashSet<(
        cadmpeg_ir::features::ConfigurationId,
        cadmpeg_ir::features::FeatureId,
    )>,
    crate::writer::write_error::WriteError,
> {
    let mut written = HashSet::new();
    for configuration in ir
        .model
        .configurations
        .iter()
        .filter(|configuration| !configuration.active)
    {
        let slot = crate::history::write::configurations::configuration_slot(
            &configuration.properties,
            configuration.ordinal,
        )
        .to_string();
        let mut records = Vec::new();
        for (id, state) in &configuration.feature_states {
            let Some(override_definition) = state.definition() else {
                continue;
            };
            let Some(feature) = ir.model.features.iter().find(|feature| &feature.id == id) else {
                continue;
            };
            let mut base = feature.evaluation.definition().clone();
            let operation = match (override_definition, &mut base) {
                (
                    FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }),
                    FeatureDefinition::Operation(FeatureOperation::Extrude { op: base_op, .. }),
                ) => {
                    *base_op = *op;
                    extrusion_family(*op).map(ConfigurationOperation::Extrusion)
                }
                (
                    FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }),
                    FeatureDefinition::Operation(FeatureOperation::Revolve { op: base_op, .. }),
                ) => {
                    *base_op = *op;
                    match op {
                        BooleanOp::NewBody => Some(ConfigurationOperation::Revolution(5)),
                        BooleanOp::Join => Some(ConfigurationOperation::Revolution(8)),
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some(operation) = operation else { continue };
            if &base != override_definition {
                continue;
            }
            let Some(record) = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|record| {
                    feature.native_ref.as_deref() == Some(record.id.as_str())
                        || (feature.native_ref.is_none()
                            && record.id
                                == crate::history::write::features::generated_feature_record_id(id))
                })
            else {
                continue;
            };
            let Some(object_id) = record.source_value() else {
                continue;
            };
            if matches!(operation, ConfigurationOperation::Extrusion(_))
                && !matches!(
                    record.input_class.as_deref(),
                    Some("moExtrusion_c" | "moICE_c")
                )
            {
                continue;
            }
            if matches!(operation, ConfigurationOperation::Revolution(_))
                && record
                    .input_class
                    .as_deref()
                    .is_some_and(|class| class != "moRevolution_c")
            {
                continue;
            }
            let class = match operation {
                ConfigurationOperation::Extrusion(_) => record
                    .input_class
                    .as_deref()
                    .filter(|class| {
                        matches!(
                            *class,
                            "moExtrusion_c" | "moICE_c" | "moCut_c" | "moExtruRefSurface_c"
                        )
                    })
                    .unwrap_or("moExtrusion_c"),
                ConfigurationOperation::Revolution(_) => "moRevolution_c",
            };
            records.push((
                id.clone(),
                class.to_owned(),
                record.name.clone(),
                object_id,
                operation,
            ));
        }
        if records.is_empty() {
            continue;
        }
        let mut payload = native
            .feature_input_lanes
            .iter()
            .filter(|lane| {
                lane.configuration.as_deref() == Some(&slot)
                    && lane.id.starts_with("sldprt:generated:")
            })
            .flat_map(|lane| lane.native_payload.iter().copied())
            .collect::<Vec<_>>();
        for (id, class, name, object_id, operation) in records {
            if let ConfigurationOperation::Revolution(form) = operation {
                payload.extend_from_slice(&form.to_le_bytes());
                payload.extend_from_slice(&[0; 4]);
            }
            append_named_object(&mut payload, &class, &name, object_id)?;
            if let ConfigurationOperation::Extrusion(family) = operation {
                append_extrusion_operation(&mut payload, family);
            }
            written.insert((configuration.id.clone(), id));
        }
        let lane = super::write_prepare::generated_configuration_lane(
            &native.feature_histories,
            &slot,
            payload,
        )?;
        native.feature_input_lanes.retain(|lane| {
            lane.configuration.as_deref() != Some(&slot)
                || !lane.id.starts_with("sldprt:generated:")
        });
        native.feature_input_lanes.push(lane);
    }
    if let Some(active) = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.active)
    {
        let slot = crate::history::write::configurations::configuration_slot(
            &active.properties,
            active.ordinal,
        )
        .to_string();
        let ids = written.iter().map(|(_, id)| id).collect::<HashSet<_>>();
        let mut payload = native
            .feature_input_lanes
            .iter()
            .filter(|lane| {
                lane.configuration.as_deref() == Some(&slot)
                    && lane.id.starts_with("sldprt:generated:")
            })
            .flat_map(|lane| lane.native_payload.iter().copied())
            .collect::<Vec<_>>();
        let mut appended = false;
        for feature in ir
            .model
            .features
            .iter()
            .filter(|feature| ids.contains(&feature.id))
        {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) =
                feature.evaluation.definition()
            else {
                continue;
            };
            let Some(family) = extrusion_family(*op) else {
                continue;
            };
            let Some(record) = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|record| {
                    feature.native_ref.as_deref() == Some(record.id.as_str())
                        || (feature.native_ref.is_none()
                            && record.id
                                == crate::history::write::features::generated_feature_record_id(
                                    &feature.id,
                                ))
                })
            else {
                continue;
            };
            let Some(class @ ("moExtrusion_c" | "moICE_c")) = record.input_class.as_deref() else {
                continue;
            };
            let Some(object_id) = record.source_value() else {
                continue;
            };
            append_named_object(&mut payload, class, &record.name, object_id)?;
            append_extrusion_operation(&mut payload, family);
            appended = true;
        }
        if appended {
            let lane = super::write_prepare::generated_configuration_lane(
                &native.feature_histories,
                &slot,
                payload,
            )?;
            native.feature_input_lanes.retain(|lane| {
                lane.configuration.as_deref() != Some(&slot)
                    || !lane.id.starts_with("sldprt:generated:")
            });
            native.feature_input_lanes.push(lane);
        }
    }
    for (_, feature_id) in &written {
        let Some(feature) = ir
            .model
            .features
            .iter()
            .find(|feature| &feature.id == feature_id)
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
        ) {
            continue;
        }
        let Some(record) = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|record| {
                feature.native_ref.as_deref() == Some(record.id.as_str())
                    || (feature.native_ref.is_none()
                        && record.id
                            == crate::history::write::features::generated_feature_record_id(
                                feature_id,
                            ))
            })
        else {
            continue;
        };
        if matches!(
            record.input_class.as_deref(),
            Some("moExtrusion_c" | "moICE_c")
        ) {
            record.kind = "Extrusion".into();
            record.properties.remove("Operation");
        }
    }
    Ok(written)
}

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
    let mut scalar_ids = ScalarIdentities::new(native, None);
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
        append_named_object(&mut payload, class, &feature.name, object)?;
        let neutral = ir.model.features.iter().find(|neutral| {
            neutral.native_ref.as_deref() == Some(feature.id.as_str())
                || (neutral.native_ref.is_none()
                    && feature.id
                        == crate::history::write::features::generated_feature_record_id(
                            &neutral.id,
                        ))
        });
        if matches!(
            class,
            "moExtrusion_c" | "moICE_c" | "moCut_c" | "moExtruRefSurface_c"
        ) {
            let operation = neutral.and_then(|feature| match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) => Some(*op),
                _ => None,
            });
            let family = operation.and_then(extrusion_family);
            if let Some(family) = family {
                append_extrusion_operation(&mut payload, family);
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
            super::write_generate::append_generated_scalar(
                &mut payload,
                class,
                name.as_str(),
                value,
                scalar_ids.take()?,
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

fn extrusion_family(operation: BooleanOp) -> Option<u16> {
    match operation {
        BooleanOp::Join => Some(0x0140),
        BooleanOp::Cut => Some(0x01ca),
        _ => None,
    }
}

fn append_named_object(
    payload: &mut Vec<u8>,
    class: &str,
    name: &str,
    object: u32,
) -> Result<(), crate::writer::write_error::WriteError> {
    let length = crate::writer::field_u16(
        &crate::writer::CAPACITY_FIELDS[28],
        cadmpeg_core::decode::u64_from_index(class.len()),
    )?;
    payload.extend_from_slice(super::CLASS_MARKER);
    payload.extend_from_slice(&length.to_le_bytes());
    payload.extend_from_slice(class.as_bytes());
    super::write_generate::append_generated_object_name(payload, name, object)
}

fn append_extrusion_operation(payload: &mut Vec<u8>, family: u16) {
    let trailer = payload.len() - 12;
    let family = family.to_le_bytes();
    payload[trailer + extrusion_trailer::FAMILY..trailer + extrusion_trailer::OBJECT_ID]
        .copy_from_slice(&[family[0], family[1], 0, 0xc0]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xfe, 0xff]);
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
    fn scalar_identity_allocation_reuses_gaps_and_reports_the_boundary() {
        let mut ids = super::ScalarIdentities {
            used: std::collections::HashSet::from([1, u32::MAX - 1]),
            next: Some(1),
        };
        assert_eq!(ids.take().unwrap(), 2);
        ids.next = Some(u32::MAX);
        assert_eq!(ids.take().unwrap(), u32::MAX);
        let error = ids.take().unwrap_err();
        assert!(
            matches!(error, crate::writer::write_error::WriteError::Capacity { field, value }
            if field.field == "feature_input.scalar_identity" && field.limit == u64::from(u32::MAX) && value == u64::from(u32::MAX) + 1)
        );
    }

    #[test]
    fn inactive_revolution_form_is_written_in_its_configuration_lane() {
        use cadmpeg_ir::features::{
            ConfigurationFeatureState, ConfigurationId, DesignConfiguration,
        };
        let mut source = crate::test_support::container::sldprt_with_body(
            &crate::test_support::parasolid::triangle_body(),
        );
        source.extend(crate::test_support::container::make_block(0x42, "Contents/Keywords", br#"<Keywords><Revolve Name="Turn" Type="Revolve" id="17" AxisOrigin="0,0,0" AxisDirection="0,0,1" Operation="Join"><Dimension Name="Angle">180deg</Dimension></Revolve></Keywords>"#));
        source.extend(crate::test_support::container::make_block(
            0x42,
            "Contents/Config-0-ResolvedFeatures",
            &crate::test_support::history::resolved_feature_classes_with_ids(&[(
                "moRevolution_c",
                "Turn",
                17,
            )]),
        ));
        let decoded = crate::SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        let feature_id = ir.model.features[0].id.clone();
        let mut alternative = ir.model.features[0].evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }) = &mut alternative
        else {
            panic!("revolution")
        };
        *op = BooleanOp::NewBody;
        let configuration_id =
            ConfigurationId::mint("synthetic:test:configuration#alternate-revolve").unwrap();
        ir.model.configurations = vec![DesignConfiguration {
            id: configuration_id.clone(),
            ordinal: 1,
            active: false,
            source_index: Some(1),
            name: Some("Alternative".into()),
            material: None,
            properties: std::collections::BTreeMap::default(),
            parameter_overrides: std::collections::BTreeMap::default(),
            bodies: None,
            parameter_values: std::collections::BTreeMap::default(),
            feature_states: [(
                feature_id.clone(),
                ConfigurationFeatureState::Definition {
                    definition: Box::new(alternative),
                },
            )]
            .into_iter()
            .collect(),
            native_ref: None,
        }];
        let mut native = crate::test_support::native::sldprt_native(decoded.ir());
        let written = super::synthesize_configuration_operations(&ir, &mut native).unwrap();
        assert!(written.contains(&(configuration_id, feature_id)));
        let lane = native
            .feature_input_lanes
            .iter()
            .find(|lane| lane.configuration.as_deref() == Some("1"))
            .unwrap();
        let class = lane
            .classes
            .iter()
            .find(|class| class.name == "moRevolution_c")
            .unwrap();
        let offset = usize::try_from(class.offset).unwrap();
        assert_eq!(
            &lane.native_payload[offset - 8..offset],
            &[5, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn inactive_extrusion_operation_is_written_in_its_configuration_lane() {
        use cadmpeg_ir::features::{
            ConfigurationFeatureState, ConfigurationId, DesignConfiguration,
        };

        let mut source = crate::test_support::container::sldprt_with_body(
            &crate::test_support::parasolid::triangle_body(),
        );
        source.extend(crate::test_support::container::make_block(
            0x42, "Contents/Keywords",
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
        let mut ir = decoded.ir().clone();
        let feature_id = ir.model.features[0].id.clone();
        ir.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition
            else {
                panic!("extrusion");
            };
            *op = BooleanOp::Join;
        });
        let mut alternative = ir.model.features[0].evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = &mut alternative
        else {
            panic!("extrusion");
        };
        *op = BooleanOp::Cut;
        let configuration_id =
            ConfigurationId::mint("synthetic:test:configuration#alternative").unwrap();
        ir.model.configurations = vec![DesignConfiguration {
            id: configuration_id.clone(),
            ordinal: 1,
            active: false,
            source_index: Some(1),
            name: Some("Alternative".into()),
            material: None,
            properties: std::collections::BTreeMap::default(),
            parameter_overrides: std::collections::BTreeMap::default(),
            bodies: None,
            parameter_values: std::collections::BTreeMap::default(),
            feature_states: [(
                feature_id.clone(),
                ConfigurationFeatureState::Definition {
                    definition: Box::new(alternative),
                },
            )]
            .into_iter()
            .collect(),
            native_ref: None,
        }];
        let mut native = crate::test_support::native::sldprt_native(decoded.ir());
        let written = super::synthesize_configuration_operations(&ir, &mut native).unwrap();
        assert!(written.contains(&(configuration_id, feature_id)));
        let lane = native
            .feature_input_lanes
            .iter()
            .find(|lane| lane.configuration.as_deref() == Some("1"))
            .unwrap();
        let name = lane.names.iter().find(|name| name.value == "Boss").unwrap();
        assert_eq!(
            super::super::operations::feature_inline_operation_fields(lane, name),
            Some((0x01ca, 0))
        );
    }

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
