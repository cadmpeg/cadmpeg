// SPDX-License-Identifier: Apache-2.0
//! Synthesis writes supported siblings from neutral content after local loss.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{DesignParameter, ParameterValue};
use cadmpeg_ir::report::{export::WritePath, Severity};

use crate::F3dCodec;

mod box_placement;
mod configuration_features;
mod cylinder_radius;
mod limits;
mod mesh_sentinel;
mod neutral_text;
mod reductions;
mod sketch_attribute;
mod sketch_constraints;
mod sketch_nurbs;
mod write_errors;

fn parameter(name: &str, ordinal: u32, value: ParameterValue) -> DesignParameter {
    DesignParameter {
        product_definition: None,
        driving: true,
        id: format!("authored:model:parameter#{ordinal}")
            .try_into()
            .expect("id"),
        owner: None,
        ordinal,
        name: name.into(),
        expression: name.into(),
        display: None,
        value: Some(value),

        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    }
}

#[test]
fn source_less_primitive_feature_survives_synthesis() {
    use cadmpeg_ir::features::{
        BooleanOp, Dimension, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
        FeatureId, FeatureOperation, PrimitiveSolid, PrimitiveSolidKind, RadialDimension,
    };
    use cadmpeg_ir::scalar::Length;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let definition = FeatureDefinition::Operation(FeatureOperation::Primitive {
        placement: None,
        solid: PrimitiveSolid::new(PrimitiveSolidKind::Cylinder {
            radius: RadialDimension::Diameter(Dimension::Literal(Length::new(20.0).unwrap())),
            height: Dimension::Literal(Length::new(30.0).unwrap()),
            angle: cadmpeg_ir::scalar::Angle::FULL_TURN.into(),
        })
        .unwrap(),
        op: BooleanOp::NewBody,
    });
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("authored:model:feature#0").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(definition.clone()),
        native_ref: None,
    });

    // Two BREP records and ten feature records are emitted.
    let mut limited = EncodeInput::new(&ir, None);
    limited.limits.max_entities = 8;
    assert!(matches!(F3dCodec.plan(limited, TargetRequest::Inherit),
        Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities));
    let mut limited = EncodeInput::new(&ir, None);
    limited.limits.max_entities = 9;
    assert!(matches!(F3dCodec.plan(limited, TargetRequest::Inherit),
        Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities));
    let mut limited = EncodeInput::new(&ir, None);
    limited.limits.max_entities = 11;
    assert!(matches!(F3dCodec.plan(limited, TargetRequest::Inherit),
        Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities));
    let mut exact = EncodeInput::new(&ir, None);
    exact.limits.max_entities = 12;
    F3dCodec
        .plan(exact, TargetRequest::Inherit)
        .expect("exact output record budget");

    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write source-less feature");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode source-less feature");
    assert_eq!(decoded.ir().model.features.len(), 1);
    let FeatureDefinition::Operation(FeatureOperation::Primitive {
        solid,
        placement,
        op,
    }) = decoded.ir().model.features[0].evaluation.definition()
    else {
        panic!("expected a cylinder feature");
    };
    assert_eq!(*op, BooleanOp::NewBody);
    assert_eq!(*placement, None);
    let PrimitiveSolidKind::Cylinder {
        radius: RadialDimension::Diameter(diameter),
        height,
        angle,
    } = solid.kind()
    else {
        panic!("expected a cylinder solid");
    };
    assert_eq!(decoded.ir().dimension_value(diameter).unwrap().get(), 20.0);
    assert_eq!(decoded.ir().dimension_value(height).unwrap().get(), 30.0);
    assert_eq!(
        decoded.ir().dimension_value(angle).unwrap(),
        cadmpeg_ir::scalar::Angle::FULL_TURN
    );
    let Dimension::Parameter(height_id) = height else {
        panic!("generated height has an owned parameter");
    };
    let mut edited = decoded.ir().clone();
    let parameter = edited
        .model
        .parameters
        .iter_mut()
        .find(|p| &p.id == height_id)
        .unwrap();
    let expected_parameter_name = parameter.name.clone();
    let value = ParameterValue::Length(Length::new(40.0).unwrap());
    parameter.expression = cadmpeg_ir::features::Expression::literal(&value);
    parameter.value = Some(value);
    let mut rewritten = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .expect("plan edited feature parameter");
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("f3d.design_parameters")));
    plan.write_to(&mut rewritten)
        .expect("write edited feature parameter");
    let rewritten = F3dCodec
        .decode(&mut Cursor::new(rewritten), &DecodeOptions::default())
        .expect("read edited feature parameter");
    let FeatureDefinition::Operation(FeatureOperation::Primitive { solid, .. }) =
        rewritten.ir().model.features[0].evaluation.definition()
    else {
        panic!("edited cylinder feature");
    };
    let PrimitiveSolidKind::Cylinder { height, .. } = solid.kind() else {
        panic!("edited cylinder solid");
    };
    assert_eq!(rewritten.ir().dimension_value(height).unwrap().get(), 40.0);
    let Dimension::Parameter(rewritten_id) = height else {
        panic!("edited height retains its parameter binding");
    };
    let rewritten_parameter = rewritten
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| &parameter.id == rewritten_id)
        .unwrap();
    assert_eq!(rewritten_parameter.name, expected_parameter_name);
    assert_eq!(
        rewritten_parameter.owner.as_ref(),
        Some(&rewritten.ir().model.features[0].id)
    );
    assert_eq!(
        rewritten_parameter.expression.literal_text().as_deref(),
        Some("40 mm")
    );
    let mut second = ir.model.features[0].clone();
    second.id = FeatureId::mint("authored:model:feature#1").unwrap();
    second.ordinal = 1;
    ir.model.features.push(second);
    let (output, _) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.features.len(), 2);
    assert!(output.ir().model.features.iter().all(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Primitive { .. })
    )));
    ir.model.features[1].source_tag = Some("unsupported variant".into());
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.features.len(), 1);
    let omitted = ir.model.features[1].id.to_string();
    let retained = ir.model.features[0].id.as_str();
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(&omitted)));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains(retained)));
    ir.model.features[1].source_tag = None;
    ir.model.features[1].needs_recompute = true;
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.features.len(), 2);
    assert!(report.losses.iter().any(|loss| {
        loss.message.contains(&omitted) && loss.code.local_code() == "evaluation.stale"
    }));
    ir.model.features[1].needs_recompute = false;
    ir.model.features[1].ordinal = u64::from(u32::MAX);
    let retained = ir.model.features[0].id.as_str();
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.features.len(), 1);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(&omitted)));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains(retained)));
    ir.model.features[1].ordinal = 1;
    ir.model.features[1].evaluation = FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Primitive {
            placement: None,
            solid: PrimitiveSolid::new(PrimitiveSolidKind::Cylinder {
                radius: RadialDimension::Diameter(Dimension::Literal(
                    Length::new(f64::from_bits(1)).unwrap(),
                )),
                height: Dimension::Literal(Length::new(30.0).unwrap()),
                angle: cadmpeg_ir::scalar::Angle::FULL_TURN.into(),
            })
            .unwrap(),
            op: BooleanOp::NewBody,
        }),
    );
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.features.len(), 1);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(&omitted)));
}

