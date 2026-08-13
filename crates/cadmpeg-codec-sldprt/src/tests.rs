// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.sldprt` byte-fixture tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn source_record_join_borrows_the_retained_source_image() {
    let payload = vec![0x5a; 4096];
    let payload_ptr = payload.as_ptr();
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    fidelity.retained_records = vec![cadmpeg_ir::source_fidelity::RetainedSourceRecord {
        id: "sldprt:file:source-image#0".into(),
        stream: "source".into(),
        offset: 0,
        byte_len: payload.len() as u64,
        sha256: cadmpeg_ir::hash::sha256_hex(&payload),
        data: Some(payload),
    }];

    let records = crate::source_records(&cadmpeg_ir::examples::unit_cube(), &fidelity).unwrap();
    let retained = records[0].data.expect("retained source bytes");
    assert_eq!(retained.as_ptr(), payload_ptr);
}

#[test]
fn decode_extracts_parametric_history() {
    let f = sldprt_with_body_and_history(&triangle_body());
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(result.ir());
    let history = &native.feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.configurations[0].material.as_deref(), Some("Steel"));
    assert_eq!(result.ir().model.configurations.len(), 1);
    assert_eq!(result.ir().model.configurations[0].name, "Default");
    assert_eq!(
        result.ir().model.configurations[0].material.as_deref(),
        Some("Steel")
    );
    assert_eq!(
        result.ir().model.configurations[0].native_ref.as_deref(),
        Some(history.configurations[0].id.as_str())
    );
    assert_eq!(history.features[0].kind, "BossExtrude");
    assert_eq!(history.features[0].xml_tag, "Extrusion");
    assert_eq!(history.features[0].parameters["Depth"], "12.5mm");
    assert_eq!(history.features[0].properties["Scope"], "Body1");
    assert_eq!(history.features[1].parent_source_id.as_deref(), Some("7"));
    assert_eq!(history.features[1].xml_tag, "EquationDrivenCurve");
    assert_eq!(result.ir().model.features.len(), 2);
    let neutral = &result.ir().model.features[0];
    assert_eq!(neutral.name.as_deref(), Some("Boss"));
    assert_eq!(
        neutral.native_ref.as_deref(),
        Some(history.features[0].id.as_str())
    );
    assert!(matches!(
        &neutral.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(profile),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(12.5),
                    },
                    draft: None,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        } if profile == &history.features[0].id
    ));
    assert_eq!(
        result.ir().model.features[1].parent.as_ref(),
        Some(&neutral.id)
    );
}

#[test]
fn decode_uses_plain_numeric_config_as_legacy_feature_input_lane() {
    let legacy = resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Config-7", &legacy));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let lanes = &sldprt_native(decoded.ir()).feature_input_lanes;
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].configuration.as_deref(), Some("7"));
    assert_eq!(lanes[0].native_payload, legacy);
}

#[test]
fn decode_prefers_explicit_feature_input_lanes_over_plain_config_streams() {
    let legacy = resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]);
    let explicit = resolved_feature_classes_with_ids(&[("Chamfer_c", "Bevel", 42)]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Config-7", &legacy));
    source.extend(make_block(
        0x42,
        "Contents/Config-7-ResolvedFeatures",
        &explicit,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let lanes = &sldprt_native(decoded.ir()).feature_input_lanes;
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].configuration.as_deref(), Some("7"));
    assert_eq!(lanes[0].native_payload, explicit);
}

#[test]
fn decode_types_non_modeling_feature_tree_nodes() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureTreeNodeRole};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Annotations" Type="Annotations" id="101"/>
            <Feature Name="Ecuaciones" Type="Ecuaciones" id="102"/>
            <Feature Name="Bodies" Type="Solid Bodies" id="103"/>
            <Feature Name="Light" Type="Direccional" id="104"/>
            <Feature Name="Unknown" Type="CustomOperation" id="105"/>
            <Sketch Name="Origen" Type="Croquis localizado" id="106"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moDetailCabinet_c", "Annotations", 101),
            ("moEqnFolder_c", "Ecuaciones", 102),
            ("moSolidBodyFolder_c", "Bodies", 103),
            ("moOriginProfileFeature_c", "Origen", 106),
        ]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let definitions = decoded
        .ir()
        .model
        .features
        .iter()
        .map(|feature| &feature.definition)
        .collect::<Vec<_>>();
    assert!(matches!(
        definitions[0],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
    assert!(matches!(
        definitions[1],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    assert!(matches!(
        definitions[2],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::SolidBodies,
            ..
        }
    ));
    assert!(matches!(definitions[3], FeatureDefinition::Native { .. }));
    assert!(matches!(definitions[4], FeatureDefinition::Native { .. }));
    assert!(matches!(
        definitions[5],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::ModelOrigin,
            ..
        }
    ));
    assert!(!decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. })));
    decoded.ir_mut().model.features[0].name = Some("Document annotations".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: decoded.ir(),
            fidelity: Some(decoded.source_fidelity()),
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
}

#[test]
fn decode_leaves_position_allocated_tree_nodes_untyped() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Luces y camaras" Type="Localized" id="6"/>
            <Feature Name="Ambiental" Type="Localized" id="12"/>
            <Feature Name="Direccional uno" Type="Localized" id="13"/>
            <Feature Name="Direccional dos" Type="Localized" id="14"/>
            <Feature Name="Direccional tres" Type="Localized" id="15"/>
            <Feature Name="Vistas" Type="Localized" id="19"/>
        </Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .all(|feature| matches!(feature.definition, FeatureDefinition::Native { .. })));
}

#[test]
fn reserved_tree_node_ids_require_builtin_record_shape() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, FeatureDefinition, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Operation" Type="Localized" id="12"/>
            <Feature Name="Attributed" Type="Localized" id="19" State="custom"/>
        </Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::Native { .. }
    ));
}

#[test]
fn decode_binds_duplicate_feature_names_by_native_object_id() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureTreeNodeRole};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Folder" Type="Custom" id="41"/>
            <Feature Name="Folder" Type="Custom" id="42"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moEqnFolder_c", "Folder", 41),
            ("moSolidBodyFolder_c", "Folder", 42),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::SolidBodies,
            ..
        }
    ));
}

#[test]
fn decode_propagates_unique_object_class_by_serialized_type_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Redondeo1" Type="Redondeo" id="41"/>
            <Feature Name="Redondeo2" Type="Redondeo" id="42"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("Fillet_c", "Redondeo2", 42)]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert!(native.feature_histories[0]
        .features
        .iter()
        .all(|feature| feature.input_class.as_deref() == Some("Fillet_c")));
}

#[test]
fn decode_binds_repeated_instances_by_class_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="LocalizedFillet" id="41"/>
            <Feature Name="TokenSeed" Type="LocalizedFillet" id="42"/>
            <Feature Name="TokenOnly" Type="OpaqueType" id="43"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[("Fillet_c", "Seed", 41)]);
    for (name, object_id) in [("TokenSeed", 42u32), ("TokenOnly", 43)] {
        payload.extend_from_slice(&0x37a5u16.to_le_bytes());
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert!(native.feature_histories[0]
        .features
        .iter()
        .all(|feature| feature.input_class.as_deref() == Some("Fillet_c")));
}

#[test]
fn decode_does_not_propagate_ambiguous_object_class_by_type_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="First" Type="Custom" id="41"/>
            <Feature Name="Second" Type="Custom" id="42"/>
            <Feature Name="Third" Type="Custom" id="43"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("Fillet_c", "First", 41),
            ("moRefPlane_c", "Second", 42),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_histories[0].features[2].input_class, None);
}

#[test]
fn decode_does_not_bind_ambiguous_repeated_class_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="FilletSeed" Type="FilletType" id="41"/>
            <Feature Name="PlaneSeed" Type="PlaneType" id="42"/>
            <Feature Name="FilletToken" Type="FilletType" id="43"/>
            <Feature Name="PlaneToken" Type="PlaneType" id="44"/>
            <Feature Name="Unknown" Type="UnknownType" id="45"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[
        ("Fillet_c", "FilletSeed", 41),
        ("moRefPlane_c", "PlaneSeed", 42),
    ]);
    for (name, object_id) in [("FilletToken", 43u32), ("PlaneToken", 44), ("Unknown", 45)] {
        payload.extend_from_slice(&0x37a5u16.to_le_bytes());
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_histories[0].features[4].input_class, None);
}

