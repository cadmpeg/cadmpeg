// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::history::resolved_features_payload;
use crate::test_support::history::resolved_features_payload_with_names;
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::history::sldprt_with_body_and_resolved_features;
use crate::test_support::native::sldprt_native;
use crate::test_support::native::update_sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::pmi::pmi_semantic_payload_record;
use crate::SldprtCodec;

const EPS_PATTERN_ANGLE: f64 = 1.0e-12;

fn decoded_object_names(payload: &[u8], parent: &str) -> Vec<crate::records::FeatureInputName> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("test context");
    crate::resolved_features::names::object_names(&ctx, payload, parent)
        .expect("compute the object names")
}

#[test]
fn semantic_writer_round_trips_all_pattern_forms() {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="NativeSeed" id="7"/>
            <Pattern Name="Rows" Type="LinearPattern" id="18" Seeds="7" Direction="1,0,0"><Dimension Name="Count">3</Dimension><Dimension Name="Spacing">10mm</Dimension></Pattern>
            <Pattern Name="Ring" Type="CircularPattern" id="19" Seeds="7" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1"><Dimension Name="Count">4</Dimension><Dimension Name="Angle">360deg</Dimension></Pattern>
            <Mirror Name="Reflect" Type="Mirror" id="20" Seeds="7" PlaneOrigin="5mm,0mm,0mm" PlaneNormal="1,0,0"/>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let seed = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds,
            pattern: admitted_pattern,
        }) if matches!(admitted_pattern.definition(), PatternTransform::Linear {
                direction: Some(actual_direction),
                distance: actual_spacing,
                count: actual_count,
                second: None,
                omitted_instances,
            } if omitted_instances.is_empty() && decoded.ir().dimension_value(actual_count) == Some(3) && *actual_direction == Vector3::new(1.0, 0.0, 0.0)
                && (seeds == &[cadmpeg_ir::features::patterns::PatternSeed::Feature(seed.clone())]) && decoded.ir().dimension_value(actual_spacing.dimension()).unwrap().get() == 10.0)
    ));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            pattern: admitted_pattern,
            ..
        }) if matches!(admitted_pattern.definition(), PatternTransform::Circular {
                axis_origin,
                axis_dir,
                angle: value,
                count: actual_count,
            } if decoded.ir().dimension_value(actual_count) == Some(4) && *axis_origin == Point3::new(0.0, 0.0, 0.0)
                && *axis_dir == Vector3::new(0.0, 0.0, 1.0)
                && (decoded.ir().dimension_value(value.dimension()).unwrap().get() - std::f64::consts::TAU).abs() < EPS_PATTERN_ANGLE)
    ));
    assert!(matches!(
        decoded.ir().model.features[3].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            pattern: admitted_pattern,
            ..
        }) if matches!(admitted_pattern.definition(), PatternTransform::Mirror {
                plane_origin,
                plane_normal,
            } if *plane_origin == Point3::new(5.0, 0.0, 0.0)
                && *plane_normal == Vector3::new(1.0, 0.0, 0.0))
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let linear = ir_edit.model.features[1].id.clone();
        ir_edit
            .set_dimension(
                &linear,
                "/pattern/distance/distance",
                cadmpeg_ir::scalar::Length::new(12.0).unwrap(),
            )
            .unwrap();
        ir_edit
            .set_dimension(&linear, "/pattern/count", 5_u32)
            .unwrap();
        let circular = ir_edit.model.features[2].id.clone();
        ir_edit
            .set_dimension(
                &circular,
                "/pattern/angle/angle",
                cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::PI).unwrap(),
            )
            .unwrap();
        ir_edit
            .set_dimension(&circular, "/pattern/count", 6_u32)
            .unwrap();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[1].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
            &mut updated_ir_edit_definition
        else {
            panic!("linear pattern");
        };
        let mut transform = pattern.definition().clone();
        let PatternTransform::Linear { direction, .. } = &mut transform else {
            panic!("pattern form");
        };
        *direction = Some(
            cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap(),
        );
        *pattern = PatternKind::new(transform).unwrap();
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[2].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
            &mut updated_ir_edit_definition
        else {
            panic!("circular pattern");
        };
        let mut transform = pattern.definition().clone();
        let PatternTransform::Circular { axis_origin, .. } = &mut transform else {
            panic!("pattern form");
        };
        *axis_origin = cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap();
        *pattern = PatternKind::new(transform).unwrap();
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[3].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
            &mut updated_ir_edit_definition
        else {
            panic!("mirror pattern");
        };
        let mut transform = pattern.definition().clone();
        let PatternTransform::Mirror {
            plane_origin,
            plane_normal,
        } = &mut transform
        else {
            panic!("pattern form");
        };
        *plane_origin =
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 0.0)).unwrap();
        *plane_normal =
            cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap();
        *pattern = PatternKind::new(transform).unwrap();
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
    }

    let mut inconsistent = decoded.ir().clone();
    inconsistent.model.features[1].dependencies.clear();
    let error = crate::test_support::plan_fixture_write_with_native_envelope(
        &inconsistent,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("pattern omits seed feature"),
        "{error}"
    );

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(features[1].properties["Seeds"], "7");
    assert_eq!(features[1].properties["Direction"], "0,1,0");
    assert_eq!(features[1].parameters["Spacing"], "12mm");
    assert_eq!(features[1].parameters["Count"], "5");
    assert_eq!(features[2].properties["AxisOrigin"], "1mm,2mm,3mm");
    assert_eq!(features[2].parameters["Count"], "6");
    assert_eq!(features[3].properties["PlaneOrigin"], "2mm,0mm,0mm");
    assert_eq!(features[3].properties["PlaneNormal"], "0,1,0");
}