#[test]
fn source_less_block_feature_survives_synthesis() {
    use cadmpeg_ir::features::{
        BooleanOp, Dimension, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
        FeatureId, FeatureOperation, FeatureRigidPlacement,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let definition = FeatureDefinition::Operation(FeatureOperation::Block {
        dimensions: Some(
            [2.0, 3.0, 4.0].map(|v| Dimension::Literal(PositiveLength::new(v).unwrap())),
        ),
        placement: Some(FeatureRigidPlacement::identity()),
        op: BooleanOp::NewBody,
    });
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("authored:model:feature#0").unwrap(),
        ordinal: 0,
        name: Some("BoxPrimitive 1".into()),
        suppressed: Some(false),
        needs_recompute: false,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: Some("BoxPrimitive".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: None,
    });
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write source-less block");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode source-less block");
    assert_eq!(decoded.ir().model.features.len(), 1);
    let FeatureDefinition::Operation(FeatureOperation::Block {
        dimensions: Some(dimensions),
        placement,
        op,
    }) = decoded.ir().model.features[0].evaluation.definition()
    else {
        panic!("expected a block")
    };
    assert_eq!(
        decoded.ir().model.features[0].name.as_deref(),
        Some("BoxPrimitive 1")
    );
    assert_eq!(
        decoded.ir().model.features[0].source_tag.as_deref(),
        Some("BoxPrimitive")
    );
    assert_eq!(decoded.ir().model.features[0].suppressed, Some(false));
    assert_eq!(*op, BooleanOp::NewBody);
    assert_eq!(*placement, Some(FeatureRigidPlacement::identity()));
    for (dimension, expected) in dimensions.iter().zip([2.0, 3.0, 4.0]) {
        assert_eq!(
            decoded.ir().dimension_value(dimension).unwrap().get(),
            expected
        );
    }
}

#[test]
fn a_document_parameter_is_authored_without_native_records_and_keeps_supported_siblings() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    ir.model.parameters = vec![
        parameter(
            "width",
            0,
            ParameterValue::Length(cadmpeg_ir::scalar::Length::new(42.0).expect("length")),
        ),
        parameter("label", 1, ParameterValue::String("omitted string".into())),
        parameter(
            "angle",
            2,
            ParameterValue::Angle(cadmpeg_ir::scalar::Angle::new(1.25).expect("angle")),
        ),
    ];
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("unsupported parameter does not refuse the document");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(
            |loss| loss.code.local_code() == "writer.design-records-omitted"
                && loss.severity == Severity::Error
        ));
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.local_code() == "record.residual-unavailable"
            && loss.message.contains("parameter-collection#3")
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode");
    assert!(decoded.source_fidelity().unrepresented().is_empty());
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    let parameters = &decoded.ir().model.parameters;
    assert_eq!(parameters.len(), 2);
    assert_eq!(parameters[0].name, "width");
    assert_eq!(parameters[0].value, ir.model.parameters[0].value);
    assert_eq!(parameters[1].name, "angle");
    assert_eq!(parameters[1].value, ir.model.parameters[2].value);
}