#[test]
fn decode_does_not_bind_object_class_by_display_name() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plane" Type="Custom" id="41"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plane", 42)]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Native { .. }
    ));
    assert_eq!(
        sldprt_native(decoded.ir()).feature_histories[0].features[0].input_class,
        None
    );
}

#[test]
fn keywords_root_id_does_not_create_feature_parentage() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords id="document"><Feature Name="Root" Type="Folder" id="1"><Feature Name="Nested" Type="Custom" id="2"/></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let history = &native.feature_histories[0];
    assert_eq!(history.properties["id"], "document");
    assert_eq!(history.features[0].parent_source_id, None);
    assert_eq!(history.features[1].parent_source_id.as_deref(), Some("1"));
    assert!(crate::validate_native(decoded.ir()).is_empty());
}

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

    let radius = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Radius")
        .unwrap();
    radius.expression = "R2".into();
    radius.value = Some(ParameterValue::Length(Length(2.0)));
    let modified_diameter = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "ModifiedDiameter")
        .unwrap();
    modified_diameter.expression = "<MOD-DIAM>4".into();
    modified_diameter.value = Some(ParameterValue::Length(Length(4.0)));
    let display_angle = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "DisplayAngle")
        .unwrap();
    display_angle.expression = format!("30{}", '\u{00b0}');
    display_angle.value = Some(ParameterValue::Angle(Angle(30.0_f64.to_radians())));

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
    decoded.ir_mut().model.features[extrusion].name = Some("Renamed equation boss".into());
    let FeatureDefinition::Extrude { extent, .. } =
        &mut decoded.ir_mut().model.features[extrusion].definition
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
    let depth = decoded
        .ir_mut()
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    depth.expression = "Width * 3".into();
    depth.value = Some(ParameterValue::Length(Length(12.0)));

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
fn decode_projects_cut_extrude_with_canonical_length() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Cut" Type="CutExtrude" id="9"><Dimension Name="Depth">0.5in</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(12.7),
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn decode_projects_compact_extrusion_with_unresolved_extent() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, ProfileRef, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Compact" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));

    decoded.ir_mut().model.features[0].name = Some("Renamed compact extrusion".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_extrusion_termination() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, FeatureDefinition, Termination};

    fn compact_extrusion_payload(through_all: bool) -> Vec<u8> {
        let mut payload = resolved_feature_classes_with_ids(&[("moExtrusion_c", "Boss", 9)]);
        let offset = payload.len();
        payload.resize(offset + 104, 0);
        if through_all {
            payload[offset..offset + 2].copy_from_slice(&[0x0c, 0x8e]);
            payload[offset + 4] = 1;
            payload[offset + 18] = 1;
            payload[offset + 30..offset + 34].copy_from_slice(&[1, 0, 0, 1]);
            payload[offset + 92] = 1;
        }
        payload
    }

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Blind"/><Extrusion Name="Boss" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &compact_extrusion_payload(true),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-1-ResolvedFeatures",
        &compact_extrusion_payload(false),
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
        .unwrap();
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
    let feature_id = feature.id.clone();
    assert!(matches!(
        decoded.ir().model.configurations[0]
            .feature_states
            .get(&feature_id)
            .map(|state| &state.definition),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ThroughAll,
                    ..
                }
            },
            ..
        })
    ));
    assert!(matches!(
        decoded.ir().model.configurations[1]
            .feature_states
            .get(&feature_id)
            .map(|state| &state.definition),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        })
    ));
    assert!(decoded
        .ir()
        .model
        .configurations
        .iter()
        .all(
            |configuration| configuration.feature_states.len() == decoded.ir().model.features.len()
        ));
    let round_trip =
        cadmpeg_ir::CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.configurations[0]
            .feature_states
            .get(&feature_id),
        decoded.ir().model.configurations[0]
            .feature_states
            .get(&feature_id)
    );

    let mut edited = decoded.ir().clone();
    let replacement = edited.model.configurations[0].feature_states[&feature_id].clone();
    edited.model.configurations[1]
        .feature_states
        .insert(feature_id, replacement);
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut Vec::new())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("configuration design-state edit has no complete native lane encoding"),
        "unexpected error: {error}"
    );
}

#[test]
fn decode_binds_adjacent_profile_feature_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="Sketch" id="8"/>
            <Extrusion Name="Boss" Type="Extrusion" id="9"/>
            <Sketch Name="Following" Type="Sketch" id="10"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile", 8),
            ("moExtrusion_c", "Boss", 9),
            ("moProfileFeature_c", "Following", 10),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile"))
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &profile.id
    ));
    assert_eq!(extrusion.dependencies, vec![profile.id.clone()]);
}

#[test]
fn decode_does_not_globalize_configuration_local_adjacent_profile() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Sketch Name="Profile A" Type="Sketch" id="7"/><Sketch Name="Profile B" Type="Sketch" id="8"/><Extrusion Name="Boss" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile A", 7),
            ("moExtrusion_c", "Boss", 9),
        ]),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile B", 8),
            ("moExtrusion_c", "Boss", 9),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(owner),
            ..
        } if owner == extrusion.native_ref.as_deref().unwrap()
    ));
    let extrusion_id = extrusion.id.clone();
    let profile_a = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile A"))
        .unwrap();
    let profile_b = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile B"))
        .unwrap();
    let state_a = &decoded.ir().model.configurations[0].feature_states[&extrusion_id];
    let state_b = &decoded.ir().model.configurations[1].feature_states[&extrusion_id];
    assert!(matches!(
        &state_a.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile),
            ..
        } if profile == &profile_a.id
    ));
    assert!(matches!(
        &state_b.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile),
            ..
        } if profile == &profile_b.id
    ));
    assert_eq!(state_a.dependencies, vec![profile_a.id.clone()]);
    assert_eq!(state_b.dependencies, vec![profile_b.id.clone()]);
}

#[test]
fn decode_binds_following_profile_marked_as_dissected_child() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Previous" Type="Sketch" id="7"/>
            <Extrusion Name="Boss" Type="Extrusion" id="9"/>
            <Sketch Name="Profile&lt;3&gt;" Type="Sketch" id="8" Description="Profile&lt;3&gt;"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Previous", 7),
            ("moICE_c", "Boss", 9),
            ("moProfileFeature_c", "Profile<3>", 8),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile<3>"))
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &profile.id
    ));
    assert_eq!(extrusion.dependencies, vec![profile.id.clone()]);
}

#[test]
fn decode_binds_profile_to_inline_extrusion_with_ambiguous_class_token() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="Sketch" id="8"/>
            <Extrusion Name="Cut" Type="Localized" id="9"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[("moProfileFeature_c", "Profile", 8)]);
    payload.extend_from_slice(&0x84c5u16.to_le_bytes());
    payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, 3]);
    for unit in "Cut".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(&[0; 4]);
    payload.extend_from_slice(&[0xca, 1, 2, 0x40]);
    payload.extend_from_slice(&9u32.to_le_bytes());
    payload.extend_from_slice(&[0; 4]);
    payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile"))
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Cut"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            op: BooleanOp::Cut,
            ..
        } if feature == &profile.id
    ));
}

