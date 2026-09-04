// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn semantic_writer_round_trips_all_pattern_forms() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, Length, PatternKind};
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
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x: 1.0, y: 0.0, z: 0.0 }),
                spacing: Length(10.0),
                count: 3,
                second: None,
            },
        } if seeds == &[cadmpeg_ir::features::PatternSeed::Feature(seed.clone())]
    ));
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Circular {
                axis_origin: Point3 { x: 0.0, y: 0.0, z: 0.0 },
                axis_dir: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
                angle: Angle(value),
                count: 4,
            },
            ..
        } if (*value - std::f64::consts::TAU).abs() < 1.0e-12
    ));
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Mirror {
                plane_origin: Point3 {
                    x: 5.0,
                    y: 0.0,
                    z: 0.0
                },
                plane_normal: Vector3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0
                },
            },
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Pattern {
            pattern:
                PatternKind::Linear {
                    direction,
                    spacing,
                    count,
                    second: _,
                },
            ..
        } = &mut ir_edit.model.features[1].definition
        else {
            panic!("linear pattern");
        };
        *direction = Some(Vector3::new(0.0, 1.0, 0.0));
        *spacing = Length(12.0);
        *count = 5;
        let FeatureDefinition::Pattern {
            pattern:
                PatternKind::Circular {
                    axis_origin,
                    angle,
                    count,
                    ..
                },
            ..
        } = &mut ir_edit.model.features[2].definition
        else {
            panic!("circular pattern");
        };
        *axis_origin = Point3::new(1.0, 2.0, 3.0);
        *angle = Angle(std::f64::consts::PI);
        *count = 6;
        let FeatureDefinition::Pattern {
            pattern:
                PatternKind::Mirror {
                    plane_origin,
                    plane_normal,
                },
            ..
        } = &mut ir_edit.model.features[3].definition
        else {
            panic!("mirror pattern");
        };
        *plane_origin = Point3::new(2.0, 0.0, 0.0);
        *plane_normal = Vector3::new(0.0, 1.0, 0.0);
    }

    let mut inconsistent = decoded.ir().clone();
    inconsistent.model.features[1].dependencies.clear();
    let error = crate::test_support::plan_inherited_write(
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
    crate::test_support::plan_inherited_write(
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
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, PatternKind};

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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::CurveDriven {
                path: None,
                spacing: Length(397.6),
                count: 16,
            },
        } if seeds.is_empty()
    ));
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(ParameterValue::Length(Length(397.6)))
    );
    assert_eq!(
        decoded.ir().model.parameters[1].value,
        Some(ParameterValue::Integer(16))
    );

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven { spacing, count, .. },
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("curve-driven pattern");
        };
        *spacing = Length(250.0);
        *count = 8;
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
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
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven {
                path: None,
                spacing: Length(250.0),
                count: 8,
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_sparse_localized_linear_pattern() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, PatternKind};

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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::Linear {
                direction: None,
                spacing: Length(2.54),
                count: 15,
                second: None,
            },
        } if seeds.is_empty()
    ));
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(ParameterValue::Integer(15))
    );
    assert_eq!(
        decoded.ir().model.parameters[1].value,
        Some(ParameterValue::Length(Length(2.54)))
    );

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Pattern {
            pattern: PatternKind::Linear { spacing, count, .. },
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("localized linear pattern");
        };
        *spacing = Length(3.5);
        *count = 12;
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
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
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Linear {
                direction: None,
                spacing: Length(3.5),
                count: 12,
                second: None,
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_pattern_count_pmi() {
    use cadmpeg_ir::features::{ParameterValue, PmiDimensionSubtype};

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
        0x49,
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
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_record(
            "D2@MatrizL1",
            "fedcba98-7654-3210-fedc-ba9876543210",
            "",
            1.0,
            "<DIM>",
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
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
    assert_eq!(
        parameter.pmi.as_ref().map(|pmi| &pmi.subtype),
        Some(&PmiDimensionSubtype::Count)
    );
    let secondary = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("secondary pattern count parameter");
    assert_eq!(secondary.value, Some(ParameterValue::Integer(1)));
    assert_eq!(
        secondary.pmi.as_ref().map(|pmi| &pmi.subtype),
        Some(&PmiDimensionSubtype::Count)
    );
    assert_eq!(
        decoded.ir().model.configurations[0].parameter_values.len(),
        decoded.ir().model.parameters.len()
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("lack a complete evaluated parameter snapshot")
    }));
    decoded.ir_mut().model.parameters[parameter_index].expression = "12".into();
    decoded.ir_mut().model.parameters[parameter_index].value = Some(ParameterValue::Integer(12));

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
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
        sldprt_native(regenerated.ir()).pmi_dimensions[0].value,
        12.0
    );
}