#[test]
fn omitted_scalar_attributes_are_named_and_geometry_survives() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    for (id, value) in [
        ("authored:model:attribute#first", 1),
        ("authored:model:attribute#second", 2),
    ] {
        ir.model.attributes.push(SourceAttribute {
            id: id.try_into().unwrap(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "authored".into(),
            values: vec![Some(AttributeValue::Integer(value))],
        });
    }
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan");
    for attribute in &ir.model.attributes {
        assert!(plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(attribute.id.as_str())));
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
}

#[test]
fn unreferenced_composite_does_not_drop_a_supported_body() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, SolvedCurveGeometry,
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let child = ir.model.curves[0].id.clone();
    let id = "authored:model:curve#composite".try_into().unwrap();
    ir.model.curves.push(Curve {
        id,
        native_ref: None,
        source_object: None,
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: child,
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }]
            .try_into()
            .unwrap(),
            self_intersect: Some(false),
        }),
    });
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("authored:model:curve#composite")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
}

#[test]
fn a_single_segment_composite_edge_keeps_the_body() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, SolvedCurveGeometry,
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let child = ir.model.edges[0].curve().unwrap().clone();
    let id = "authored:model:curve#composite".try_into().unwrap();
    ir.model.curves.push(Curve {
        id,
        native_ref: None,
        source_object: None,
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: child,
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }]
            .try_into()
            .unwrap(),
            self_intersect: Some(false),
        }),
    });
    ir.model.edges[0].set_param_range(None);
    ir.model.edges[0]
        .set_curve(Some(ir.model.curves.last().unwrap().id.clone()))
        .unwrap();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("authored:model:curve#composite")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
}

#[test]
fn unedited_typed_mesh_sentinel_replays() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_with_mesh_surface_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;

    let source = f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.source_fidelity().unrepresented().is_empty());
    let plan = F3dCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::VerbatimReplay { .. }
    ));
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    assert_eq!(output, source);
}

#[test]
fn source_less_synthesis_preserves_native_brep_name() {
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    let (output, _) = super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert_eq!(
        f3d_native(output.ir()).body_native_keys[0]
            .source_brep
            .as_deref(),
        Some("Body1.smbh")
    );
    let mut ir = decoded.ir().clone();
    f3d_native_mut(&mut ir).body_native_keys[0].source_brep = Some("BREP.synthetic.smbh".into());
    let before = f3d_native(&ir).body_native_keys[0].source_brep.clone();
    let (output, _) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(
        f3d_native(output.ir()).body_native_keys[0].source_brep,
        before
    );
}

#[test]
fn source_less_compact_custom_attributes_preserve_values() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_with_attribute_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;

    let source = f3d_with_smbh(&synthetic_geometry_with_attribute_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.attributes.len(), 2);
    let plan = F3dCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    for attribute in &decoded.ir().model.attributes {
        assert!(!plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(attribute.id.as_str())));
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.attributes, decoded.ir().model.attributes);

    let mut renamed = decoded.ir().clone();
    renamed.model.attributes[1].name = "ATTRIB_CUSTOM-attrib_edited".into();
    let (output, _) = super::source_less_analytic::synthesize_document(&renamed, None);
    assert_eq!(output.ir().model.attributes, renamed.model.attributes);

    let mut invalidated = decoded.ir().clone();
    let native_id = crate::test_support::native_test::f3d_native(&invalidated).creation_timestamps
        [0]
    .id
    .clone();
    invalidated.model.attributes[1].values[2] =
        Some(cadmpeg_ir::attributes::AttributeValue::Integer(2));
    let (output, report) = super::source_less_analytic::synthesize_document(&invalidated, None);
    invalidated.model.attributes[1].native_ref = None;
    assert_eq!(output.ir().model.attributes, invalidated.model.attributes);
    assert!(crate::test_support::native_test::f3d_native(output.ir())
        .creation_timestamps
        .is_empty());
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(&native_id)));

    let mut renamed_family = decoded.ir().clone();
    renamed_family.model.attributes[1].values[1] = Some(
        cadmpeg_ir::attributes::AttributeValue::String("Timestamp_attrib_defx".into()),
    );
    let (output, report) = super::source_less_analytic::synthesize_document(&renamed_family, None);
    renamed_family.model.attributes[1].native_ref = None;
    assert_eq!(
        output.ir().model.attributes,
        renamed_family.model.attributes
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(&native_id)));

    let mut dangling = decoded.ir().clone();
    dangling.model.attributes[1].values[0] = Some(
        cadmpeg_ir::attributes::AttributeValue::Reference("f3d:brep:entity#-1x".into()),
    );
    let (output, report) = super::source_less_analytic::synthesize_document(&dangling, None);
    assert_eq!(
        output.ir().model.attributes[0],
        dangling.model.attributes[0]
    );
    assert!(report.losses.iter().any(|loss| {
        loss.message
            .contains(dangling.model.attributes[1].id.as_str())
            && loss.message.contains("successor reference")
    }));

    let mut edited = decoded.ir().clone();
    let timestamp = &mut edited.model.attributes[1];
    let Some(cadmpeg_ir::attributes::AttributeValue::Float(value)) = &mut timestamp.values[3]
    else {
        panic!("timestamp value");
    };
    *value = cadmpeg_ir::scalar::FiniteReal::new(value.get() + 10.0).unwrap();
    let (output, _) = super::source_less_analytic::synthesize_document(&edited, None);
    assert_eq!(output.ir().model.attributes, edited.model.attributes);

    let mut edited = decoded.ir().clone();
    edited.model.attributes[0].values[9] = Some(cadmpeg_ir::attributes::AttributeValue::Integer(9));
    edited.model.attributes[0].values[14] =
        Some(cadmpeg_ir::attributes::AttributeValue::Integer(43));
    let (output, _) = super::source_less_analytic::synthesize_document(&edited, None);
    assert_eq!(output.ir().model.attributes, edited.model.attributes);
    assert_eq!(
        crate::test_support::native_test::f3d_native(output.ir()).body_native_keys[0].asm_body_key,
        Some(43)
    );
    assert_eq!(
        crate::test_support::native_test::f3d_native(output.ir()).persistent_design_links[0]
            .design_reference,
        9
    );
}