#[test]
fn decode_resolves_feature_topology_selections() {
    use cadmpeg_ir::features::{
        BodySelection, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition,
        PathRef, ProfileRef, Termination,
    };

    // Two bodies so the combine has disjoint operands: a body cannot be both
    // the target and the tool of its own boolean.
    let mut body_bytes = Vec::new();
    body_bytes.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body_bytes.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body_bytes.extend(owned_triangle(0, 700, 0.0));
    body_bytes.extend(owned_triangle(200, 701, 10.0));
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body_bytes)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(base.ir().model.bodies.len(), 2);
    let body = &base.ir().model.bodies[0].id.0;
    let tool_body = &base.ir().model.bodies[1].id.0;
    let face = &base.ir().model.faces[0].id.0;
    let edge = &base.ir().model.edges[0].id.0;
    let keywords = format!(
        r#"<Keywords>
            <Fillet Name="Round" Type="Fillet" id="1" Edges="{edge}"><Dimension Name="Radius">1mm</Dimension></Fillet>
            <DeleteFace Name="Delete" Type="DeleteFace" id="2" Faces="{face}" Heal="true"/>
            <Combine Name="Union" Type="Combine" id="3" Target="{body}" Tools="{tool_body}" Operation="Join"/>
            <Extrusion Name="UpTo" Type="BossExtrude" id="4" Profile="{face}" EndCondition="ToFace" Face="{face}" Operation="Join"/>
            <Hole Name="Drill" Type="Hole" id="5" Face="{face}" EndCondition="ThroughAll"><Dimension Name="Diameter">2mm</Dimension></Hole>
            <Sweep Name="Rail" Type="Sweep" id="6" Profile="{face}" Path="{edge}" Operation="NewBody"/>
        </Keywords>"#
    );
    let mut source = sldprt_with_body(&body_bytes);
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    let body_id = decoded.ir().model.bodies[0].id.clone();
    let tool_body_id = decoded.ir().model.bodies[1].id.clone();

    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Resolved { edges, native }, ..
        }] if edges == &[base.ir().model.edges[0].id.clone()] && native == edge)
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Resolved { faces, native },
            ..
        } if faces == &[base.ir().model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Resolved { bodies, native },
            tools: BodySelection::Resolved { .. },
            ..
        } if bodies == &[base.ir().model.bodies[0].id.clone()] && native == body
    ));
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Faces(profile_faces),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToFace {
                        face: FaceSelection::Resolved { faces, native },
                        ..
                    },
                    ..
                }
            },
            ..
        } if profile_faces == &[base.ir().model.faces[0].id.clone()]
            && faces == &[base.ir().model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir().model.features[4].definition,
        FeatureDefinition::Hole {
            face: Some(FaceSelection::Resolved { faces, native }),
            ..
        } if faces == &[base.ir().model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir().model.features[5].definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Faces(faces)),
            path: Some(PathRef::Edges(edges)),
            ..
        } if faces == std::slice::from_ref(&face_id) && edges == std::slice::from_ref(&edge_id)
    ));

    if let FeatureDefinition::Fillet { groups } = &mut decoded.ir_mut().model.features[0].definition
    {
        groups[0].edges = EdgeSelection::Edges(vec![edge_id.clone()]);
    }
    if let FeatureDefinition::DeleteFace { faces, .. } =
        &mut decoded.ir_mut().model.features[1].definition
    {
        *faces = FaceSelection::Faces(vec![face_id.clone()]);
    }
    if let FeatureDefinition::Combine { target, tools, .. } =
        &mut decoded.ir_mut().model.features[2].definition
    {
        *target = BodySelection::Bodies(vec![body_id.clone()]);
        *tools = BodySelection::Bodies(vec![tool_body_id.clone()]);
    }
    if let FeatureDefinition::Extrude {
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: Termination::ToFace { face, .. },
                        ..
                    },
            },
        ..
    } = &mut decoded.ir_mut().model.features[3].definition
    {
        *face = FaceSelection::Faces(vec![face_id.clone()]);
    }
    if let FeatureDefinition::Hole { face, .. } = &mut decoded.ir_mut().model.features[4].definition
    {
        *face = Some(FaceSelection::Faces(vec![face_id.clone()]));
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let records = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(records[0].properties["Edges"], edge_id.0);
    assert_eq!(records[1].properties["Faces"], face_id.0);
    assert_eq!(records[2].properties["Target"], body_id.0);
    assert_eq!(records[2].properties["Tools"], tool_body_id.0);
    assert_eq!(records[3].properties["Face"], face_id.0);
    assert_eq!(records[3].properties["Profile"], face_id.0);
    assert_eq!(records[4].properties["Face"], face_id.0);
    assert_eq!(records[5].properties["Profile"], face_id.0);
    assert_eq!(records[5].properties["Path"], edge_id.0);
}

#[test]
fn decode_reports_unresolved_feature_output_scope() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Scoped" Type="Custom" id="1" Scope="MissingBody"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.ir().model.features[0].outputs.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 feature(s) retain non-empty native output scopes that do not resolve to model bodies."
    }));
}

#[test]
fn decode_projects_generic_extrusion_with_explicit_operation() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Generic" Type="Extrusion" id="10" Operation="NewBody"><Dimension Name="Depth">6mm</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(6.0),
                    },
                    ..
                }
            },
            op: BooleanOp::NewBody,
            ..
        }
    ));
}

#[test]
fn decode_dispatches_typed_features_by_xml_family() {
    use cadmpeg_ir::features::{ChamferSpec, FeatureDefinition, HoleKind, Length, RadiusSpec};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="CustomSketch" id="51"/>
            <ReferencePoint Name="Origin" Type="CustomDatum" id="52" Position="1mm,2mm,3mm"/>
            <Fillet Name="Round" Type="CustomFillet" id="53" Dependencies="51,52,51" Algorithm="RollingBall"><Dimension Name="Radius">2mm</Dimension></Fillet>
            <Chamfer Name="Bevel" Type="CustomChamfer" id="54"><Dimension Name="Distance">3mm</Dimension></Chamfer>
            <Hole Name="Drill" Type="CustomHole" id="55"><Dimension Name="Diameter">4mm</Dimension><Dimension Name="Depth">5mm</Dimension></Hole>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sketch { .. }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::DatumPoint { .. }
    ));
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: Length(2.0),
            },
            ..
        }])
    ));
    assert_eq!(
        decoded.ir().model.features[2].dependencies,
        vec![
            decoded.ir().model.features[0].id.clone(),
            decoded.ir().model.features[1].id.clone(),
        ]
    );
    assert_eq!(
        decoded.ir().model.features[2].source_properties["Algorithm"],
        "RollingBall"
    );
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::Distance {
                distance: Length(3.0),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Simple,
            diameter: Some(Length(4.0)),
            ..
        }
    ));

    let FeatureDefinition::Fillet { groups } = &mut decoded.ir_mut().model.features[2].definition
    else {
        panic!("typed custom fillet");
    };
    let RadiusSpec::Constant { radius } = &mut groups[0].radius else {
        panic!("constant fillet");
    };
    *radius = Length(2.5);
    decoded.ir_mut().model.features[2]
        .source_properties
        .insert("Algorithm".into(), "FaceBlend".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[2].kind, "CustomFillet");
    assert_eq!(native[2].parameters["Radius"], "2.5mm");
    assert_eq!(native[2].properties["Algorithm"], "FaceBlend");
    assert_eq!(
        regenerated.ir().model.features[2].source_properties["Algorithm"],
        "FaceBlend"
    );
    assert_eq!(
        regenerated.ir().model.features[2].dependencies,
        vec![
            regenerated.ir().model.features[0].id.clone(),
            regenerated.ir().model.features[1].id.clone(),
        ]
    );
    regenerated.ir_mut().model.features[2].dependencies.pop();
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            regenerated.ir(),
            regenerated.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("dependencies are inconsistent with its operands"),
        "{error}"
    );
}

