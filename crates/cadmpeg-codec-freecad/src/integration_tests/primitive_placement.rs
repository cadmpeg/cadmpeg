// SPDX-License-Identifier: Apache-2.0
//! Primitive frames from CC0 recipe placements and explicit container frames.

use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

const EPS_PRIMITIVE_FRAME: f64 = 1.0e-12;

fn operation(feature: &cadmpeg_ir::features::Feature) -> &FeatureOperation {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(operation)
        | FeatureDefinition::PostProcess { operation, .. } => operation,
    }
}

#[test]
fn primitive_placement_cc0_recipe_frames_and_validator() {
    for version in ["0.21.2", "1.0.2", "1.1.4"] {
        for (variant, translation) in [
            ('a', [0.0, 0.0, 0.0]),
            ('b', [5.0, -7.0, 0.0]),
            ('c', [0.0, 0.0, 0.0]),
            ('d', [5.0, -7.0, 3.0]),
        ] {
            let path = format!(
                "{}/../../corpus/freecad_fcstd/fixtures/recipe-{version}-F14{variant}.FCStd",
                env!("CARGO_MANIFEST_DIR")
            );
            let bytes = std::fs::read(path).unwrap();
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let primitive = decoded
                .ir()
                .model
                .features
                .iter()
                .find_map(|f| {
                    if let FeatureOperation::Primitive { placement, .. } = operation(f) {
                        Some(placement.unwrap())
                    } else {
                        None
                    }
                })
                .unwrap();
            let rows = primitive.rows();
            for i in 0..3 {
                assert!(
                    (rows[i][3] - translation[i]).abs() < EPS_PRIMITIVE_FRAME,
                    "{version} F14{variant}"
                );
                for j in 0..3 {
                    let expected = if variant == 'c' {
                        [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]][i][j]
                    } else {
                        if i == j {
                            1.0
                        } else {
                            0.0
                        }
                    };
                    assert!((rows[i][j] - expected).abs() < EPS_PRIMITIVE_FRAME);
                }
            }
            let attachments = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::AttachmentRecord>("attachments")
                .unwrap();
            let attachment = attachments
                .iter()
                .find(|a| {
                    a.object.ends_with(if variant == 'd' {
                        "#AdditiveCylinder"
                    } else {
                        "#AdditiveBox"
                    })
                })
                .unwrap();
            assert_eq!(
                attachment.map_mode.unwrap().to_string(),
                if variant == 'a' {
                    "0"
                } else if variant == 'c' {
                    "5"
                } else {
                    "1"
                }
            );
            assert_eq!(attachment.supports.len(), usize::from(variant != 'a'));
            if variant != 'a' {
                let support = attachment.supports[0].as_ref().unwrap();
                assert_eq!(
                    support.object(),
                    Some(if variant == 'c' {
                        "fcstd:native:object#XZ_Plane"
                    } else if version == "0.21.2" {
                        "fcstd:native:object#SketchDatumPoint"
                    } else {
                        "fcstd:native:object#Sketch"
                    })
                );
                assert_eq!(
                    support.subelements(),
                    if variant == 'c' {
                        vec![String::new()]
                    } else {
                        vec!["Vertex1".to_owned()]
                    }
                );
            }
            assert_eq!(
                attachment.offset().unwrap().rows(),
                cadmpeg_ir::transform::Transform::identity().rows()
            );
            assert_valid_document(decoded.ir());
            assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
            assert!(!decoded
                .report()
                .losses
                .iter()
                .any(|l| l.code.local_code() == "feature.primitive-placement-unresolved"));
        }
    }
}

fn box_document(mode: &str, placement: &str) -> String {
    format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="PartDesign::Body" name="Body"/><Object type="PartDesign::AdditiveBox" name="Box"/></Objects>
<ObjectData Count="2"><Object name="Body"><Properties Count="2">
<Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Box"/></LinkList></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="20" Pz="30" Q0="0" Q1="0" Q2="1" Q3="0"/></Property>
</Properties></Object><Object name="Box"><Properties Count="5">
<Property name="Length" type="App::PropertyLength"><Float value="20"/></Property>
<Property name="Width" type="App::PropertyLength"><Float value="30"/></Property>
<Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
<Property name="MapMode" type="App::PropertyEnumeration"><Integer value="{mode}"/></Property>
{placement}</Properties></Object></ObjectData></Document>"#
    )
}

const PLACEMENT: &str = r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="-7" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>"#;