#[test]
fn semantic_writer_retains_unresolved_native_pattern_construction() {
    use cadmpeg_ir::features::{FeatureDefinition, PatternForm, PatternKind};

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
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Linear),
            },
        } if seeds.is_empty()
    ));
    decoded.ir_mut().model.features[0].name = Some("Renamed pattern".into());

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
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
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Linear),
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_generic_pattern_type() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, PatternKind};

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
        let FeatureDefinition::Pattern {
            pattern: PatternKind::Linear { spacing, count, .. },
            ..
        } = &mut ir_edit.model.features[1].definition
        else {
            panic!("generic linear pattern");
        };
        *spacing = Length(6.0);
        *count = 3;
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
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

#[test]
fn semantic_writer_round_trips_typed_sweep() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, PathRef, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="ProfileA" Type="Sketch" id="21"/>
            <Sketch Name="Path" Type="Sketch" id="22"/>
            <Sketch Name="ProfileB" Type="Sketch" id="23"/>
            <Sweep Name="Pipe" Type="Sweep" id="24" Profile="21" Path="22" Operation="NewBody"><Dimension Name="Scale">1.5</Dimension><Dimension Name="Twist">90deg</Dimension></Sweep>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let profile_a = decoded.ir().model.features[0].id.clone();
    let path = decoded.ir().model.features[1].native_ref.clone().unwrap();
    let profile_b = decoded.ir().model.features[2].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Feature(profile)),
            path: Some(PathRef::Native(path_ref)),
            mode: cadmpeg_ir::features::SweepMode::NewBody,
            twist: Some(Angle(twist)),
            scale: Some(1.5),
            ..
        } if profile == &profile_a
            && path_ref == &path
            && (*twist - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Sweep {
            section,
            mode,
            twist,
            scale,
            ..
        } = &mut ir_edit.model.features[3].definition
        else {
            panic!("typed sweep");
        };
        *section =
            cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Feature(profile_b.clone()));
        *mode = cadmpeg_ir::features::SweepMode::Solid {
            op: cadmpeg_ir::features::BooleanKind::Join,
        };
        *twist = Some(Angle(std::f64::consts::PI));
        *scale = Some(2.0);
        ir_edit.model.features[3]
            .dependencies
            .retain(|dependency| dependency != &profile_a);
        ir_edit.model.features[3].dependencies.insert(0, profile_b);
    }

    let mut inconsistent = decoded.ir().clone();
    inconsistent.model.features[3].dependencies.remove(0);
    let error = crate::test_support::plan_inherited_write(
        &inconsistent,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("profile feature is not a preceding dependency"),
        "{error}"
    );

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[3];
    assert_eq!(feature.properties["Profile"], "23");
    assert_eq!(feature.properties["Path"], "22");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.parameters["Scale"], "2");
    assert_eq!(
        feature.parameters["Twist"],
        format!("{}rad", std::f64::consts::PI)
    );
}

#[test]
fn semantic_writer_round_trips_sparse_surface_sweep() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moSweep_c", "Surface-Sweep1", 137)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(_),
            path: None,
            mode: cadmpeg_ir::features::SweepMode::Surface,
            twist: None,
            scale: None,
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Sweep { twist, .. } = &mut ir_edit.model.features[0].definition
        else {
            panic!("surface sweep");
        };
        *twist = Some(Angle(0.5));
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "Surface-Sweep");
    assert_eq!(native.parameters["Twist"], "0.5rad");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("Path"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(_),
            path: None,
            mode: cadmpeg_ir::features::SweepMode::Surface,
            twist: Some(Angle(0.5)),
            scale: None,
            ..
        }
    ));
}

