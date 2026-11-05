// SPDX-License-Identifier: Apache-2.0

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{
    BooleanOp, FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef,
};
use std::io::Cursor;

pub(super) fn source_less_extrusion() -> cadmpeg_ir::CadIr {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Extrusion Name="Boss" id="9"><Dimension Name="D1">20</Dimension></Extrusion></Keywords>"#));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &crate::test_support::history::resolved_feature_classes_with_ids(&[(
            "moExtrusion_c",
            "Boss",
            9,
        )]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    let feature = &mut ir.model.features[0];
    feature.native_ref = None;
    feature.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition else {
            panic!("extrusion");
        };
        *op = BooleanOp::Join;
    });
    ir
}

fn assert_history_is_well_formed(bytes: &[u8]) {
    let scan = crate::test_support::container::scan(bytes);
    let section = scan
        .sections()
        .find(|section| section.name().is_some_and(|name| name.contains("Keywords")))
        .expect("history section");
    let text = crate::container::xml_text(section.payload()).expect("history text");
    roxmltree::Document::parse(&text).unwrap_or_else(|error| panic!("{error}: {text}"));
}

#[test]
fn foreign_source_tag_keeps_history_and_document_properties_readable() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    let mut ir = source_less_extrusion();
    ir.model.features[0].source_tag = Some("Fem::FemAnalysis".into());
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#label").unwrap(),
        native_ref: None,
        target: AttributeTarget::Document,
        name: "Label".into(),
        values: vec![Some(AttributeValue::String("Example".into()))],
    });
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert_history_is_well_formed(&bytes);
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
    )));
    assert!(output.ir().model.attributes.iter().any(|attribute| {
        attribute.name == "Label"
            && attribute.values == vec![Some(AttributeValue::String("Example".into()))]
    }));
}

#[test]
fn foreign_native_kind_keeps_history_and_document_properties_readable() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    let mut ir = source_less_extrusion();
    ir.model.features[0].source_tag = Some("Fem::FemAnalysis".into());
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Fem::FemAnalysis".into(),
            parameters: Default::default(),
        }));
    ir.model.features[0].source_content = Default::default();
    ir.model.parameters.clear();
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#label").unwrap(),
        native_ref: None,
        target: AttributeTarget::Document,
        name: "Label".into(),
        values: vec![Some(AttributeValue::String("Example".into()))],
    });
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert_history_is_well_formed(&bytes);
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!output.ir().model.features.is_empty());
    assert!(output.ir().model.attributes.iter().any(|attribute| {
        attribute.name == "Label"
            && attribute.values == vec![Some(AttributeValue::String("Example".into()))]
    }));
}

#[test]
fn source_less_extrusion_keeps_its_unresolved_profile() {
    let mut ir = source_less_extrusion();
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("retains its solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: BooleanOp::Join,
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            ..
        })
    )));
}

#[test]
fn source_less_extrusion_keeps_its_unresolved_operation() {
    let mut ir = source_less_extrusion();
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition else {
            panic!("extrusion");
        };
        *op = BooleanOp::Unresolved;
    });
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("retains its solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: BooleanOp::Unresolved,
            ..
        })
    )));
}

#[test]
fn source_less_extrusion_keeps_its_unresolved_extent() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
    let mut ir = source_less_extrusion();
    ir.model.parameters.clear();
    ir.model.features[0].source_content = Default::default();
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = definition
        else {
            panic!("extrusion");
        };
        *extent = ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Unresolved {},
                draft: None,
            },
        };
    });
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("retains its solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Unresolved {},
                    ..
                }
            },
            ..
        })
    )));
}

#[test]
fn edited_extrusion_can_clear_its_operation() {
    for original in [BooleanOp::Join, BooleanOp::Cut] {
        let mut source = source_less_extrusion();
        source.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition
            else {
                panic!("extrusion")
            };
            *op = original;
        });
        let plan = SldprtCodec
            .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        ir.model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Boss"))
            .unwrap()
            .evaluation
            .edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition
                else {
                    panic!("extrusion")
                };
                *op = BooleanOp::Unresolved;
            });
        for fidelity in [None, Some(decoded.source_fidelity())] {
            let plan = SldprtCodec
                .plan(EncodeInput::new(&ir, fidelity), TargetRequest::Inherit)
                .unwrap();
            assert!(!plan
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("retains its solved result")));
            let mut bytes = Vec::new();
            plan.write_to(&mut bytes).unwrap();
            let output = SldprtCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(
                output.ir().model.features.iter().any(|feature| matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Extrude {
                        op: BooleanOp::Unresolved,
                        ..
                    })
                )),
                "initial {original:?}, fidelity {}, features {:?}",
                fidelity.is_some(),
                output.ir().model.features
            );
        }
    }
}