#[test]
fn decode_projects_compact_combine_with_unresolved_semantics() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Compact" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moCombineBodies_c", "Compact", 119)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            op: BooleanOp::Unresolved,
            keep_tools: false,
        }
    ));

    decoded.ir_mut().model.features[0].name = Some("Renamed compact combine".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            op: BooleanOp::Unresolved,
            keep_tools: false,
        }
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_combine_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

    fn append_body_path(payload: &mut Vec<u8>, local_id: u32) {
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&[0, 3, 0, 0]);
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[
            0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49,
            0xb2, 0x54,
        ]);
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&[0x32, 0x80, 0, 0]);
        payload.extend_from_slice(&[1; 12]);
        payload.extend_from_slice(&local_id.to_le_bytes());
    }

    fn combine_payload(has_selection: bool) -> Vec<u8> {
        let mut payload =
            resolved_feature_classes_with_ids(&[("moCombineBodies_c", "Combine", 119)]);
        if has_selection {
            append_body_path(&mut payload, 6);
            append_body_path(&mut payload, 7);
        }
        payload
    }

    let resolved_selection = combine_payload(true);
    assert_eq!(
        (12..resolved_selection.len())
            .filter(
                |offset| crate::resolved_features::terminations::compact_body_path_at(
                    &resolved_selection,
                    *offset
                )
                .is_some()
            )
            .count(),
        2
    );

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Combine" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_selection,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &combine_payload(false),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));
    let feature_id = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        } if target.starts_with("sldprt:feature-input:body-path:")
            && tools.starts_with("sldprt:feature-input:body-path:")
    ));
    assert!(matches!(
        decoded.ir().model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));

    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-1-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Combine" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swName="Part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &combine_payload(false),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_selection,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature_id = decoded.ir().model.features[0].id.clone();
    assert!(decoded.ir().model.configurations[0].active.is_active());
    assert_eq!(decoded.ir().model.configurations[0].source_index, Some(1));
    assert!(matches!(
        &decoded.ir().model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        } if target.starts_with("sldprt:feature-input:body-path:")
            && tools.starts_with("sldprt:feature-input:body-path:")
    ));
}

#[test]
fn decode_projects_fixed_reference_plane_frame() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut resolved = resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plano", 42)]);
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    resolved.extend_from_slice(&("moFixedRefPlnData_c".len() as u16).to_le_bytes());
    resolved.extend_from_slice(b"moFixedRefPlnData_c");
    let mut frame = [0u8; 97];
    frame[0..8].copy_from_slice(&2.5f64.to_le_bytes());
    frame[8..16].copy_from_slice(&(-0.25f64).to_le_bytes());
    frame[16..24].copy_from_slice(&1.5f64.to_le_bytes());
    frame[24..32].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[32..40].copy_from_slice(&0.0f64.to_le_bytes());
    frame[40..48].copy_from_slice(&0.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..57].copy_from_slice(&0.0f64.to_le_bytes());
    frame[57..65].copy_from_slice(&0.0f64.to_le_bytes());
    frame[65..73].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[73..81].copy_from_slice(&0.0f64.to_le_bytes());
    frame[81..89].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[89..97].copy_from_slice(&0.0f64.to_le_bytes());
    resolved.extend_from_slice(&frame);

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plano" Type="Plano" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumPlane {
            origin: Point3 {
                x: 2500.0,
                y: -250.0,
                z: 1500.0,
            },
            normal: Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0,
            },
            u_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            },
        }
    ));
}

#[test]
fn decode_rejects_nonorthogonal_fixed_reference_plane_frame() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut resolved = resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plane", 42)]);
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    resolved.extend_from_slice(&("moFixedRefPlnData_c".len() as u16).to_le_bytes());
    resolved.extend_from_slice(b"moFixedRefPlnData_c");
    let mut frame = [0u8; 97];
    frame[24..32].copy_from_slice(&1.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..57].copy_from_slice(&1.0f64.to_le_bytes());
    frame[73..81].copy_from_slice(&1.0f64.to_le_bytes());
    resolved.extend_from_slice(&frame);

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plane" Type="Plane" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumPlaneUnresolved
    ));
}

#[test]
fn incomplete_coordinate_system_projects_as_typed_unresolved() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Fixture" Type="Coordinate System" id="28"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumCoordinateSystemUnresolved
    ));
}

#[test]
fn decode_projects_generic_revolution_with_explicit_operation() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, RevolveExtent, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolution Name="Generic" Type="GenericRevolution" id="43" Operation="Cut" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1"><Dimension Name="Angle">180deg</Dimension></Revolution></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle },
                }),
                ..
            },
            op: BooleanOp::Cut,
        } if (angle.0 - std::f64::consts::PI).abs() < 1e-12
    ));
}

#[test]
fn decode_projects_compact_solid_sweep_join_operation() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    add_solidworks_version(&mut source, 17_000);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    let mut resolved = 15u32.to_le_bytes().to_vec();
    resolved.extend_from_slice(&[0; 8]);
    resolved.extend(resolved_feature_classes_with_ids(&[(
        "moSweep_c",
        "Sweep",
        137,
    )]));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Solid {
                op: BooleanOp::Join
            },
            ..
        }
    ));
}

#[test]
fn decode_projects_compact_solid_sweep_general_curve_path() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    let mut resolved = resolved_feature_classes_with_ids(&[("moSweep_c", "Sweep", 137)]);
    let path_offset = resolved.len();
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    let path_class = b"moGeneralCurveRef_w";
    resolved.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(path_class);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            path: Some(PathRef::Native(path)),
            ..
        } if path.ends_with(&format!(":{path_offset}"))
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_sweep_path() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    fn sweep_payload(has_path: bool) -> Vec<u8> {
        let mut payload = resolved_feature_classes_with_ids(&[("moSweep_c", "Sweep", 137)]);
        if has_path {
            payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
            let path_class = b"moGeneralCurveRef_w";
            payload.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
            payload.extend_from_slice(path_class);
        }
        payload
    }

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &sweep_payload(true),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &sweep_payload(false),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep { path: None, .. }
    ));
    let feature_id = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Sweep {
            path: Some(PathRef::Native(path)),
            ..
        } if path.starts_with("sldprt:feature-input:general-curve-ref:")
    ));
    assert!(matches!(
        decoded.ir().model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Sweep { path: None, .. }
    ));
}

#[test]
fn decode_projects_native_surface_sweep_class_without_localized_type() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Operacion1" Type="Personalizado" id="137"/></Keywords>"#,
    ));
    let mut resolved =
        resolved_feature_classes_with_ids(&[("moSweepRefSurface_c", "Operacion1", 137)]);
    let path_offset = resolved.len();
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    let path_class = b"moGeneralCurveRef_w";
    resolved.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(path_class);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Surface,
            path: Some(PathRef::Native(ref path)),
            ..
        }
        if path.ends_with(&format!(":{path_offset}"))
    ));
}

#[test]
fn decode_projects_surface_sweep_reference_curve_profile() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Helix1" Type="Helix/Spiral" id="119"/>
            <Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/>
        </Keywords>"#,
    ));
    let mut resolved = resolved_feature_classes_with_ids(&[
        ("moHelix_c", "Helix1", 119),
        ("moSweepRefSurface_c", "Surface-Sweep1", 137),
    ]);
    resolved.extend_from_slice(&[0xdd, 0x94, 0xff, 0xff, 1, 0]);
    let class = b"moCompReferenceCurve_c";
    resolved.extend_from_slice(&(class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(class);
    let prefix = resolved.len();
    resolved.resize(prefix + 133, 0);
    resolved[prefix..prefix + 10].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    resolved[prefix + 45..prefix + 61].fill(0xff);
    let reference = prefix + 81;
    resolved[reference..reference + 4].copy_from_slice(&119u32.to_le_bytes());
    resolved[reference + 4..reference + 8].copy_from_slice(&0x5edf_5674u32.to_le_bytes());
    resolved[reference + 16..reference + 20].copy_from_slice(&0x65u32.to_le_bytes());
    resolved[reference + 24..reference + 28].fill(0xff);
    for offset in [reference + 32, reference + 36, reference + 40] {
        resolved[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    resolved[reference + 48..reference + 52].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let helix = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Helix1"))
        .unwrap();
    let sweep = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap();
    assert!(matches!(
        &sweep.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Feature(feature)),
            ..
        } if feature == &helix.id
    ));
    assert!(sweep.dependencies.contains(&helix.id));

    let mut changed_profile = decoded.ir().clone();
    let FeatureDefinition::Sweep { section, .. } = &mut changed_profile
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap()
        .definition
    else {
        unreachable!("typed surface sweep");
    };
    *section = cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Native("other".into()));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &changed_profile,
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("changes a reference-curve sweep profile"));

    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap()
        .name = Some("Renamed surface sweep".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Renamed surface sweep"))
            .map(|feature| &feature.definition),
        Some(FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Feature(_)),
            ..
        })
    ));
}