#[test]
fn semantic_writer_retains_native_solid_sweep_with_unresolved_operation() {
    use cadmpeg_ir::features::{FeatureDefinition, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Operacion1" Type="Personalizado" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moSweep_c", "Operacion1", 137)]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(_),
            path: None,
            mode: SweepMode::Unresolved,
            twist: None,
            scale: None,
            ..
        }
    ));
    decoded.ir_mut().model.features[0].name = Some("Renamed sweep".into());

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Unresolved,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_typed_loft() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, PathRef, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="SectionA" Type="Sketch" id="31"/>
            <Sketch Name="SectionB" Type="Sketch" id="32"/>
            <Sketch Name="SectionC" Type="Sketch" id="33"/>
            <Sketch Name="GuideA" Type="Sketch" id="34"/>
            <Sketch Name="GuideB" Type="Sketch" id="36"/>
            <Loft Name="Transition" Type="Loft" id="35" Profiles="31,32,33" Guides="34" Operation="NewBody" Closed="false"/>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native_refs = decoded.ir().model.features[..5]
        .iter()
        .map(|feature| feature.native_ref.clone().unwrap())
        .collect::<Vec<_>>();
    let feature_refs = decoded.ir().model.features[..5]
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    assert!(matches!(
        &decoded.ir().model.features[5].definition,
        FeatureDefinition::Loft {
            sections,
            guides,
            op: BooleanOp::NewBody,
            closed: false,
            ..
        } if sections == &vec![
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(feature_refs[0].clone())),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(feature_refs[1].clone())),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(feature_refs[2].clone())),
        ] && guides == &vec![PathRef::Native(native_refs[3].clone())]
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Loft {
            sections,
            guides,
            op,
            closed,
            ..
        } = &mut ir_edit.model.features[5].definition
        else {
            panic!("typed loft");
        };
        sections.swap(0, 2);
        *guides = vec![PathRef::Native(native_refs[4].clone())];
        *op = BooleanOp::Join;
        *closed = true;
        ir_edit.model.features[5].dependencies = vec![
            feature_refs[2].clone(),
            feature_refs[1].clone(),
            feature_refs[0].clone(),
            feature_refs[4].clone(),
        ];
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[5];
    assert_eq!(feature.properties["Profiles"], "33,32,31");
    assert_eq!(feature.properties["Guides"], "36");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.properties["Closed"], "true");
}

#[test]
fn semantic_writer_retains_unresolved_native_loft_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Loft Name="Unknown loft" Type="Custom" id="151"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Loft {
            ref sections,
            ref guides,
            op: BooleanOp::Unresolved,
            closed: false,
            ..
        } if sections.is_empty() && guides.is_empty()
    ));
    decoded.ir_mut().model.features[0].name = Some("Renamed loft".into());

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert!(!native.properties.contains_key("Profiles"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(!native.properties.contains_key("Closed"));
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Loft {
            op: BooleanOp::Unresolved,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_boundary_boss_as_loft() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="SectionA" Type="Sketch" id="41"/>
            <Sketch Name="SectionB" Type="Sketch" id="42"/>
            <Boundary Name="Blend" Type="BoundaryBoss" id="43" Profiles="41,42"/>
            <Boundary Name="Pocket" Type="BoundaryCut" id="44" Profiles="41,42"/>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let refs = decoded.ir().model.features[..2]
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Loft {
            sections,
            guides,
            op: BooleanOp::Join,
            closed: false,
            ..
        } if sections == &vec![
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(refs[0].clone())),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(refs[1].clone())),
        ] && guides.is_empty()
    ));
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Loft {
            op: BooleanOp::Cut,
            closed: false,
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Loft {
            sections, closed, ..
        } = &mut ir_edit.model.features[2].definition
        else {
            panic!("typed boundary loft");
        };
        sections.reverse();
        *closed = true;
        ir_edit.model.features[2].dependencies.reverse();
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[2];
    assert_eq!(feature.xml_tag, "Boundary");
    assert_eq!(feature.kind, "BoundaryBoss");
    assert_eq!(feature.properties["Profiles"], "42,41");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.properties["Closed"], "true");
    assert!(matches!(
        &regenerated.ir().model.features[3].definition,
        FeatureDefinition::Loft {
            op: BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn semantic_writer_retains_partial_native_rib_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, RibDraft};
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Rib Name="Unknown web" Type="Rib" id="42" Direction="0,1,0"><Dimension Name="Thickness">NaNmm</Dimension><Dimension Name="Draft">NaNrad</Dimension></Rib></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: None,
                direction: Some(Vector3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0
                }),
                thickness: None,
                side: None,
                draft: RibDraft::Unresolved,
            },
            op: BooleanOp::Unresolved,
        }
    ));
    let mut detached = decoded.ir().clone();
    detached.model.features[0].native_ref = None;
    let error = crate::test_support::plan_inherited_write(
        &detached,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("unresolved rib construction"));

    decoded.ir_mut().model.features[0].name = Some("Renamed web".into());
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.name, "Renamed web");
    assert_eq!(native.properties["Direction"], "0,1,0");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("BothSides"));
    assert!(!native.properties.contains_key("Operation"));
    assert_eq!(native.parameters["Thickness"], "NaNmm");
    assert_eq!(native.parameters["Draft"], "NaNrad");
}

