// SPDX-License-Identifier: Apache-2.0
//! Parameter, equation, and configuration-local scalar decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_projects_every_dimension_as_a_neutral_parameter() {
    use cadmpeg_ir::features::{Angle, DimensionDisplay, Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    let keywords = format!(
        r#"<Keywords><Feature Name="Inputs" Type="EquationDriven" id="16">
            <Dimension Name="Angle">90deg</Dimension>
            <Dimension Name="DisplayAngle">45.00{degree}</Dimension>
            <Dimension Name="Count">4</Dimension>
            <Dimension Name="Diameter">{diameter}2.5</Dimension>
            <Dimension Name="ModifiedDiameter">&lt;MOD-DIAM&gt;3.18</Dimension>
            <Dimension Name="Enabled">true</Dimension>
            <Dimension Name="Expression">D1@Sketch1 * 2</Dimension>
            <Dimension Name="Length">0.5in</Dimension>
            <Dimension Name="Radius">R0.5</Dimension>
            <Dimension Name="Ratio">1.25</Dimension>
        </Feature></Keywords>"#,
        degree = '\u{00b0}',
        diameter = '\u{2300}',
    );
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameters = &decoded.ir().model.parameters;
    assert_eq!(parameters.len(), 10);
    assert_eq!(
        parameters
            .iter()
            .map(|parameter| (parameter.ordinal, parameter.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, "Angle"),
            (1, "DisplayAngle"),
            (2, "Count"),
            (3, "Diameter"),
            (4, "ModifiedDiameter"),
            (5, "Enabled"),
            (6, "Expression"),
            (7, "Length"),
            (8, "Radius"),
            (9, "Ratio"),
        ]
    );
    let value = |name: &str| {
        parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .and_then(|parameter| parameter.value.as_ref())
    };
    assert!(matches!(
        value("Angle"),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
    assert!(matches!(
        value("DisplayAngle"),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
    ));
    assert_eq!(value("Count"), Some(&ParameterValue::Integer(4)));
    assert_eq!(
        value("Diameter"),
        Some(&ParameterValue::Length(Length(2.5)))
    );
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "Diameter")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Diameter)
    );
    assert_eq!(
        value("ModifiedDiameter"),
        Some(&ParameterValue::Length(Length(3.18)))
    );
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "ModifiedDiameter")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Diameter)
    );
    assert_eq!(value("Enabled"), Some(&ParameterValue::Boolean(true)));
    assert_eq!(value("Expression"), None);
    assert_eq!(value("Length"), Some(&ParameterValue::Length(Length(12.7))));
    assert_eq!(value("Radius"), Some(&ParameterValue::Length(Length(0.5))));
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Radius)
    );
    assert_eq!(value("Ratio"), Some(&ParameterValue::Real(1.25)));
    assert!(parameters
        .iter()
        .all(|parameter| parameter.owner.as_ref() == Some(&decoded.ir().model.features[0].id)));

    {
        let mut ir = decoded.ir_mut();
        let radius = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Radius")
            .unwrap();
        radius.expression = "R2".into();
        radius.value = Some(ParameterValue::Length(Length(2.0)));
        let modified_diameter = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "ModifiedDiameter")
            .unwrap();
        modified_diameter.expression = "<MOD-DIAM>4".into();
        modified_diameter.value = Some(ParameterValue::Length(Length(4.0)));
        let display_angle = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "DisplayAngle")
            .unwrap();
        display_angle.expression = format!("30{}", '\u{00b0}');
        display_angle.value = Some(ParameterValue::Angle(Angle(30.0_f64.to_radians())));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native_parameters =
        &sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters;
    assert_eq!(native_parameters["Radius"], "R2");
    assert_eq!(native_parameters["ModifiedDiameter"], "<MOD-DIAM>4");
    assert_eq!(
        native_parameters["DisplayAngle"],
        format!("30{}", '\u{00b0}')
    );
    assert!(matches!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.value.as_ref()),
        Some(ParameterValue::Length(Length(2.0)))
    ));
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Radius)
    );
    assert!(matches!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "DisplayAngle")
            .and_then(|parameter| parameter.value.as_ref()),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "ModifiedDiameter")
            .map(|parameter| (parameter.display, parameter.value.as_ref())),
        Some((
            Some(DimensionDisplay::Diameter),
            Some(&ParameterValue::Length(Length(4.0)))
        ))
    );
}