#[test]
fn source_less_direct_color_keeps_unchanged_attribute_values() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_with_body_color_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    let source = f3d_with_smbh(&synthetic_geometry_with_body_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (output, report) = super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert_eq!(output.ir().model.attributes, decoded.ir().model.attributes);
    assert!(!report.losses.iter().any(|loss| loss
        .message
        .contains(decoded.ir().model.attributes[0].id.as_str())));
    let mut edited = decoded.ir().clone();
    edited.model.bodies[0].color = cadmpeg_ir::topology::Color::new(0.7, 0.4, 0.2, 1.0);
    let (output, _) = super::source_less_analytic::synthesize_document(&edited, None);
    assert_eq!(
        output.ir().model.bodies[0].color,
        edited.model.bodies[0].color
    );
}

#[test]
fn direct_opacity_reduces_color_without_dropping_geometry() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color::from_rgba8(10, 20, 30, 100));
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.appearance-properties-omitted"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
    assert_eq!(
        decoded.ir().model.bodies[0]
            .color
            .and_then(|color| color.with_alpha(1.0)),
        ir.model.bodies[0]
            .color
            .and_then(|color| color.with_alpha(1.0))
    );
}

#[test]
fn an_unsupported_face_keeps_the_other_faces_as_an_open_shell() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let surface = ir.model.faces[0].surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .unwrap()
        .geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
    );
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("an unsupported face does not refuse the body");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.geometry-omitted"
            && loss.severity == Severity::Error));
    assert!(plan.report().losses.iter().any(|loss| {
        loss.message.contains("model.surfaces:") && loss.message.contains(surface.as_str())
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write open shell");
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read open shell");
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), 5);
    assert_eq!(
        output.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(output.ir(), Vec::new())
            .unwrap()
            .is_ok()
    );
}

#[test]
fn neutral_configurations_and_parameters_are_authored_together() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    ir.model.parameters.push(parameter(
        "width",
        0,
        ParameterValue::Length(cadmpeg_ir::scalar::Length::new(12.0).unwrap()),
    ));
    // D13 clears the active configuration's overrides, so the configuration
    // under test is non-active and the parameter carries a literal expression
    // of its own instead of inheriting the folded override.
    {
        let value = ir.model.parameters[0]
            .value
            .clone()
            .expect("parameter value");
        ir.model.parameters[0].expression = cadmpeg_ir::features::Expression::literal(&value);
    }
    // D13: the base is the active configuration, whose overrides stay empty.
    // A retained override is asserted on a non-active configuration.
    ir.model
        .configurations
        .push(cadmpeg_ir::features::DesignConfiguration {
            id: "authored:model:configuration#0".try_into().unwrap(),
            ordinal: 0,
            active: false,
            source_index: None,
            name: Some("Wide".into()),
            material: Some("Steel".into()),
            properties: [(
                cadmpeg_core::text::NonBlankString::new("activation_rule:0").unwrap(),
                "width > 10".into(),
            )]
            .into(),
            parameter_overrides: [(ir.model.parameters[0].id.clone(), "24 mm".into())].into(),
            bodies: None,
            parameter_values: std::collections::BTreeMap::default(),
            feature_states: std::collections::BTreeMap::default(),
            native_ref: None,
        });
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("author configuration archive");
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read configuration archive");
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.parameters.len(), 1);
    let configuration = &output.ir().model.configurations[0];
    assert_eq!(configuration.name.as_deref(), Some("Wide"));
    assert_eq!(configuration.material.as_deref(), Some("Steel"));
    // D13: the base is the active configuration, so the configuration under
    // test stays non-active and its override is retained.
    assert!(!configuration.active);
    assert_eq!(
        configuration
            .parameter_overrides
            .values()
            .next()
            .and_then(cadmpeg_ir::features::Expression::literal_text)
            .as_deref(),
        Some("24 mm")
    );
    assert!(configuration
        .properties
        .values()
        .any(|value| value == "width > 10"));
}

#[test]
fn an_authored_empty_document_writes_a_valid_archive_and_reports_empty_geometry() {
    let ir = cadmpeg_ir::CadIr::empty();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("empty documents have a valid F3D representation");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.taxonomy()
            == cadmpeg_ir::report::loss::LossTaxonomy::NoExportableSolids));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write empty archive");
    for depth in [
        cadmpeg_core::decode::DecodeDepth::Geometry,
        cadmpeg_core::decode::DecodeDepth::Full,
    ] {
        let output = F3dCodec
            .decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .expect("read empty archive");
        assert!(output.ir().model.bodies.is_empty());
        assert!(!output.report().geometry_transferred());
        assert_eq!(output.ir().scope, depth.required_scope());
        assert!(output.source_fidelity().unrepresented().is_empty());
        assert!(!output.report().losses.iter().any(|loss| matches!(
            loss.code.local_code(),
            "geometry.brep-not-transferred" | "topology.graph-not-built" | "depth.not-delivered"
        )));
    }
}