#[test]
fn primitive_placement_composes_body_and_validator_rejects_changed_frame() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&box_document("0", PLACEMENT))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let primitive = decoded
        .ir()
        .model
        .features
        .iter()
        .find_map(|f| {
            if let FeatureOperation::Primitive { placement, .. } = operation(f) {
                *placement
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        primitive.rows(),
        [
            [-1.0, 0.0, 0.0, 5.0],
            [0.0, -1.0, 0.0, 27.0],
            [0.0, 0.0, 1.0, 33.0],
            [0.0, 0.0, 0.0, 1.0]
        ]
    );
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    let mut value = serde_json::to_value(decoded.ir()).unwrap();
    let features = value["model"]["features"].as_array_mut().unwrap();
    let feature = features.iter_mut().find(|f| f["name"] == "Box").unwrap();
    feature["definition"]["placement"] =
        serde_json::to_value(cadmpeg_ir::features::FeatureRigidPlacement::identity()).unwrap();
    let changed = serde_json::from_value(value).unwrap();
    assert!(crate::test_support::validate_native(&changed)
        .iter()
        .any(|f| f.message.contains("primitive placement")));
}

#[test]
fn primitive_placement_unknown_mode_and_missing_frame_emit_loss() {
    for (mode, placement) in [("2", PLACEMENT), ("0", ""), ("54", PLACEMENT)] {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&box_document(mode, placement))),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(decoded
            .ir()
            .model
            .features
            .iter()
            .any(|f| f.name.as_deref() == Some("Box")
                && matches!(operation(f), FeatureOperation::Native { .. })));
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|l| l.code.local_code() == "feature.primitive-placement-unresolved"));
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    }
}

#[test]
fn primitive_placement_all_eight_additive_and_subtractive_families() {
    let dimensions = [
        ("Box", "Length=20 Width=30 Height=10"),
        ("Cylinder", "Radius=4 Height=12 Angle=360"),
        ("Cone", "Radius1=4 Radius2=2 Height=12 Angle=360"),
        ("Sphere", "Radius=4 Angle1=-90 Angle2=90 Angle3=360"),
        (
            "Ellipsoid",
            "Radius1=3 Radius2=5 Radius3=4 Angle1=-90 Angle2=90 Angle3=360",
        ),
        (
            "Torus",
            "Radius1=5 Radius2=2 Angle1=-180 Angle2=180 Angle3=360",
        ),
        ("Prism", "Polygon=6 Circumradius=4 Height=12"),
        (
            "Wedge",
            "Xmin=0 Ymin=0 Zmin=0 X2min=1 Z2min=1 Xmax=10 Ymax=10 Zmax=10 X2max=9 Z2max=9",
        ),
    ];
    for (family, dimensions) in dimensions {
        for prefix in ["Additive", "Subtractive"] {
            let properties = dimensions.split_whitespace().map(|pair| {
                let (name, value) = pair.split_once('=').unwrap();
                let (runtime, tag) = if name == "Polygon" { ("IntegerConstraint", "Integer") }
                    else if name.starts_with("Angle") { ("Angle", "Float") }
                    else if family == "Wedge" { ("Distance", "Float") }
                    else { ("Length", "Float") };
                format!(r#"<Property name="{name}" type="App::Property{runtime}"><{tag} value="{value}"/></Property>"#)
            }).collect::<String>();
            let count = dimensions.split_whitespace().count() + 1;
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="PartDesign::{prefix}{family}" name="Primitive"/></Objects><ObjectData Count="1"><Object name="Primitive"><Properties Count="{count}">{properties}{PLACEMENT}</Properties></Object></ObjectData></Document>"#
            );
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            let feature = &decoded.ir().model.features[0];
            let FeatureOperation::Primitive {
                placement: Some(placement),
                op,
                ..
            } = operation(feature)
            else {
                panic!("{prefix}{family} remains native")
            };
            assert_eq!(placement.rows()[0][3], 5.0);
            assert_eq!(placement.rows()[1][3], -7.0);
            assert_eq!(placement.rows()[2][3], 3.0);
            assert_eq!(
                *op,
                if prefix == "Additive" {
                    cadmpeg_ir::features::BooleanOp::Join
                } else {
                    cadmpeg_ir::features::BooleanOp::Cut
                }
            );
            assert_valid_document(decoded.ir());
            assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
        }
    }
}

#[test]
fn primitive_placement_resolved_attachment_does_not_apply_offset_twice() {
    let document = box_document("1", PLACEMENT)
        .replace("<Properties Count=\"5\">", "<Properties Count=\"6\">")
        .replace(
            PLACEMENT,
            &format!(
                "{PLACEMENT}{}",
                PLACEMENT.replace("name=\"Placement\"", "name=\"AttachmentOffset\"")
            ),
        );
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|f| f.name.as_deref() == Some("Box"))
        .unwrap();
    let FeatureOperation::Primitive {
        placement: Some(placement),
        ..
    } = operation(feature)
    else {
        panic!("primitive")
    };
    assert_eq!(placement.rows()[0][3], 5.0);
    assert_eq!(placement.rows()[1][3], 27.0);
    assert_eq!(placement.rows()[2][3], 33.0);
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
}