#[test]
fn decode_projects_generated_surface_sweep_profile_path() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/>
            <Feature Name="Surface-Sweep2" Type="Surface-Sweep" id="211"/>
        </Keywords>"#,
    ));
    let mut resolved =
        resolved_feature_classes_with_ids(&[("moSweepRefSurface_c", "Surface-Sweep1", 137)]);
    resolved.extend_from_slice(&[0xdd, 0x94, 0xff, 0xff, 1, 0]);
    let class = b"moCompReferenceCurve_c";
    resolved.extend_from_slice(&(class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(class);
    resolved.extend_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    resolved.extend(resolved_feature_classes_with_ids(&[(
        "moSweepRefSurface_c",
        "Surface-Sweep2",
        211,
    )]));
    let wrapper = resolved.len();
    resolved.extend_from_slice(&[0xdd, 0x94, 0xa3, 0x92, 0x2b, 0x80, 0x02, 0, 0, 4, 0, 0]);
    let marker = resolved.len() + 12;
    resolved.resize(marker + 18, 0);
    resolved[marker - 12..marker - 8].copy_from_slice(&2u32.to_le_bytes());
    resolved[marker - 8..marker - 4].copy_from_slice(&[4, 2, 0, 0]);
    resolved[marker..marker + 16].copy_from_slice(&[
        0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2,
        0x54,
    ]);
    let entry = marker + 18;
    resolved.resize(entry + 32, 0);
    resolved[entry..entry + 2].copy_from_slice(&0x8c20u16.to_le_bytes());
    resolved[entry + 4..entry + 8].copy_from_slice(&[0x34, 0x80, 0x37, 0]);
    resolved[entry + 8..entry + 12].copy_from_slice(&137u32.to_le_bytes());
    resolved[entry + 12..entry + 16].copy_from_slice(&0x5edf_56e2u32.to_le_bytes());
    resolved[entry + 16..entry + 20].copy_from_slice(&7u32.to_le_bytes());
    resolved[entry + 28..entry + 32].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let first = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap();
    let second = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep2"))
        .unwrap();
    assert!(matches!(
        &second.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Generated {
                curves,
                native,
            }),
            ..
        } if curves.len() == 1
            && curves[0].feature == first.id
            && curves[0].local_id == "7"
            && native.ends_with(&wrapper.to_string())
    ));
    assert!(second.dependencies.contains(&first.id));
}

#[test]
fn decode_retains_e1_feature_input_operands() {
    let mut payload = resolved_features_payload(&[0, 1, 2]);
    let mut replacements = 0;
    for index in 0..payload.len().saturating_sub(1) {
        if payload[index..index + 2] == [0xd6, 0x80] {
            payload[index] = 0xe1;
            replacements += 1;
        }
    }
    assert_eq!(replacements, 2);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let scalar = &native.feature_input_lanes[0].scalars[0];
    assert!(native.feature_input_lanes[0]
        .references
        .iter()
        .all(|reference| reference.kind == crate::records::FeatureInputOperandKind::E1));
    assert!(scalar.entity_indices.is_empty());
    assert_eq!(
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .collect::<Vec<_>>(),
        [
            (crate::records::FeatureInputOperandKind::E1, 0),
            (crate::records::FeatureInputOperandKind::E1, 2),
        ]
    );
}

#[test]
fn decode_resolves_feature_input_operands_by_compatible_ordinal() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0, 0, 2], &["Sketch1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature_ref = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .and_then(|feature| feature.native_ref.as_deref())
        .expect("native sketch feature");
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    let scalar = &lane.scalars[0];
    assert!(lane
        .references
        .iter()
        .all(|reference| reference.feature_ref.as_deref() == Some(feature_ref)));
    assert_eq!(scalar.operands[0].entity_index, 0);
    assert_eq!(
        scalar.operands[0].entity_ref.as_deref(),
        Some(lane.sketch_entities[0].id.as_str())
    );
    assert_eq!(scalar.operands[1].entity_index, 2);
    assert_eq!(scalar.operands[1].entity_ref, None);

    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_projects_unambiguous_resolved_feature_parameter() {
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
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss-Extrude1"))
        .expect("projected extrusion feature");
    let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } = &feature.definition
    else {
        panic!("typed extrusion feature");
    };
    assert_eq!(
        extent,
        &cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination: cadmpeg_ir::features::Termination::Blind {
                    length: cadmpeg_ir::features::Length(25.0),
                },
                draft: None,
                offset: None,
            }
        }
    );
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
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:scalar#")));
    let native = sldprt_native(decoded.ir());
    let scalar = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| Some(scalar.id.as_str()) == parameter.native_ref.as_deref())
        .expect("parameter scalar");
    assert_eq!(scalar.feature_ref.as_deref(), feature.native_ref.as_deref());
    assert_eq!(
        native.feature_input_lanes[0].relation_bindings[0].scalar_ref,
        scalar.id
    );
    assert_eq!(
        native.feature_input_lanes[0].relation_bindings[0]
            .feature_ref
            .as_deref(),
        feature.native_ref.as_deref()
    );
}

#[test]
fn decode_projects_hyphenated_extrusion_operations() {
    for (kind, expected) in [
        ("Boss-Extrude", cadmpeg_ir::features::BooleanOp::Join),
        ("Cut-Extrude", cadmpeg_ir::features::BooleanOp::Cut),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            format!(
                "<Keywords><Extrusion Name=\"Extrude1\" Type=\"{kind}\"><Dimension Name=\"D1\">25</Dimension></Extrusion></Keywords>"
            )
            .as_bytes(),
        ));

        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
        ));
    }
}

#[test]
fn decode_binds_generic_extrusion_to_its_dissectable_sketch_child() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8" DissectableChildren="3"><Dimension Name="D1">25</Dimension></Extrusion><Sketch Name="Sketch1" Type="Sketch" id="3"/></Keywords>"#,
    ));
    let original = source.clone();

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let extrusion = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrude1"))
        .expect("projected extrusion feature");
    let sketch = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert_eq!(extrusion.dependencies, vec![sketch.id.clone()]);
    assert!(sketch.ordinal < extrusion.ordinal);
    assert!(matches!(
        &extrusion.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Feature(profile),
            ..
        } if profile == &sketch.id
    ));
    cadmpeg_test_support::roundtrip::verbatim_replay_holds(
        &SldprtCodec,
        "decode_projects_sketch_feature_dependencies",
        &original,
    );
}