fn history(ordinal: u64) -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmDeltaState, AsmHistory};
    let id = format!("f3d:authored:asm-history#{ordinal}");
    AsmHistory::new(
        id.clone(),
        ordinal,
        Some(crate::history_records::AsmPreamble {
            stream_size: 1,
            history_entry_count: 1,
        }),
        false,
        vec![AsmDeltaState {
            id: format!("f3d:authored/history-{ordinal}:delta-state#0"),
            parent: id,
            byte_offset: Some(0),
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 1,
            partner_ref: None,
            owner_ref: -1,
            bulletin_boards: vec![],
            records: vec![],
            transition: None,
        }],
    )
}

#[test]
fn an_empty_model_history_is_unrepresented_when_design_is_not_projected() {
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut ir = cadmpeg_ir::CadIr::empty();
    f3d_native_mut(&mut ir).asm_histories = vec![history(0)];
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("typed empty history is supported");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write typed history");
    let full = F3dCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("read typed history");
    assert_eq!(f3d_native(full.ir()).asm_histories.len(), 1);
    assert!(full.source_fidelity().unrepresented().is_empty());
    let geometry = F3dCodec
        .decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .expect("read empty model without history projection");
    assert!(geometry.ir().scope.model);
    assert!(!geometry.report().geometry_transferred());
    assert!(!geometry.ir().scope.design);
    assert!(f3d_native(geometry.ir()).asm_histories.is_empty());
    assert!(geometry
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
}

#[test]
fn unsupported_typed_histories_reduce_without_losing_neutral_geometry() {
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut projected = history(0);
    projected.states_mut()[0].transition = Some(crate::history_records::AsmHistoricalTransition {
        previous_state_id: None,
        records: crate::history_records::AsmHistoricalEntityDelta {
            inserted: vec![3],
            ..Default::default()
        },
        topology: crate::history_records::AsmHistoricalTopologyDelta::default(),
        record_revisions: vec![],
        rows: crate::history_records::AsmHistoricalRowsDelta::default(),
    });
    let mut external_owner = history(0);
    external_owner.states_mut()[0].owner_ref = 7;
    for (histories, expected) in [
        (vec![history(0), history(1)], 1),
        (vec![external_owner], 0),
        (vec![projected], 0),
        (
            {
                let mut record = history(0);
                record.binding_budget_exceeded = true;
                vec![record]
            },
            0,
        ),
    ] {
        let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
        f3d_native_mut(&mut ir).asm_histories = histories;
        let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
        assert_eq!(output.ir().model.bodies.len(), 1);
        assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
        assert_eq!(f3d_native(output.ir()).asm_histories.len(), expected);
        assert_eq!(
            report
                .native_dispositions
                .disposition("f3d", "asm_histories"),
            if expected == 0 {
                cadmpeg_ir::codec::write::NativeDisposition::Dropped
            } else {
                cadmpeg_ir::codec::write::NativeDisposition::Written
            }
        );
        assert!(report.losses.iter().any(|loss| {
            matches!(
                loss.code.local_code(),
                "writer.native-records-omitted" | "native.records-omitted"
            )
        }));
    }
}

#[test]
fn admitted_relaxed_native_type_identifiers_reemit_without_omission() {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    f3d_native_mut(&mut ir).design_types = [
        "11111111-2222-3333-4444-555555555555",
        "____________________________________",
    ]
    .iter()
    .enumerate()
    .map(|(ordinal, guid)| {
        SegmentType::try_new(
            format!("f3d:authored/MetaStream.dat:design-type#{ordinal}"),
            SegmentTypeData {
                class_tag: (256u32 + u32::try_from(ordinal).expect("fixture ordinal"))
                    .to_string()
                    .try_into()
                    .expect("fixture class tag"),
                byte_offset: Some(u64::try_from(ordinal).expect("ordinal")),
                type_guid: (*guid)
                    .to_owned()
                    .try_into()
                    .expect("reader type identifier"),
                type_guid_offset: Some(0),
                base_type_guid: BaseTypeGuid::Absent,
                version: 1,
                version_offset: Some(0),
                module: "Vendor".into(),
                entities: ReferenceRun::unlocated(vec![]),
            },
        )
        .expect("typed registration")
    })
    .collect();
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert!(f3d_native(output.ir())
        .design_types
        .iter()
        .any(|record| { record.type_guid.as_str() == "11111111-2222-3333-4444-555555555555" }));
    assert!(f3d_native(output.ir())
        .design_types
        .iter()
        .any(|record| { record.type_guid.as_str() == "____________________________________" }));
    assert!(!report.losses.iter().any(|loss| {
        loss.code.local_code() == "writer.native-records-omitted"
            && loss.message.contains("design_types")
    }));
}

#[test]
fn independent_native_streams_may_reuse_local_registry_and_recipe_identities() {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::identity::{RecordedValue, ReferenceRun};
    use crate::records::recipes::{
        ConstructionRecipe, ConstructionRecipeDesign, ConstructionRecipeKind,
    };
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    {
        let mut native = f3d_native_mut(&mut ir);
        for stream in ["first", "second"] {
            native.design_types.push(
                SegmentType::try_new(
                    format!("f3d:{stream}/MetaStream.dat:design-type#0"),
                    SegmentTypeData {
                        class_tag: (256u32).to_string().try_into().expect("fixture class tag"),
                        byte_offset: Some(0),
                        type_guid: "11111111-2222-3333-4444-555555555555"
                            .to_owned()
                            .try_into()
                            .expect("guid"),
                        type_guid_offset: Some(0),
                        base_type_guid: BaseTypeGuid::Absent,
                        version: 1,
                        version_offset: Some(0),
                        module: "Vendor".into(),
                        entities: ReferenceRun::unlocated(vec![]),
                    },
                )
                .expect("typed registration"),
            );
            native.construction_recipes.push(ConstructionRecipe {
                id: format!("f3d:{stream}/BulkStream.dat:recipe#0"),
                byte_offset: Some(0),
                kind: ConstructionRecipeKind::Body,
                design: Some(ConstructionRecipeDesign {
                    id: RecordedValue {
                        value: "333".into(),
                        offset: Some(0),
                    },
                    selector: None,
                }),
                recipe_index: 0,
                record_index: Some(RecordedValue {
                    value: 777,
                    offset: Some(0),
                }),
            });
        }
    }
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert_eq!(f3d_native(output.ir()).construction_recipes.len(), 1);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.native-records-omitted"));
}

#[test]
fn independent_body_keys_and_browser_suffixes_are_reminted_without_geometry_loss() {
    use crate::records::bodies::{BodyVisibility, BodyVisibilityWire};
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    use cadmpeg_asm::brep::records::BodyNativeKey;
    let first = cadmpeg_ir::examples::unit_cube().expect("cube");
    let text = first
        .to_canonical_json()
        .expect("json")
        .replace("synthetic:cube:", "synthetic:second:");
    let mut second = cadmpeg_ir::CadIr::from_json(&text).expect("second cube");
    let mut original = first;
    macro_rules! append { ($($field:ident),*) => { $(original.model.$field.append(&mut second.model.$field);)* }; }
    append!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves
    );
    original.model.bodies[0].visible = Some(true);
    original.model.bodies[1].visible = Some(false);
    for missing_first in [false, true] {
        let mut ir = original.clone();
        {
            let bodies = ir.model.bodies.clone();
            let namespace = cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                cadmpeg_asm::asm_format!("f3d"),
            );
            for ordinal in 0..bodies.len() {
                if !missing_first || ordinal == 1 {
                    let row_id = cadmpeg_asm::brep::records::identity::NativeRowId::row_id(
                        &namespace,
                        "body-native-key",
                        u32::try_from(ordinal).expect("ordinal"),
                    );
                    ir.model.bodies[ordinal].native_ref = Some(row_id.to_string());
                }
            }
            let mut native = f3d_native_mut(&mut ir);
            for (ordinal, body) in bodies.iter().enumerate() {
                let key = if missing_first { 1 } else { 42 };
                if !missing_first || ordinal == 1 {
                    native.body_native_keys.push(BodyNativeKey {
                        source_namespace: namespace.clone(),
                        record_index: u32::try_from(ordinal).expect("ordinal"),
                        body_ordinal: u32::try_from(ordinal).expect("ordinal"),
                        source_brep: None,
                        asm_body_key: Some(key),
                    });
                    native.body_visibilities.push(
                        BodyVisibility::try_from(BodyVisibilityWire {
                            id: format!("f3d:part{ordinal}/BulkStream.dat:body-visibility#{key}"),
                            body: cadmpeg_asm::brep::records::identity::NativeRowId::row_id(
                                &namespace,
                                "body-native-key",
                                u32::try_from(ordinal).expect("ordinal"),
                            ),
                            stream: format!("part{ordinal}/BulkStream.dat"),
                            byte_offset: 0,
                            asm_body_key_offset: 0,
                            asm_body_key: key,
                            entity_suffix: 84,
                            visible: body.visible.expect("visibility"),
                        })
                        .expect("typed browser visibility"),
                    );
                }
            }
        }
        let (output, _) = super::source_less_analytic::synthesize_document(&ir, None);
        assert_eq!(output.ir().model.bodies.len(), 2);
        assert_eq!(output.ir().model.faces.len(), 12);
        assert_eq!(
            output
                .ir()
                .model
                .bodies
                .iter()
                .map(|body| body.visible)
                .collect::<Vec<_>>(),
            vec![Some(true), Some(false)]
        );
        let native = f3d_native(output.ir());
        let keys = native
            .body_native_keys
            .iter()
            .filter_map(|record| record.asm_body_key)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&if missing_first { 1 } else { 42 }));
        assert_eq!(
            native
                .body_visibilities
                .iter()
                .map(|record| record.entity_suffix)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            2
        );
    }
}

