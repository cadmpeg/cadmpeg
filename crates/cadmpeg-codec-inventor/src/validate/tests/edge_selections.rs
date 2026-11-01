// SPDX-License-Identifier: Apache-2.0
use super::{empty_data, TestResult};
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation, ParameterValue};

#[test]
fn historical_treatment_validation_checks_each_input_state_and_operand() -> TestResult {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    for chamfer in [false, true] {
        for control_form in 0..if chamfer { 2 } else { 3 } {
            for schema in [38, 40, 42, 44] {
                for change in 0..11 {
                    let (mut inventory, feature) =
                        crate::feature::edge_selection::historical::tests::fixture(&ctx, chamfer);
                    if control_form != 0 {
                        let ordinal = if chamfer { 81 } else { 83 };
                        inventory
                            .properties
                            .iter_mut()
                            .find(|p| p.identity.record_ordinal == ordinal)
                            .expect("legacy control")
                            .edit(|payload| {
                                if let crate::feature::PmDcFeaturePropertyKind::Boolean {
                                    value,
                                    ..
                                } = &mut payload.kind
                                {
                                    *value = false;
                                }
                            });
                    }
                    if control_form == 2 {
                        inventory.labels[0] = crate::feature::tests::test_label(
                            100,
                            1,
                            crate::feature::CORNER_ROUND_CLASS_ID,
                            &[],
                        );
                        inventory
                            .properties
                            .iter_mut()
                            .find(|p| p.identity.record_ordinal == 80)
                            .expect("corner control")
                            .edit(|payload| {
                                if let crate::feature::PmDcFeaturePropertyKind::Boolean {
                                    value,
                                    ..
                                } = &mut payload.kind
                                {
                                    *value = false;
                                }
                            });
                    }
                    let mut features = vec![feature];
                    let mut states = crate::feature::edge_selection::historical::project(
                        &ctx,
                        &mut features,
                        &inventory,
                    )?;
                    assert_eq!(states.len(), 1);
                    let mut data = empty_data();
                    data.active_carrier =
                        crate::feature::edge_selection::historical::tests::carrier(schema);
                    data.pm_dc_features = inventory.features;
                    data.pm_dc_feature_properties = inventory.properties;
                    data.pm_dc_feature_labels = inventory.labels;
                    let (raw, parameter) = crate::feature::fillet::tests::parameter(
                        49,
                        ParameterValue::Length(
                            cadmpeg_ir::scalar::Length::new(2.0).ok_or("length")?,
                        ),
                    );
                    data.pm_dc_parameters.push(raw);
                    let mut ir = cadmpeg_ir::CadIr::empty();
                    ir.model.parameters.push(parameter);
                    match change {
                        1 => states.clear(),
                        2 => states[0].edge_geometry.clear(),
                        3 => states[0].edge_geometry[0].tangent = None,
                        4 => states[0].native_ref = Some("other".into()),
                        5 => {
                            let duplicate = states[0].clone();
                            states.push(duplicate);
                        }
                        6 => features[0].evaluation.edit(|definition, _| {
                            let selection = match definition {
                                FeatureDefinition::Operation(FeatureOperation::Fillet {
                                    groups,
                                }) => &mut groups.iter_mut().next().expect("group").edges,
                                FeatureDefinition::Operation(FeatureOperation::Chamfer {
                                    groups,
                                    ..
                                }) => &mut groups.iter_mut().next().expect("group").edges,
                                _ => unreachable!(),
                            };
                            if let EdgeSelection::Historical { native, .. } = selection {
                                *native = cadmpeg_core::text::NonBlankString::new("other")
                                    .expect("native");
                            }
                        }),
                        7 => {
                            data.active_carrier =
                                crate::feature::edge_selection::historical::tests::carrier(45);
                        }
                        8 => {
                            if let crate::native::ActiveCarrierRecord::Selected { family, .. } =
                                &mut data.active_carrier
                            {
                                *family = crate::kernel::KernelFamily::Acis;
                            }
                        }
                        9 => {
                            data.active_carrier =
                                crate::native::ActiveCarrierRecord::NotApplicable {
                                    id: "synthetic:carrier".into(),
                                }
                        }
                        10 => {
                            let ordinal = if chamfer { 80 } else { 81 };
                            data.pm_dc_feature_properties
                                .iter_mut()
                                .find(|p| p.identity.record_ordinal == ordinal)
                                .expect("Boolean control")
                                .edit(|payload| {
                                    if let crate::feature::PmDcFeaturePropertyKind::Boolean {
                                        value,
                                        ..
                                    } = &mut payload.kind
                                    {
                                        *value = false;
                                    }
                                });
                        }
                        _ => {}
                    }
                    ir.model.features = features;
                    ir.model.feature_input_topologies = states;
                    let mut found = Vec::new();
                    super::super::validate_features(&ctx, &ir, &data, &mut found)?;
                    assert_eq!(
                        found
                            .iter()
                            .filter(|finding| {
                                finding.message
                        == "Inventor neutral edge treatment operands differ from their PmDc slots"
                            })
                            .count(),
                        usize::from(change != 0),
                        "{found:?}"
                    );
                }
            }
        }
    }
    Ok(())
}
