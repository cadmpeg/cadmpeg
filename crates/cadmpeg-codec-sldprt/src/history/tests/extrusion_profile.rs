// SPDX-License-Identifier: Apache-2.0
//! Extrusion projection and adjacent-profile binding decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

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
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
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
