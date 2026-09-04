// SPDX-License-Identifier: Apache-2.0
//! Design booleans-patterns transfer unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::{Angle, FeatureDefinition, Length};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

const EPS_PATTERN_ANGLE: f64 = 1.0e-12;

#[test]
fn transfers_ordered_part_boolean_operands_and_infers_dependencies() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Part::Box" name="A" id="1"/>
 <Object type="Part::Box" name="B" id="2"/>
 <Object type="Part::Box" name="C" id="3"/>
 <Object type="Part::Cut" name="Cut" id="4"/>
 <Object type="Part::MultiFuse" name="Fuse" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="A"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="B"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="C"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Cut"><Properties Count="2"><Property name="Base" type="App::PropertyLink"><Link value="A"/></Property><Property name="Tool" type="App::PropertyLink"><Link value="B"/></Property></Properties></Object>
 <Object name="Fuse"><Properties Count="1"><Property name="Shapes" type="App::PropertyLinkList"><LinkList count="2"><Link value="Cut"/><Link value="C"/></LinkList></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part booleans");
    let feature = |name: &str| {
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
    };
    assert!(matches!(
        feature("Cut").definition,
        cadmpeg_ir::features::FeatureDefinition::Combine {
            op: cadmpeg_ir::features::BooleanKind::Cut,
            ..
        }
    ));
    assert_eq!(
        feature("Cut")
            .dependencies
            .iter()
            .map(|id| id.0.as_str())
            .collect::<Vec<_>>(),
        ["fcstd:design:feature#A", "fcstd:design:feature#B"]
    );
    let cadmpeg_ir::features::FeatureDefinition::Combine {
        target,
        tools,
        op,
        keep_tools,
    } = &feature("Fuse").definition
    else {
        panic!("multi-fuse");
    };
    assert_eq!(*op, cadmpeg_ir::features::BooleanKind::Join);
    assert!(!keep_tools);
    assert!(matches!(
        target,
        cadmpeg_ir::features::BodySelection::Native(value) if value.ends_with(":link:0")
    ));
    assert!(matches!(
        tools,
        cadmpeg_ir::features::BodySelection::Native(value) if value.ends_with(":links:1..2")
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
pub(crate) fn transfers_partdesign_boolean_base_and_group_rules() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Part::Box" name="A" id="1"/><Object type="Part::Box" name="B" id="2"/><Object type="Part::Box" name="C" id="3"/>
 <Object type="PartDesign::Boolean" name="Fuse" id="4"/><Object type="PartDesign::Boolean" name="Cut" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="A"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="B"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="C"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Fuse"><Properties Count="2"><Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property><Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="A"/><Link value="B"/><Link value="C"/></LinkList></Property></Properties></Object>
 <Object name="Cut"><Properties Count="3"><Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property><Property name="BaseFeature" type="App::PropertyLink"><Link value="A"/></Property><Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="B"/><Link value="C"/></LinkList></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("PartDesign booleans");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("boolean")
            .definition
    };
    assert!(matches!(
        definition("Fuse"),
        cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(target),
            tools: cadmpeg_ir::features::BodySelection::Native(tools),
            op: cadmpeg_ir::features::BooleanKind::Join,
            keep_tools: false,
        } if target.ends_with(":Group:link:2")
            && tools.ends_with(":Group:links:0..2")
    ));
    assert!(matches!(
        definition("Cut"),
        cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(target),
            tools: cadmpeg_ir::features::BodySelection::Native(tools),
            op: cadmpeg_ir::features::BooleanKind::Cut,
            keep_tools: false,
        } if target.ends_with(":BaseFeature") && tools.ends_with(":Group")
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn distinguishes_absent_and_malformed_partdesign_boolean_type() {
    for (type_property, expected_native) in [
        ("", false),
        (
            r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="bad"/></Property>"#,
            true,
        ),
        (
            r#"<Property name="Type" type="App::PropertyString"><String value="1"/></Property>"#,
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
        let property_count = if type_property.is_empty() { 2 } else { 3 };
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="Part::Box" name="A"/><Object type="Part::Box" name="B"/><Object type="PartDesign::Boolean" name="Boolean"/></Objects>
<ObjectData Count="3">
<Object name="A"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
<Object name="B"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
<Object name="Boolean"><Properties Count="{property_count}">{type_property}<Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="A"/><Link value="B"/></LinkList></Property><Property name="Shape" type="Part::PropertyPartShape"><Part file="Boolean.Shape.brp"/></Property></Properties></Object>
</ObjectData></Document>"#
        );
        let brep = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("Boolean.Shape.brp", brep),
                ])),
                &DecodeOptions::default(),
            )
            .expect("PartDesign boolean selector");
        let definition = &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Boolean"))
            .expect("Boolean feature")
            .definition;
        if expected_native {
            assert!(matches!(
                definition,
                FeatureDefinition::Native { kind, .. } if kind.as_str() == "PartDesign::Boolean"
            ));
        } else {
            assert!(matches!(
                definition,
                FeatureDefinition::Combine {
                    op: cadmpeg_ir::features::BooleanKind::Join,
                    ..
                }
            ));
        }
        assert_valid_document(result.ir());
    }
}

