// SPDX-License-Identifier: Apache-2.0
//! Design primitives transfer unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::{AngularTermination, BooleanOp, FeatureDefinition, RevolveExtent};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn transfers_revolution_fillet_and_chamfer_semantics() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6" Dependencies="1">
 <ObjectDeps Name="Sketch" Count="0"/>
 <ObjectDeps Name="Revolution" Count="1"><Dep Name="Sketch"/></ObjectDeps>
 <ObjectDeps Name="Fillet" Count="1"><Dep Name="Revolution"/></ObjectDeps>
 <ObjectDeps Name="Chamfer" Count="1"><Dep Name="Fillet"/></ObjectDeps>
 <ObjectDeps Name="LegacyChamfer" Count="1"><Dep Name="Chamfer"/></ObjectDeps>
 <ObjectDeps Name="Profileless" Count="0"/>
 <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
 <Object type="PartDesign::Revolution" name="Revolution" id="2"/>
 <Object type="PartDesign::Fillet" name="Fillet" id="3"/>
 <Object type="PartDesign::Chamfer" name="Chamfer" id="4"/>
 <Object type="PartDesign::Chamfer" name="LegacyChamfer" id="5"/>
 <Object type="PartDesign::Revolution" name="Profileless" id="6"/>
</Objects>
<ObjectData Count="6">
 <Object name="Sketch"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Revolution"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
 <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>
 <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="180"/></Property>
 </Properties></Object>
 <Object name="Fillet"><Properties Count="3">
  <Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Revolution" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Radius" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="UseAllEdges" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Chamfer"><Properties Count="5">
  <Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Fillet" count="1"><Sub value="Edge2"/></LinkSub></Property>
  <Property name="ChamferType" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Size" type="App::PropertyLength"><Float value="1.5"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="30"/></Property>
  <Property name="FlipDirection" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="LegacyChamfer"><Properties Count="2">
  <Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Chamfer" count="1"><Sub value="Edge3"/></LinkSub></Property>
  <Property name="Size" type="App::PropertyLength"><Float value="0.75"/></Property>
 </Properties></Object>
 <Object name="Profileless"><Properties Count="4">
  <Property name="Sketch" type="App::PropertyLink"><Link value=""/></Property>
  <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="360"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("core operations");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(matches!(
        definition("Revolution"),
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: Some(cadmpeg_ir::features::ProfileRef::Sketch(_)),
                extent: Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle }
                }),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join
        } if (angle.0 - std::f64::consts::PI).abs() < 1.0e-12
    ));
    assert!(matches!(
        definition("Fillet"),
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups,
        }
        if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::All,
            radius: cadmpeg_ir::features::RadiusSpec::Constant { radius: cadmpeg_ir::features::Length(2.0) },
            tangency_weight: None,
        }])
    ));
    assert!(matches!(
        definition("Chamfer"),
        cadmpeg_ir::features::FeatureDefinition::Chamfer {
            groups,
            flip_direction: true,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: cadmpeg_ir::features::ChamferSpec::DistanceAngle { distance: cadmpeg_ir::features::Length(1.5), angle }, ..
        }] if (angle.0 - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12)
    ));
    assert!(matches!(
        definition("LegacyChamfer"),
        cadmpeg_ir::features::FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
                spec: cadmpeg_ir::features::ChamferSpec::Distance {
                    distance: cadmpeg_ir::features::Length(0.75)
                },
                ..
            }])
    ));
    assert!(matches!(
        definition("Profileless"),
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction { profile: None, .. },
            ..
        }
    ));
}

#[test]
fn distinguishes_absent_and_malformed_dress_up_flags() {
    fn definition<'a>(
        result: &'a cadmpeg_ir::codec::DecodeResult,
        name: &str,
    ) -> &'a FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    }

    fn document(target: &str, replacement: Option<&str>) -> String {
        let property = |name: &str, value: &str, include: bool| {
            if !include {
                String::new()
            } else if target == name {
                replacement.unwrap_or_default().to_owned()
            } else {
                format!(
                    r#"<Property name="{name}" type="App::PropertyBool"><Bool value="{value}"/></Property>"#
                )
            }
        };
        let fillet_properties = format!(
            r#"<Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Edge1"/></LinkSub></Property>
<Property name="Radius" type="App::PropertyLength"><Float value="2"/></Property>
{use_all_edges}"#,
            use_all_edges = property("UseAllEdges", "true", true),
        );
        let chamfer_properties = format!(
            r#"<Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Edge1"/></LinkSub></Property>
<Property name="ChamferType" type="App::PropertyEnumeration"><Integer value="2"/></Property>
<Property name="Size" type="App::PropertyLength"><Float value="1.5"/></Property>
<Property name="Angle" type="App::PropertyAngle"><Float value="30"/></Property>
{use_all_edges}
{flip_direction}"#,
            use_all_edges = property("UseAllEdges", "true", true),
            flip_direction = property("FlipDirection", "false", true),
        );
        let fillet_count = fillet_properties.matches("<Property ").count();
        let chamfer_count = chamfer_properties.matches("<Property ").count();
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="Part::Feature" name="Base"/><Object type="PartDesign::Fillet" name="Fillet"/><Object type="PartDesign::Chamfer" name="Chamfer"/></Objects>
<ObjectData Count="3"><Object name="Base"><Properties Count="0"/></Object>
<Object name="Fillet"><Properties Count="{fillet_count}">{fillet_properties}</Properties></Object>
<Object name="Chamfer"><Properties Count="{chamfer_count}">{chamfer_properties}</Properties></Object></ObjectData></Document>"#
        )
    }

    fn assert_fillet(definition: &FeatureDefinition, all_edges: bool) {
        assert!(matches!(
            definition,
            FeatureDefinition::Fillet { groups }
                if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup { edges, .. }]
                    if matches!((all_edges, edges),
                        (true, cadmpeg_ir::features::EdgeSelection::All)
                            | (false, cadmpeg_ir::features::EdgeSelection::Native(_))))
        ));
    }

    fn assert_chamfer(definition: &FeatureDefinition, all_edges: bool, flip_direction: bool) {
        assert!(matches!(
            definition,
            FeatureDefinition::Chamfer {
                groups,
                flip_direction: actual_flip,
            } if *actual_flip == flip_direction
                && matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup { edges, .. }]
                    if matches!((all_edges, edges),
                        (true, cadmpeg_ir::features::EdgeSelection::All)
                            | (false, cadmpeg_ir::features::EdgeSelection::Native(_))))
        ));
    }

    let malformed_values = [
        r#"<Property name="TARGET" type="App::PropertyString"><String value="true"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyInteger"><Integer value="1"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Bool value="1"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Wrapper><Bool value="true"/></Wrapper></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Bool value="false"/><Bool value="true"/></Property>"#,
    ];
    for target in ["UseAllEdges", "FlipDirection"] {
        let absent = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document(target, None))),
                &DecodeOptions::default(),
            )
            .expect("absent dress-up flag");
        assert_fillet(definition(&absent, "Fillet"), target != "UseAllEdges");
        assert_chamfer(
            definition(&absent, "Chamfer"),
            target != "UseAllEdges",
            false,
        );
        assert_valid_document(absent.ir());

        let valid_property = format!(
            r#"<Property name="{target}" type="App::PropertyBool"><Bool value="true"/></Property>"#
        );
        let valid = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document(target, Some(&valid_property)))),
                &DecodeOptions::default(),
            )
            .expect("valid dress-up flag");
        assert_fillet(definition(&valid, "Fillet"), true);
        assert_chamfer(
            definition(&valid, "Chamfer"),
            true,
            target == "FlipDirection",
        );
        assert_valid_document(valid.ir());

        for malformed in malformed_values {
            let replacement = malformed.replace("TARGET", target);
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document(target, Some(&replacement)))),
                    &DecodeOptions::default(),
                )
                .expect("malformed dress-up flag");
            if target == "UseAllEdges" {
                assert!(matches!(
                    definition(&result, "Fillet"),
                    FeatureDefinition::Native { kind, .. } if kind == "PartDesign::Fillet"
                ));
                assert!(matches!(
                    definition(&result, "Chamfer"),
                    FeatureDefinition::Native { kind, .. } if kind == "PartDesign::Chamfer"
                ));
                assert_eq!(result.report().losses.len(), 2);
            } else {
                assert_fillet(definition(&result, "Fillet"), true);
                assert!(matches!(
                    definition(&result, "Chamfer"),
                    FeatureDefinition::Native { kind, .. } if kind == "PartDesign::Chamfer"
                ));
                assert_eq!(result.report().losses.len(), 1);
            }
            assert_valid_document(result.ir());
        }
    }
}