#[test]
fn an_unprojected_native_record_header_does_not_refuse_unrelated_geometry() {
    use crate::records::decal::DesignRecordHeader;
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    f3d_native_mut(&mut ir).design_record_headers = vec![DesignRecordHeader {
        id: "f3d:authored/BulkStream.dat:design-record-header#0".into(),
        record_index: 123,
        class_tag: "256".to_owned().try_into().expect("class"),
        byte_offset: None,
    }];
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert!(f3d_native(output.ir()).design_record_headers.is_empty());
    assert_eq!(
        report
            .native_dispositions
            .disposition("f3d", "design_record_headers"),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.records-omitted"));
}

#[test]
fn historical_bulletin_slots_are_omitted_when_their_revisions_have_no_typed_writer() {
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let source = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh(),
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("public history fixture");
    let mut ir = decoded.ir().clone();
    {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native
            .asm_histories
            .iter()
            .flat_map(crate::history_records::AsmHistory::states)
            .any(|state| !state.bulletin_boards.is_empty()));
        for state in native
            .asm_histories
            .iter_mut()
            .flat_map(crate::history_records::AsmHistory::states_mut)
        {
            state.records.clear();
            state.transition = None;
        }
    }
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.bodies.len(), ir.model.bodies.len());
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    let histories = &f3d_native(output.ir()).asm_histories;
    assert_eq!(histories.len(), 1);
    assert!(histories[0]
        .states()
        .iter()
        .all(|state| state.bulletin_boards.is_empty()));
    assert_eq!(
        report
            .native_dispositions
            .disposition("f3d", "asm_histories"),
        cadmpeg_ir::codec::write::NativeDisposition::Written
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.records-omitted"));
}