#[test]
fn semantic_writer_round_trips_sparse_curve_driven_pattern() {
    use cadmpeg_ir::{
        features::{
            patterns::PatternTransform, FeatureDefinition, FeatureOperation, ParameterValue,
        },
        scalar::Length,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Curve Pattern1" Type="CrvPattern" id="169"><Dimension Name="D3">397.6</Dimension><Dimension Name="D1">16</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds,
            pattern: admitted_pattern,
        }) if matches!(admitted_pattern.definition(), PatternTransform::CurveDriven {
                path: None,
                spacing: actual_spacing,
                count: actual_count,
            } if decoded.ir().dimension_value(actual_count) == Some(16) && (seeds.is_empty()) && decoded.ir().dimension_value(actual_spacing).unwrap().get() == 397.6)
    ));
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(ParameterValue::Length(Length::new(397.6).unwrap()))
    );
    assert_eq!(
        decoded.ir().model.parameters[1].value,
        Some(ParameterValue::Integer(16))
    );

    {
        let id = decoded.ir().model.features[0].id.clone();
        let mut ir_edit = decoded.ir_mut();
        ir_edit
            .set_dimension(
                &id,
                "/pattern/spacing",
                cadmpeg_ir::scalar::PositiveLength::new(250.0).unwrap(),
            )
            .unwrap();
        ir_edit.set_dimension(&id, "/pattern/count", 8_u32).unwrap();
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "CrvPattern");
    assert_eq!(native.parameters["D3"], "250");
    assert_eq!(native.parameters["D1"], "8");
    assert!(!native.parameters.contains_key("Spacing"));
    assert!(!native.parameters.contains_key("Count"));
    assert!(!native.properties.contains_key("Seeds"));
    assert!(!native.properties.contains_key("Path"));
    assert!(
        matches!(&(regenerated.ir().model.features[0].evaluation.definition()),
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                pattern: admitted_pattern,
                ..
            }) if matches!(admitted_pattern.definition(), PatternTransform::CurveDriven {
                    path: None,
                    spacing: actual_spacing,
                    count: actual_count,
                } if regenerated.ir().dimension_value(actual_count) == Some(8) && regenerated.ir().dimension_value(actual_spacing).unwrap().get() == 250.0)
        )
    );
}

