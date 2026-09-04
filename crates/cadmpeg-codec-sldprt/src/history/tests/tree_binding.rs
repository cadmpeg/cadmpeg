// SPDX-License-Identifier: Apache-2.0
//! Feature-tree typing, class-token, and name-binding decode tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

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
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
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
        result
            .ir()
            .model
            .feature_parent(&result.ir().model.features[1].id),
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
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
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
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
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
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, FeatureDefinition, LinearTermination};

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
                    termination: LinearTermination::Unresolved,
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