#[test]
fn applies_legacy_partdesign_chamfer_flip_migration() {
    fn document(program_version: Option<&str>, chamfer_type: u32) -> String {
        let program_version = program_version.map_or(String::new(), |version| {
            format!(r#" ProgramVersion="{version}""#)
        });
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1"{program_version}>
<Objects Count="2"><Object type="Part::Feature" name="Base"/><Object type="PartDesign::Chamfer" name="Chamfer"/></Objects>
<ObjectData Count="2"><Object name="Base"><Properties Count="0"/></Object>
<Object name="Chamfer"><Properties Count="7">
<Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Edge1"/></LinkSub></Property>
<Property name="ChamferType" type="App::PropertyEnumeration"><Integer value="{chamfer_type}"/></Property>
<Property name="Size" type="App::PropertyLength"><Float value="1.5"/></Property>
<Property name="Size2" type="App::PropertyLength"><Float value="2"/></Property>
<Property name="Angle" type="App::PropertyAngle"><Float value="30"/></Property>
<Property name="UseAllEdges" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="FlipDirection" type="App::PropertyBool"><Bool value="true"/></Property>
</Properties></Object></ObjectData></Document>"#
        )
    }

    fn flip_direction(result: &cadmpeg_ir::codec::DecodeResult) -> bool {
        let definition = &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Chamfer"))
            .expect("chamfer")
            .definition;
        match definition {
            FeatureDefinition::Chamfer { flip_direction, .. } => *flip_direction,
            definition => panic!("unexpected chamfer definition: {definition:?}"),
        }
    }

    for (program_version, chamfer_type, expected) in [
        (Some("0.21R1234"), 1, false),
        (Some("0.21R1234"), 2, false),
        (Some("0.21R1234"), 0, true),
        (Some("1.0R1234"), 1, true),
        (None, 1, true),
    ] {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document(program_version, chamfer_type))),
                &DecodeOptions::default(),
            )
            .expect("versioned chamfer");
        assert_eq!(flip_direction(&result), expected);
        assert_valid_document(result.ir());
        assert!(result.report().losses.is_empty());
    }
}