#[test]
fn native_arena_declarations_cover_the_synthesis_input() {
    use crate::test_support::native_test::f3d_native_mut;
    let mut ir = cadmpeg_ir::CadIr::empty();
    f3d_native_mut(&mut ir).asm_histories = vec![history(0)];
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("typed empty history is supported");
    cadmpeg_test_support::contract::assert_declarations_cover_input("f3d", &ir, plan.report());
}

#[test]
fn synthesis_preserves_brep_record_indices_when_curves_follow_topology() {
    use crate::test_support::native_test::f3d_native;
    use crate::test_support::smbh_geometry_test::synthetic_geometry_with_degenerate_curve_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;

    let source = f3d_with_smbh(&synthetic_geometry_with_degenerate_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode synthetic BREP");
    let (output, _) = super::source_less_analytic::synthesize_document(decoded.ir(), None);
    let original = f3d_native(decoded.ir());
    let written = f3d_native(output.ir());
    assert_eq!(original.edge_ownerships, written.edge_ownerships);
    assert_eq!(original.edge_continuities, written.edge_continuities);
    assert_eq!(original.vertex_ownerships, written.vertex_ownerships);

    let mut edited = decoded.ir().clone();
    let old = edited.model.edges[0].id.clone();
    let renamed = cadmpeg_ir::ids::EdgeId::mint("authored:model:edge#renamed").unwrap();
    edited.model.edges[0].id = renamed.clone();
    for coedge in &mut edited.model.coedges {
        if coedge.edge == old {
            coedge.edge = renamed.clone();
        }
    }
    let (output, _) = super::source_less_analytic::synthesize_document(&edited, None);
    assert_eq!(
        original.edge_ownerships,
        f3d_native(output.ir()).edge_ownerships
    );
}

#[test]
fn brep_index_framing_uses_caller_entity_limit() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_with_degenerate_curve_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;

    let source = f3d_with_smbh(&synthetic_geometry_with_degenerate_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode synthetic BREP");
    let mut input = EncodeInput::new(decoded.ir(), None);
    input.limits.max_entities = 0;
    assert!(matches!(
        F3dCodec.plan(input, TargetRequest::Inherit),
        Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(_))
    ));
}