#[test]
pub(crate) fn transfers_uniform_irregular_and_two_axis_patterns() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="PartDesign::LinearPattern" name="Uniform" id="2"/>
 <Object type="PartDesign::LinearPattern" name="Custom" id="3"/>
 <Object type="PartDesign::LinearPattern" name="TwoAxis" id="4"/>
 <Object type="PartDesign::PolarPattern" name="PolarCustom" id="5"/>
 <Object type="PartDesign::LinearPattern" name="NativeDirection" id="6"/>
 <Object type="PartDesign::Feature" name="Seed" id="1"/>
</Objects>
<ObjectData Count="6">
 <Object name="Seed"><Properties Count="0"/></Object>
 <Object name="Uniform"><Properties Count="7">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="-1" valueZ="0"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>
  <Property name="Occurrences2" type="App::PropertyIntegerConstraint"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="Custom"><Properties Count="6">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Offset" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
  <Property name="Spacings" type="App::PropertyFloatList"><FloatList file="CustomSpacings"/></Property>
 </Properties></Object>
 <Object name="TwoAxis"><Properties Count="11">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
  <Property name="Direction2" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property>
  <Property name="Reversed2" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Mode2" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Offset2" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Occurrences2" type="App::PropertyIntegerConstraint"><Integer value="3"/></Property>
  <Property name="SpacingPattern2" type="App::PropertyFloatList"><FloatList file="TwoAxisSpacingPattern2"/></Property>
 </Properties></Object>
 <Object name="PolarCustom"><Properties Count="7">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Offset" type="App::PropertyAngle"><Float value="30"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>
  <Property name="Spacings" type="App::PropertyFloatList"><FloatList file="PolarSpacings"/></Property>
  <Property name="SpacingPattern" type="App::PropertyFloatList"><FloatList file="PolarSpacingPattern"/></Property>
 </Properties></Object>
 <Object name="NativeDirection"><Properties Count="4">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyLinkSub"><LinkSub value="Seed" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let float_list = |values: &[f64]| {
        let mut bytes = Vec::with_capacity(4 + values.len() * 8);
        bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    };
    let custom_spacings = float_list(&[2.0, 7.0]);
    let two_axis_spacing_pattern = float_list(&[1.0, 4.0]);
    let polar_spacings = float_list(&[-1.0, -1.0, -1.0]);
    let polar_spacing_pattern = float_list(&[10.0, 20.0]);
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("CustomSpacings", &custom_spacings),
                ("TwoAxisSpacingPattern2", &two_axis_spacing_pattern),
                ("PolarSpacings", &polar_spacings),
                ("PolarSpacingPattern", &polar_spacing_pattern),
            ])),
            &DecodeOptions::default(),
        )
        .expect("linear patterns");
    let feature = |name: &str| {
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
    };
    assert!(matches!(
        &feature("Seed").definition,
        cadmpeg_ir::features::FeatureDefinition::StoredGeometry
    ));
    assert!(matches!(
        &feature("Uniform").definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            seeds,
            pattern: cadmpeg_ir::features::PatternKind::Linear {
                direction: Some(direction),
                spacing: cadmpeg_ir::features::Length(4.0),
                count: 4,
                ..
            },
        } if seeds.len() == 1 && direction.y == 1.0
    ));
    assert!(matches!(
        &feature("Custom").definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::LinearOffsets { direction: Some(direction), offsets },
            ..
        } if direction.x == 1.0 && offsets.iter().map(|offset| offset.0).collect::<Vec<_>>() == [0.0, 2.0, 9.0]
    ));
    let cadmpeg_ir::features::FeatureDefinition::Pattern {
        pattern: cadmpeg_ir::features::PatternKind::Composite { stages },
        ..
    } = &feature("TwoAxis").definition
    else {
        panic!("two-axis pattern")
    };
    assert_eq!(stages.len(), 2);
    assert!(matches!(
        *stages[0].pattern,
        cadmpeg_ir::features::PatternKind::Linear { count: 3, .. }
    ));
    assert!(matches!(
        &*stages[1].pattern,
        cadmpeg_ir::features::PatternKind::LinearOffsets { direction: Some(direction), offsets }
            if direction.y == -1.0 && offsets.iter().map(|offset| offset.0).collect::<Vec<_>>() == [0.0, 1.0, 5.0]
    ));
    assert_eq!(
        stages[1].combination,
        cadmpeg_ir::features::PatternStageCombination::CartesianProduct
    );
    assert!(matches!(
        &feature("PolarCustom").definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::CircularAngles { angles, .. },
            ..
        } if angles.iter().zip([0.0, 10.0, 30.0, 40.0]).all(|(angle, expected)|
            (angle.0.to_degrees() - expected).abs() < 1.0e-12)
    ));
    assert!(matches!(
        &feature("NativeDirection").definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Linear {
                direction: None,
                spacing: cadmpeg_ir::features::Length(4.0),
                count: 3,
                ..
            },
            ..
        }
    ));
    assert_eq!(feature("Uniform").dependencies.len(), 1);
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());
    let census = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace")
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .expect("design census");
    assert_eq!(census.len(), 6);
    assert!(census.iter().any(|record| {
        record.object == "fcstd:native:object#Seed"
            && record.semantic_kind == "stored_geometry"
            && record.neutral
            && !record.post_processed
    }));
    assert!(census.iter().any(|record| {
        record.object == "fcstd:native:object#Custom"
            && record.semantic_kind == "pattern"
            && record.neutral
    }));
    let baseline_findings = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).findings;
    assert!(
        baseline_findings
            .iter()
            .all(|finding| finding.check != cadmpeg_ir::Check::Identity),
        "{baseline_findings:?}"
    );
    let mut corrupted = result.ir().clone();
    let mut stale_census = census;
    stale_census[0].neutral = !stale_census[0].neutral;
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("design_census", &stale_census)
        .expect("replace design census");
    let corrupted_findings = crate::validate_native(&corrupted);
    assert!(
        corrupted_findings.iter().any(|finding| finding
            .message
            .contains("design census does not match projected feature semantics")),
        "{corrupted_findings:?}"
    );
}