#[test]
fn distinguishes_absent_and_malformed_part_extrusion_flags() {
    fn definition(result: &cadmpeg_ir::codec::DecodeResult) -> &FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrusion"))
            .expect("extrusion feature")
            .definition
    }

    let base_properties = [
        (
            "Solid",
            r#"<Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "Reversed",
            r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "Symmetric",
            r#"<Property name="Symmetric" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
    ];
    let document = |target: &str, replacement: Option<&str>| {
        let mut properties = String::from(
            r#"<Property name="Base" type="App::PropertyLink"><Link value="Profile"/></Property><Property name="Dir" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="8"/></Property><Property name="DirMode" type="App::PropertyEnumeration"><Integer value="0"/></Property><Property name="LengthFwd" type="App::PropertyDistance"><Float value="6"/></Property><Property name="LengthRev" type="App::PropertyDistance"><Float value="2"/></Property><Property name="TaperAngle" type="App::PropertyAngle"><Float value="0"/></Property><Property name="TaperAngleRev" type="App::PropertyAngle"><Float value="0"/></Property>"#,
        );
        for (name, property) in base_properties {
            if name != target {
                properties.push_str(property);
            }
        }
        if let Some(replacement) = replacement {
            properties.push_str(replacement);
        }
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="2"><Object type="Part::Feature" name="Profile" id="1"/><Object type="Part::Extrusion" name="Extrusion" id="2"/></Objects><ObjectData Count="2"><Object name="Profile"><Properties Count="0"/></Object><Object name="Extrusion"><Properties Count="{count}">{properties}</Properties></Object></ObjectData></Document>"#,
            count = properties.matches("<Property ").count(),
        )
    };
    let decode = |document: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("extrusion flag document")
    };
    let assert_native = |result: &cadmpeg_ir::codec::DecodeResult| {
        assert!(matches!(
            definition(result),
            FeatureDefinition::Native { kind, .. } if kind == "Part::Extrusion"
        ));
        assert_eq!(result.report().losses.len(), 1);
        assert!(result.report().losses.iter().all(|loss| {
            loss.code.namespace == "fcstd"
                && loss.code.code == "feature.native-kind-retained"
                && loss.severity == cadmpeg_ir::Severity::Blocking
        }));
    };

    for target in ["Solid", "Reversed", "Symmetric"] {
        let result = decode(&document(target, None));
        assert!(result.report().losses.is_empty(), "{target}");
        let FeatureDefinition::Extrude {
            direction,
            extent,
            solid,
            ..
        } = definition(&result)
        else {
            panic!("{target} absent carrier");
        };
        match target {
            "Solid" => assert_eq!(*solid, Some(false)),
            "Reversed" => assert!(matches!(
                direction,
                cadmpeg_ir::features::ExtrudeDirection::Explicit(vector)
                    if vector.z == 1.0
            )),
            "Symmetric" => assert!(matches!(
                extent,
                cadmpeg_ir::features::ExtrudeExtent::TwoSided { .. }
            )),
            _ => unreachable!(),
        }
    }

    let valid = [
        (
            "Solid",
            r#"<Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "Reversed",
            r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "Symmetric",
            r#"<Property name="Symmetric" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
    ];
    for (target, replacement) in valid {
        let result = decode(&document(target, Some(replacement)));
        assert!(result.report().losses.is_empty(), "{target}");
        let FeatureDefinition::Extrude {
            direction,
            extent,
            solid,
            ..
        } = definition(&result)
        else {
            panic!("{target} valid carrier");
        };
        match target {
            "Solid" => assert_eq!(*solid, Some(true)),
            "Reversed" => assert!(matches!(
                direction,
                cadmpeg_ir::features::ExtrudeDirection::Explicit(vector)
                    if vector.z == -1.0
            )),
            "Symmetric" => assert!(matches!(
                extent,
                cadmpeg_ir::features::ExtrudeExtent::Symmetric { .. }
            )),
            _ => unreachable!(),
        }
    }

    let malformed_values = [
        ("App::PropertyString", r#"<String value="false"/>"#),
        ("App::PropertyInteger", r#"<Integer value="0"/>"#),
        ("App::PropertyBool", r#"<Bool value="bad"/>"#),
        (
            "App::PropertyBool",
            r#"<Wrapper><Bool value="false"/></Wrapper>"#,
        ),
        (
            "App::PropertyBool",
            r#"<Bool value="false"/><Bool value="true"/>"#,
        ),
        ("App::PropertyBool", r#"<Bool value="1"/>"#),
        ("App::PropertyBool", r#"<Bool value="0"/>"#),
        ("App::PropertyBool", r#"<Bool value="2"/>"#),
    ];
    for target in ["Solid", "Reversed", "Symmetric"] {
        for (type_name, value) in malformed_values {
            let replacement =
                format!(r#"<Property name="{target}" type="{type_name}">{value}</Property>"#);
            assert_native(&decode(&document(target, Some(&replacement))));
        }
    }
}

#[test]
fn distinguishes_absent_and_malformed_revolution_flags() {
    fn definition<'a>(
        result: &'a cadmpeg_ir::codec::DecodeResult,
        name: &str,
    ) -> &'a FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    }

    let part_design_flags = [
        (
            "Midplane",
            r#"<Property name="Midplane" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "Reversed",
            r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "AllowMultiFace",
            r#"<Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
    ];
    let standalone_flags = [
        (
            "Symmetric",
            r#"<Property name="Symmetric" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "Solid",
            r#"<Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
    ];
    let properties = |name: &str, target_name: &str, target: &str, replacement: Option<&str>| {
        let mut value = if name == "PartDesignRevolution" {
            String::from(
                r#"<Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property><Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property><Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property><Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property><Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property>"#,
            )
        } else {
            String::from(
                r#"<Property name="Source" type="App::PropertyLink"><Link value="Sketch"/></Property><Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property><Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property><Property name="Angle" type="App::PropertyAngle"><Float value="120"/></Property>"#,
            )
        };
        let flags = if name == "PartDesignRevolution" {
            &part_design_flags[..]
        } else {
            &standalone_flags[..]
        };
        for &(property_name, property) in flags {
            if !(name == target_name && property_name == target) {
                value.push_str(property);
            }
        }
        if name == target_name {
            if let Some(replacement) = replacement {
                value.push_str(replacement);
            }
        }
        value
    };
    let document = |target_name: &str, target: &str, replacement: Option<&str>| {
        let design = properties("PartDesignRevolution", target_name, target, replacement);
        let standalone = properties("StandaloneRevolution", target_name, target, replacement);
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="3"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/><Object type="PartDesign::Revolution" name="PartDesignRevolution" id="2"/><Object type="Part::Revolution" name="StandaloneRevolution" id="3"/></Objects><ObjectData Count="3"><Object name="Sketch"><Properties Count="0"/></Object><Object name="PartDesignRevolution"><Properties Count="{design_count}">{design}</Properties></Object><Object name="StandaloneRevolution"><Properties Count="{standalone_count}">{standalone}</Properties></Object></ObjectData></Document>"#,
            design_count = design.matches("<Property ").count(),
            standalone_count = standalone.matches("<Property ").count(),
        )
    };
    let decode = |document: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("revolution flag document")
    };
    let assert_native = |result: &cadmpeg_ir::codec::DecodeResult, name: &str, kind: &str| {
        assert!(
            matches!(definition(result, name), FeatureDefinition::Native { kind: value, .. } if value == kind)
        );
        assert_eq!(result.report().losses.len(), 1);
        assert!(result.report().losses.iter().all(|loss| {
            loss.code.namespace == "fcstd"
                && loss.code.code == "feature.native-kind-retained"
                && loss.severity == cadmpeg_ir::Severity::Blocking
        }));
    };

    for (name, target) in [
        ("PartDesignRevolution", "Midplane"),
        ("PartDesignRevolution", "Reversed"),
        ("PartDesignRevolution", "AllowMultiFace"),
        ("StandaloneRevolution", "Symmetric"),
        ("StandaloneRevolution", "Solid"),
    ] {
        let result = decode(&document(name, target, None));
        assert!(result.report().losses.is_empty(), "{name}.{target}");
        match (name, target) {
            ("PartDesignRevolution", "Midplane") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        extent: Some(RevolveExtent::OneSided { .. }),
                        ..
                    },
                    ..
                }
            )),
            ("PartDesignRevolution", "Reversed") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        axis: Some(axis), ..
                    },
                    ..
                } if axis.direction.y == 1.0
            )),
            ("PartDesignRevolution", "AllowMultiFace") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        allow_multi_profile_faces: Some(false),
                        ..
                    },
                    ..
                }
            )),
            ("StandaloneRevolution", "Symmetric") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        extent: Some(RevolveExtent::OneSided { .. }),
                        ..
                    },
                    ..
                }
            )),
            ("StandaloneRevolution", "Solid") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        solid: Some(false),
                        ..
                    },
                    ..
                }
            )),
            _ => unreachable!(),
        }
    }

    let valid = [
        (
            "PartDesignRevolution",
            "Midplane",
            r#"<Property name="Midplane" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "PartDesignRevolution",
            "Reversed",
            r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "PartDesignRevolution",
            "AllowMultiFace",
            r#"<Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "StandaloneRevolution",
            "Symmetric",
            r#"<Property name="Symmetric" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "StandaloneRevolution",
            "Solid",
            r#"<Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
    ];
    for (name, target, replacement) in valid {
        let result = decode(&document(name, target, Some(replacement)));
        assert!(result.report().losses.is_empty(), "{name}.{target}");
        match (name, target) {
            ("PartDesignRevolution", "Midplane") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        extent: Some(RevolveExtent::Symmetric { .. }),
                        ..
                    },
                    ..
                }
            )),
            ("PartDesignRevolution", "Reversed") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        axis: Some(axis), ..
                    },
                    ..
                } if axis.direction.y == -1.0
            )),
            ("PartDesignRevolution", "AllowMultiFace") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        allow_multi_profile_faces: Some(false),
                        ..
                    },
                    ..
                }
            )),
            ("StandaloneRevolution", "Symmetric") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        extent: Some(RevolveExtent::Symmetric { .. }),
                        ..
                    },
                    ..
                }
            )),
            ("StandaloneRevolution", "Solid") => assert!(matches!(
                definition(&result, name),
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        solid: Some(true),
                        ..
                    },
                    ..
                }
            )),
            _ => unreachable!(),
        }
    }

    let malformed_values = [
        ("App::PropertyString", r#"<String value="false"/>"#),
        ("App::PropertyInteger", r#"<Integer value="0"/>"#),
        ("App::PropertyBool", r#"<Bool value="bad"/>"#),
        (
            "App::PropertyBool",
            r#"<Wrapper><Bool value="false"/></Wrapper>"#,
        ),
        (
            "App::PropertyBool",
            r#"<Bool value="false"/><Bool value="true"/>"#,
        ),
        ("App::PropertyBool", r#"<Bool value="1"/>"#),
        ("App::PropertyBool", r#"<Bool value="0"/>"#),
        ("App::PropertyBool", r#"<Bool value="2"/>"#),
    ];
    for (name, target) in [
        ("PartDesignRevolution", "Midplane"),
        ("PartDesignRevolution", "Reversed"),
        ("PartDesignRevolution", "AllowMultiFace"),
        ("StandaloneRevolution", "Symmetric"),
        ("StandaloneRevolution", "Solid"),
    ] {
        for (type_name, value) in malformed_values {
            let replacement =
                format!(r#"<Property name="{target}" type="{type_name}">{value}</Property>"#);
            assert_native(
                &decode(&document(name, target, Some(&replacement))),
                name,
                if name == "PartDesignRevolution" {
                    "PartDesign::Revolution"
                } else {
                    "Part::Revolution"
                },
            );
        }
    }
}

#[test]
fn transfers_non_default_revolution_branches() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
 <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
 <Object type="PartDesign::Revolution" name="ToFirst" id="2"/>
 <Object type="PartDesign::Revolution" name="ToFace" id="3"/>
 <Object type="PartDesign::Revolution" name="TwoAngles" id="4"/>
 <Object type="PartDesign::Revolution" name="Midplane" id="5"/>
 <Object type="PartDesign::Groove" name="ThroughAll" id="6"/>
 <Object type="Part::Revolution" name="Standalone" id="7"/>
</Objects>
<ObjectData Count="7">
 <Object name="Sketch"><Properties Count="0"/></Object>
 <Object name="ToFirst"><Properties Count="4">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="2" valueZ="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="2"/></Property>
 </Properties></Object>
 <Object name="ToFace"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="UpToFace" type="App::PropertyLinkSub"><LinkSub value="Standalone" count="1"><Sub value="Face1"/></LinkSub></Property>
 </Properties></Object>
 <Object name="TwoAngles"><Properties Count="6">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="4"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="120"/></Property>
  <Property name="Angle2" type="App::PropertyAngle"><Float value="30"/></Property>
 </Properties></Object>
 <Object name="Midplane"><Properties Count="10">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="3" valueZ="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property>
  <Property name="Midplane" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="ReferenceAxis" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="1"><Sub value="H_Axis"/></LinkSub></Property>
  <Property name="FuseOrder" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="ThroughAll"><Properties Count="4">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="Standalone"><Properties Count="8">
  <Property name="Source" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="4"/></Property>
  <Property name="Angle" type="App::PropertyFloatConstraint"><Float value="45"/></Property>
  <Property name="Symmetric" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AxisLink" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="FaceMakerClass" type="App::PropertyString"><String value="Part::FaceMakerUnified"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("revolution branches");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("ToFirst"),
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(axis),
                extent: Some(RevolveExtent::OneSided {
                    termination: AngularTermination::ToFirst
                }),
                ..
            },
            ..
        } if axis.direction.y == 1.0 && axis.origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    ));
    assert!(matches!(
        definition("ToFace"),
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: AngularTermination::ToFace { .. }
                }),
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        definition("TwoAngles"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { extent: Some(RevolveExtent::TwoSided { first: AngularTermination::Angle { angle: first }, second: AngularTermination::Angle { angle: second } }), .. }, .. }
            if (first.0 - 120_f64.to_radians()).abs() < 1.0e-12 && (second.0 - 30_f64.to_radians()).abs() < 1.0e-12
    ));
    assert!(matches!(
        definition("Midplane"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { axis: Some(axis), extent: Some(RevolveExtent::Symmetric { termination: AngularTermination::Angle { .. } }), axis_reference: Some(cadmpeg_ir::features::PathRef::Native(reference)), fuse_order: Some(cadmpeg_ir::features::RevolutionFuseOrder::FeatureFirst), solid: Some(true), allow_multi_profile_faces: Some(false), .. }, .. }
            if axis.direction.y == -1.0 && reference.ends_with(":ReferenceAxis")
    ));
    assert!(matches!(
        definition("ThroughAll"),
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: AngularTermination::ThroughAll
                }),
                ..
            },
            op: BooleanOp::Cut
        }
    ));
    assert!(matches!(
        definition("Standalone"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { profile: Some(cadmpeg_ir::features::ProfileRef::Sketch(_)), axis: Some(axis), extent: Some(RevolveExtent::Symmetric { termination: AngularTermination::Angle { .. } }), axis_reference: Some(cadmpeg_ir::features::PathRef::Native(reference)), solid: Some(true), face_maker: Some(face_maker), .. }, op: BooleanOp::NewBody }
            if axis.direction.z == 1.0 && reference.ends_with(":AxisLink")
                && *face_maker == cadmpeg_ir::features::FaceMaker::Unified
    ));
}