#[test]
fn semantic_writer_round_trips_sparse_localized_linear_pattern() {
    use cadmpeg_ir::{
        features::{
            patterns::PatternTransform, FeatureDefinition, FeatureOperation, ParameterValue,
        },
        scalar::Length,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="MatrizL1" Type="MatrizL" id="132"><Dimension Name="D1">15</Dimension><Dimension Name="D3">2.54</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moLPattern_c", "MatrizL1", 132)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds,
            pattern: admitted_pattern,
        }) if matches!(admitted_pattern.definition(), PatternTransform::Linear {
                direction: None,
                distance: actual_spacing,
                count: actual_count,
                second: None,
                omitted_instances,
            } if omitted_instances.is_empty() && decoded.ir().dimension_value(actual_count) == Some(15) && (seeds.is_empty()) && decoded.ir().dimension_value(actual_spacing.dimension()).unwrap().get() == 2.54)
    ));
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(ParameterValue::Integer(15))
    );
    assert_eq!(
        decoded.ir().model.parameters[1].value,
        Some(ParameterValue::Length(Length::new(2.54).unwrap()))
    );

    {
        let id = decoded.ir().model.features[0].id.clone();
        let mut ir_edit = decoded.ir_mut();
        ir_edit
            .set_dimension(
                &id,
                "/pattern/distance/distance",
                cadmpeg_ir::scalar::Length::new(3.5).unwrap(),
            )
            .unwrap();
        ir_edit
            .set_dimension(&id, "/pattern/count", 12_u32)
            .unwrap();
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "MatrizL");
    assert_eq!(native.input_class.as_deref(), Some("moLPattern_c"));
    assert_eq!(native.parameters["D1"], "12");
    assert_eq!(native.parameters["D3"], "3.5");
    assert!(!native.parameters.contains_key("Count"));
    assert!(!native.parameters.contains_key("Spacing"));
    assert!(!native.properties.contains_key("Seeds"));
    assert!(!native.properties.contains_key("Direction"));
    assert!(
        matches!(&(regenerated.ir().model.features[0].evaluation.definition()),
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                pattern: admitted_pattern,
                ..
            }) if matches!(admitted_pattern.definition(), PatternTransform::Linear {
                    direction: None,
                    distance: actual_spacing,
                    count: actual_count,
                    second: None,
                    omitted_instances,
                } if omitted_instances.is_empty() && regenerated.ir().dimension_value(actual_count) == Some(12) && regenerated.ir().dimension_value(actual_spacing.dimension()).unwrap().get() == 3.5)
        )
    );
}

#[test]
fn semantic_writer_round_trips_pattern_count_pmi() {
    use cadmpeg_ir::features::ParameterValue;
    use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="MatrizL1" Type="MatrizL" id="132"><Dimension Name="D1">15</Dimension><Dimension Name="D3">2.54</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moLPattern_c", "MatrizL1", 132)]),
    ));
    source.extend(make_block(
        0x0021_0049,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_record(
            "D1@MatrizL1",
            "01234567-89ab-cdef-0123-456789abcdef",
            "",
            15.0,
            "<DIM>",
        ),
    ));
    source.extend(make_block(
        0x0021_004a,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_record(
            "D2@MatrizL1",
            "fedcba98-7654-3210-fedc-ba9876543210",
            "",
            1.0,
            "<DIM>",
        ),
    ));

    let pmi_dates: Vec<_> = crate::test_support::container::scan(&source)
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some("Contents/PMISemanticDataDB"))
        .map(|block| block.type_id)
        .collect();
    assert_eq!(pmi_dates, [0x0021_0049, 0x0021_004a]);

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                &(source),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let parameter_index = decoded
        .ir()
        .model
        .parameters
        .iter()
        .position(|parameter| parameter.name == "D1")
        .expect("pattern count parameter");
    let parameter = &decoded.ir().model.parameters[parameter_index];
    assert_eq!(parameter.value, Some(ParameterValue::Integer(15)));
    let annotation = decoded
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| Some(&annotation.id) == parameter.pmi.as_ref())
        .unwrap();
    assert!(
        matches!(&annotation.definition, PmiDefinition::Dimension(dimension) if dimension.kind() == &DimensionKind::Other("count".into()) && dimension.nominal().and_then(cadmpeg_ir::Dimension::parameter) == Some(&parameter.id))
    );
    let secondary = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("secondary pattern count parameter");
    assert_eq!(secondary.value, Some(ParameterValue::Integer(1)));
    let annotation = decoded
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| Some(&annotation.id) == secondary.pmi.as_ref())
        .unwrap();
    assert!(
        matches!(&annotation.definition, PmiDefinition::Dimension(dimension) if dimension.kind() == &DimensionKind::Other("count".into()) && dimension.nominal().and_then(cadmpeg_ir::Dimension::parameter) == Some(&secondary.id))
    );
    assert!(decoded
        .ir()
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active)
        .all(|configuration| configuration.parameter_values.is_empty()
            && configuration.parameter_overrides.is_empty()
            && configuration.feature_states.is_empty()));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("lack a complete evaluated parameter snapshot")
    }));
    let id = decoded.ir().model.parameters[parameter_index].id.clone();
    decoded
        .ir_mut()
        .set_parameter_expression(&id, "12".into(), Some(ParameterValue::Integer(12)))
        .unwrap();

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D1")
            .and_then(|parameter| parameter.value.as_ref()),
        Some(&ParameterValue::Integer(12))
    );
    assert_eq!(
        sldprt_native(regenerated.ir()).pmi_dimensions[0]
            .value
            .get(),
        12.0
    );
}