#[test]
fn edited_extrusion_clears_operation_in_its_native_element_tag() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><BossExtrude Name="Boss" id="9"><Dimension Name="D1">20</Dimension></BossExtrude></Keywords>"#));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &crate::test_support::history::resolved_feature_classes_with_ids(&[(
            "moExtrusion_c",
            "Boss",
            9,
        )]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition else {
            panic!("extrusion")
        };
        *op = BooleanOp::Unresolved;
    });
    for fidelity in [None, Some(decoded.source_fidelity())] {
        let plan = SldprtCodec
            .plan(EncodeInput::new(&ir, fidelity), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(
            output.ir().model.features.iter().any(|feature| matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude {
                    op: BooleanOp::Unresolved,
                    ..
                })
            )),
            "fidelity {}, features {:?}",
            fidelity.is_some(),
            output.ir().model.features
        );
    }
}

#[test]
fn source_less_surface_extrusion_is_authored() {
    let mut ir = source_less_extrusion();
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { solid, .. }) = definition
        else {
            panic!("extrusion")
        };
        *solid = Some(false);
    });
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("retains its solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            solid: Some(false),
            op: BooleanOp::Join,
            ..
        })
    )));
}

#[test]
fn edited_extrusion_changes_between_solid_and_surface() {
    for original in [true, false] {
        let mut source = source_less_extrusion();
        source.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { solid, .. }) = definition
            else {
                panic!("extrusion")
            };
            *solid = Some(original);
        });
        let plan = SldprtCodec
            .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        ir.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { solid, .. }) = definition
            else {
                panic!("extrusion")
            };
            *solid = Some(!original);
        });
        for fidelity in [None, Some(decoded.source_fidelity())] {
            let plan = SldprtCodec
                .plan(EncodeInput::new(&ir, fidelity), TargetRequest::Inherit)
                .unwrap();
            assert!(!plan
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains("retains its solved result")));
            let mut bytes = Vec::new();
            plan.write_to(&mut bytes).unwrap();
            let output = SldprtCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert!(output.ir().model.features.iter().any(|feature| matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude { solid: Some(value), op: BooleanOp::Join, .. }) if *value == !original
            )), "original {original}, fidelity {}", fidelity.is_some());
        }
    }
}

#[test]
fn source_less_feature_uses_its_operation_instead_of_stale_native_controls() {
    let mut ir = source_less_extrusion();
    ir.model.features[0].source_properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableRoot"),
        "true".into(),
    );
    ir.model.features[0]
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("Note"), "kept".into());
    ir.model.finalize();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native::sldprt_native(output.ir());
    let feature = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|feature| feature.name == "Boss")
        .unwrap();
    assert!(!feature.properties.contains_key("DissectableRoot"));
    assert_eq!(
        feature.properties.get("Note").map(String::as_str),
        Some("kept")
    );
    assert!(output.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: BooleanOp::Join,
            ..
        })
    )));
}

#[test]
fn source_less_shell_retains_unresolved_construction() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Shell Name="Partial" Type="Shell" id="151"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Shell { .. })
    ));
    let expected = ir.model.features[0].evaluation.definition().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.features[0].native_ref = None;
    let id = ir.model.features[0].id.clone();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.evaluation.definition() == &expected));
}

#[test]
fn source_less_hole_retains_absent_diameter() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Hole Name="Partial" Type="Hole" id="151"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Hole { .. })
    ));
    let expected = ir.model.features[0].evaluation.definition().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.features[0].native_ref = None;
    let id = ir.model.features[0].id.clone();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.evaluation.definition() == &expected));
}

#[test]
fn source_less_hole_retains_partial_entry_dimensions() {
    fn normalized_definition(ir: &cadmpeg_ir::CadIr) -> serde_json::Value {
        fn replace(value: &mut serde_json::Value, ir: &cadmpeg_ir::CadIr) {
            match value {
                serde_json::Value::String(id) => {
                    if let Some(parameter) = ir
                        .model
                        .parameters
                        .iter()
                        .find(|p| p.id.as_str() == id.as_str())
                    {
                        *id = parameter.name.clone();
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        replace(value, ir);
                    }
                }
                serde_json::Value::Object(values) => {
                    for value in values.values_mut() {
                        replace(value, ir);
                    }
                }
                _ => {}
            }
        }
        let mut value = serde_json::to_value(ir.model.features[0].evaluation.definition()).unwrap();
        replace(&mut value, ir);
        value
    }
    for (name, value) in [
        ("CounterboreDiameter", "12mm"),
        ("CounterboreDepth", "4mm"),
        ("CountersinkDiameter", "12mm"),
        ("CountersinkAngle", "90deg"),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        let keywords = format!(
            "<Keywords><Hole Name=\"Partial\" Type=\"Hole\" id=\"151\"><Dimension Name=\"Diameter\">8mm</Dimension><Dimension Name=\"{name}\">{value}</Dimension></Hole></Keywords>"
        );
        source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        let expected = ir.model.features[0].evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) = &expected else {
            panic!("fixture must decode as a hole");
        };
        use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind};
        assert!(
            matches!(
                shape.construction(),
                HoleConstruction::Form {
                    kind: HoleKind::PartialCounterbore(_) | HoleKind::PartialCountersink(_),
                    ..
                }
            ),
            "{name}: {:?}",
            shape.construction()
        );
        ir.source = None;
        ir.native = cadmpeg_ir::Native::default();
        ir.model.features[0].native_ref = None;
        let normalized = normalized_definition(&ir);
        let plan = SldprtCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(normalized_definition(output.ir()), normalized, "{name}");
        for parameter in &ir.model.parameters {
            let written = output
                .ir()
                .model
                .parameters
                .iter()
                .find(|p| p.name == parameter.name)
                .unwrap_or_else(|| panic!("missing parameter {}", parameter.name));
            assert_eq!(written.value, parameter.value, "{}", parameter.name);
        }
    }
}