#[test]
pub(crate) fn transfers_part_and_partdesign_analytic_primitives() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3" Dependencies="1">
 <ObjectDeps Name="Box" Count="0"/>
 <ObjectDeps Name="AddCylinder" Count="1"><Dep Name="Box"/></ObjectDeps>
 <ObjectDeps Name="CutCone" Count="1"><Dep Name="AddCylinder"/></ObjectDeps>
 <Object type="Part::Box" name="Box" id="1"/>
 <Object type="PartDesign::AdditiveCylinder" name="AddCylinder" id="2"/>
 <Object type="PartDesign::SubtractiveCone" name="CutCone" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Box"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="20"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="30"/></Property>
 </Properties></Object>
 <Object name="AddCylinder"><Properties Count="3">
  <Property name="Radius" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="180"/></Property>
 </Properties></Object>
 <Object name="CutCone"><Properties Count="4">
  <Property name="Radius1" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Radius2" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="6"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="360"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("primitives");
    assert_eq!(result.ir().ir_version(), cadmpeg_ir::IR_VERSION);
    let feature = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("primitive")
            .definition
    };
    assert!(matches!(
        feature("Box"),
        cadmpeg_ir::features::FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::Box {
                length: cadmpeg_ir::features::Length(10.0),
                width: cadmpeg_ir::features::Length(20.0),
                height: cadmpeg_ir::features::Length(30.0),
            },
            op: cadmpeg_ir::features::BooleanOp::NewBody,
        }
    ));
    assert!(matches!(
        feature("AddCylinder"),
        cadmpeg_ir::features::FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::Cylinder {
                angle: cadmpeg_ir::features::Angle(angle),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        } if (angle - std::f64::consts::PI).abs() < 1.0e-12
    ));
    assert!(matches!(
        feature("CutCone"),
        cadmpeg_ir::features::FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::Cone { .. },
            op: cadmpeg_ir::features::BooleanOp::Cut,
        }
    ));
    assert!(result.report().losses.is_empty());
    let findings = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).findings;
    assert!(
        findings
            .iter()
            .all(|finding| finding.check != cadmpeg_ir::Check::GeometricConsistency),
        "{findings:#?}"
    );
}