#[test]
fn decode_projects_feature_input_extrusion_operations() {
    fn operation_payload(
        code: u32,
        object_id: u32,
        name: &str,
        class_name: &str,
        direct_class: bool,
        padding: usize,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&code.to_le_bytes());
        payload.extend(std::iter::repeat_n(0, padding));
        if direct_class {
            payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
            payload.extend_from_slice(&(class_name.len() as u16).to_le_bytes());
            payload.extend_from_slice(class_name.as_bytes());
        } else {
            payload.extend_from_slice(&0x84d8u16.to_le_bytes());
        }
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff]);
        payload.push(name.encode_utf16().count() as u8);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload
    }

    fn inline_operation_payload(family: u8, operation: u8, object_id: u32) -> Vec<u8> {
        let class_name = if family == 0x40 {
            "moExtrusion_c"
        } else {
            "moICE_c"
        };
        let mut payload = operation_payload(14, object_id, "Extrude1", class_name, true, 8);
        payload.truncate(payload.len() - 12);
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[family, 1, operation, 0x40]);
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
        payload
    }

    for (code, expected, class_name, layouts) in [
        (
            3,
            cadmpeg_ir::features::BooleanOp::Join,
            "moICE_c",
            &[(true, 8), (true, 4), (false, 8), (false, 4)][..],
        ),
        (
            1,
            cadmpeg_ir::features::BooleanOp::Join,
            "moExtrusion_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            1,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            2,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            10,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            0,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            22_993,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
    ] {
        for &(direct_class, padding) in layouts {
            let mut source = sldprt_with_body(&triangle_body());
            add_solidworks_version(&mut source, if padding == 8 { 17_000 } else { 11_000 });
            source.extend(make_block(
                0x42,
                "Contents/Keywords",
                br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
            ));
            source.extend(make_block(
                0x45,
                "Contents/Config-0-ResolvedFeatures",
                &operation_payload(code, 8, "Extrude1", class_name, direct_class, padding),
            ));

            let decoded = SldprtCodec
                .decode(&mut Cursor::new(source), &DecodeOptions::default())
                .unwrap();
            let feature = decoded
                .ir()
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some("Extrude1"))
                .expect("projected extrusion feature");
            assert!(
                matches!(
                    &feature.definition,
                    cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
                ),
                "code {code}, class {class_name}, direct {direct_class}, padding {padding}: {:?}",
                feature.definition
            );
        }
    }

    for code in [4, 11, 20] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
        ));
        source.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &operation_payload(code, 8, "Extrude1", "moICE_c", true, 8),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude {
                op: cadmpeg_ir::features::BooleanOp::Unresolved,
                ..
            }
        ));
        if code == 11 {
            assert!(decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains(
                    "typed feature(s) retain native or unresolved required operation operands"
                )));
        }
    }

    for (kind, expected) in [
        ("BossExtrude", cadmpeg_ir::features::BooleanOp::Join),
        ("CutExtrude", cadmpeg_ir::features::BooleanOp::Cut),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        add_solidworks_version(&mut source, 17_000);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            format!(
                "<Keywords><Extrusion Name=\"Extrude1\" Type=\"{kind}\" id=\"8\"><Dimension Name=\"D1\">25</Dimension></Extrusion></Keywords>"
            )
            .as_bytes(),
        ));
        source.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &operation_payload(11, 8, "Extrude1", "moICE_c", true, 8),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
        ));
    }

    for (family, operation, expected) in [
        (0x40, 0, cadmpeg_ir::features::BooleanOp::Join),
        (0xca, 2, cadmpeg_ir::features::BooleanOp::Cut),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
        ));
        source.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &inline_operation_payload(family, operation, 8),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
        ));
    }
}

#[test]
fn decode_does_not_project_ambiguous_resolved_feature_parameter() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    let mut payload = resolved_features_payload(&[0]);
    payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, 2]);
    payload.extend_from_slice(&[b'D', 0, b'1', 0]);
    payload.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
    ]);
    payload.extend_from_slice(&0.050f64.to_le_bytes());
    payload.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
    ]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "D1"));
}

#[test]
fn decode_projects_unambiguous_resolved_sketch_parameter() {
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
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch { .. }
    ));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected sketch D1 parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:scalar#")));
}

#[test]
fn decode_projects_owned_native_sketch_relation() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    let cadmpeg_ir::features::FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch),
        ..
    } = &feature.definition
    else {
        panic!("bound sketch feature");
    };
    let native = sldprt_native(decoded.ir());
    assert!(native.feature_input_lanes[0]
        .sketch_entities
        .iter()
        .all(|entity| entity.feature_ref.as_deref() == feature.native_ref.as_deref()));
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected relation parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.is_some())
        .expect("projected native relation");
    assert_eq!(&constraint.sketch, sketch);
    assert!(constraint
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:relation-instance#")));
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            entities,
            parameter: Some(relation_parameter),
            operands,
            ..
        } if native_kind == "sgPntPntDist"
            && entities.is_empty()
            && relation_parameter == &parameter.id
            && operands.len() == 2
            && operands[0].native_kind == "d6"
            && operands[0].object_index == 0
            && operands[0].native_ref.is_some()
            && operands[1].native_kind == "d6"
            && operands[1].object_index == 2
            && operands[1].native_ref.is_none()
    ));
    let findings = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).findings;
    assert!(findings.is_empty(), "{findings:#?}");
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_groups_compact_relation_scalar_pair() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
        panic!("one compact relation instance");
    };
    assert_eq!(relation.scalar_refs.len(), 2);
    let driving = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| scalar.role == crate::records::FeatureInputScalarRole::Driving)
        .expect("driving scalar");
    let display = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| scalar.role == crate::records::FeatureInputScalarRole::Display)
        .expect("display scalar");
    assert_eq!(
        relation.parameter_scalar_ref.as_deref(),
        Some(driving.id.as_str())
    );
    assert_eq!(
        relation.display_scalar_ref.as_deref(),
        Some(display.id.as_str())
    );
    assert_eq!(relation.operands.len(), 2);
    assert_eq!(relation.operands[0].entity_index, 0);
    assert_eq!(relation.operands[1].entity_index, 2);

    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
        .expect("projected compact relation");
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            parameter: Some(parameter),
            ..
        } if native_kind == "sgPntPntDist"
            && decoded.ir().model.parameters.iter().any(|candidate| {
                &candidate.id == parameter
                    && candidate.native_ref.as_deref() == Some(driving.id.as_str())
            })
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_starts_another_relation_after_two_repeated_operand_scalars() {
    let mut source = sldprt_with_tagged_compact_relation_names(
        &triangle_body(),
        "sgPntPntDist",
        [[0xd6, 0x80]; 2],
        &["Sketch1", "D1", "D2", "D3"],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    assert_eq!(native.feature_input_lanes[0].relation_instances.len(), 2);
    assert_eq!(
        native.feature_input_lanes[0]
            .relation_instances
            .iter()
            .map(|relation| relation.scalar_refs.len())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
}

#[test]
fn decode_groups_native_tagged_point_line_relations() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgPntLineDist",
        [[0x7b, 0x83], [0x86, 0x83]],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving point-line parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.references.len(), 4);
    assert!(lane
        .references
        .iter()
        .enumerate()
        .all(|(ordinal, reference)| {
            reference.kind
                == crate::records::FeatureInputOperandKind::Native(if ordinal % 2 == 0 {
                    0x837b
                } else {
                    0x8386
                })
        }));
    let [relation] = lane.relation_instances.as_slice() else {
        panic!("one point-line relation instance");
    };
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::PointLineDistance
    );
    let constraint = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
        .expect("projected point-line relation");
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            operands,
            ..
        } if native_kind == "sgPntLineDist"
            && operands[0].native_kind == "7b83"
            && operands[1].native_kind == "8683"
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_uses_relation_units_for_bare_integer_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgPntPntVertDist",
        [[0xcb, 0x8d]; 2],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">25</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving vertical-distance parameter");
    assert_eq!(parameter.expression, "25");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(25.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_uses_relation_units_for_boolean_shaped_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_tagged_compact_relation_scalar(
        &triangle_body(),
        "sgPntPntDist",
        [[0xd6, 0x80]; 2],
        0.001,
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">1</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving distance parameter");
    assert_eq!(parameter.expression, "1");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(1.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_uses_relation_units_for_bare_integer_angles() {
    use cadmpeg_ir::features::{Angle, ParameterValue};

    let mut source =
        sldprt_with_tagged_compact_relation(&triangle_body(), "sgAnglDim", [[0xda, 0x8d]; 2]);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">25</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving angle parameter");
    assert_eq!(parameter.expression, "25");
    assert_eq!(parameter.value, Some(ParameterValue::Angle(Angle(0.025))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_groups_unary_circle_diameter_relations() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgCircleDim",
        [[0xfe, 0x83], [0, 0]],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">&lt;MOD-DIAM&gt;25mm</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
        panic!("one circle-diameter relation instance");
    };
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::CircleDiameter
    );
    assert_eq!(relation.operands.len(), 1);
    assert_eq!(
        relation.operands[0].kind,
        crate::records::FeatureInputOperandKind::Native(0x83fe)
    );
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("diameter parameter");
    assert_eq!(
        relation.parameter_scalar_ref.as_deref(),
        parameter.native_ref.as_deref()
    );
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            constraint.native_ref.as_deref() == Some(relation.id.as_str())
                && matches!(
                    &constraint.definition,
                    SketchConstraintDefinition::Native {
                        native_kind,
                        parameter: Some(bound_parameter),
                        operands,
                        ..
                    } if native_kind == "sgCircleDim"
                        && bound_parameter == &parameter.id
                        && operands.len() == 1
                        && operands[0].native_kind == "fe83"
                )
        }));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_groups_each_circle_dimension_operand_tag() {
    for tag in [
        [0xcc, 0x80],
        [0xfe, 0x83],
        [0xb6, 0x8a],
        [0x9d, 0x92],
        [0x69, 0xbd],
        [0x46, 0x81],
    ] {
        let mut source =
            sldprt_with_tagged_compact_relation(&triangle_body(), "sgCircleDim", [tag, [0, 0]]);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(decoded.ir());
        let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
            panic!("one circle-diameter relation for tag {tag:02x?}");
        };
        assert_eq!(
            relation.family,
            crate::records::FeatureInputRelationFamily::CircleDiameter
        );
        let [operand] = relation.operands.as_slice() else {
            panic!("one circle-diameter operand for tag {tag:02x?}");
        };
        assert_eq!(
            operand.kind,
            crate::records::FeatureInputOperandKind::Native(u16::from_le_bytes(tag))
        );
        assert_eq!(operand.entity_index, 0);
    }
}

