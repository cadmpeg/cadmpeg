// SPDX-License-Identifier: Apache-2.0
//! Primitive frames from CC0 recipe placements and explicit container frames.

use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, ParameterValue, PrimitiveSolidKind,
};
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchGeometryDefinition};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

const EPS_PRIMITIVE_FRAME: f64 = 1.0e-12;
const EPS_PRIMITIVE_VOLUME: f64 = 1.0e-6;
const EPS_PRIMITIVE_AREA: f64 = 1.0e-6;
const EPS_PRIMITIVE_BOX: f64 = 1.0e-6;

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
            let model = &decoded.ir().model;
            let feature = model
                .features
                .iter()
                .find(|feature| matches!(operation(feature), FeatureOperation::Primitive { .. }))
                .unwrap();
            let FeatureOperation::Primitive {
                solid,
                placement: Some(primitive),
                ..
            } = operation(feature)
            else {
                panic!("primitive placement")
            };
            let body_feature = model
                .features
                .iter()
                .find(|candidate| candidate.name.as_deref() == Some("Body"))
                .unwrap_or_else(|| panic!("{version} F14{variant}: body feature"));
            let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) =
                body_feature.evaluation.definition()
            else {
                panic!("{version} F14{variant}: body tree")
            };
            let expected_group = match variant {
                'a' | 'c' => &["AdditiveBox"][..],
                'b' if version == "0.21.2" => &["AdditiveBox", "Sketch", "SketchDatumPoint"][..],
                'b' => &["AdditiveBox", "Sketch"][..],
                'd' if version == "0.21.2" => &[
                    "DatumPlane",
                    "Sketch",
                    "AdditiveCylinder",
                    "SketchDatumPoint",
                ][..],
                'd' => &["DatumPlane", "Sketch", "AdditiveCylinder"][..],
                _ => unreachable!(),
            };
            let actual_group = children
                .iter()
                .map(|child| {
                    model
                        .features
                        .iter()
                        .find(|candidate| &candidate.id == child)
                        .and_then(|candidate| candidate.name.as_deref())
                        .unwrap_or_else(|| panic!("{version} F14{variant}: body group child"))
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual_group.as_slice(),
                expected_group,
                "{version} F14{variant}: Body.Group"
            );
            let expected_tip = if variant == 'd' {
                "AdditiveCylinder"
            } else {
                "AdditiveBox"
            };
            let actual_tip = children.active_child().as_ref().and_then(|tip| {
                model
                    .features
                    .iter()
                    .find(|candidate| &candidate.id == tip)
                    .and_then(|candidate| candidate.name.as_deref())
            });
            assert_eq!(
                actual_tip,
                Some(expected_tip),
                "{version} F14{variant}: Body.Tip"
            );
            let native_properties = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::PropertyRecord>("properties")
                .unwrap();
            let body_id = crate::native::native_id("object", "Body");
            let group = native_properties
                .iter()
                .find(|property| property.owner == body_id && property.name == "Group")
                .unwrap_or_else(|| panic!("{version} F14{variant}: Body.Group record"));
            let actual_native_group = group
                .links()
                .iter()
                .map(|link| {
                    link.as_ref()
                        .and_then(crate::native::LinkTarget::object)
                        .unwrap_or_else(|| panic!("{version} F14{variant}: Body.Group link"))
                        .to_owned()
                })
                .collect::<Vec<_>>();
            let expected_native_group = expected_group
                .iter()
                .map(|name| crate::native::native_id("object", name))
                .collect::<Vec<_>>();
            assert_eq!(
                actual_native_group, expected_native_group,
                "{version} F14{variant}: source Body.Group"
            );
            let tip = native_properties
                .iter()
                .find(|property| property.owner == body_id && property.name == "Tip")
                .unwrap_or_else(|| panic!("{version} F14{variant}: Body.Tip record"));
            let [Some(tip)] = tip.links() else {
                panic!("{version} F14{variant}: Body.Tip link")
            };
            assert_eq!(
                tip.object(),
                Some(crate::native::native_id("object", expected_tip).as_str()),
                "{version} F14{variant}: source Body.Tip"
            );
            if variant == 'd' {
                assert!(
                    matches!(solid.kind(), PrimitiveSolidKind::Cylinder { radius, height, angle } if (radius.get() - 4.0).abs() < EPS_PRIMITIVE_FRAME && (height.get() - 12.0).abs() < EPS_PRIMITIVE_FRAME && (angle.get() - std::f64::consts::TAU).abs() < EPS_PRIMITIVE_FRAME)
                );
            } else {
                assert!(
                    matches!(solid.kind(), PrimitiveSolidKind::Box { length, width, height } if (length.get() - 20.0).abs() < EPS_PRIMITIVE_FRAME && (width.get() - 30.0).abs() < EPS_PRIMITIVE_FRAME && (height.get() - 10.0).abs() < EPS_PRIMITIVE_FRAME)
                );
            }
            let report =
                cadmpeg_measure::test_support::round_trip::measured("F14", decoded.ir()).unwrap();
            let [body_id] = feature.evaluation.outputs().as_slice() else {
                panic!("{version} F14{variant}: expected one result")
            };
            let body = report
                .bodies()
                .iter()
                .find(|body| body.body() == body_id.as_str())
                .unwrap();
            let volume = body
                .volume()
                .as_ref()
                .unwrap()
                .measured()
                .unwrap()
                .value()
                .value
                .get();
            let area = body
                .area()
                .as_ref()
                .unwrap()
                .measured()
                .unwrap()
                .value()
                .value
                .get();
            let (expected_volume, expected_area, min, max) = match variant {
                'a' => (
                    20.0 * 30.0 * 10.0,
                    2.0 * (20.0 * 30.0 + 20.0 * 10.0 + 30.0 * 10.0),
                    [0.0, 0.0, 0.0],
                    [20.0, 30.0, 10.0],
                ),
                'b' => (
                    20.0 * 30.0 * 10.0,
                    2.0 * (20.0 * 30.0 + 20.0 * 10.0 + 30.0 * 10.0),
                    [5.0, -7.0, 0.0],
                    [25.0, 23.0, 10.0],
                ),
                'c' => (
                    20.0 * 30.0 * 10.0,
                    2.0 * (20.0 * 30.0 + 20.0 * 10.0 + 30.0 * 10.0),
                    [0.0, -10.0, 0.0],
                    [20.0, 0.0, 30.0],
                ),
                'd' => (
                    std::f64::consts::PI * 4.0 * 4.0 * 12.0,
                    2.0 * std::f64::consts::PI * 4.0 * (4.0 + 12.0),
                    [1.0, -11.0, 3.0],
                    [9.0, -3.0, 15.0],
                ),
                _ => unreachable!(),
            };
            assert!(
                (volume - expected_volume).abs() < EPS_PRIMITIVE_VOLUME,
                "{version} F14{variant}: volume {volume}"
            );
            assert!(
                (area - expected_area).abs() < EPS_PRIMITIVE_AREA,
                "{version} F14{variant}: area {area}"
            );
            let bbox = body.bbox().measured().unwrap().value();
            for (actual, expected) in bbox
                .min
                .coordinates()
                .into_iter()
                .chain(bbox.max.coordinates())
                .zip(min.into_iter().chain(max))
            {
                assert!(
                    (actual.get() - expected).abs() < EPS_PRIMITIVE_BOX,
                    "{version} F14{variant}: bounding box"
                );
            }
            let rows = primitive.rows();
            for i in 0..3 {
                assert!(
                    (rows[i][3] - translation[i]).abs() < EPS_PRIMITIVE_FRAME,
                    "{version} F14{variant}"
                );
                for j in 0..3 {
                    let expected = if variant == 'c' {
                        [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]][i][j]
                    } else if i == j {
                        1.0
                    } else {
                        0.0
                    };
                    assert!((rows[i][j] - expected).abs() < EPS_PRIMITIVE_FRAME);
                }
            }
            if matches!(variant, 'b' | 'd') {
                let sketch = model
                    .sketches
                    .iter()
                    .find(|sketch| sketch.name.as_deref() == Some("Sketch"))
                    .unwrap();
                let point = model
                    .sketch_entities
                    .iter()
                    .find(|entity| {
                        entity.sketch == sketch.id
                            && matches!(
                                entity.geometry.definition(),
                                SketchGeometryDefinition::Point { .. }
                            )
                    })
                    .unwrap();
                let SketchGeometryDefinition::Point { position } = point.geometry.definition()
                else {
                    unreachable!()
                };
                assert!((position.get().u - 5.0).abs() < EPS_PRIMITIVE_FRAME);
                assert!((position.get().v + 7.0).abs() < EPS_PRIMITIVE_FRAME);
                for (name, expected) in [("point_x", 5.0), ("point_y", -7.0)] {
                    let constraint = model
                        .sketch_constraints
                        .iter()
                        .find(|constraint| {
                            constraint.sketch == sketch.id
                                && constraint.name.as_deref() == Some(name)
                        })
                        .unwrap();
                    assert_eq!(constraint.name.as_deref(), Some(name));
                    assert_eq!(constraint.driving, Some(true));
                    assert_eq!(
                        constraint.label_distance,
                        Some(10.0_f64.try_into().unwrap()),
                        "{version} F14{variant}: {name} label distance"
                    );
                    assert_eq!(
                        constraint.label_position,
                        Some(0.0_f64.try_into().unwrap()),
                        "{version} F14{variant}: {name} label position"
                    );
                    let parameter = match constraint.definition.kind() {
                        SketchConstraintDefinitionInput::HorizontalDistance {
                            parameter, ..
                        } if name == "point_x" => parameter,
                        SketchConstraintDefinitionInput::VerticalDistance { parameter, .. }
                            if name == "point_y" =>
                        {
                            parameter
                        }
                        other => panic!("{version} F14{variant}: {name}: {other:?}"),
                    };
                    let value = model
                        .parameters
                        .iter()
                        .find(|candidate| &candidate.id == parameter)
                        .unwrap();
                    let Some(ParameterValue::Length(value)) = value.value.as_ref() else {
                        panic!("{name}: length")
                    };
                    assert!((value.get() - expected).abs() < EPS_PRIMITIVE_FRAME);
                }
                if version == "0.21.2" {
                    let datum_id = crate::native::native_id("object", "SketchDatumPoint");
                    let expressions = native_properties
                        .iter()
                        .find(|property| {
                            property.owner == datum_id && property.name == "ExpressionEngine"
                        })
                        .unwrap_or_else(|| {
                            panic!("{version} F14{variant}: SketchDatumPoint expressions")
                        })
                        .values()
                        .iter()
                        .filter(|value| value.tag == "Expression")
                        .map(|value| {
                            (
                                value.attributes.get("path").map_or_else(
                                    || panic!("{version} F14{variant}: expression path"),
                                    String::as_str,
                                ),
                                value.attributes.get("expression").map_or_else(
                                    || panic!("{version} F14{variant}: expression body"),
                                    String::as_str,
                                ),
                            )
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(
                        expressions.as_slice(),
                        &[
                            (
                                ".Placement.Base.x",
                                "Sketch.Placement.Base.x + Sketch.Constraints.point_x"
                            ),
                            (
                                ".Placement.Base.y",
                                "Sketch.Placement.Base.y + Sketch.Constraints.point_y"
                            ),
                            (".Placement.Base.z", "Sketch.Placement.Base.z"),
                        ],
                        "{version} F14{variant}: native datum point expressions"
                    );
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
            let properties = dimensions.split_whitespace().fold(String::new(), |mut out, pair| {
                use std::fmt::Write as _;
                let (name, value) = pair.split_once('=').unwrap();
                let (runtime, tag) = if name == "Polygon" { ("IntegerConstraint", "Integer") }
                    else if name.starts_with("Angle") { ("Angle", "Float") }
                    else if family == "Wedge" { ("Distance", "Float") }
                    else { ("Length", "Float") };
                write!(out, r#"<Property name="{name}" type="App::Property{runtime}"><{tag} value="{value}"/></Property>"#).unwrap();
                out
            });
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