#[test]
fn retains_vendor_qualified_primitive_like_types_as_native_objects() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1">
 <Object type="Part::VendorBox" name="VendorBox" id="1"/>
</Objects>
<ObjectData Count="1">
 <Object name="VendorBox"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="20"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="30"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("vendor primitive-like object");
    let objects = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace")
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    assert!(objects
        .iter()
        .any(|object| { object.type_name == "Part::VendorBox" && object.raw_xml.is_some() }));
    assert!(result.ir().model.features.is_empty());
}

#[test]
fn transfers_parametric_part_helix_and_spiral_construction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Helix" name="Helix" id="1"/>
 <Object type="Part::Spiral" name="Spiral" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Helix"><Properties Count="7">
  <Property name="Pitch" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="20"/></Property>
  <Property name="Radius" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="12"/></Property>
  <Property name="SegmentLength" type="App::PropertyQuantityConstraint"><Float value="0.5"/></Property>
  <Property name="LocalCoord" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Style" type="App::PropertyEnumeration"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="Spiral"><Properties Count="4">
  <Property name="Growth" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Radius" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="Rotations" type="App::PropertyQuantity"><Float value="3.5"/></Property>
  <Property name="SegmentLength" type="App::PropertyQuantityConstraint"><Float value="0.25"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("parametric curves");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Helix"),
        cadmpeg_ir::features::FeatureDefinition::Helix {
            radius: cadmpeg_ir::features::Length(3.0),
            shape: cadmpeg_ir::features::HelixShape::Conical {
                pitch,
                cone_angle: cadmpeg_ir::features::Angle(angle),
            },
            revolutions: 5.0,
            clockwise: true,
            segment_turns: Some(0.5),
            construction_style: Some(cadmpeg_ir::features::HelixConstructionStyle::Corrected),
            ..
        } if (pitch.get().0 - 4.0).abs() < 1.0e-12
            && (*angle - 12_f64.to_radians()).abs() < 1.0e-12
    ));
    assert!(matches!(
        definition("Spiral"),
        cadmpeg_ir::features::FeatureDefinition::Helix {
            radius: cadmpeg_ir::features::Length(5.0),
            shape: cadmpeg_ir::features::HelixShape::Spiral {
                radial_growth: cadmpeg_ir::features::Length(2.0),
            },
            revolutions: 3.5,
            segment_turns: Some(0.25),
            construction_style: None,
            ..
        }
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn transfers_complete_additive_and_outside_subtractive_helices() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
 <Object type="PartDesign::AdditiveHelix" name="Spring" id="2"/>
 <Object type="PartDesign::SubtractiveHelix" name="OutsideCut" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Profile"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Spring"><Properties Count="14">
  <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Profile" count="1"><Sub value=""/></LinkSub></Property>
  <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Pitch" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Turns" type="App::PropertyFloatConstraint"><Float value="2.5"/></Property>
  <Property name="Growth" type="App::PropertyDistance"><Float value="1"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="14.0362434679"/></Property>
  <Property name="LeftHanded" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Outside" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Tolerance" type="App::PropertyFloatConstraint"><Float value="0.25"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="OutsideCut"><Properties Count="11">
  <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Profile" count="1"><Sub value=""/></LinkSub></Property>
  <Property name="ReferenceAxis" type="App::PropertyLinkSub"><LinkSub value="Profile" count="1"><Sub value="N_Axis"/></LinkSub></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="Pitch" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Turns" type="App::PropertyFloatConstraint"><Float value="3"/></Property>
  <Property name="Growth" type="App::PropertyDistance"><Float value="2"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="0"/></Property>
  <Property name="LeftHanded" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Outside" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("helical sweeps");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(
        matches!(definition("Spring"), cadmpeg_ir::features::FeatureDefinition::HelicalSweep {
        construction,
        op: cadmpeg_ir::features::BooleanOp::Join,
    } if construction.law == cadmpeg_ir::features::HelicalSweepLaw::PitchTurnsAngle
        && construction.axis_origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
        && construction.left_handed && construction.reversed
        && construction.turns == 2.5 && construction.tolerance == Some(0.25)
        && construction.allow_multi_profile_faces == Some(false))
    );
    assert!(
        matches!(definition("OutsideCut"), cadmpeg_ir::features::FeatureDefinition::HelicalSweep {
        construction,
        op: cadmpeg_ir::features::BooleanOp::Intersect,
    } if construction.law == cadmpeg_ir::features::HelicalSweepLaw::HeightTurnsGrowth
        && construction.pitch.0 == 0.0 && construction.radial_growth.0 == 2.0
        && construction.axis_direction == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        && construction.tolerance == Some(0.1)
        && construction.allow_multi_profile_faces == Some(false))
    );
    assert!(result.report().losses.is_empty());
}