#[test]
fn decode_uses_declaration_to_disambiguate_native_relation_tags() {
    let cases = [
        (
            "sgPntPntDist",
            [0x7b, 0x83],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgLLDist",
            [0x86, 0x83],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgPntPntDist",
            [0x7c, 0xbc],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgLLDist",
            [0x87, 0xbc],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgPntPntHorDist",
            [0xcb, 0x8d],
            crate::records::FeatureInputRelationFamily::PointPointHorizontalDistance,
        ),
        (
            "sgPntPntVertDist",
            [0xcb, 0x8d],
            crate::records::FeatureInputRelationFamily::PointPointVerticalDistance,
        ),
        (
            "sgAnglDim",
            [0xda, 0x8d],
            crate::records::FeatureInputRelationFamily::Angle,
        ),
    ];
    for (class, tag, family) in cases {
        let mut source = sldprt_with_tagged_compact_relation(&triangle_body(), class, [tag; 2]);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let parameter = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D2")
            .expect("driving relation parameter");
        if family == crate::records::FeatureInputRelationFamily::Angle {
            assert_eq!(parameter.expression, "0.025rad");
            assert_eq!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Angle(
                    cadmpeg_ir::features::Angle(0.025)
                ))
            );
        } else {
            assert_eq!(parameter.expression, "25mm");
            assert_eq!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Length(
                    cadmpeg_ir::features::Length(25.0)
                ))
            );
        }
        let native = sldprt_native(decoded.ir());
        let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
            panic!("one native-tagged relation instance for {class}");
        };
        assert_eq!(relation.family, family);
        assert!(relation.operands.iter().all(|operand| operand.kind
            == crate::records::FeatureInputOperandKind::Native(u16::from_le_bytes(tag))));
        assert!(decoded
            .ir()
            .model
            .sketch_constraints
            .iter()
            .any(|constraint| {
                constraint.native_ref.as_deref() == Some(relation.id.as_str())
                    && matches!(
                        &constraint.definition,
                        cadmpeg_ir::sketches::SketchConstraintDefinition::Native {
                            native_kind,
                            ..
                        } if native_kind == class
                    )
            }));
    }
}

#[test]
fn decode_and_validate_compact_delete_body_selection() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Body-Delete/Keep 1" Type="Body-Delete/Keep " id="41"/></Keywords>"#,
    ));
    let mut payload =
        resolved_feature_classes_with_ids(&[("moDeleteBody_c", "Body-Delete/Keep 1", 41)]);
    payload.extend([0xff, 0xff, 0x01, 0x00]);
    payload.extend(18u16.to_le_bytes());
    payload.extend(b"moDeleteBodyData_c");
    payload.extend([0x08, 0x00]);
    let token = 0x89a4u16;
    let mut state = [0u8; 83];
    state[0..2].copy_from_slice(&token.to_le_bytes());
    state[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    state[11..15].copy_from_slice(&287u32.to_le_bytes());
    state[15..19].copy_from_slice(&287u32.to_le_bytes());
    state[47..63].fill(0xff);
    payload.extend(state);
    payload.extend([0x30, 0x80]);
    payload.extend(1u32.to_le_bytes());
    payload.extend([0; 4]);
    payload.extend(11000u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(287u32.to_le_bytes());
    payload.extend(115u32.to_le_bytes());
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend([0; 12]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("body delete/keep feature(s)")));
    let mut native = sldprt_native(decoded.ir());
    let [selection] = native.feature_input_lanes[0].body_selections.as_slice() else {
        panic!("one compact body selection");
    };
    assert_eq!(selection.local_body_ids, [287, 115]);
    assert_eq!(selection.body_state_ids, [287]);
    assert_eq!(
        selection.mode,
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected)
    );

    let mut legacy = decoded.ir().native.namespace("sldprt").unwrap().clone();
    legacy.version = 5;
    for record in legacy
        .arenas
        .get_mut("feature_input_body_selections")
        .unwrap()
    {
        let mut fields = record.fields();
        fields.remove("mode");
        *record = cadmpeg_ir::NativeRecord::new(record.id().to_string(), fields);
    }
    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert_eq!(
        migrated.feature_input_lanes[0].body_selections[0].mode,
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected)
    );
    assert!(selection.feature_ref.starts_with("sldprt:history:feature#"));
    let delete_feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body-Delete/Keep 1"))
        .expect("delete-body feature");
    assert!(matches!(
        &delete_feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, mode }
            if bodies == &cadmpeg_ir::features::BodySelection::Local {
                bodies: vec!["287".into(), "115".into()],
                native: "sldprt:feature-input:body-ids:287,115".into(),
            } && *mode == cadmpeg_ir::features::BodyRetentionMode::DeleteSelected
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();

    decoded
        .ir_mut()
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Body-Delete/Keep 1"))
        .expect("delete-body feature")
        .name = Some("Renamed Delete Body".into());
    let mut renamed = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut renamed)
        .unwrap();
    let renamed = SldprtCodec
        .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
        .unwrap();
    let renamed_native = sldprt_native(renamed.ir());
    assert!(!renamed_native.feature_histories[0].features[0]
        .properties
        .contains_key("Bodies"));
    assert_eq!(
        renamed_native.feature_input_lanes[0].body_selections[0].local_body_ids,
        [287, 115]
    );

    {
        let delete_feature = decoded
            .ir_mut()
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Renamed Delete Body"))
            .expect("delete-body feature");
        let cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, .. } =
            &mut delete_feature.definition
        else {
            panic!("typed delete-body feature");
        };
        *bodies =
            cadmpeg_ir::features::BodySelection::Native("sldprt:feature-input:body-ids:287".into());
    }
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("changes a compact body selection"));

    {
        let delete_feature = decoded
            .ir_mut()
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Renamed Delete Body"))
            .expect("delete-body feature");
        let cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, mode } =
            &mut delete_feature.definition
        else {
            unreachable!("typed delete-body feature");
        };
        *bodies = cadmpeg_ir::features::BodySelection::Local {
            bodies: vec!["287".into(), "115".into()],
            native: "sldprt:feature-input:body-ids:287,115".into(),
        };
        *mode = cadmpeg_ir::features::BodyRetentionMode::KeepSelected;
    }
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("changes a compact body retention mode"));

    native.feature_input_lanes[0].body_selections[0]
        .body_state_ids
        .push(287);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
    native.feature_input_lanes[0].body_selections[0].body_state_ids = vec![287];

    native.feature_input_lanes[0].body_selections[0].mode =
        Some(cadmpeg_ir::features::BodyRetentionMode::KeepSelected);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
    native.feature_input_lanes[0].body_selections[0].mode =
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected);

    native.feature_input_lanes[0].body_selections[0].local_body_ids[0] = 288;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
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