#[test]
fn semantic_writer_round_trips_typed_rib() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, FeatureDefinition, Length, ProfileRef, RibDraft, RibSide,
    };
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="RibProfile" Type="Sketch" id="41"/><Rib Name="Web" Type="Rib" id="42" Profile="41" Direction="0,1,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension><Dimension Name="Draft">5deg</Dimension></Rib></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let profile_ref = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(ProfileRef::Feature(profile)),
                direction: Some(Vector3 { x: 0.0, y: 1.0, z: 0.0 }),
                thickness: Some(Length(2.0)),
                side: Some(RibSide::OneSided),
                draft: RibDraft::Angle(Angle(value)),
            },
            op: BooleanOp::Join,
        } if profile == &profile_ref && (*value - 5f64.to_radians()).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Rib { construction, op } = &mut ir_edit.model.features[1].definition
        else {
            panic!("typed rib");
        };
        construction.direction = Some(Vector3::new(1.0, 0.0, 0.0));
        construction.thickness = Some(Length(3.0));
        construction.side = Some(RibSide::Centered);
        construction.draft = RibDraft::None;
        *op = BooleanOp::NewBody;
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[1];
    assert_eq!(feature.properties["Profile"], "41");
    assert_eq!(feature.properties["Direction"], "1,0,0");
    assert_eq!(feature.properties["BothSides"], "true");
    assert_eq!(feature.properties["Operation"], "NewBody");
    assert_eq!(feature.parameters["Thickness"], "3mm");
    assert!(!feature.parameters.contains_key("Draft"));
}

#[test]
fn semantic_writer_preserves_parametric_history() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.points[0].position.z += 1.0;
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert("Depth".into(), "15mm".into());
    });

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    let native = sldprt_native(regenerated.ir());
    let history = &native.feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.configurations[0].name, "Default");
    assert_eq!(history.configurations[0].material.as_deref(), Some("Steel"));
    assert_eq!(history.features.len(), 2);
    assert_eq!(history.features[0].kind, "BossExtrude");
    assert_eq!(history.features[0].parameters["Depth"], "15mm");
    assert_eq!(history.features[1].parent_source_id.as_deref(), Some("7"));
}

#[test]
fn semantic_writer_applies_neutral_feature_edits() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.points[0].position.z += 1.0;
        let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed extrusion feature");
        };
        *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination: cadmpeg_ir::features::LinearTermination::Blind {
                    length: cadmpeg_ir::features::Length(18.0),
                },
                draft: None,
                offset: None,
            },
        };
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "18mm"
    );
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: cadmpeg_ir::features::Length(18.0),
                    },
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_rejects_conflicting_feature_edits() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed extrusion feature");
        };
        *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination: cadmpeg_ir::features::LinearTermination::Blind {
                    length: cadmpeg_ir::features::Length(18.0),
                },
                draft: None,
                offset: None,
            },
        };
        update_sldprt_native(&mut ir_edit, |native| {
            native.feature_histories[0].features[0]
                .parameters
                .insert("Depth".into(), "20mm".into());
        });
    }

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("conflicting neutral and native"));
}