#[test]
fn semantic_writer_retains_unresolved_native_pattern_construction() {
    use cadmpeg_ir::features::{patterns::PatternTransform, FeatureDefinition, FeatureOperation};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Unknown pattern" Type="Custom" id="132"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moLPattern_c", "Unknown pattern", 132)]),
    ));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                &(source),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds,
            pattern: admitted_pattern,
        }) if matches!(
            admitted_pattern.definition(),
            PatternTransform::Unresolved {
                form: Some(cadmpeg_ir::features::patterns::PatternForm::Linear)
            }
                if seeds.is_empty()
        )
    ));
    decoded.ir_mut().model.features[0].name = Some("Renamed pattern".into());

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.name, "Renamed pattern");
    assert!(!native.properties.contains_key("Seeds"));
    assert!(!native.properties.contains_key("Direction"));
    assert!(!native.parameters.contains_key("Count"));
    assert!(!native.parameters.contains_key("Spacing"));
    assert!(
        matches!(&(regenerated.ir().model.features[0].evaluation.definition()),
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                pattern: admitted_pattern,
                ..
            }) if matches!(admitted_pattern.definition(), PatternTransform::Unresolved { form: Some(cadmpeg_ir::features::patterns::PatternForm::Linear) })
        )
    );
}

#[test]
fn semantic_writer_round_trips_generic_pattern_type() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Seed" Type="NativeSeed" id="61"/><Pattern Name="Rows" Type="CustomPattern" id="62" PatternType="Linear" Seeds="61" Direction="1,0,0"><Dimension Name="Count">2</Dimension><Dimension Name="Spacing">4mm</Dimension></Pattern></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let owner = ir_edit.model.features[1].id.clone();
        ir_edit
            .set_dimension(
                &owner,
                "/pattern/distance/distance",
                cadmpeg_ir::scalar::Length::new(6.0).unwrap(),
            )
            .unwrap();
        ir_edit
            .set_dimension(&owner, "/pattern/count", 3u32)
            .unwrap();
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[1];
    assert_eq!(feature.kind, "CustomPattern");
    assert_eq!(feature.properties["PatternType"], "Linear");
    assert_eq!(feature.parameters["Spacing"], "6mm");
    assert_eq!(feature.parameters["Count"], "3");
}

mod feature_input;
mod sweep_loft;

mod renames;

mod omissions;