#[test]
fn decode_projects_nested_feature_input_profile_as_a_sketch() {
    use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchGeometry, SketchLocus};

    let source = sldprt_with_nested_sketch_profile(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(decoded.ir().model.sketches.len(), 1);
    assert_eq!(decoded.ir().model.sketch_entities.len(), 3);
    assert_eq!(decoded.ir().model.sketch_constraints.len(), 3);
    let sketch = &decoded.ir().model.sketches[0];
    assert_eq!(sketch.configuration.as_deref(), Some("0"));
    let (origin, normal, _) = sketch
        .resolved_placement()
        .expect("resolved sketch placement");
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(sketch.profiles.len(), 1);
    assert_eq!(sketch.profiles[0].len(), 3);
    assert!(decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .all(|entity| matches!(entity.geometry, SketchGeometry::Line { .. })));
    assert!(decoded.ir().model.sketch_entities.iter().all(|entity| {
        entity
            .native_ref
            .as_deref()
            .is_some_and(|id| id.contains(":sldprt:brep:edge#"))
            && entity.endpoint_refs.len() == 2
            && entity
                .endpoint_refs
                .iter()
                .all(|id| id.contains(":sldprt:brep:point#"))
    }));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .all(|constraint| {
            matches!(
                &constraint.definition,
                SketchConstraintDefinition::CoincidentLoci { loci }
                    if loci.len() == 2
                        && loci.iter().all(|locus| matches!(
                            locus,
                            SketchLocus::Start(_) | SketchLocus::End(_)
                        ))
            )
        }));
    assert!(sketch.native_ref.as_deref().is_some_and(|native_ref| {
        native_ref.starts_with("sldprt:feature-input:resolved-features#")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
}

#[test]
fn decode_binds_profile_stream_by_feature_object_interval() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch = decoded
        .ir()
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.name.as_deref() == Some("Sketch1"))
        .expect("named feature-input sketch");
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sketch history feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(id),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_sweep() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sketch1" Type="Sweep"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [sketch] = decoded.ir().model.sketches.as_slice() else {
        panic!("one enclosed sweep profile stream");
    };
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sweep history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Sketch(id)),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_sweep() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sketch1" Type="Sweep"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sweep history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(_),
            ..
        }
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Sketch1" Type="Boss-Extrude"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [sketch] = decoded.ir().model.sketches.as_slice() else {
        panic!("one enclosed extrusion profile stream");
    };
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("extrusion history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(id),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_binds_configuration_sketch_state_after_geometry_projection() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default" id="0"/><Sketch Name="Sketch1" Type="Sketch" id="0"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert!(matches!(
        &decoded.ir().model.configurations[0].feature_states[&feature.id].definition,
        FeatureDefinition::Sketch {
            sketch: Some(configuration_sketch),
            ..
        } if decoded.ir().model.sketches.iter().any(|sketch| &sketch.id == configuration_sketch)
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Sketch1" Type="Boss-Extrude"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("extrusion history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            ..
        }
    ));
}

#[test]
fn decode_binds_unique_sketch_history_to_profile_consumers() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="21"/><Rib Name="Web" Type="Rib" id="22" Profile="21" Direction="0,1,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension></Rib></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch_id = decoded.ir().model.sketches[0].id.clone();
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(value), ..
        } if value == &sketch_id
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(ProfileRef::Sketch(value)),
                ..
            },
            ..
        } if value == &sketch_id
    )));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut written)
        .unwrap();
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(round_trip
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(
            feature.definition,
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(_),
                ..
            }
        )));
}

#[test]
fn matching_numbered_sketch_alias_binds_the_base_geometry() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureId, ProfileRef,
        Termination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchId};

    let sketch_id = SketchId("sketch".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
            entity: cadmpeg_ir::sketches::SketchEntityId("sketch:entity".into()),
            reversed: false,
        }]],
        native_ref: None,
    };
    let neutral =
        |id: &str, name: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
            id: FeatureId(id.into()),
            ordinal: 0,
            name: Some(name.into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Sketch".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: Some(native_ref.into()),
        };
    let mut features = vec![
        neutral(
            "base",
            "Profile",
            "native-base",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "alias",
            "Profile<3>",
            "native-alias",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "different",
            "Profile<4>",
            "native-different",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "consumer",
            "Boss",
            "native-consumer",
            FeatureDefinition::Extrude {
                profile: ProfileRef::Native("native-alias".into()),
                direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Unresolved,
                        draft: None,
                        offset: None,
                    },
                },
                op: BooleanOp::Join,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        ),
    ];
    let native = |id: &str, name: &str, depth: &str| crate::records::Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: name.into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("Depth".into(), depth.into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: vec![crate::records::FeatureContent::Dimension("Depth".into())],
    };
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native("native-base", "Profile", "2mm"),
            native("native-alias", "Profile<3>", "2mm"),
            native("native-different", "Profile<4>", "3mm"),
        ],
    };

    crate::history::bind_unique_sketch_feature(&mut features, &[sketch], &[history]);

    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert_eq!(features[1].dependencies, vec![FeatureId("base".into())]);
    assert!(matches!(
        &features[2].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert!(matches!(
        &features[3].definition,
        FeatureDefinition::Extrude { profile: ProfileRef::Sketch(id), .. } if id == &sketch_id
    ));
    assert_eq!(features[3].dependencies, vec![FeatureId("base".into())]);
}

#[test]
fn decode_binds_multiple_sketch_history_nodes_by_exact_name() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, ProfileRef};

    let mut source = sldprt_with_nested_nurbs_sketches(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="feature input spline sketch" Type="Sketch" id="21"/><Sketch Name="feature input rational spline sketch" Type="Sketch" id="22"/><Sweep Name="Pipe" Type="Sweep" id="23" Profile="21" Path="22" Operation="NewBody"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let bound = decoded
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } => Some(sketch.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bound.len(), 2);
    let sweep = decoded
        .ir()
        .model
        .features
        .iter()
        .find_map(|feature| match &feature.definition {
            FeatureDefinition::Sweep {
                section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Sketch(profile)),
                path: Some(PathRef::Sketch(path)),
                ..
            } => Some((profile, path)),
            _ => None,
        })
        .expect("bound sweep");
    assert_ne!(sweep.0, sweep.1);
    assert!(bound.contains(sweep.0) && bound.contains(sweep.1));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
}

#[test]
fn decode_does_not_bind_duplicate_sketch_names_by_order() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    let mut payload = resolved_features_payload(&[1, 1]);
    for _ in 0..2 {
        payload.extend(parasolid_with_body(
            "Duplicate",
            "SCH_SW_33103_11000",
            &nurbs_sketch_body(false),
        ));
    }
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Duplicate" Type="Sketch" id="21"/><Sketch Name="Duplicate" Type="Sketch" id="22"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.sketches.len(), 2);
    assert!(decoded.ir().model.features.iter().all(|feature| matches!(
        feature.definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    )));
}

#[test]
fn decode_distinguishes_full_circle_sketch_geometry() {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_circular_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.sketches[0].profiles[0].len(), 1);
    assert!(matches!(
        decoded.ir().model.sketch_entities[0].geometry,
        SketchGeometry::Circle {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            radius: Length(1000.0),
        }
    ));
}

#[test]
fn decode_projects_full_ellipse_sketch_geometry() {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_elliptical_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        decoded.ir().model.sketch_entities[0].geometry,
        SketchGeometry::Ellipse {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            major_angle: Angle(value),
            major_radius: Length(2000.0),
            minor_radius: Length(1000.0),
            start_angle: None,
            end_angle: None,
        } if (value - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
}

#[test]
fn decode_projects_non_rational_and_rational_nurbs_sketch_geometry() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_nurbs_sketches(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let splines = decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| match &entity.geometry {
            SketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            } => Some((degree, knots, control_points, weights, periodic)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(splines.len(), 2);
    assert!(splines.iter().all(|(degree, knots, points, _, periodic)| {
        **degree == 2
            && knots.as_slice() == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
            && points.len() == 3
            && !**periodic
    }));
    assert!(splines
        .iter()
        .any(|(_, _, _, weights, _)| weights.is_none()));
    assert!(splines
        .iter()
        .any(|(_, _, _, weights, _)| { weights.as_deref() == Some(&[1.0, 0.5, 1.0]) }));
}

#[path = "integration_tests.rs"]
mod integration_tests;