#[test]
fn primitive_placement_nested_part_uses_n12_container_frames() {
    for part_x in [100.0, 120.0] {
        let document = box_document("0", PLACEMENT)
            .replace("<Objects Count=\"2\">", "<Objects Count=\"3\"><Object type=\"App::Part\" name=\"Part\"/>")
            .replace("<ObjectData Count=\"2\">", "<ObjectData Count=\"3\">")
            .replace("Px=\"10\" Py=\"20\" Pz=\"30\" Q0=\"0\" Q1=\"0\" Q2=\"1\" Q3=\"0\"", "Px=\"2\" Py=\"3\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"")
            .replace("</ObjectData>", &format!(r#"<Object name="Part"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Body"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="{part_x}" Py="20" Pz="5" Q0="0" Q1="0" Q2="0.7071067811865475" Q3="0.7071067811865476"/></Property></Properties></Object></ObjectData>"#));
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|f| f.name.as_deref() == Some("Box"))
            .unwrap();
        let FeatureOperation::Primitive {
            placement: Some(placement),
            ..
        } = operation(feature)
        else {
            panic!("primitive")
        };
        for (row, expected) in [part_x + 4.0, 27.0, 8.0].into_iter().enumerate() {
            assert!((placement.rows()[row][3] - expected).abs() < EPS_PRIMITIVE_FRAME);
        }
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    }
}

#[test]
fn primitive_placement_missing_container_frame_and_cycle_emit_loss() {
    let document = box_document("0", PLACEMENT);
    let body_placement = r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="20" Pz="30" Q0="0" Q1="0" Q2="1" Q3="0"/></Property>"#;
    for document in [
        document
            .replace(body_placement, "")
            .replace("<Properties Count=\"2\">", "<Properties Count=\"1\">"),
        document.replace(
            "<LinkList count=\"1\"><Link value=\"Box\"/>",
            "<LinkList count=\"2\"><Link value=\"Body\"/><Link value=\"Box\"/>",
        ),
    ] {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|f| f.name.as_deref() == Some("Box"))
            .unwrap();
        assert!(matches!(
            operation(feature),
            FeatureOperation::Native { .. }
        ));
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|l| l.code.local_code() == "feature.primitive-placement-unresolved"));
    }
}

#[test]
fn primitive_placement_ambiguous_support_names_remain_native_with_loss() {
    let aliases = ["Support", "AttachmentSupport"].map(|name| format!(r#"<Property name="{name}" type="App::PropertyLinkSubList"><LinkSubList count="0"/></Property>"#)).join("");
    let document = box_document("1", PLACEMENT)
        .replace("<Properties Count=\"5\">", "<Properties Count=\"7\">")
        .replace(PLACEMENT, &format!("{PLACEMENT}{aliases}"));
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|f| f.name.as_deref() == Some("Box"))
        .unwrap();
    assert!(matches!(
        operation(feature),
        FeatureOperation::Native { .. }
    ));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|l| l.code.local_code() == "feature.primitive-placement-unresolved"));
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
}

#[test]
fn primitive_placement_refuses_multiple_part_parents() {
    let document = box_document("0", PLACEMENT).replace("PartDesign::Body", "App::Part")
        .replace("<Objects Count=\"2\">", "<Objects Count=\"3\"><Object type=\"App::Part\" name=\"OtherBody\"/>")
        .replace("<ObjectData Count=\"2\">", "<ObjectData Count=\"3\">")
        .replace("</ObjectData>", &format!(r#"<Object name="OtherBody"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Box"/></LinkList></Property>{PLACEMENT}</Properties></Object></ObjectData>"#));
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("multiple parent containers"));
}

#[test]
fn primitive_placement_validator_refuses_neutral_definition_for_unknown_mode() {
    let decode = |mode| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(&box_document(mode, PLACEMENT))),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let known = decode("0");
    let unknown = decode("2");
    let known_json = serde_json::to_value(known.ir()).unwrap();
    let mut definition = known_json["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Box")
        .unwrap()["definition"]
        .clone();
    definition.as_object_mut().unwrap().remove("placement");
    let mut value = serde_json::to_value(unknown.ir()).unwrap();
    value["model"]["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["name"] == "Box")
        .unwrap()["definition"] = definition;
    let changed = serde_json::from_value(value).unwrap();
    assert!(crate::test_support::validate_native(&changed)
        .iter()
        .any(|f| f.message.contains("primitive placement")));
}