#[test]
fn distinguishes_absent_and_malformed_pattern_modes() {
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

    let pattern_document = |linear_mode: &str, polar_mode: &str, mode2: &str| {
        let linear_count = 5 + usize::from(!linear_mode.is_empty());
        let polar_count = 5 + usize::from(!polar_mode.is_empty());
        let two_axis_count = 8 + usize::from(!mode2.is_empty());
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4"><Object type="PartDesign::Feature" name="Seed" id="1"/><Object type="PartDesign::LinearPattern" name="Linear" id="2"/><Object type="PartDesign::PolarPattern" name="Polar" id="3"/><Object type="PartDesign::LinearPattern" name="TwoAxis" id="4"/></Objects>
<ObjectData Count="4">
 <Object name="Seed"><Properties Count="0"/></Object>
 <Object name="Linear"><Properties Count="{linear_count}"><Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property><Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property><Property name="Length" type="App::PropertyLength"><Float value="8"/></Property><Property name="Offset" type="App::PropertyLength"><Float value="3"/></Property><Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>{linear_mode}</Properties></Object>
 <Object name="Polar"><Properties Count="{polar_count}"><Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property><Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property><Property name="Angle" type="App::PropertyAngle"><Float value="180"/></Property><Property name="Offset" type="App::PropertyAngle"><Float value="45"/></Property><Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>{polar_mode}</Properties></Object>
 <Object name="TwoAxis"><Properties Count="{two_axis_count}"><Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property><Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property><Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property><Property name="Length" type="App::PropertyLength"><Float value="8"/></Property><Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property><Property name="Direction2" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property><Property name="Length2" type="App::PropertyLength"><Float value="6"/></Property><Property name="Occurrences2" type="App::PropertyIntegerConstraint"><Integer value="2"/></Property>{mode2}</Properties></Object>
</ObjectData></Document>"#
        )
    };
    let decode = |document: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("pattern selector document")
    };

    let absent = decode(&pattern_document("", "", ""));
    assert!(matches!(
        definition(&absent, "Linear"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Linear {
                spacing: Length(4.0),
                count: 3,
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        definition(&absent, "Polar"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Circular {
                angle: Angle(angle),
                count: 3,
                ..
            },
            ..
        } if (*angle - std::f64::consts::PI).abs() < EPS_PATTERN_ANGLE
    ));
    let FeatureDefinition::Pattern {
        pattern: cadmpeg_ir::features::PatternKind::Composite { stages },
        ..
    } = definition(&absent, "TwoAxis")
    else {
        panic!("two-axis absent modes");
    };
    assert!(matches!(
        &*stages[0].pattern,
        cadmpeg_ir::features::PatternKind::Linear {
            spacing: Length(4.0),
            count: 3,
            ..
        }
    ));
    assert!(matches!(
        &*stages[1].pattern,
        cadmpeg_ir::features::PatternKind::Linear {
            spacing: Length(6.0),
            count: 2,
            ..
        }
    ));
    assert!(absent.report().losses.is_empty());

    let malformed_values = [
        ("App::PropertyEnumeration", "<Integer value=\"bad\"/>"),
        ("App::PropertyString", "<String value=\"0\"/>"),
        ("App::PropertyInteger", "<Integer value=\"0\"/>"),
        (
            "App::PropertyEnumeration",
            "<Wrapper><Integer value=\"0\"/></Wrapper>",
        ),
        (
            "App::PropertyEnumeration",
            "<Integer value=\"0\"/><Integer value=\"1\"/>",
        ),
        ("App::PropertyEnumeration", "<Integer value=\"-1\"/>"),
        ("App::PropertyEnumeration", "<Integer value=\"99\"/>"),
    ];
    for target in ["Linear", "Polar", "TwoAxis"] {
        for (type_name, value) in malformed_values {
            let property_name = if target == "TwoAxis" { "Mode2" } else { "Mode" };
            let property = format!(
                r#"<Property name="{property_name}" type="{type_name}">{value}</Property>"#
            );
            let valid_mode = r#"<Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#;
            let valid_mode2 = r#"<Property name="Mode2" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#;
            let linear_mode = if target == "Linear" {
                property.as_str()
            } else {
                valid_mode
            };
            let polar_mode = if target == "Polar" {
                property.as_str()
            } else {
                valid_mode
            };
            let mode2 = if target == "TwoAxis" {
                property.as_str()
            } else {
                valid_mode2
            };
            let result = decode(&pattern_document(linear_mode, polar_mode, mode2));
            let kind = if target == "Polar" {
                "PartDesign::PolarPattern"
            } else {
                "PartDesign::LinearPattern"
            };
            assert!(matches!(
                definition(&result, target),
                FeatureDefinition::Native { kind: actual, .. } if actual.as_str() == kind
            ));
            assert_eq!(result.report().losses.len(), 1);
            assert!(result.report().losses.iter().all(|loss| {
                loss.code.namespace == "fcstd"
                    && loss.code.code == "feature.native-kind-retained"
                    && loss.severity == cadmpeg_ir::Severity::Blocking
            }));
        }
    }
}

#[test]
fn distinguishes_absent_and_malformed_pattern_occurrence_and_reversal_carriers() {
    fn property_fragment(
        object: &str,
        name: &str,
        normal: &str,
        mutation: Option<(&str, &str, Option<&str>)>,
    ) -> String {
        match mutation {
            Some((target_object, target_name, replacement))
                if target_object == object && target_name == name =>
            {
                replacement.unwrap_or_default().to_owned()
            }
            _ => normal.to_owned(),
        }
    }

    fn object_fragment(name: &str, type_name: &str, properties: Vec<String>) -> String {
        let properties = properties
            .into_iter()
            .filter(|property| !property.is_empty())
            .collect::<String>();
        let count = properties.matches("<Property ").count();
        format!(
            r#"<Object name="{name}"><Properties Count="{count}">{properties}</Properties></Object>"#
        )
        .replace("><Properties", &format!(" type=\"{type_name}\"><Properties"))
    }

    fn pattern_document(mutation: Option<(&str, &str, Option<&str>)>) -> String {
        let originals = r#"<Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>"#;
        let direction = r#"<Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property>"#;
        let direction2 = r#"<Property name="Direction2" type="App::PropertyVector"><PropertyVector valueX="0" valueY="1" valueZ="0"/></Property>"#;
        let axis = r#"<Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>"#;
        let mode = r#"<Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#;
        let mode2 = r#"<Property name="Mode2" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#;
        let length =
            r#"<Property name="Length" type="App::PropertyLength"><Float value="8"/></Property>"#;
        let length2 =
            r#"<Property name="Length2" type="App::PropertyLength"><Float value="6"/></Property>"#;
        let offset =
            r#"<Property name="Offset" type="App::PropertyLength"><Float value="3"/></Property>"#;
        let offset2 =
            r#"<Property name="Offset2" type="App::PropertyLength"><Float value="2"/></Property>"#;
        let angle =
            r#"<Property name="Angle" type="App::PropertyAngle"><Float value="180"/></Property>"#;
        let angular_offset =
            r#"<Property name="Offset" type="App::PropertyAngle"><Float value="45"/></Property>"#;
        let factor =
            r#"<Property name="Factor" type="App::PropertyFloat"><Float value="1.5"/></Property>"#;

        let linear = object_fragment(
            "Linear",
            "PartDesign::LinearPattern",
            vec![
                originals.to_owned(),
                direction.to_owned(),
                mode.to_owned(),
                length.to_owned(),
                offset.to_owned(),
                property_fragment(
                    "Linear",
                    "Occurrences",
                    r#"<Property name="Occurrences" type="App::PropertyIntegerConstraint"><Integer value="4"/></Property>"#,
                    mutation,
                ),
                property_fragment(
                    "Linear",
                    "Reversed",
                    r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>"#,
                    mutation,
                ),
            ],
        );
        let legacy_linear = object_fragment(
            "LegacyLinear",
            "PartDesign::LinearPattern",
            vec![
                originals.to_owned(),
                direction.to_owned(),
                mode.to_owned(),
                length.to_owned(),
                r#"<Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>"#.to_owned(),
                r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>"#.to_owned(),
            ],
        );
        let polar = object_fragment(
            "Polar",
            "PartDesign::PolarPattern",
            vec![
                originals.to_owned(),
                axis.to_owned(),
                mode.to_owned(),
                angle.to_owned(),
                angular_offset.to_owned(),
                property_fragment(
                    "Polar",
                    "Occurrences",
                    r#"<Property name="Occurrences" type="App::PropertyIntegerConstraint"><Integer value="4"/></Property>"#,
                    mutation,
                ),
                property_fragment(
                    "Polar",
                    "Reversed",
                    r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>"#,
                    mutation,
                ),
            ],
        );
        let two_axis = object_fragment(
            "TwoAxis",
            "PartDesign::LinearPattern",
            vec![
                originals.to_owned(),
                direction.to_owned(),
                mode.to_owned(),
                length.to_owned(),
                r#"<Property name="Occurrences" type="App::PropertyIntegerConstraint"><Integer value="3"/></Property>"#.to_owned(),
                r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>"#.to_owned(),
                direction2.to_owned(),
                mode2.to_owned(),
                length2.to_owned(),
                offset2.to_owned(),
                property_fragment(
                    "TwoAxis",
                    "Occurrences2",
                    r#"<Property name="Occurrences2" type="App::PropertyIntegerConstraint"><Integer value="3"/></Property>"#,
                    mutation,
                ),
                property_fragment(
                    "TwoAxis",
                    "Reversed2",
                    r#"<Property name="Reversed2" type="App::PropertyBool"><Bool value="true"/></Property>"#,
                    mutation,
                ),
            ],
        );
        let inactive_two_axis = object_fragment(
            "InactiveTwoAxis",
            "PartDesign::LinearPattern",
            vec![
                originals.to_owned(),
                direction.to_owned(),
                mode.to_owned(),
                length.to_owned(),
                r#"<Property name="Occurrences" type="App::PropertyIntegerConstraint"><Integer value="3"/></Property>"#.to_owned(),
                r#"<Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>"#.to_owned(),
                direction2.to_owned(),
                mode2.to_owned(),
                length2.to_owned(),
                property_fragment(
                    "InactiveTwoAxis",
                    "Occurrences2",
                    r#"<Property name="Occurrences2" type="App::PropertyIntegerConstraint"><Integer value="1"/></Property>"#,
                    mutation,
                ),
                property_fragment(
                    "InactiveTwoAxis",
                    "Reversed2",
                    r#"<Property name="Reversed2" type="App::PropertyBool"><Bool value="false"/></Property>"#,
                    mutation,
                ),
            ],
        );
        let scaled = object_fragment(
            "Scaled",
            "PartDesign::Scaled",
            vec![
                originals.to_owned(),
                factor.to_owned(),
                property_fragment(
                    "Scaled",
                    "Occurrences",
                    r#"<Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>"#,
                    mutation,
                ),
            ],
        );

        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
 <Object type="PartDesign::Feature" name="Seed" id="1"/>
 <Object type="PartDesign::LinearPattern" name="Linear" id="2"/>
 <Object type="PartDesign::LinearPattern" name="LegacyLinear" id="3"/>
 <Object type="PartDesign::PolarPattern" name="Polar" id="4"/>
 <Object type="PartDesign::LinearPattern" name="TwoAxis" id="5"/>
 <Object type="PartDesign::LinearPattern" name="InactiveTwoAxis" id="6"/>
 <Object type="PartDesign::Scaled" name="Scaled" id="7"/>
</Objects>
<ObjectData Count="7">
 <Object name="Seed"><Properties Count="0"/></Object>
 {linear}
 {legacy_linear}
 {polar}
 {two_axis}
 {inactive_two_axis}
 {scaled}
</ObjectData></Document>"#
        )
    }

    fn decode(document: &str) -> cadmpeg_ir::codec::DecodeResult {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("pattern carrier document")
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

    fn assert_native(result: &cadmpeg_ir::codec::DecodeResult, name: &str, kind: &str) {
        let actual = definition(result, name);
        assert!(
            matches!(
                actual,
                FeatureDefinition::Native { kind: actual, .. } if actual.as_str() == kind
            ),
            "{name}: {actual:?}"
        );
        assert_eq!(result.report().losses.len(), 1);
        assert!(result.report().losses.iter().all(|loss| {
            loss.code.namespace == "fcstd"
                && loss.code.code == "feature.native-kind-retained"
                && loss.severity == cadmpeg_ir::Severity::Blocking
        }));
    }

    let selected = decode(&pattern_document(None));
    assert!(matches!(
        definition(&selected, "Linear"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Linear {
                direction: Some(direction),
                count: 4,
                ..
            },
            ..
        } if *direction == cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0)
    ));
    assert!(matches!(
        definition(&selected, "LegacyLinear"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Linear { count: 4, .. },
            ..
        }
    ));
    assert!(matches!(
        definition(&selected, "Polar"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Circular {
                axis_dir,
                angle: Angle(angle),
                count: 4,
                ..
            },
            ..
        } if *axis_dir == cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
            && (*angle - std::f64::consts::PI).abs() < EPS_PATTERN_ANGLE
    ));
    let FeatureDefinition::Pattern {
        pattern: cadmpeg_ir::features::PatternKind::Composite { stages },
        ..
    } = definition(&selected, "TwoAxis")
    else {
        panic!("selected two-axis pattern");
    };
    assert_eq!(stages.len(), 2);
    assert!(matches!(
        &*stages[0].pattern,
        cadmpeg_ir::features::PatternKind::Linear { count: 3, .. }
    ));
    assert!(matches!(
        &*stages[1].pattern,
        cadmpeg_ir::features::PatternKind::Linear {
            direction: Some(direction),
            count: 3,
            ..
        } if *direction == cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0)
    ));
    assert!(matches!(
        definition(&selected, "Scaled"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Scale { count: 4, .. },
            ..
        }
    ));
    assert!(matches!(
        definition(&selected, "InactiveTwoAxis"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Linear { count: 3, .. },
            ..
        }
    ));
    assert!(selected.report().losses.is_empty());

    let absent = [
        ("Linear", "Reversed", None),
        ("Linear", "Occurrences", None),
        ("Polar", "Reversed", None),
        ("Polar", "Occurrences", None),
        ("TwoAxis", "Reversed2", None),
        ("TwoAxis", "Occurrences2", None),
        ("Scaled", "Occurrences", None),
    ];
    for (object, name, replacement) in absent {
        let result = decode(&pattern_document(Some((object, name, replacement))));
        assert!(result.report().losses.is_empty(), "{object}.{name}");
        match (object, name) {
            ("Linear", "Reversed") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Pattern {
                    pattern: cadmpeg_ir::features::PatternKind::Linear {
                        direction: Some(direction),
                        count: 4,
                        ..
                    },
                    ..
                } if *direction == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            )),
            ("Linear", "Occurrences") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Pattern {
                    pattern: cadmpeg_ir::features::PatternKind::Linear { count: 2, .. },
                    ..
                }
            )),
            ("Polar", "Reversed") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Pattern {
                    pattern: cadmpeg_ir::features::PatternKind::Circular {
                        axis_dir,
                        count: 4,
                        ..
                    },
                    ..
                } if *axis_dir == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
            )),
            ("Polar", "Occurrences") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Pattern {
                    pattern: cadmpeg_ir::features::PatternKind::Circular {
                        count: 3,
                        axis_dir,
                        ..
                    },
                    ..
                } if *axis_dir == cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
            )),
            ("TwoAxis", "Reversed2") => {
                let FeatureDefinition::Pattern {
                    pattern: cadmpeg_ir::features::PatternKind::Composite { stages },
                    ..
                } = definition(&result, object)
                else {
                    panic!("absent second reversal");
                };
                assert!(matches!(
                    &*stages[1].pattern,
                    cadmpeg_ir::features::PatternKind::Linear {
                        direction: Some(direction),
                        count: 3,
                        ..
                    } if *direction == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
                ));
            }
            ("TwoAxis", "Occurrences2") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Pattern {
                    pattern: cadmpeg_ir::features::PatternKind::Linear { count: 3, .. },
                    ..
                }
            )),
            ("Scaled", "Occurrences") => assert!(matches!(
                definition(&result, object),
                FeatureDefinition::Pattern {
                    pattern: cadmpeg_ir::features::PatternKind::Scale { count: 2, .. },
                    ..
                }
            )),
            _ => unreachable!(),
        }
    }

    let inactive_reversal = decode(&pattern_document(Some((
        "InactiveTwoAxis",
        "Reversed2",
        Some(
            r#"<Property name="Reversed2" type="App::PropertyString"><String value="true"/></Property>"#,
        ),
    ))));
    assert!(matches!(
        definition(&inactive_reversal, "InactiveTwoAxis"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Linear { count: 3, .. },
            ..
        }
    ));
    assert!(inactive_reversal.report().losses.is_empty());

    let boolean_variants = [
        ("App::PropertyString", r#"<String value="true"/>"#),
        ("App::PropertyInteger", r#"<Integer value="1"/>"#),
        ("App::PropertyBool", r#"<Bool value="1"/>"#),
        (
            "App::PropertyBool",
            r#"<Wrapper><Bool value="true"/></Wrapper>"#,
        ),
        (
            "App::PropertyBool",
            r#"<Bool value="true"/><Bool value="false"/>"#,
        ),
    ];
    for (object, name) in [
        ("Linear", "Reversed"),
        ("Polar", "Reversed"),
        ("TwoAxis", "Reversed2"),
    ] {
        for (type_name, value) in boolean_variants {
            let replacement =
                format!(r#"<Property name="{name}" type="{type_name}">{value}</Property>"#);
            let result = decode(&pattern_document(Some((
                object,
                name,
                Some(replacement.as_str()),
            ))));
            assert_native(
                &result,
                object,
                if object == "Polar" {
                    "PartDesign::PolarPattern"
                } else {
                    "PartDesign::LinearPattern"
                },
            );
        }
    }

    let constrained_variants = [
        ("App::PropertyFloat", r#"<Float value="4"/>"#),
        ("App::PropertyEnumeration", r#"<Integer value="4"/>"#),
        (
            "App::PropertyIntegerConstraint",
            r#"<Integer value="bad"/>"#,
        ),
        (
            "App::PropertyIntegerConstraint",
            r#"<Wrapper><Integer value="4"/></Wrapper>"#,
        ),
        (
            "App::PropertyIntegerConstraint",
            r#"<Integer value="4"/><Integer value="4"/>"#,
        ),
        ("App::PropertyIntegerConstraint", r#"<Integer value="-1"/>"#),
        ("App::PropertyIntegerConstraint", r#"<Integer value="0"/>"#),
    ];
    for object in ["Linear", "Polar"] {
        for (type_name, value) in constrained_variants {
            let replacement =
                format!(r#"<Property name="Occurrences" type="{type_name}">{value}</Property>"#);
            let result = decode(&pattern_document(Some((
                object,
                "Occurrences",
                Some(replacement.as_str()),
            ))));
            assert_native(
                &result,
                object,
                if object == "Polar" {
                    "PartDesign::PolarPattern"
                } else {
                    "PartDesign::LinearPattern"
                },
            );
        }
    }

    let second_occurrence_variants = [
        ("App::PropertyInteger", r#"<Integer value="3"/>"#),
        ("App::PropertyFloat", r#"<Float value="3"/>"#),
        (
            "App::PropertyIntegerConstraint",
            r#"<Integer value="bad"/>"#,
        ),
        (
            "App::PropertyIntegerConstraint",
            r#"<Wrapper><Integer value="3"/></Wrapper>"#,
        ),
        (
            "App::PropertyIntegerConstraint",
            r#"<Integer value="3"/><Integer value="3"/>"#,
        ),
        ("App::PropertyIntegerConstraint", r#"<Integer value="-1"/>"#),
        ("App::PropertyIntegerConstraint", r#"<Integer value="0"/>"#),
    ];
    for (type_name, value) in second_occurrence_variants {
        let replacement =
            format!(r#"<Property name="Occurrences2" type="{type_name}">{value}</Property>"#);
        let result = decode(&pattern_document(Some((
            "TwoAxis",
            "Occurrences2",
            Some(replacement.as_str()),
        ))));
        assert_native(&result, "TwoAxis", "PartDesign::LinearPattern");
    }

    let scaled_occurrence_variants = [
        ("App::PropertyIntegerConstraint", r#"<Integer value="4"/>"#),
        ("App::PropertyFloat", r#"<Float value="4"/>"#),
        ("App::PropertyInteger", r#"<Integer value="bad"/>"#),
        (
            "App::PropertyInteger",
            r#"<Wrapper><Integer value="4"/></Wrapper>"#,
        ),
        (
            "App::PropertyInteger",
            r#"<Integer value="4"/><Integer value="4"/>"#,
        ),
        ("App::PropertyInteger", r#"<Integer value="-1"/>"#),
        ("App::PropertyInteger", r#"<Integer value="0"/>"#),
    ];
    for (type_name, value) in scaled_occurrence_variants {
        let replacement =
            format!(r#"<Property name="Occurrences" type="{type_name}">{value}</Property>"#);
        let result = decode(&pattern_document(Some((
            "Scaled",
            "Occurrences",
            Some(replacement.as_str()),
        ))));
        assert_native(&result, "Scaled", "PartDesign::Scaled");
    }
}

#[test]
fn resolves_datum_references_for_polar_and_mirror_patterns() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
 <Object type="Part::Box" name="Seed" id="1"/>
 <Object type="App::Line" name="Axis" id="2"/>
 <Object type="App::Plane" name="Plane" id="3"/>
 <Object type="PartDesign::PolarPattern" name="Ring" id="4"/>
 <Object type="PartDesign::Mirrored" name="Mirror" id="5"/>
 <Object type="PartDesign::Body" name="Body" id="6"/>
 <Object type="PartDesign::Mirrored" name="FaceMirror" id="7"/>
</Objects>
<ObjectData Count="7">
 <Object name="Seed"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Axis"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Plane"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Ring"><Properties Count="5">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Axis" type="App::PropertyLinkSub"><LinkSub value="Axis" count="1"><Sub value=""/></LinkSub></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="360"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>
 </Properties></Object>
 <Object name="Mirror"><Properties Count="2">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="0"/></Property>
  <Property name="MirrorPlane" type="App::PropertyLinkSub"><LinkSub value="Plane" count="1"><Sub value=""/></LinkSub></Property>
 </Properties></Object>
 <Object name="FaceMirror"><Properties Count="2">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="2"><Link value="Seed"/><Link value="Plane"/></LinkList></Property>
  <Property name="MirrorPlane" type="App::PropertyLinkSub"><LinkSub value="Seed" count="1"><Sub value="Face1"/></LinkSub></Property>
 </Properties></Object>
 <Object name="Body"><Properties Count="1">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="Seed"/><Link value="Mirror"/><Link value="FaceMirror"/></LinkList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("referenced patterns");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("pattern")
            .definition
    };
    assert!(matches!(
        definition("Ring"),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Circular {
                axis_origin,
                axis_dir,
                angle: cadmpeg_ir::features::Angle(angle),
                count: 4,
            },
            ..
        } if *axis_origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && *axis_dir == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
            && (*angle - std::f64::consts::TAU).abs() < 1.0e-12
    ));
    assert!(matches!(
        definition("Mirror"),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Mirror {
                plane_origin,
                plane_normal,
            },
            ..
        } if *plane_origin == cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
            && *plane_normal == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    ));
    assert!(matches!(
        definition("FaceMirror"),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::MirrorReference {
                plane: cadmpeg_ir::features::FaceSelection::Native(plane),
            },
            ..
        } if plane.ends_with(":MirrorPlane")
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn rejects_ambiguous_axis_and_plane_reference_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="8">
 <Object type="Part::Box" name="Seed" id="1"/>
 <Object type="PartDesign::Line" name="AxisLine" id="2"/>
 <Object type="PartDesign::CoordinateSystem" name="AxisSystem" id="3"/>
 <Object type="PartDesign::Plane" name="PlaneA" id="4"/>
 <Object type="PartDesign::Plane" name="PlaneB" id="5"/>
 <Object type="PartDesign::PolarPattern" name="MultipleTargets" id="6"/>
 <Object type="PartDesign::PolarPattern" name="MultipleSelectors" id="7"/>
 <Object type="PartDesign::Mirrored" name="MultiplePlanes" id="8"/>
</Objects>
<ObjectData Count="8">
 <Object name="Seed"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="AxisLine"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="AxisSystem"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="PlaneA"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="PlaneB"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="MultipleTargets"><Properties Count="4">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Axis" type="App::PropertyLinkSubList"><LinkSubList count="2"><Link obj="AxisLine" sub=""/><Link obj="AxisSystem" sub=""/></LinkSubList></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="MultipleSelectors"><Properties Count="4">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Axis" type="App::PropertyLinkSub"><LinkSub value="AxisSystem" count="2"><Sub value="Z_Axis"/><Sub value="X_Axis"/></LinkSub></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="MultiplePlanes"><Properties Count="2">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="MirrorPlane" type="App::PropertyLinkSubList"><LinkSubList count="2"><Link obj="PlaneA" sub=""/><Link obj="PlaneB" sub=""/></LinkSubList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("ambiguous datum references");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("pattern")
            .definition
    };
    for name in ["MultipleTargets", "MultipleSelectors"] {
        assert!(matches!(
            definition(name),
            FeatureDefinition::Native { kind, .. } if kind.as_str() == "PartDesign::PolarPattern"
        ));
    }
    assert!(matches!(
        definition("MultiplePlanes"),
        FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::MirrorReference {
                plane: cadmpeg_ir::features::FaceSelection::Native(plane),
            },
            ..
        } if plane.ends_with(":MirrorPlane")
    ));
    assert_eq!(result.report().losses.len(), 2);
    assert!(result.report().losses.iter().all(|loss| {
        loss.code.namespace == "fcstd"
            && loss.code.code == "feature.native-kind-retained"
            && loss.severity == cadmpeg_ir::Severity::Blocking
    }));
}

#[test]
fn transfers_progressive_scale_and_ordered_multi_transform_stages() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="Seed" id="1"/>
 <Object type="PartDesign::LinearPattern" name="Linear" id="2"/>
 <Object type="PartDesign::Scaled" name="Scaled" id="3"/>
 <Object type="PartDesign::MultiTransform" name="Multi" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Seed"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Linear"><Properties Count="5">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="0"/></Property>
  <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="Scaled"><Properties Count="3">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="0"/></Property>
  <Property name="Factor" type="App::PropertyFloat"><Float value="2.5"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="Multi"><Properties Count="2">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Transformations" type="App::PropertyLinkList"><LinkList count="2"><Link value="Linear"/><Link value="Scaled"/></LinkList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("scaled multi-transform");
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
        definition("Scaled"),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Scale {
                center: cadmpeg_ir::features::PatternScaleCenter::FirstSeedCentroid,
                final_factor: 2.5,
                count: 3,
            },
            ..
        }
    ));
    let cadmpeg_ir::features::FeatureDefinition::Pattern {
        pattern: cadmpeg_ir::features::PatternKind::Composite { stages },
        ..
    } = definition("Multi")
    else {
        panic!("expected composite pattern");
    };
    assert_eq!(stages.len(), 2);
    assert_eq!(
        stages[0].combination,
        cadmpeg_ir::features::PatternStageCombination::Initialize
    );
    assert!(matches!(
        *stages[0].pattern,
        cadmpeg_ir::features::PatternKind::Linear { count: 3, .. }
    ));
    assert_eq!(
        stages[1].combination,
        cadmpeg_ir::features::PatternStageCombination::AlignedSlices
    );
    assert!(matches!(
        *stages[1].pattern,
        cadmpeg_ir::features::PatternKind::Scale { count: 3, .. }
    ));
    assert!(result.report().losses.is_empty());
}