#[test]
fn parameter_references_distinguish_reserved_expression_syntax() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="sin">1</Dimension><Dimension Name="pi">2</Dimension><Dimension Name="iif">3</Dimension><Dimension Name="Width">4mm</Dimension><Dimension Name="Driven">sin(30deg) + pi + iif(Width = 4mm, 1, 2) + &quot;sin&quot; + &quot;pi&quot; + &quot;iif&quot;</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter_id = |name: &str| {
        decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
            .id
            .clone()
    };
    let expected_dependencies = vec![
        parameter_id("Width"),
        parameter_id("sin"),
        parameter_id("pi"),
        parameter_id("iif"),
    ];
    assert_eq!(
        decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Driven")
            .unwrap()
            .dependencies,
        expected_dependencies
    );

    for (old_name, new_name) in [
        ("sin", "Sine input"),
        ("pi", "Pi input"),
        ("iif", "Choice input"),
    ] {
        decoded
            .ir_mut()
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == old_name)
            .unwrap()
            .name = new_name.into();
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let driven = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Driven")
        .unwrap();
    assert_eq!(
        driven.expression,
        "sin(30deg) + pi + iif(Width = 4mm, 1, 2) + \"Sine input\" + \"Pi input\" + \"Choice input\""
    );
    assert_eq!(driven.dependencies.len(), 4);
}