#[test]
fn source_less_sketch_point_survives_synthesis() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchPlacement,
        SketchProfiles,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch_id = "authored:model:sketch#1".try_into().unwrap();
    let placement = SketchPlacement::try_resolved(
        Point3::new(10.0, 20.0, 30.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(0.0, 1.0 + 2.0e-11, 0.0),
    )
    .unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: Some("Sketch_1".into()),
        configuration: None,
        visible: None,
        placement,
        profiles: SketchProfiles::default(),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity::new(
        "authored:model:sketch-entity#1".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(12.0, -7.0),
        })
        .unwrap(),
    ));
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan sketch");
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code.local_code() == "writer.design-records-omitted" }));
    plan.write_to(&mut bytes).expect("write sketch");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode sketch");
    assert_eq!(decoded.ir().model.sketches.len(), 1);
    assert_eq!(
        decoded.ir().model.sketches[0].name.as_deref(),
        Some("Sketch_1")
    );
    assert_eq!(decoded.ir().model.sketches[0].placement, placement);
    assert_eq!(decoded.ir().model.sketch_entities.len(), 1);
    assert_eq!(
        decoded.ir().model.sketch_entities[0].geometry,
        ir.model.sketch_entities[0].geometry
    );
    assert_eq!(
        decoded.ir().model.sketch_entities[0].sketch,
        decoded.ir().model.sketches[0].id
    );
    let mut edited = decoded.ir().clone();
    let changed = SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(25.0, 35.0),
    })
    .unwrap();
    edited.model.sketch_entities[0].geometry = changed.clone();
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write edited sketch");
    let edited_output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode edited sketch");
    assert_eq!(edited_output.ir().model.sketch_entities.len(), 1);
    assert_eq!(
        edited_output.ir().model.sketch_entities[0].geometry,
        changed
    );
}
#[test]
fn source_less_sketch_line_survives_synthesis() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
        SketchPlacement, SketchProfiles,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch_id = "authored:model:sketch#1".try_into().unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: Some("Layout_7".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: None,
    });
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(10.0, 20.0),
        end: Point2::new(40.0, 60.0),
    })
    .unwrap();
    ir.model.sketch_entities.push(SketchEntity::new(
        "authored:model:sketch-entity#1".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        geometry.clone(),
    ));
    let second = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(40.0, 60.0),
        end: Point2::new(40.0, 100.0),
    })
    .unwrap();
    ir.model.sketch_entities.push(SketchEntity::new(
        "authored:model:sketch-entity#2".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        second.clone(),
    ));
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan line sketch");
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.design-records-omitted"));
    plan.write_to(&mut bytes).expect("write line sketch");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode line sketch");
    assert_eq!(decoded.ir().model.sketches.len(), 1);
    assert_eq!(
        decoded.ir().model.sketches[0].name.as_deref(),
        Some("Layout_7")
    );
    assert_eq!(decoded.ir().model.sketch_entities.len(), 2);
    assert_eq!(decoded.ir().model.sketch_entities[0].geometry, geometry);
    assert_eq!(decoded.ir().model.sketch_entities[1].geometry, second);
    assert_eq!(
        decoded.ir().model.sketch_entities[0].sketch,
        decoded.ir().model.sketches[0].id
    );
    ir.model.sketch_entities.push(SketchEntity::new(
        "authored:model:sketch-entity#3".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(40.0, 100.0),
            end: Point2::new(10.0, 20.0),
        })
        .unwrap(),
    ));
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan closed chain without an authored profile");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(ir.model.sketches[0].id.as_str())));
    ir.model.sketch_entities[1].geometry =
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(40.0, 100.0),
            end: Point2::new(40.0, 60.0),
        })
        .unwrap();
    let uses = ir
        .model
        .sketch_entities
        .iter()
        .enumerate()
        .map(|(index, entity)| SketchEntityUse {
            entity: entity.id().clone(),
            reversed: index == 1,
        })
        .collect::<Vec<_>>();
    ir.model.sketches[0].profiles.try_push(uses).unwrap();
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan closed profile");
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.design-records-omitted"));
    plan.write_to(&mut bytes).expect("write closed profile");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode closed profile");
    assert_eq!(decoded.ir().model.sketch_entities.len(), 3);
    assert_eq!(decoded.ir().model.sketches[0].profiles.len(), 1);
    let profile = &decoded.ir().model.sketches[0].profiles[0];
    assert_eq!(profile.len(), 3);
    for (index, (use_, entity)) in profile
        .iter()
        .zip(&decoded.ir().model.sketch_entities)
        .enumerate()
    {
        assert_eq!(&use_.entity, entity.id());
        assert_eq!(use_.reversed, index == 1);
    }
}

#[test]
fn source_less_sketch_circle_profile_survives_synthesis() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
        SketchPlacement, SketchProfiles,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch_id = "authored:model:sketch#circle".try_into().unwrap();
    let entity_id = "authored:model:sketch-entity#circle".try_into().unwrap();
    let mut profiles = SketchProfiles::default();
    profiles.push_single(SketchEntityUse {
        entity: entity_id,
        reversed: false,
    });
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: Some("Profile_1".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles,
        native_ref: None,
    });
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(10.0, 20.0),
        radius: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
    })
    .unwrap();
    ir.model.sketch_entities.push(SketchEntity::new(
        ir.model.sketches[0].profiles[0][0].entity.clone(),
        ir.model.sketches[0].id.clone(),
        geometry.clone(),
    ));
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan circle sketch");
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.design-records-omitted"));
    plan.write_to(&mut bytes).expect("write circle sketch");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode circle sketch");
    assert_eq!(decoded.ir().model.sketch_entities.len(), 1);
    assert_eq!(decoded.ir().model.sketch_entities[0].geometry, geometry);
    assert_eq!(decoded.ir().model.sketches.len(), 1);
    let profile = &decoded.ir().model.sketches[0].profiles[0];
    assert_eq!(profile.len(), 1);
    assert_eq!(
        profile[0].entity,
        *decoded.ir().model.sketch_entities[0].id()
    );
    assert!(!profile[0].reversed);
}

#[test]
fn source_less_sketch_arc_survives_synthesis() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchPlacement,
        SketchProfiles,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch_id = "authored:model:sketch#arc".try_into().unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: Some("Arc_1".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: None,
    });
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(10.0, 20.0),
        radius: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
        start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
        end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
    })
    .unwrap();
    ir.model.sketch_entities.push(SketchEntity::new(
        "authored:model:sketch-entity#arc".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        geometry.clone(),
    ));
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan arc sketch");
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.design-records-omitted"));
    plan.write_to(&mut bytes).expect("write arc sketch");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode arc sketch");
    assert_eq!(decoded.ir().model.sketch_entities.len(), 1);
    assert_eq!(decoded.ir().model.sketch_entities[0].geometry, geometry);
    assert!(decoded.ir().model.sketches[0].profiles.is_empty());
}