#[test]
fn distinguishes_absent_and_malformed_helix_carriers() {
    fn document(mutation: Option<(&str, &str, &str)>) -> String {
        let choose = |object: &str, name: &str, default: &str| {
            mutation
                .filter(|(target_object, target_name, _)| {
                    *target_object == object && *target_name == name
                })
                .map_or_else(
                    || default.to_owned(),
                    |(_, _, replacement)| replacement.to_owned(),
                )
        };
        let part_helix = [
            choose(
                "PartHelix",
                "Pitch",
                r#"<Property name="Pitch" type="App::PropertyLength"><Float value="4"/></Property>"#,
            ),
            choose(
                "PartHelix",
                "Height",
                r#"<Property name="Height" type="App::PropertyLength"><Float value="20"/></Property>"#,
            ),
            choose(
                "PartHelix",
                "Radius",
                r#"<Property name="Radius" type="App::PropertyLength"><Float value="3"/></Property>"#,
            ),
            choose(
                "PartHelix",
                "Angle",
                r#"<Property name="Angle" type="App::PropertyAngle"><Float value="12"/></Property>"#,
            ),
            choose(
                "PartHelix",
                "SegmentLength",
                r#"<Property name="SegmentLength" type="App::PropertyQuantityConstraint"><Float value="0.5"/></Property>"#,
            ),
            choose(
                "PartHelix",
                "LocalCoord",
                r#"<Property name="LocalCoord" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
            ),
            choose(
                "PartHelix",
                "Style",
                r#"<Property name="Style" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
            ),
        ]
        .join("");
        let spiral = [
            choose(
                "Spiral",
                "Growth",
                r#"<Property name="Growth" type="App::PropertyLength"><Float value="2"/></Property>"#,
            ),
            choose(
                "Spiral",
                "Radius",
                r#"<Property name="Radius" type="App::PropertyLength"><Float value="5"/></Property>"#,
            ),
            choose(
                "Spiral",
                "Rotations",
                r#"<Property name="Rotations" type="App::PropertyQuantityConstraint"><Float value="3.5"/></Property>"#,
            ),
            choose(
                "Spiral",
                "SegmentLength",
                r#"<Property name="SegmentLength" type="App::PropertyQuantityConstraint"><Float value="0.25"/></Property>"#,
            ),
        ]
        .join("");
        let additive = [
            r#"<Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>"#
                .to_owned(),
            r#"<Property name="Base" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>"#
                .to_owned(),
            r#"<Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>"#
                .to_owned(),
            choose(
                "AdditiveHelix",
                "Mode",
                r#"<Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "Pitch",
                r#"<Property name="Pitch" type="App::PropertyLength"><Float value="4"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "Height",
                r#"<Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "Turns",
                r#"<Property name="Turns" type="App::PropertyFloatConstraint"><Float value="2.5"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "Growth",
                r#"<Property name="Growth" type="App::PropertyDistance"><Float value="1"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "Angle",
                r#"<Property name="Angle" type="App::PropertyAngle"><Float value="14"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "LeftHanded",
                r#"<Property name="LeftHanded" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "Reversed",
                r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "Outside",
                r#"<Property name="Outside" type="App::PropertyBool"><Bool value="false"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "Tolerance",
                r#"<Property name="Tolerance" type="App::PropertyFloatConstraint"><Float value="0.25"/></Property>"#,
            ),
            choose(
                "AdditiveHelix",
                "AllowMultiFace",
                r#"<Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>"#,
            ),
        ]
        .join("");
        let subtractive = [
            r#"<Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>"#
                .to_owned(),
            r#"<Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>"#
                .to_owned(),
            r#"<Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>"#
                .to_owned(),
            choose(
                "SubtractiveHelix",
                "Mode",
                r#"<Property name="Mode" type="App::PropertyEnumeration"><Integer value="3"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "Pitch",
                r#"<Property name="Pitch" type="App::PropertyLength"><Float value="10"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "Height",
                r#"<Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "Turns",
                r#"<Property name="Turns" type="App::PropertyFloatConstraint"><Float value="3"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "Growth",
                r#"<Property name="Growth" type="App::PropertyDistance"><Float value="2"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "Angle",
                r#"<Property name="Angle" type="App::PropertyAngle"><Float value="0"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "LeftHanded",
                r#"<Property name="LeftHanded" type="App::PropertyBool"><Bool value="false"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "Reversed",
                r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "Outside",
                r#"<Property name="Outside" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "Tolerance",
                r#"<Property name="Tolerance" type="App::PropertyFloatConstraint"><Float value="0.25"/></Property>"#,
            ),
            choose(
                "SubtractiveHelix",
                "AllowMultiFace",
                r#"<Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>"#,
            ),
        ]
        .join("");
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Part::Helix" name="PartHelix" id="1"/>
 <Object type="Part::Spiral" name="Spiral" id="2"/>
 <Object type="Sketcher::SketchObject" name="Profile" id="3"/>
 <Object type="PartDesign::AdditiveHelix" name="AdditiveHelix" id="4"/>
 <Object type="PartDesign::SubtractiveHelix" name="SubtractiveHelix" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="PartHelix"><Properties Count="{part_helix_count}">{part_helix}</Properties></Object>
 <Object name="Spiral"><Properties Count="{spiral_count}">{spiral}</Properties></Object>
 <Object name="Profile"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="AdditiveHelix"><Properties Count="{additive_count}">{additive}</Properties></Object>
 <Object name="SubtractiveHelix"><Properties Count="{subtractive_count}">{subtractive}</Properties></Object>
</ObjectData></Document>"#,
            part_helix_count = part_helix.matches("<Property ").count(),
            spiral_count = spiral.matches("<Property ").count(),
            additive_count = additive.matches("<Property ").count(),
            subtractive_count = subtractive.matches("<Property ").count(),
        )
    }

    fn definition<'a>(
        result: &'a cadmpeg_ir::codec::DecodeResult,
        name: &str,
    ) -> &'a FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    }

    let decode = |document: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("helix carrier document")
    };
    let assert_native = |result: &cadmpeg_ir::codec::DecodeResult, name: &str, kind: &str| {
        let actual = definition(result, name);
        assert!(
            matches!(actual, FeatureDefinition::Native { kind: value, .. } if value == kind),
            "{name} expected native {kind}, got {actual:?}"
        );
        assert_eq!(result.report().losses.len(), 1);
        assert!(result.report().losses.iter().all(|loss| {
            loss.code.namespace == "fcstd"
                && loss.code.code == "feature.native-kind-retained"
                && loss.severity == cadmpeg_ir::Severity::Blocking
        }));
    };

    for (object, name) in [
        ("PartHelix", "LocalCoord"),
        ("PartHelix", "Style"),
        ("PartHelix", "SegmentLength"),
        ("Spiral", "SegmentLength"),
        ("AdditiveHelix", "Mode"),
        ("AdditiveHelix", "LeftHanded"),
        ("AdditiveHelix", "Reversed"),
        ("AdditiveHelix", "Outside"),
        ("AdditiveHelix", "Tolerance"),
        ("AdditiveHelix", "AllowMultiFace"),
        ("SubtractiveHelix", "Outside"),
    ] {
        let result = decode(&document(Some((object, name, ""))));
        assert!(result.report().losses.is_empty(), "{object}.{name}");
        match (object, name) {
            ("PartHelix", "LocalCoord") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Helix {
                    clockwise: false,
                    ..
                }
            )),
            ("PartHelix", "Style") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Helix {
                    construction_style: Some(cadmpeg_ir::features::HelixConstructionStyle::Legacy),
                    ..
                }
            )),
            ("PartHelix", "SegmentLength") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Helix {
                    segment_turns: None,
                    ..
                }
            )),
            ("Spiral", "SegmentLength") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Helix {
                    segment_turns: Some(value),
                    ..
                } if *value == 1.0
            )),
            ("AdditiveHelix", "Mode") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::HelicalSweep {
                    construction,
                    ..
                } if construction.law == cadmpeg_ir::features::HelicalSweepLaw::PitchHeightAngle
            )),
            ("AdditiveHelix", "LeftHanded") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::HelicalSweep { construction, .. }
                    if !construction.left_handed
            )),
            ("AdditiveHelix", "Reversed") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::HelicalSweep { construction, .. }
                    if !construction.reversed
            )),
            ("AdditiveHelix", "Outside") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::HelicalSweep {
                    op: cadmpeg_ir::features::BooleanOp::Join,
                    ..
                }
            )),
            ("AdditiveHelix", "Tolerance") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::HelicalSweep { construction, .. }
                    if construction.tolerance == Some(0.1)
            )),
            ("AdditiveHelix", "AllowMultiFace") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::HelicalSweep { construction, .. }
                    if construction.allow_multi_profile_faces == Some(false)
            )),
            ("SubtractiveHelix", "Outside") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::HelicalSweep {
                    op: cadmpeg_ir::features::BooleanOp::Cut,
                    ..
                }
            )),
            _ => unreachable!(),
        }
    }

    let malformed_enumerations = [
        ("App::PropertyString", r#"<String value="1"/>"#),
        ("App::PropertyInteger", r#"<Integer value="1"/>"#),
        ("App::PropertyEnumeration", r#"<Integer value="bad"/>"#),
        (
            "App::PropertyEnumeration",
            r#"<Wrapper><Integer value="1"/></Wrapper>"#,
        ),
        (
            "App::PropertyEnumeration",
            r#"<Integer value="0"/><Integer value="1"/>"#,
        ),
        ("App::PropertyEnumeration", r#"<Integer value="-1"/>"#),
        ("App::PropertyEnumeration", r#"<Integer value="99"/>"#),
    ];
    for (object, name) in [
        ("PartHelix", "LocalCoord"),
        ("PartHelix", "Style"),
        ("AdditiveHelix", "Mode"),
    ] {
        for (type_name, value) in malformed_enumerations {
            let replacement =
                format!(r#"<Property name="{name}" type="{type_name}">{value}</Property>"#);
            let result = decode(&document(Some((object, name, &replacement))));
            assert_native(
                &result,
                object,
                if object == "PartHelix" {
                    "Part::Helix"
                } else {
                    "PartDesign::AdditiveHelix"
                },
            );
        }
    }

    let malformed_quantities = [
        ("App::PropertyQuantity", r#"<Float value="0.5"/>"#),
        ("App::PropertyQuantityConstraint", r#"<Float value="bad"/>"#),
        (
            "App::PropertyQuantityConstraint",
            r#"<Wrapper><Float value="0.5"/></Wrapper>"#,
        ),
        (
            "App::PropertyQuantityConstraint",
            r#"<Float value="0.5"/><Float value="0.25"/>"#,
        ),
    ];
    for (type_name, value) in malformed_quantities {
        let replacement =
            format!(r#"<Property name="SegmentLength" type="{type_name}">{value}</Property>"#);
        let result = decode(&document(Some((
            "PartHelix",
            "SegmentLength",
            &replacement,
        ))));
        assert_native(&result, "PartHelix", "Part::Helix");
    }

    let malformed_booleans = [
        ("App::PropertyString", r#"<String value="true"/>"#),
        ("App::PropertyInteger", r#"<Integer value="1"/>"#),
        ("App::PropertyBool", r#"<Bool value="bad"/>"#),
        (
            "App::PropertyBool",
            r#"<Wrapper><Bool value="true"/></Wrapper>"#,
        ),
        (
            "App::PropertyBool",
            r#"<Bool value="false"/><Bool value="true"/>"#,
        ),
        ("App::PropertyBool", r#"<Bool value="1"/>"#),
        ("App::PropertyBool", r#"<Bool value="2"/>"#),
    ];
    for (object, name) in [
        ("AdditiveHelix", "LeftHanded"),
        ("AdditiveHelix", "Reversed"),
        ("AdditiveHelix", "AllowMultiFace"),
        ("SubtractiveHelix", "Outside"),
    ] {
        for (type_name, value) in malformed_booleans {
            let replacement =
                format!(r#"<Property name="{name}" type="{type_name}">{value}</Property>"#);
            let result = decode(&document(Some((object, name, &replacement))));
            assert_native(
                &result,
                object,
                if object == "AdditiveHelix" {
                    "PartDesign::AdditiveHelix"
                } else {
                    "PartDesign::SubtractiveHelix"
                },
            );
        }
    }

    let malformed_tolerance = [
        ("App::PropertyFloat", r#"<Float value="0.25"/>"#),
        ("App::PropertyFloatConstraint", r#"<Float value="bad"/>"#),
        (
            "App::PropertyFloatConstraint",
            r#"<Wrapper><Float value="0.25"/></Wrapper>"#,
        ),
        (
            "App::PropertyFloatConstraint",
            r#"<Float value="0.25"/><Float value="0.5"/>"#,
        ),
    ];
    for (type_name, value) in malformed_tolerance {
        let replacement =
            format!(r#"<Property name="Tolerance" type="{type_name}">{value}</Property>"#);
        let result = decode(&document(Some((
            "AdditiveHelix",
            "Tolerance",
            &replacement,
        ))));
        assert_native(&result, "AdditiveHelix", "PartDesign::AdditiveHelix");
    }
}

#[test]
fn transfers_remaining_partdesign_analytic_primitives() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="PartDesign::AdditiveEllipsoid" name="Ellipsoid" id="1"/>
 <Object type="PartDesign::SubtractivePrism" name="Prism" id="2"/>
 <Object type="PartDesign::AdditiveWedge" name="Wedge" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Ellipsoid"><Properties Count="6">
  <Property name="Radius1" type="App::PropertyLength"><Float value="3"/></Property><Property name="Radius2" type="App::PropertyLength"><Float value="5"/></Property><Property name="Radius3" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Angle1" type="App::PropertyAngle"><Float value="-45"/></Property><Property name="Angle2" type="App::PropertyAngle"><Float value="60"/></Property><Property name="Angle3" type="App::PropertyAngle"><Float value="270"/></Property>
 </Properties></Object>
 <Object name="Prism"><Properties Count="3"><Property name="Polygon" type="App::PropertyIntegerConstraint"><Integer value="7"/></Property><Property name="Circumradius" type="App::PropertyLength"><Float value="4"/></Property><Property name="Height" type="App::PropertyLength"><Float value="9"/></Property></Properties></Object>
 <Object name="Wedge"><Properties Count="10">
  <Property name="Xmin" type="App::PropertyDistance"><Float value="-2"/></Property><Property name="Ymin" type="App::PropertyDistance"><Float value="-1"/></Property><Property name="Zmin" type="App::PropertyDistance"><Float value="0"/></Property><Property name="X2min" type="App::PropertyDistance"><Float value="1"/></Property><Property name="Z2min" type="App::PropertyDistance"><Float value="2"/></Property>
  <Property name="Xmax" type="App::PropertyDistance"><Float value="8"/></Property><Property name="Ymax" type="App::PropertyDistance"><Float value="6"/></Property><Property name="Zmax" type="App::PropertyDistance"><Float value="10"/></Property><Property name="X2max" type="App::PropertyDistance"><Float value="7"/></Property><Property name="Z2max" type="App::PropertyDistance"><Float value="8"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("remaining primitives");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(
        matches!(definition("Ellipsoid"), cadmpeg_ir::features::FeatureDefinition::Primitive { solid: cadmpeg_ir::features::PrimitiveSolid::Ellipsoid { x_radius, y_radius, z_radius, .. }, op: cadmpeg_ir::features::BooleanOp::Join } if x_radius.0 == 5.0 && y_radius.0 == 5.0 && z_radius.0 == 3.0)
    );
    assert!(
        matches!(definition("Prism"), cadmpeg_ir::features::FeatureDefinition::Primitive { solid: cadmpeg_ir::features::PrimitiveSolid::Prism { sides: 7, circumradius, height }, op: cadmpeg_ir::features::BooleanOp::Cut } if circumradius.0 == 4.0 && height.0 == 9.0)
    );
    assert!(
        matches!(definition("Wedge"), cadmpeg_ir::features::FeatureDefinition::Primitive { solid: cadmpeg_ir::features::PrimitiveSolid::Wedge { xmin, ymax, .. }, op: cadmpeg_ir::features::BooleanOp::Join } if xmin.0 == -2.0 && ymax.0 == 6.0)
    );
    assert!(result.report().losses.is_empty());
}

#[test]
fn rejects_nested_and_duplicate_design_scalar_and_vector_roots() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Box" name="Box" id="1"/>
 <Object type="Part::Revolution" name="Revolution" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Box"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Wrapper><Float value="99"/></Wrapper><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/><Float value="3"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="4"/></Property>
 </Properties></Object>
 <Object name="Revolution"><Properties Count="2">
  <Property name="Axis" type="App::PropertyVector"><Wrapper><PropertyVector valueX="0" valueY="9" valueZ="0"/></Wrapper><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("native fallback for misframed design values");
    for name in ["Box", "Revolution"] {
        let feature = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature");
        assert!(matches!(
            feature.definition,
            FeatureDefinition::Native { .. }
        ));
    }
}

#[test]
fn distinguishes_absent_and_malformed_partdesign_revolution_type() {
    for (type_property, expected_native) in [
        ("", false),
        (
            r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="bad"/></Property>"#,
            true,
        ),
        (
            r#"<Property name="Type" type="App::PropertyString"><String value="4"/></Property>"#,
            true,
        ),
        (
            r#"<Property name="Type" type="App::PropertyEnumeration"><Wrapper><Integer value="0"/></Wrapper></Property>"#,
            true,
        ),
        (
            r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/><Integer value="1"/></Property>"#,
            true,
        ),
        (
            r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="-1"/></Property>"#,
            true,
        ),
        (
            r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="99"/></Property>"#,
            true,
        ),
    ] {
        let property_count = if type_property.is_empty() { 5 } else { 6 };
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="PartDesign::Revolution" name="Revolution"/></Objects>
<ObjectData Count="2">
<Object name="Sketch"><Properties Count="0"/></Object>
<Object name="Revolution"><Properties Count="{property_count}"><Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property><Property name="Base" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property><Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property>{type_property}<Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property><Property name="Angle2" type="App::PropertyAngle"><Float value="30"/></Property></Properties></Object>
</ObjectData></Document>"#
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect("PartDesign revolution selector");
        let definition = &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Revolution"))
            .expect("Revolution feature")
            .definition;
        if expected_native {
            assert!(matches!(
                definition,
                FeatureDefinition::Native { kind, .. } if kind == "PartDesign::Revolution"
            ));
        } else {
            assert!(matches!(
                definition,
                FeatureDefinition::Revolve {
                    construction: cadmpeg_ir::features::RevolutionConstruction {
                        extent: Some(RevolveExtent::OneSided {
                            termination: AngularTermination::Angle { .. }
                        }),
                        ..
                    },
                    ..
                }
            ));
        }
        assert_valid_document(result.ir());
    }
}