#[test]
fn semantic_writer_accepts_matching_resolved_feature_edits() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed extrusion feature");
        };
        *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination: cadmpeg_ir::features::LinearTermination::Blind {
                    length: cadmpeg_ir::features::Length(50.0),
                },
                draft: None,
                offset: None,
            },
        };
        update_sldprt_native(&mut ir_edit, |native| {
            native.feature_histories[0].part_name = Some("Edited".into());
            let scalar = &mut native.feature_input_lanes[0].scalars[0];
            scalar.value = 0.05;
            let offset = usize::try_from(scalar.offset).unwrap();
            native.feature_input_lanes[0].native_payload[offset..offset + 8]
                .copy_from_slice(&0.05f64.to_le_bytes());
        });
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: cadmpeg_ir::features::Length(50.0),
                    },
                    ..
                }
            },
            ..
        }
    ));
}

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
        .all(|class| class.role == FeatureInputClassRole::SketchEntity));
    assert_eq!(
        lane.classes[3].role,
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
    assert_eq!(lane.scalars[0].value, 0.025);
    assert_eq!(lane.scalars[0].object_id, 1);
    assert_eq!(lane.scalars[0].entity_indices, [0, 2]);
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
    assert!(lane
        .classes
        .iter()
        .enumerate()
        .all(|(ordinal, class)| class.ordinal == ordinal as u32));
    assert!(lane
        .sketch_entities
        .windows(2)
        .all(|entities| entities[0].offset < entities[1].offset));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.ordinal == ordinal as u32));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.local_id == Some(ordinal as u32 + 1)));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.state_value == Some(ordinal as f64 + 1.0)));
    let by_ordinal = |ordinal| {
        lane.sketch_entities
            .iter()
            .find(|entity| entity.ordinal == ordinal)
            .unwrap()
    };
    assert_eq!(by_ordinal(0).kind, SketchInputKind::Point);
    assert_eq!(
        by_ordinal(1).kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Distance)
    );
    assert_eq!(
        by_ordinal(2).kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Angle)
    );
    assert_eq!(
        by_ordinal(3).kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Radius)
    );
    assert_eq!(
        by_ordinal(4).kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Coincident)
    );
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        let entity = native.feature_input_lanes[0]
            .sketch_entities
            .iter_mut()
            .find(|entity| entity.ordinal == 1)
            .unwrap();
        entity.kind = SketchInputKind::Native(5);
        entity.state_value = Some(12.5);
    });

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let scan = container::scan_bytes(&encoded);
    assert_eq!(
        scan.blocks
            .iter()
            .filter(|block| block.section.as_deref() == Some("Contents/Config-0-ResolvedFeatures"))
            .count(),
        1
    );
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let entity = &sldprt_native(regenerated.ir()).feature_input_lanes[0].sketch_entities[1];
    assert_eq!(
        entity.kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Vertical)
    );
    assert_eq!(entity.state_value, Some(12.5));
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_input_lanes[0]
            .sketch_entities
            .iter()
            .find(|entity| entity.ordinal == 1)
            .unwrap()
            .kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Vertical)
    );
}

#[test]
fn semantic_writer_rejects_edited_feature_input_class_index() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].classes[0].name = "sgOtherHandle".into();
    });
    assert!(crate::validate_native(decoded.ir())
        .iter()
        .any(|finding| finding.message.contains("class index does not match")));

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("has edited class declarations"));
}

#[test]
fn semantic_writer_rewrites_feature_input_name_values() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].names[1].value = "Depth".into();
    });
    assert!(crate::validate_native(decoded.ir()).is_empty());

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_input_lanes[0].names[1].value,
        "Depth"
    );
}

#[test]
fn semantic_writer_rejects_edited_feature_input_scalar_index() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].scalars[0].value = 0.050;
    });
    assert!(crate::validate_native(decoded.ir())
        .iter()
        .any(|finding| finding.message.contains("scalar index does not match")));

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("has edited named scalars"));
}

#[test]
fn semantic_writer_updates_linked_resolved_feature_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "D1")
            .expect("projected D1 parameter");
        parameter.expression = "50mm".into();
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(50.0),
        ));
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("regenerated D1 parameter");
    assert_eq!(parameter.expression, "50mm");
    let native_ref = parameter.native_ref.as_deref().expect("linked scalar");
    let native = sldprt_native(regenerated.ir());
    let scalar = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.scalars)
        .find(|scalar| scalar.id == native_ref)
        .expect("regenerated scalar");
    assert_eq!(scalar.value, 0.05);
}

#[test]
fn semantic_writer_updates_resolved_scalar_from_feature_edit() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed extrusion feature");
        };
        *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination: cadmpeg_ir::features::LinearTermination::Blind {
                    length: cadmpeg_ir::features::Length(50.0),
                },
                draft: None,
                offset: None,
            },
        };
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: cadmpeg_ir::features::Length(50.0),
                    },
                    ..
                }
            },
            ..
        }
    ));
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_input_lanes[0].scalars[0].value,
        0.05
    );
}

#[test]
fn semantic_writer_types_resolved_relation_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Sketch1", "D1"]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "D1")
            .expect("projected D1 parameter");
        parameter.expression = "0.5".into();
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Real(0.5));
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("regenerated D1 parameter");
    assert_eq!(parameter.expression, "500mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(500.0)
        ))
    );
    let native_ref = parameter.native_ref.as_deref().expect("linked scalar");
    let native = sldprt_native(regenerated.ir());
    let scalar = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.scalars)
        .find(|scalar| scalar.id == native_ref)
        .expect("regenerated scalar");
    assert_eq!(scalar.value, 0.5);
}