#[test]
fn source_less_hole_retains_unevaluated_entry_form() {
    for (name, value) in [
        ("CounterboreDiameter", "NaNmm"),
        ("CounterboreDepth", "NaNmm"),
        ("CountersinkDiameter", "NaNmm"),
        ("CountersinkAngle", "NaNrad"),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        let keywords = format!(
            "<Keywords><Hole Name=\"Partial\" Type=\"Hole\" id=\"151\"><Dimension Name=\"{name}\">{value}</Dimension></Hole></Keywords>"
        );
        source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        let expected = ir.model.features[0].evaluation.definition().clone();
        use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind};
        let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) = &expected else {
            panic!("fixture must decode as a hole");
        };
        assert!(matches!(
            shape.construction(),
            HoleConstruction::Form {
                kind: HoleKind::Unresolved(Some(_)),
                ..
            }
        ));
        ir.source = None;
        ir.native = cadmpeg_ir::Native::default();
        ir.model.features[0].native_ref = None;
        let plan = SldprtCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(
            output
                .ir()
                .model
                .features
                .iter()
                .any(|feature| feature.evaluation.definition() == &expected),
            "{name}"
        );
        let before = ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap();
        let after = output
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap();
        assert_eq!(after.expression, before.expression, "{name}");
        assert_eq!(after.value, before.value, "{name}");
    }
}

#[test]
fn source_less_scale_retains_unresolved_construction() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Scale Name="Partial" Type="Scale" id="151"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let expected = ir.model.features[0].evaluation.definition().clone();
    assert!(matches!(
        expected,
        FeatureDefinition::Operation(FeatureOperation::Scale { .. })
    ));
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.features[0].native_ref = None;
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.evaluation.definition() == &expected));
}

#[test]
fn source_less_thicken_retains_absent_thickness() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Thicken Name="Partial" Type="Thicken" id="151"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            thickness: None,
            ..
        })
    ));
    let expected = ir.model.features[0].evaluation.definition().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.features[0].native_ref = None;
    let id = ir.model.features[0].id.clone();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.evaluation.definition() == &expected));
}

#[test]
fn source_less_edge_treatments_retain_unresolved_construction() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Fillet Name="Partial fillet" Type="Fillet" id="151"/><Chamfer Name="Partial chamfer" Type="Chamfer" id="152"/></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
    ));
    assert!(matches!(
        ir.model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { .. })
    ));
    let expected = ir
        .model
        .features
        .iter()
        .map(|feature| feature.evaluation.definition().clone())
        .collect::<Vec<_>>();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    for feature in &mut ir.model.features {
        feature.native_ref = None;
    }
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for feature in &ir.model.features {
        assert!(!plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(feature.id.as_str())
                && loss.message.contains("solved result")));
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    for expected in expected {
        assert!(output
            .ir()
            .model
            .features
            .iter()
            .any(|feature| feature.evaluation.definition() == &expected));
    }
}

#[test]
fn source_less_draft_retains_unresolved_construction() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Draft Name="Partial" Type="Draft" id="151"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Draft { .. })
    ));
    let expected = ir.model.features[0].evaluation.definition().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.features[0].native_ref = None;
    let id = ir.model.features[0].id.clone();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.evaluation.definition() == &expected));
}

#[test]
fn source_less_combine_retains_unresolved_operands() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Combine Name="Partial" Type="Combine" id="151" Operation="Join"/></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Combine { .. })
    ));
    let expected = ir.model.features[0].evaluation.definition().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.features[0].native_ref = None;
    let id = ir.model.features[0].id.clone();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.evaluation.definition() == &expected));
}

#[test]
fn source_less_surface_cut_retains_unresolved_operands() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><CutWithSurface Name="Partial" Type="SurfaceCut" id="151"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::CutWithSurface { .. })
    ));
    let expected = ir.model.features[0].evaluation.definition().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.features[0].native_ref = None;
    let id = ir.model.features[0].id.clone();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str()) && loss.message.contains("solved result")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.evaluation.definition() == &expected));
}