#[test]
fn semantic_writer_patches_resolved_feature_sketch_types() {
    use crate::records::{FeatureInputClassRole, SketchInputKind};

    assert_eq!(
        serde_json::from_str::<SketchInputKind>(r#""curve""#).unwrap(),
        SketchInputKind::LineOrCircle
    );
    assert_eq!(
        serde_json::to_string(&SketchInputKind::LineOrCircle).unwrap(),
        r#""line_or_circle""#
    );

    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0, 1, 2, 3, 9]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_input_lanes.len(), 1);
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.configuration.as_deref(), Some("0"));
    assert_eq!(
        lane.classes
            .iter()
            .map(|class| class.name.as_str())
            .collect::<Vec<_>>(),
        [
            "sgPointHandle",
            "sgLineHandle",
            "sgArcHandle",
            "sgPntPntDist"
        ]
    );
    assert!(lane.classes[..3]
        .iter()
        .all(|class| class.role() == FeatureInputClassRole::SketchEntity));
    assert_eq!(
        lane.classes[3].role(),
        FeatureInputClassRole::SketchConstraint
    );
    assert_eq!(
        lane.names
            .iter()
            .map(|name| name.value.as_str())
            .collect::<Vec<_>>(),
        ["Sketch1", "Boss-Extrude1", "D1"]
    );
    assert_eq!(lane.scalars.len(), 1);
    assert_eq!(lane.scalars[0].name, lane.names[2].id);
    assert_eq!(lane.scalars[0].value.get(), 0.025);
    assert_eq!(lane.scalars[0].object_id, 1);
    assert_eq!(lane.scalars[0].entity_indices(), [0, 2]);
    assert_eq!(lane.references.len(), 2);
    assert_eq!(lane.references[0].object_index, 0);
    assert_eq!(lane.references[1].object_index, 2);
    assert!(lane
        .references
        .iter()
        .all(|reference| reference.kind == crate::records::FeatureInputOperandKind::D6));
    assert_eq!(lane.scalars[0].operands.len(), 2);
    assert_eq!(lane.scalars[0].operands[0].entity_index, 0);
    assert_eq!(lane.scalars[0].operands[1].entity_index, 2);
    assert_eq!(
        lane.scalars[0].operands[0].reference_ref,
        lane.references[0].id
    );
    assert_eq!(
        lane.scalars[0].operands[1].reference_ref,
        lane.references[1].id
    );
    assert!(lane.scalars[0]
        .operands
        .iter()
        .all(|operand| operand.kind == crate::records::FeatureInputOperandKind::D6));
    assert_eq!(lane.relation_bindings.len(), 1);
    assert_eq!(
        lane.relation_bindings[0].family,
        crate::records::FeatureInputRelationFamily::PointPointDistance
    );
    assert_eq!(lane.relation_bindings[0].class_ref, lane.classes[3].id);
    assert_eq!(lane.relation_bindings[0].scalar_ref, lane.scalars[0].id);
    assert_eq!(lane.relation_bindings[0].feature_ref, None);
    assert_eq!(
        lane.scalars[0].role,
        crate::records::FeatureInputScalarRole::Driving
    );
    assert!(
        lane.classes
            .iter()
            .enumerate()
            .all(|(ordinal, class)| class.ordinal
                == u32::try_from(ordinal).expect("ordinal fits u32"))
    );
    assert!(lane
        .sketch_entities
        .windows(2)
        .all(|entities| entities[0].offset() < entities[1].offset()));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.ordinal()
            == u32::try_from(ordinal).expect("ordinal fits u32")));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.local_id()
            == Some(u32::try_from(ordinal).expect("ordinal fits u32") + 1)));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(
            |(ordinal, entity)| entity.state_value.map(cadmpeg_ir::scalar::FiniteReal::get)
                == Some(
                    cadmpeg_core::convert::f64_from_index(ordinal).expect("ordinal is exact") + 1.0
                )
        ));
    let by_ordinal = |ordinal| {
        lane.sketch_entities
            .iter()
            .find(|entity| entity.ordinal() == ordinal)
            .unwrap()
    };
    assert_eq!(by_ordinal(0).kind(), SketchInputKind::Point);
    assert_eq!(
        by_ordinal(1).kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Distance)
    );
    assert_eq!(
        by_ordinal(2).kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Angle)
    );
    assert_eq!(
        by_ordinal(3).kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Radius)
    );
    assert_eq!(
        by_ordinal(4).kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Coincident)
    );
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        let entity = native.feature_input_lanes[0]
            .sketch_entities
            .iter_mut()
            .find(|entity| entity.ordinal() == 1)
            .unwrap();
        entity.reclassify(SketchInputKind::from_native_code(5));
        entity.state_value = cadmpeg_ir::scalar::FiniteReal::new(12.5);
    });

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&encoded);
    assert_eq!(
        scan.blocks
            .iter()
            .filter(|block| block.section.name() == Some("Contents/Config-0-ResolvedFeatures"))
            .count(),
        1
    );
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let entity = &sldprt_native(regenerated.ir()).feature_input_lanes[0].sketch_entities[1];
    assert_eq!(
        entity.kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Vertical)
    );
    assert_eq!(
        entity.state_value.map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(12.5)
    );
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_input_lanes[0]
            .sketch_entities
            .iter()
            .find(|entity| entity.ordinal() == 1)
            .unwrap()
            .kind(),
        SketchInputKind::Relation(crate::records::SketchRelationKind::Vertical)
    );
}