#[test]
fn decode_evaluates_parameter_dependency_expressions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Width">4mm</Dimension><Dimension Name="Copies">3</Dimension><Dimension Name="Double width">Width * 2</Dimension><Dimension Name="Per copy">&quot;Double width&quot; / Copies</Dimension><Dimension Name="Forward">Later + 1mm</Dimension><Dimension Name="Later">2mm</Dimension><Dimension Name="Scientific">1e-3 * Width</Dimension><Dimension Name="Mixed units">1ft + 1in + 1mil + 1uin + 1um + 1nm + 1&#197;</Dimension><Dimension Name="Power">2^3^2</Dimension><Dimension Name="Sine">sin(30deg)</Dimension><Dimension Name="Inverse sine">arcsin(0.5)</Dimension><Dimension Name="Absolute">abs(-2mm)</Dimension><Dimension Name="Root">sqr(9)</Dimension><Dimension Name="Sign negative">sgn(-2)</Dimension><Dimension Name="Sign zero">sgn(0)</Dimension><Dimension Name="Sign positive">sgn(2)</Dimension><Dimension Name="Pi">pi</Dimension><Dimension Name="Conditional">iif(Width >= 4mm, Width * 2, 1mm)</Dimension><Dimension Name="Leading equals">=iif(Copies&lt;&gt;3, 1, 2)</Dimension><Dimension Name="Comparison">Width = 4mm</Dimension><Dimension Name="Invalid">Width + Copies</Dimension><Dimension Name="Invalid area">Width^2</Dimension><Dimension Name="Invalid branches">iif(true, Width, Copies)</Dimension><Dimension Name="Invalid nested domain">sgn(arcsin(2))</Dimension></Feature></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let values = decoded
        .ir()
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.value.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        values["Double width"],
        Some(ParameterValue::Length(Length(8.0)))
    );
    assert_eq!(
        values["Per copy"],
        Some(ParameterValue::Length(Length(8.0 / 3.0)))
    );
    assert_eq!(values["Forward"], Some(ParameterValue::Length(Length(3.0))));
    assert_eq!(
        values["Scientific"],
        Some(ParameterValue::Length(Length(0.004)))
    );
    assert_eq!(
        values["Mixed units"],
        Some(ParameterValue::Length(Length(
            304.8 + 25.4 + 0.0254 + 25.4e-6 + 1.0e-3 + 1.0e-6 + 1.0e-7
        )))
    );
    assert_eq!(values["Power"], Some(ParameterValue::Integer(512)));
    assert!(
        matches!(values["Sine"], Some(ParameterValue::Real(value)) if (value - 0.5).abs() < 1e-12)
    );
    assert!(matches!(
        values["Inverse sine"],
        Some(ParameterValue::Angle(cadmpeg_ir::features::Angle(value)))
            if (value - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
    assert_eq!(
        values["Absolute"],
        Some(ParameterValue::Length(Length(2.0)))
    );
    assert_eq!(values["Root"], Some(ParameterValue::Real(3.0)));
    assert_eq!(values["Sign negative"], Some(ParameterValue::Integer(-1)));
    assert_eq!(values["Sign zero"], Some(ParameterValue::Integer(0)));
    assert_eq!(values["Sign positive"], Some(ParameterValue::Integer(1)));
    assert_eq!(
        values["Pi"],
        Some(ParameterValue::Real(std::f64::consts::PI))
    );
    assert_eq!(
        values["Conditional"],
        Some(ParameterValue::Length(Length(8.0)))
    );
    assert_eq!(values["Leading equals"], Some(ParameterValue::Integer(2)));
    assert_eq!(values["Comparison"], Some(ParameterValue::Boolean(true)));
    assert_eq!(values["Invalid"], None);
    assert_eq!(values["Invalid area"], None);
    assert_eq!(values["Invalid branches"], None);
    assert_eq!(values["Invalid nested domain"], None);
    let ordinal = |name: &str| {
        decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
            .ordinal
    };
    assert!(ordinal("Later") < ordinal("Forward"));
    assert!(!cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("parameter dependency")));
}

#[test]
fn decode_projects_evaluated_equations_into_feature_semantics() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Equation boss" Type="BossExtrude" id="7" Operation="Join" EndCondition="Blind"><Dimension Name="Base">4mm</Dimension><Dimension Name="Depth">Base * 2</Dimension></Extrusion></Keywords>"#,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(8.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }
    ));
    let depth = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    assert_eq!(depth.expression, "Base * 2");
    assert_eq!(
        depth.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(Length(8.0)))
    );
    let native = &sldprt_native(decoded.ir()).feature_histories[0].features[0];
    assert_eq!(native.parameters["Depth"], "Base * 2");

    decoded.ir_mut().model.features[0].name = Some("Renamed equation boss".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "Base * 2"
    );
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(8.0)
                    },
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn equations_container_projects_a_typed_tree_node_owning_global_parameters() {
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureTreeNodeRole, Length, ParameterValue,
        Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Width">4mm</Dimension></Feature><Extrusion Name="Equation boss" Type="BossExtrude" id="8" Operation="Join" EndCondition="Blind"><Dimension Name="Depth">Width * 2</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let equations = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Equations"))
        .expect("equations node");
    assert!(matches!(
        equations.definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    let width = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .expect("width parameter");
    assert_eq!(width.owner.as_ref(), Some(&equations.id));
    let depth = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    assert_eq!(depth.dependencies, vec![width.id.clone()]);
    assert_eq!(depth.value, Some(ParameterValue::Length(Length(8.0))));

    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("Equation boss"))
        .expect("extrusion");
    {
        let mut ir = decoded.ir_mut();
        ir.model.features[extrusion].name = Some("Renamed equation boss".into());
        let FeatureDefinition::Extrude { extent, .. } =
            &mut ir.model.features[extrusion].definition
        else {
            panic!("typed extrusion");
        };
        *extent = ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(12.0),
                },
                draft: None,
                offset: None,
            },
        };
        let depth = ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Depth")
            .expect("depth parameter");
        depth.expression = "Width * 3".into();
        depth.value = Some(ParameterValue::Length(Length(12.0)));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let equations = regenerated
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Equations"))
        .expect("equations node");
    assert!(matches!(
        equations.definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    let depth = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    assert_eq!(depth.expression, "Width * 3");
    assert_eq!(depth.value, Some(ParameterValue::Length(Length(12.0))));
    assert_eq!(depth.dependencies.len(), 1);
    let extrusion = regenerated
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Renamed equation boss"))
        .expect("extrusion");
    assert!(matches!(
        extrusion.definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(12.0)
                    },
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn feature_rename_rewrites_only_its_qualified_parameter_references() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="D1">2mm</Dimension></Feature><Feature Name="Sketch2" Type="Sketch" id="11"><Dimension Name="D1">3mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="12"><Dimension Name="Result">D1@Sketch1 + D1@Sketch2</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap()
        .name = Some("Profile".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let result = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "D1@Profile + D1@Sketch2");
    assert_eq!(result.dependencies.len(), 2);
}

#[test]
fn decode_applies_owned_feature_units_to_resolved_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round1" Type="Fillet"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Round1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .expect("projected fillet feature");
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected D1 parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_preserves_configuration_local_parameter_values() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, RadiusSpec};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Large"/><Fillet Name="Round1" Type="Fillet"><Dimension Name="D1">30mm</Dimension><Dimension Name="D2">D1 * 2</Dimension></Fillet></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names_relation_and_scalar(
            &[0],
            &["Round1", "D1"],
            "sgPntPntDist",
            0.025,
        ),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_features_payload_with_names_relation_and_scalar(
            &[0],
            &["Round1", "D1"],
            "sgPntPntDist",
            0.050,
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("parameter expression(s) cannot regenerate")));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    let dependent = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .unwrap();
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(30.0))));
    assert_eq!(parameter.native_ref, None);
    assert_eq!(
        decoded.ir().model.configurations[0]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(25.0)))
    );
    assert_eq!(
        decoded.ir().model.configurations[1]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );
    assert_eq!(
        decoded.ir().model.configurations[0]
            .parameter_values
            .get(&dependent.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );
    assert_eq!(
        decoded.ir().model.configurations[1]
            .parameter_values
            .get(&dependent.id),
        Some(&ParameterValue::Length(Length(100.0)))
    );
    let round_trip =
        cadmpeg_ir::CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.configurations[1]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );

    let parameter_id = parameter.id.clone();
    let dependent_id = dependent.id.clone();
    let feature_id = parameter.owner.clone();
    let mut incoherent = decoded.ir().clone();
    incoherent.model.configurations[1]
        .parameter_values
        .insert(parameter_id.clone(), ParameterValue::Length(Length(75.0)));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &incoherent,
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("configuration parameter values are inconsistent with their expressions"),
        "unexpected error: {error}"
    );

    let mut edited = decoded.ir().clone();
    edited.model.configurations[1]
        .parameter_values
        .insert(parameter_id.clone(), ParameterValue::Length(Length(75.0)));
    edited.model.configurations[1]
        .parameter_values
        .insert(dependent_id, ParameterValue::Length(Length(150.0)));
    let FeatureDefinition::Fillet { groups, .. } = &mut edited.model.configurations[1]
        .feature_states
        .get_mut(feature_id.as_ref().expect("feature-owned parameter"))
        .unwrap()
        .definition
    else {
        panic!("configuration fillet state");
    };
    groups[0].radius = RadiusSpec::Constant {
        radius: Length(75.0),
    };

    let mut conflicting = edited.clone();
    update_sldprt_native(&mut conflicting, |native| {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.configuration.as_deref() == Some("1"))
            .unwrap();
        let scalar = &mut lane.scalars[0];
        scalar.value = 0.060;
        let offset = usize::try_from(scalar.offset).unwrap();
        lane.native_payload[offset..offset + 8].copy_from_slice(&0.060f64.to_le_bytes());
    });
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &conflicting,
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT configuration design-state edits"));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let regenerated_parameter = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    let regenerated_feature = regenerated
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .unwrap();
    assert_eq!(
        regenerated.ir().model.configurations[1]
            .parameter_values
            .get(&regenerated_parameter.id),
        Some(&ParameterValue::Length(Length(75.0)))
    );
    assert!(matches!(
        regenerated.ir().model.configurations[1].feature_states[&regenerated_feature.id].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: Length(75.0)
            },
            ..
        }])
    ));
}

#[test]
fn decode_separates_document_expression_from_evaluated_feature_scalar() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ParameterValue,
        Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="42"><Dimension Name="D1">2.5</Dimension></Extrusion></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Boss", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .expect("projected extrusion");
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(25.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }
    ));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected D1 parameter");
    assert_eq!(parameter.expression, "2.5");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(25.0))));
    assert!(parameter.native_ref.is_some());
}
