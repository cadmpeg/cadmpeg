// SPDX-License-Identifier: Apache-2.0
//! Design holes-extrude transfer unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::{
    Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, ExtrusionDirectionSource, FeatureDefinition,
    InnerWireTaper, Length, LinearTermination, PathRef,
};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
pub(crate) fn transfers_branch_complete_threaded_counterdrill_hole() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Sketcher::SketchObject" name="Locations" id="1"/>
 <Object type="PartDesign::Hole" name="Hole" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Locations"><Properties Count="2">
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
 <Object name="Hole"><Properties Count="26">
  <Property name="Profile" type="App::PropertyLink"><Link value="Locations"/></Property>
  <Property name="BaseProfileType" type="App::PropertyInteger"><Integer value="7"/></Property>
  <Property name="Diameter" type="App::PropertyLength"><Float value="6.8"/></Property>
  <Property name="HoleCutType" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="HoleCutDiameter" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="HoleCutDepth" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="HoleCutCountersinkAngle" type="App::PropertyAngle"><Float value="90"/></Property>
  <Property name="DepthType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="DrillPoint" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="DrillPointAngle" type="App::PropertyAngle"><Float value="118"/></Property>
  <Property name="DrillForDepth" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Tapered" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="TaperedAngle" type="App::PropertyAngle"><Float value="60"/></Property>
  <Property name="ThreadType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="ThreadSize" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="M6"/><Enum value="M8"/></CustomEnumList></Property>
  <Property name="ThreadClass" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Enum value="6H"/></CustomEnumList></Property>
  <Property name="Threaded" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="ModelThread" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="CosmeticThread" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="ThreadPitch" type="App::PropertyLength"><Float value="1.25"/></Property>
  <Property name="ThreadDiameter" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="ThreadDirection" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="ThreadDepthType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="ThreadDepth" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="UseCustomThreadClearance" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("hole");
    let hole = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Hole"))
        .expect("hole feature");
    let cadmpeg_ir::features::FeatureDefinition::Hole {
        profile,
        profile_filter,
        construction,
        extent,
        bottom,
        taper_angle,
        allow_multi_profile_faces,
        ..
    } = &hole.definition
    else {
        panic!("typed hole");
    };
    let cadmpeg_ir::features::HoleConstruction::Form {
        kind,
        specification: Some(specification),
    } = construction
    else {
        panic!("standard hole construction");
    };
    assert!(matches!(
        profile,
        Some(cadmpeg_ir::features::ProfileRef::Sketch(_))
    ));
    assert_eq!(
        *profile_filter,
        Some(cadmpeg_ir::features::HoleProfileFilter {
            points: true,
            circles: true,
            arcs: true,
        })
    );
    assert!(matches!(
        kind,
        cadmpeg_ir::features::HoleKind::Counterdrill {
            diameter: cadmpeg_ir::features::Length(12.0),
            entry_diameter: None,
            depth: cadmpeg_ir::features::Length(2.0),
            angle: cadmpeg_ir::features::Angle(angle),
        } if (*angle - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
    ));
    assert!(matches!(
        extent,
        Some(cadmpeg_ir::features::LinearTermination::ThroughAll)
    ));
    assert!(matches!(
        bottom,
        Some(cadmpeg_ir::features::HoleBottom::Angled {
            depth_to_tip: true,
            ..
        })
    ));
    assert!(taper_angle.is_some());
    assert_eq!(*allow_multi_profile_faces, Some(true));
    let cadmpeg_ir::features::HoleSpecification::Threaded {
        standard,
        designation,
        class,
        modeled,
        cosmetic,
        hand,
        depth,
        ..
    } = specification.as_ref()
    else {
        panic!("thread specification");
    };
    assert_eq!(standard, "ISO metric");
    assert_eq!(designation.as_deref(), Some("M8"));
    assert_eq!(class.as_deref(), Some("6H"));
    assert!(*modeled && !cosmetic);
    assert_eq!(*hand, cadmpeg_ir::features::ThreadHand::Left);
    assert!(matches!(
        depth,
        cadmpeg_ir::features::HoleThreadDepth::Blind {
            depth: cadmpeg_ir::features::Length(12.0)
        }
    ));
    assert_eq!(hole.dependencies.len(), 1);
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
fn distinguishes_absent_and_malformed_hole_enumerations() {
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

    let hole_document = |target: &str, type_name: &str, value: &str| {
        let property = if target.is_empty() {
            String::new()
        } else {
            format!(r#"<Property name="{target}" type="{type_name}">{value}</Property>"#)
        };
        let thread_type = if target == "ThreadType" || target.is_empty() {
            String::new()
        } else {
            r#"<Property name="ThreadType" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#
                .to_owned()
        };
        let properties = format!("{thread_type}{property}");
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Locations" id="1"/><Object type="PartDesign::Hole" name="Hole" id="2"/></Objects>
<ObjectData Count="2"><Object name="Locations"><Properties Count="0"/></Object><Object name="Hole"><Properties Count="{count}"><Property name="Profile" type="App::PropertyLink"><Link value="Locations"/></Property><Property name="Diameter" type="App::PropertyLength"><Float value="6"/></Property><Property name="Depth" type="App::PropertyLength"><Float value="25"/></Property><Property name="DrillPointAngle" type="App::PropertyAngle"><Float value="118"/></Property>{properties}</Properties></Object></ObjectData></Document>"#,
            count = 4 + properties.matches("<Property ").count(),
        )
    };
    let decode = |document: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("hole enumeration document")
    };

    let absent = decode(&hole_document("", "", ""));
    assert!(matches!(
        definition(&absent, "Hole"),
        FeatureDefinition::Hole {
            profile_filter: Some(cadmpeg_ir::features::HoleProfileFilter {
                points: false,
                circles: true,
                arcs: true,
            }),
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Simple,
                specification: None,
            },
            extent: Some(LinearTermination::Blind {
                length: Length(25.0),
            }),
            bottom: Some(cadmpeg_ir::features::HoleBottom::Angled { .. }),
            ..
        }
    ));
    assert!(absent.report().losses.is_empty());

    let malformed_values = [
        ("App::PropertyEnumeration", r#"<Integer value="bad"/>"#),
        ("App::PropertyString", r#"<String value="0"/>"#),
        ("App::PropertyInteger", r#"<Integer value="0"/>"#),
        (
            "App::PropertyEnumeration",
            r#"<Wrapper><Integer value="0"/></Wrapper>"#,
        ),
        (
            "App::PropertyEnumeration",
            r#"<Integer value="0"/><Integer value="1"/>"#,
        ),
        ("App::PropertyEnumeration", r#"<Integer value="-1"/>"#),
        ("App::PropertyEnumeration", r#"<Integer value="99"/>"#),
    ];
    for target in [
        "ThreadType",
        "HoleCutType",
        "DepthType",
        "DrillPoint",
        "ThreadDepthType",
        "ThreadDirection",
    ] {
        for (type_name, value) in malformed_values {
            let result = decode(&hole_document(target, type_name, value));
            assert!(matches!(
                definition(&result, "Hole"),
                FeatureDefinition::Native { kind, .. } if kind == "PartDesign::Hole"
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
fn uses_only_direct_custom_hole_enumeration_labels() {
    fn hole_definition(result: &cadmpeg_ir::codec::DecodeResult) -> &FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Hole"))
            .expect("hole feature")
            .definition
    }

    fn retains_thread_size_property(
        result: &cadmpeg_ir::codec::DecodeResult,
        raw_value: &str,
    ) -> bool {
        result
            .ir()
            .native
            .namespace("fcstd")
            .and_then(|namespace| {
                namespace
                    .arena_as::<crate::native::PropertyRecord>("properties")
                    .ok()
            })
            .is_some_and(|properties| {
                properties.iter().any(|property| {
                    property.name == "ThreadSize" && property.raw_xml.contains(raw_value)
                })
            })
    }

    let hole_document = |type_name: &str, value: &str| {
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Locations" id="1"/><Object type="PartDesign::Hole" name="Hole" id="2"/></Objects>
<ObjectData Count="2"><Object name="Locations"><Properties Count="0"/></Object><Object name="Hole"><Properties Count="7"><Property name="Profile" type="App::PropertyLink"><Link value="Locations"/></Property><Property name="Diameter" type="App::PropertyLength"><Float value="6"/></Property><Property name="Depth" type="App::PropertyLength"><Float value="25"/></Property><Property name="DrillPointAngle" type="App::PropertyAngle"><Float value="118"/></Property><Property name="ThreadType" type="App::PropertyEnumeration"><Integer value="1"/></Property><Property name="Threaded" type="App::PropertyBool"><Bool value="true"/></Property><Property name="ThreadSize" type="{type_name}">{value}</Property></Properties></Object></ObjectData></Document>"#,
        )
    };
    let decode = |type_name: &str, value: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(&hole_document(type_name, value))),
                &DecodeOptions::default(),
            )
            .expect("hole enumeration label document")
    };
    let cases = [
        (
            "valid direct custom list",
            "App::PropertyEnumeration",
            r#"<Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="M6"/><Enum value="M8"/></CustomEnumList>"#,
            Some("M8"),
        ),
        (
            "non-custom enumeration",
            "App::PropertyEnumeration",
            r#"<Integer value="0"/>"#,
            None,
        ),
        (
            "missing custom marker",
            "App::PropertyEnumeration",
            r#"<Integer value="0"/><CustomEnumList count="1"><Enum value="M6"/></CustomEnumList>"#,
            None,
        ),
        (
            "invalid custom marker",
            "App::PropertyEnumeration",
            r#"<Integer value="0" CustomEnum="false"/><CustomEnumList count="1"><Enum value="M6"/></CustomEnumList>"#,
            None,
        ),
        (
            "missing custom list",
            "App::PropertyEnumeration",
            r#"<Integer value="0" CustomEnum="true"/>"#,
            None,
        ),
        (
            "nested enum is not a leaf",
            "App::PropertyEnumeration",
            r#"<Integer value="0" CustomEnum="true"/><CustomEnumList count="2"><Wrapper><Enum value="wrong"/></Wrapper><Enum value="M8"/></CustomEnumList>"#,
            None,
        ),
        (
            "count mismatch",
            "App::PropertyEnumeration",
            r#"<Integer value="0" CustomEnum="true"/><CustomEnumList count="2"><Enum value="M6"/></CustomEnumList>"#,
            None,
        ),
        (
            "uppercase label attribute",
            "App::PropertyEnumeration",
            r#"<Integer value="0" CustomEnum="true"/><CustomEnumList count="1"><Enum Value="M6"/></CustomEnumList>"#,
            None,
        ),
        (
            "out of range index",
            "App::PropertyEnumeration",
            r#"<Integer value="2" CustomEnum="true"/><CustomEnumList count="2"><Enum value="M6"/><Enum value="M8"/></CustomEnumList>"#,
            None,
        ),
        (
            "wrong runtime type",
            "App::PropertyInteger",
            r#"<Integer value="0"><Enum value="wrong"/></Integer>"#,
            None,
        ),
    ];

    for (case, type_name, value, expected) in cases {
        let result = decode(type_name, value);
        let FeatureDefinition::Hole {
            construction:
                cadmpeg_ir::features::HoleConstruction::Form {
                    specification: Some(specification),
                    ..
                },
            ..
        } = hole_definition(&result)
        else {
            panic!("{case}: expected typed hole");
        };
        let designation = match specification.as_ref() {
            cadmpeg_ir::features::HoleSpecification::Clearance { designation, .. }
            | cadmpeg_ir::features::HoleSpecification::Threaded { designation, .. } => designation,
        };
        assert_eq!(designation.as_deref(), expected, "{case}: label selection");
        assert!(result.report().losses.is_empty(), "{case}");
        assert!(
            retains_thread_size_property(&result, value),
            "{case}: native property was not retained"
        );
    }
}

#[test]
fn distinguishes_absent_and_malformed_hole_flags() {
    fn definition(result: &cadmpeg_ir::codec::DecodeResult) -> &FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Hole"))
            .expect("hole feature")
            .definition
    }

    let base_properties = [
        (
            "BaseProfileType",
            r#"<Property name="BaseProfileType" type="App::PropertyInteger"><Integer value="7"/></Property>"#,
        ),
        (
            "Threaded",
            r#"<Property name="Threaded" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "ModelThread",
            r#"<Property name="ModelThread" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "CosmeticThread",
            r#"<Property name="CosmeticThread" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "DrillForDepth",
            r#"<Property name="DrillForDepth" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "Tapered",
            r#"<Property name="Tapered" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "UseCustomThreadClearance",
            r#"<Property name="UseCustomThreadClearance" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "AllowMultiFace",
            r#"<Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "ThreadType",
            r#"<Property name="ThreadType" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
        ),
        (
            "ThreadDirection",
            r#"<Property name="ThreadDirection" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#,
        ),
        (
            "ThreadDepthType",
            r#"<Property name="ThreadDepthType" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#,
        ),
    ];
    let hole_document = |target: &str, replacement: Option<&str>| {
        let mut properties = String::from(
            r#"<Property name="Profile" type="App::PropertyLink"><Link value="Locations"/></Property><Property name="Diameter" type="App::PropertyLength"><Float value="6"/></Property><Property name="Depth" type="App::PropertyLength"><Float value="25"/></Property><Property name="DrillPointAngle" type="App::PropertyAngle"><Float value="118"/></Property><Property name="TaperedAngle" type="App::PropertyAngle"><Float value="60"/></Property><Property name="CustomThreadClearance" type="App::PropertyLength"><Float value="0.2"/></Property>"#,
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
            r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="2"><Object type="Sketcher::SketchObject" name="Locations" id="1"/><Object type="PartDesign::Hole" name="Hole" id="2"/></Objects><ObjectData Count="2"><Object name="Locations"><Properties Count="0"/></Object><Object name="Hole"><Properties Count="{count}">{properties}</Properties></Object></ObjectData></Document>"#,
            count = properties.matches("<Property ").count(),
        )
    };
    let decode = |document: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("hole flag document")
    };
    let assert_native = |result: &cadmpeg_ir::codec::DecodeResult| {
        assert!(matches!(
            definition(result),
            FeatureDefinition::Native { kind, .. } if kind == "PartDesign::Hole"
        ));
        assert_eq!(result.report().losses.len(), 1);
        assert!(result.report().losses.iter().all(|loss| {
            loss.code.namespace == "fcstd"
                && loss.code.code == "feature.native-kind-retained"
                && loss.severity == cadmpeg_ir::Severity::Blocking
        }));
    };

    let targets = [
        "Threaded",
        "ModelThread",
        "CosmeticThread",
        "DrillForDepth",
        "Tapered",
        "UseCustomThreadClearance",
        "AllowMultiFace",
        "BaseProfileType",
    ];
    for target in targets {
        let result = decode(&hole_document(target, None));
        assert!(result.report().losses.is_empty(), "{target}");
        let FeatureDefinition::Hole {
            profile_filter,
            bottom,
            taper_angle,
            construction,
            allow_multi_profile_faces,
            ..
        } = definition(&result)
        else {
            panic!("{target} absent carrier");
        };
        let specification = match construction {
            cadmpeg_ir::features::HoleConstruction::Form { specification, .. } => specification,
            cadmpeg_ir::features::HoleConstruction::NativeThread { .. } => {
                panic!("{target} native thread")
            }
        };
        let Some(specification) = specification.as_deref() else {
            panic!("{target} thread specification");
        };
        let (modeled, cosmetic, clearance) = match specification {
            cadmpeg_ir::features::HoleSpecification::Clearance {
                modeled,
                cosmetic,
                clearance,
                ..
            }
            | cadmpeg_ir::features::HoleSpecification::Threaded {
                modeled,
                cosmetic,
                clearance,
                ..
            } => (*modeled, *cosmetic, *clearance),
        };
        match target {
            "Threaded" => assert!(matches!(
                specification,
                cadmpeg_ir::features::HoleSpecification::Clearance { .. }
            )),
            "ModelThread" => assert!(!modeled),
            "CosmeticThread" => assert!(!cosmetic),
            "DrillForDepth" => assert!(matches!(
                bottom,
                Some(cadmpeg_ir::features::HoleBottom::Angled {
                    depth_to_tip: false,
                    ..
                })
            )),
            "Tapered" => assert!(taper_angle.is_none()),
            "UseCustomThreadClearance" => assert!(clearance.is_none()),
            "AllowMultiFace" => assert_eq!(*allow_multi_profile_faces, Some(false)),
            "BaseProfileType" => assert_eq!(
                *profile_filter,
                Some(cadmpeg_ir::features::HoleProfileFilter {
                    points: false,
                    circles: true,
                    arcs: true,
                })
            ),
            _ => unreachable!(),
        }
    }

    let valid = [
        (
            "Threaded",
            r#"<Property name="Threaded" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "ModelThread",
            r#"<Property name="ModelThread" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "CosmeticThread",
            r#"<Property name="CosmeticThread" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "DrillForDepth",
            r#"<Property name="DrillForDepth" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "Tapered",
            r#"<Property name="Tapered" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "UseCustomThreadClearance",
            r#"<Property name="UseCustomThreadClearance" type="App::PropertyBool"><Bool value="true"/></Property>"#,
        ),
        (
            "AllowMultiFace",
            r#"<Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>"#,
        ),
        (
            "BaseProfileType",
            r#"<Property name="BaseProfileType" type="App::PropertyInteger"><Integer value="1"/></Property>"#,
        ),
    ];
    for (target, replacement) in valid {
        let result = decode(&hole_document(target, Some(replacement)));
        assert!(result.report().losses.is_empty(), "{target}");
        let FeatureDefinition::Hole {
            profile_filter,
            bottom,
            taper_angle,
            construction,
            allow_multi_profile_faces,
            ..
        } = definition(&result)
        else {
            panic!("{target} valid carrier");
        };
        let specification = match construction {
            cadmpeg_ir::features::HoleConstruction::Form { specification, .. } => specification,
            cadmpeg_ir::features::HoleConstruction::NativeThread { .. } => {
                panic!("{target} native thread")
            }
        };
        let Some(specification) = specification.as_deref() else {
            panic!("{target} thread specification");
        };
        let (modeled, cosmetic, clearance) = match specification {
            cadmpeg_ir::features::HoleSpecification::Clearance {
                modeled,
                cosmetic,
                clearance,
                ..
            }
            | cadmpeg_ir::features::HoleSpecification::Threaded {
                modeled,
                cosmetic,
                clearance,
                ..
            } => (*modeled, *cosmetic, *clearance),
        };
        match target {
            "Threaded" => assert!(matches!(
                specification,
                cadmpeg_ir::features::HoleSpecification::Threaded { .. }
            )),
            "ModelThread" => assert!(modeled),
            "CosmeticThread" => assert!(cosmetic),
            "DrillForDepth" => assert!(matches!(
                bottom,
                Some(cadmpeg_ir::features::HoleBottom::Angled {
                    depth_to_tip: true,
                    ..
                })
            )),
            "Tapered" => assert!(taper_angle.is_some()),
            "UseCustomThreadClearance" => assert_eq!(clearance, Some(Length(0.2))),
            "AllowMultiFace" => assert_eq!(*allow_multi_profile_faces, Some(false)),
            "BaseProfileType" => assert_eq!(
                *profile_filter,
                Some(cadmpeg_ir::features::HoleProfileFilter {
                    points: true,
                    circles: false,
                    arcs: false,
                })
            ),
            _ => unreachable!(),
        }
    }

    let malformed_bool_values = [
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
        ("App::PropertyBool", r#"<Bool value="2"/>"#),
    ];
    for target in [
        "Threaded",
        "ModelThread",
        "CosmeticThread",
        "DrillForDepth",
        "Tapered",
        "UseCustomThreadClearance",
        "AllowMultiFace",
    ] {
        for (type_name, value) in malformed_bool_values {
            let replacement =
                format!(r#"<Property name="{target}" type="{type_name}">{value}</Property>"#);
            assert_native(&decode(&hole_document(target, Some(&replacement))));
        }
    }

    let malformed_integer_values = [
        ("App::PropertyString", r#"<String value="1"/>"#),
        ("App::PropertyEnumeration", r#"<Integer value="1"/>"#),
        ("App::PropertyInteger", r#"<Integer value="bad"/>"#),
        (
            "App::PropertyInteger",
            r#"<Wrapper><Integer value="1"/></Wrapper>"#,
        ),
        (
            "App::PropertyInteger",
            r#"<Integer value="1"/><Integer value="7"/>"#,
        ),
        ("App::PropertyInteger", r#"<Integer value="-1"/>"#),
    ];
    for (type_name, value) in malformed_integer_values {
        let replacement =
            format!(r#"<Property name="BaseProfileType" type="{type_name}">{value}</Property>"#);
        assert_native(&decode(&hole_document(
            "BaseProfileType",
            Some(&replacement),
        )));
    }
    for value in ["0", "8"] {
        let replacement = format!(
            r#"<Property name="BaseProfileType" type="App::PropertyInteger"><Integer value="{value}"/></Property>"#
        );
        assert_native(&decode(&hole_document(
            "BaseProfileType",
            Some(&replacement),
        )));
    }

    let high_bits = decode(&hole_document(
        "BaseProfileType",
        Some(
            r#"<Property name="BaseProfileType" type="App::PropertyInteger"><Integer value="99"/></Property>"#,
        ),
    ));
    assert!(high_bits.report().losses.is_empty());
    assert!(matches!(
        definition(&high_bits),
        FeatureDefinition::Hole {
            profile_filter: Some(cadmpeg_ir::features::HoleProfileFilter {
                points: true,
                circles: true,
                arcs: false,
            }),
            ..
        }
    ));
}

#[test]
fn resolves_deprecated_fcstd_hole_cut_indices() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1" ProgramVersion="0.18R4">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Locations"/><Object type="PartDesign::Hole" name="Hole"/></Objects>
<ObjectData Count="2">
 <Object name="Locations"><Properties Count="0"/></Object>
 <Object name="Hole"><Properties Count="8">
  <Property name="Profile" type="App::PropertyLink"><Link value="Locations"/></Property>
  <Property name="Diameter" type="App::PropertyLength"><Float value="4.4"/></Property>
  <Property name="HoleCutType" type="App::PropertyEnumeration"><Integer value="5"/></Property>
  <Property name="HoleCutDiameter" type="App::PropertyLength"><Float value="6"/></Property>
  <Property name="HoleCutDepth" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="DepthType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="DrillPoint" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="ThreadType" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("legacy hole");
    let hole = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Hole"))
        .expect("hole feature");
    assert!(matches!(
        hole.definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Counterbore {
                    diameter: cadmpeg_ir::features::Length(6.0),
                    depth: cadmpeg_ir::features::Length(5.0),
                },
                ..
            },
            ..
        }
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
pub(crate) fn transfers_non_default_extrusion_termination_branches() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="9">
  <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
  <Object type="PartDesign::Pad" name="ToLast" id="2"/>
  <Object type="PartDesign::Pad" name="ToFirst" id="3"/>
  <Object type="PartDesign::Pad" name="ToFace" id="4"/>
  <Object type="PartDesign::Pad" name="ToShape" id="5"/>
  <Object type="PartDesign::Pocket" name="ThroughAll" id="6"/>
  <Object type="PartDesign::Pad" name="Symmetric" id="7"/>
  <Object type="Part::Extrusion" name="PartExtrusion" id="8"/>
  <Object type="Part::Extrusion" name="NegativeProfileNormal" id="9"/>
</Objects>
<ObjectData Count="9">
  <Object name="Sketch"><Properties Count="0"/></Object>
  <Object name="ToLast"><Properties Count="2">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  </Properties></Object>
  <Object name="ToFirst"><Properties Count="2">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  </Properties></Object>
  <Object name="ToFace"><Properties Count="3">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="3"/></Property>
    <Property name="UpToFace" type="App::PropertyLinkSub"><LinkSub value="PartExtrusion" count="1"><Sub value="Face1"/></LinkSub></Property>
  </Properties></Object>
  <Object name="ToShape"><Properties Count="3">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="5"/></Property>
    <Property name="UpToShape" type="App::PropertyLinkSub"><LinkSub value="PartExtrusion" count="1"><Sub value="Face2"/></LinkSub></Property>
  </Properties></Object>
  <Object name="ThroughAll"><Properties Count="2">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  </Properties></Object>
  <Object name="Symmetric"><Properties Count="5">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="12"/></Property>
    <Property name="Midplane" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="TaperAngle" type="App::PropertyAngle"><Float value="5"/></Property>
  </Properties></Object>
  <Object name="PartExtrusion"><Properties Count="12">
    <Property name="Base" type="App::PropertyLink"><Link value="Sketch"/></Property>
 <Property name="Dir" type="App::PropertyVector"><PropertyVector valueX="0" valueY="2" valueZ="0"/></Property>
    <Property name="LengthFwd" type="App::PropertyLength"><Float value="7"/></Property>
    <Property name="LengthRev" type="App::PropertyLength"><Float value="3"/></Property>
    <Property name="TaperAngle" type="App::PropertyAngle"><Float value="2"/></Property>
    <Property name="TaperAngleRev" type="App::PropertyAngle"><Float value="4"/></Property>
    <Property name="DirMode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
    <Property name="DirLink" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="1"><Sub value="Edge1"/></LinkSub></Property>
    <Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="FaceMakerClass" type="App::PropertyString"><String value="Part::FaceMakerUnified"/></Property>
    <Property name="FaceMakerMode" type="App::PropertyEnumeration"><Integer value="4"/></Property>
    <Property name="InnerWireTaper" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  </Properties></Object>
  <Object name="NegativeProfileNormal"><Properties Count="6">
    <Property name="Base" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="DirMode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
    <Property name="LengthFwd" type="App::PropertyLength"><Float value="5"/></Property>
    <Property name="LengthRev" type="App::PropertyLength"><Float value="-1"/></Property>
    <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>
  </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("extrusion termination branches");
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
        definition("ToLast"),
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToLast,
                    ..
                }
            },
            ..
        }
    ));
    assert!(matches!(
        definition("ToFirst"),
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToFirst,
                    ..
                }
            },
            ..
        }
    ));
    assert!(matches!(
        definition("ToFace"),
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToFace { .. },
                    ..
                }
            },
            ..
        }
    ));
    assert!(matches!(
        definition("ToShape"),
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToShape { .. },
                    ..
                }
            },
            ..
        }
    ));
    assert!(matches!(
        definition("ThroughAll"),
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ThroughAll,
                    ..
                }
            },
            op: BooleanOp::Cut,
            ..
        }
    ));
    assert!(matches!(
        definition("Symmetric"),
        FeatureDefinition::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: direction,
                ..
            },
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length },
                    draft: Some(Angle(draft)),
                    ..
                }
            },
            ..
        } if direction.z == -1.0 && length.0 == 12.0 && (*draft - 5_f64.to_radians()).abs() < 1.0e-12
    ));
    assert!(matches!(
        definition("PartExtrusion"),
        FeatureDefinition::Extrude {
            profile: _,
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: direction,
                source: Some(ExtrusionDirectionSource::Edge {
                    reference: PathRef::Native(reference),
                }),
            },
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind { length: first },
                    draft: Some(Angle(draft)),
                    ..
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind { length: second },
                    draft: Some(Angle(reverse_draft)),
                    ..
                },
            },
            solid: Some(true),
            face_maker: Some(face_maker),
            inner_wire_taper: Some(InnerWireTaper::SameAsOuter),
            op: BooleanOp::NewBody,
            ..
        } if direction.y == 1.0 && first.0 == 7.0 && second.0 == 3.0
            && (*draft - 2_f64.to_radians()).abs() < 1.0e-12
            && (*reverse_draft - 4_f64.to_radians()).abs() < 1.0e-12
            && reference.ends_with(":DirLink")
            && *face_maker == cadmpeg_ir::features::FaceMaker::Unified
    ));
    assert!(matches!(
        definition("NegativeProfileNormal"),
        FeatureDefinition::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: direction,
                source: Some(ExtrusionDirectionSource::ProfileNormal),
            },
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length },
                    ..
                }
            },
            ..
        } if direction.z == -1.0 && length.0 == 5.0
    ));
}

#[test]
fn derives_extrusion_direction_from_a_non_sketch_profile_frame() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Part2DObjectPython" name="Profile"/><Object type="PartDesign::Pocket" name="Pocket"/></Objects>
<ObjectData Count="2">
 <Object name="Profile"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0.7071067811865476" Q2="0" Q3="0.7071067811865476"/></Property></Properties></Object>
 <Object name="Pocket"><Properties Count="3"><Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Profile" count="0"/></Property><Property name="Length" type="App::PropertyLength"><Float value="5"/></Property><Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("non-sketch profile extrusion");
    let pocket = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pocket"))
        .expect("pocket feature");
    assert!(matches!(
        &pocket.definition,
        FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Native(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: direction,
                source: Some(cadmpeg_ir::features::ExtrusionDirectionSource::ProfileNormal),
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        } if (direction.x - 1.0).abs() < 1.0e-12
            && direction.y.abs() < 1.0e-12
            && direction.z.abs() < 1.0e-12
    ));
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn refuses_malformed_non_sketch_profile_frame_before_design_transfer() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Part2DObjectPython" name="Profile"/><Object type="PartDesign::Pocket" name="Pocket"/></Objects>
<ObjectData Count="2">
 <Object name="Profile"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0"/></Property></Properties></Object>
 <Object name="Pocket"><Properties Count="3"><Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Profile" count="0"/></Property><Property name="Length" type="App::PropertyLength"><Float value="5"/></Property><Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("malformed non-sketch profile placement");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn transfers_part_extrusion_symmetric_direction_magnitude() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Extrusion" name="Extrusion" id="2"/>
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
</Objects>
<ObjectData Count="2">
 <Object name="Profile"><Properties Count="0"/></Object>
 <Object name="Extrusion"><Properties Count="6">
  <Property name="Base" type="App::PropertyLink"><Link value="Profile"/></Property>
 <Property name="Dir" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="12"/></Property>
  <Property name="DirMode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Symmetric" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="TaperAngle" type="App::PropertyAngle"><Float value="3"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("symmetric Part extrusion");
    let definition = &result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrusion"))
        .expect("extrusion")
        .definition;
    assert!(matches!(
        definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::Symmetric {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind { length },
                    draft: Some(cadmpeg_ir::features::Angle(draft)),
                    ..
                }
            },
            solid: Some(false),
            ..
        } if length.0 == 12.0 && (*draft - 3_f64.to_radians()).abs() < 1.0e-12
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn distinguishes_absent_and_malformed_part_extrusion_direction_mode() {
    fn extrusion_definition(
        result: &cadmpeg_ir::codec::DecodeResult,
    ) -> &cadmpeg_ir::features::FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrusion"))
            .expect("extrusion feature")
            .definition
    }

    let malformed_values = [
        "<Integer value=\"bad\"/>",
        "<String value=\"0\"/>",
        "<Wrapper><Integer value=\"0\"/></Wrapper>",
        "<Integer value=\"0\"/><Integer value=\"1\"/>",
        "<Integer value=\"-1\"/>",
        "<Integer value=\"99\"/>",
    ];
    let absent_document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Profile"/><Object type="Part::Extrusion" name="Extrusion"/></Objects>
<ObjectData Count="2"><Object name="Profile"><Properties Count="0"/></Object>
<Object name="Extrusion"><Properties Count="5"><Property name="Base" type="App::PropertyLink"><Link value="Profile"/></Property><Property name="Dir" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="5"/></Property><Property name="LengthFwd" type="App::PropertyDistance"><Float value="5"/></Property><Property name="LengthRev" type="App::PropertyDistance"><Float value="0"/></Property><Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(absent_document)),
            &DecodeOptions::default(),
        )
        .expect("absent direction mode");
    assert!(matches!(
        extrusion_definition(&result),
        FeatureDefinition::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: direction,
                source: Some(ExtrusionDirectionSource::Custom),
            },
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(5.0)
                    },
                    ..
                }
            },
            ..
        } if direction.z == 1.0
    ));
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());

    for value in malformed_values {
        let property = if value.starts_with("<String") {
            format!(r#"<Property name="DirMode" type="App::PropertyString">{value}</Property>"#)
        } else {
            format!(
                r#"<Property name="DirMode" type="App::PropertyEnumeration">{value}</Property>"#
            )
        };
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Profile"/><Object type="Part::Extrusion" name="Extrusion"/></Objects>
<ObjectData Count="2"><Object name="Profile"><Properties Count="0"/></Object>
<Object name="Extrusion"><Properties Count="6"><Property name="Base" type="App::PropertyLink"><Link value="Profile"/></Property><Property name="Dir" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="5"/></Property>{property}<Property name="LengthFwd" type="App::PropertyDistance"><Float value="5"/></Property><Property name="LengthRev" type="App::PropertyDistance"><Float value="0"/></Property><Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object></ObjectData></Document>"#
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect("malformed direction mode");
        assert!(matches!(
            extrusion_definition(&result),
            FeatureDefinition::Native { kind, .. } if kind == "Part::Extrusion"
        ));
        assert_valid_document(result.ir());
    }
}

#[test]
fn preserves_linkless_partdesign_extrusion_profile_and_direction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::Pad" name="Pad" id="1"/></Objects>
<ObjectData Count="1"><Object name="Pad"><Properties Count="4">
 <Property name="Sketch" type="App::PropertyLink"><Link value=""/></Property>
 <Property name="Length" type="App::PropertyLength"><Float value="5"/></Property>
 <Property name="Midplane" type="App::PropertyBool"><Bool value="false"/></Property>
 <Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("linkless pad");
    let definition = &result.ir().model.features[0].definition;
    assert!(matches!(
        definition,
        FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Native(profile),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::OneSided { .. },
            ..
        } if profile.ends_with(":Sketch")
    ));
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn rejects_ambiguous_profile_carriers_without_selecting_a_sketch() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Sketcher::SketchObject" name="SketchA" id="1"/>
 <Object type="Sketcher::SketchObject" name="SketchB" id="2"/>
 <Object type="PartDesign::Pad" name="MultipleTargets" id="3"/>
 <Object type="PartDesign::Pad" name="CompetingAliases" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="SketchA"><Properties Count="0"/></Object>
 <Object name="SketchB"><Properties Count="0"/></Object>
 <Object name="MultipleTargets"><Properties Count="2">
  <Property name="Profile" type="App::PropertyLinkSubList"><LinkSubList count="2"><Link obj="SketchA" sub=""/><Link obj="SketchB" sub=""/></LinkSubList></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="5"/></Property>
 </Properties></Object>
 <Object name="CompetingAliases"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="SketchA"/></Property>
  <Property name="Sketch" type="App::PropertyLink"><Link value="SketchB"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="5"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("ambiguous profile carriers");
    for name in ["MultipleTargets", "CompetingAliases"] {
        let feature = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("pad feature");
        assert!(matches!(
            &feature.definition,
            FeatureDefinition::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Native(profile),
                ..
            } if profile.ends_with(":Profile")
        ));
    }
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn transfers_partdesign_mixed_extrusion_side_controls() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Part::Box" name="Target" id="2"/>
 <Object type="PartDesign::Pad" name="Mixed" id="3"/>
 <Object type="PartDesign::Pocket" name="Symmetric" id="4"/>
 <Object type="PartDesign::Pad" name="LegacyTwoLengths" id="5"/>
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
</Objects>
<ObjectData Count="5">
 <Object name="Profile"><Properties Count="0"/></Object>
 <Object name="Target"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Mixed"><Properties Count="15">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="SideType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="-5"/></Property>
  <Property name="Type2" type="App::PropertyEnumeration"><Integer value="5"/></Property>
  <Property name="UpToShape2" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Target" sub="Face2"/></LinkSubList></Property>
 <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="3" valueZ="0"/></Property>
  <Property name="UseCustomVector" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="ReferenceAxis" type="App::PropertyLinkSub"><LinkSub value="Profile" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="TaperAngle" type="App::PropertyAngle"><Float value="2"/></Property>
  <Property name="TaperAngle2" type="App::PropertyAngle"><Float value="-3"/></Property>
  <Property name="Offset" type="App::PropertyDistance"><Float value="1"/></Property>
  <Property name="Offset2" type="App::PropertyDistance"><Float value="-2"/></Property>
  <Property name="AlongSketchNormal" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Symmetric"><Properties Count="4">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="SideType" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Offset" type="App::PropertyDistance"><Float value="0.5"/></Property>
 </Properties></Object>
    <Object name="LegacyTwoLengths"><Properties Count="5">
    <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="4"/></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="6"/></Property>
    <Property name="Length2" type="App::PropertyLength"><Float value="2"/></Property>
    <Property name="Midplane" type="App::PropertyBool"><Bool value="true"/></Property>
    </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("mixed extrusion controls");
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
        definition("Mixed"),
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind { length: Length(-5.0) },
                    draft: Some(Angle(first_draft)),
                    offset: Some(Length(1.0)),
                },
                second: ExtrudeSide {
                    termination: LinearTermination::ToShape { .. },
                    draft: Some(Angle(second_draft)),
                    offset: Some(Length(-2.0)),
                },
            },
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector: direction,
                source: Some(ExtrusionDirectionSource::Edge {
                    reference: PathRef::Native(reference),
                }),
            },
            length_along_profile_normal: Some(false),
            allow_multi_profile_faces: Some(true),
            ..
        } if direction.y == 1.0
            && reference.ends_with(":ReferenceAxis")
            && (*first_draft - 2_f64.to_radians()).abs() < 1.0e-12
            && (*second_draft + 3_f64.to_radians()).abs() < 1.0e-12
    ));
    assert!(matches!(
        definition("Symmetric"),
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::ThroughAll,
                    offset: Some(Length(0.5)),
                    ..
                }
            },
            ..
        }
    ));
    assert!(matches!(
        definition("LegacyTwoLengths"),
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(6.0)
                    },
                    ..
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(2.0)
                    },
                    ..
                },
            },
            ..
        }
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn distinguishes_absent_and_malformed_partdesign_extrusion_selectors() {
    fn pad_definition(
        result: &cadmpeg_ir::codec::DecodeResult,
    ) -> &cadmpeg_ir::features::FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Pad"))
            .expect("pad feature")
            .definition
    }

    let malformed_values = [
        "<Integer value=\"bad\"/>",
        "<String value=\"4\"/>",
        "<Wrapper><Integer value=\"4\"/></Wrapper>",
        "<Integer value=\"4\"/><Integer value=\"0\"/>",
        "<Integer value=\"-1\"/>",
        "<Integer value=\"99\"/>",
    ];
    for target in ["SideType", "Type", "Type2"] {
        let absent_document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="PartDesign::Pad" name="Pad"/></Objects>
<ObjectData Count="2"><Object name="Sketch"><Properties Count="0"/></Object>
<Object name="Pad"><Properties Count="{count}"><Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>{side_type}{pad_type}{pad_type2}<Property name="Length" type="App::PropertyLength"><Float value="6"/></Property><Property name="Length2" type="App::PropertyLength"><Float value="2"/></Property><Property name="UseCustomVector" type="App::PropertyBool"><Bool value="true"/></Property><Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property></Properties></Object></ObjectData></Document>"#,
            count = 7,
            side_type = if target == "SideType" {
                ""
            } else {
                r#"<Property name="SideType" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#
            },
            pad_type = if target == "Type" {
                ""
            } else {
                r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#
            },
            pad_type2 = if target == "Type2" {
                ""
            } else {
                r#"<Property name="Type2" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#
            },
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&absent_document)),
                &DecodeOptions::default(),
            )
            .expect("absent extrusion selector");
        if target == "SideType" {
            assert!(matches!(
                pad_definition(&result),
                FeatureDefinition::Extrude {
                    extent: ExtrudeExtent::OneSided {
                        side: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: Length(6.0)
                            },
                            ..
                        }
                    },
                    ..
                }
            ));
        } else {
            assert!(matches!(
                pad_definition(&result),
                FeatureDefinition::Extrude {
                    extent: ExtrudeExtent::TwoSided { .. },
                    ..
                }
            ));
        }
        assert_valid_document(result.ir());

        for value in malformed_values {
            let property = match value {
                value if value.starts_with("<String") => {
                    format!(
                        r#"<Property name="{target}" type="App::PropertyString">{value}</Property>"#
                    )
                }
                value => format!(
                    r#"<Property name="{target}" type="App::PropertyEnumeration">{value}</Property>"#
                ),
            };
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="PartDesign::Pad" name="Pad"/></Objects>
<ObjectData Count="2"><Object name="Sketch"><Properties Count="0"/></Object>
<Object name="Pad"><Properties Count="{count}"><Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>{side_type}{pad_type}{pad_type2}<Property name="Length" type="App::PropertyLength"><Float value="6"/></Property><Property name="Length2" type="App::PropertyLength"><Float value="2"/></Property><Property name="UseCustomVector" type="App::PropertyBool"><Bool value="true"/></Property><Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property></Properties></Object></ObjectData></Document>"#,
                count = if target == "Type" { 7 } else { 8 },
                side_type = if target == "SideType" {
                    property.as_str()
                } else if target == "Type" {
                    ""
                } else {
                    r#"<Property name="SideType" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#
                },
                pad_type = if target == "Type" {
                    property.as_str()
                } else {
                    r#"<Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#
                },
                pad_type2 = if target == "Type2" {
                    property.as_str()
                } else {
                    r#"<Property name="Type2" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#
                },
            );
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                )
                .expect("malformed extrusion selector");
            assert!(matches!(
                pad_definition(&result),
                FeatureDefinition::Native { kind, .. } if kind == "PartDesign::Pad"
            ));
            assert_valid_document(result.ir());
        }
    }
}

#[test]
fn distinguishes_absent_and_malformed_partdesign_extrusion_flags() {
    fn feature_definition<'a>(
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

    fn flag_document(target: &str, replacement: Option<&str>) -> String {
        let property = |name: &str, value: &str| {
            if target == name {
                replacement.unwrap_or_default().to_owned()
            } else {
                format!(
                    r#"<Property name="{name}" type="App::PropertyBool"><Bool value="{value}"/></Property>"#
                )
            }
        };
        let side_properties = format!(
            r#"<Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
<Property name="SideType" type="App::PropertyEnumeration"><Integer value="0"/></Property>
<Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
<Property name="Length" type="App::PropertyLength"><Float value="6"/></Property>
<Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property>
{use_custom}
{along_normal}
{reversed}
{allow_multi_face}
{midplane}"#,
            use_custom = property("UseCustomVector", "true"),
            along_normal = property("AlongSketchNormal", "false"),
            reversed = property("Reversed", "false"),
            allow_multi_face = property("AllowMultiFace", "true"),
            midplane = property("Midplane", "false"),
        );
        let count = side_properties.matches("<Property ").count();
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="PartDesign::Pad" name="Pad"/><Object type="PartDesign::Pocket" name="Pocket"/></Objects>
<ObjectData Count="3"><Object name="Sketch"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
<Object name="Pad"><Properties Count="{count}">{side_properties}</Properties></Object>
<Object name="Pocket"><Properties Count="{count}">{side_properties}</Properties></Object></ObjectData></Document>"#
        )
    }

    fn assert_typed(result: &cadmpeg_ir::codec::DecodeResult, name: &str) {
        assert!(matches!(
            feature_definition(result, name),
            FeatureDefinition::Extrude { .. }
        ));
        assert_valid_document(result.ir());
    }

    let malformed_values = [
        r#"<Property name="TARGET" type="App::PropertyString"><String value="true"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyInteger"><Integer value="1"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Bool value="1"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Wrapper><Bool value="true"/></Wrapper></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Bool value="false"/><Bool value="true"/></Property>"#,
    ];
    for target in [
        "Midplane",
        "UseCustomVector",
        "AlongSketchNormal",
        "Reversed",
        "AllowMultiFace",
    ] {
        let absent = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&flag_document(target, None))),
                &DecodeOptions::default(),
            )
            .expect("absent extrusion flag");
        for name in ["Pad", "Pocket"] {
            assert_typed(&absent, name);
        }
        match target {
            "Midplane" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&absent, name),
                        FeatureDefinition::Extrude {
                            extent: ExtrudeExtent::OneSided { .. },
                            ..
                        }
                    ));
                }
            }
            "AlongSketchNormal" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&absent, name),
                        FeatureDefinition::Extrude {
                            length_along_profile_normal: Some(true),
                            ..
                        }
                    ));
                }
            }
            "AllowMultiFace" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&absent, name),
                        FeatureDefinition::Extrude {
                            allow_multi_profile_faces: Some(false),
                            ..
                        }
                    ));
                }
            }
            "Reversed" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&absent, name),
                        FeatureDefinition::Extrude {
                            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                                vector: direction,
                                ..
                            },
                            ..
                        } if direction.x == 1.0
                    ));
                }
            }
            "UseCustomVector" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&absent, name),
                        FeatureDefinition::Extrude {
                            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                                vector: direction,
                                ..
                            },
                            ..
                        } if direction.z == 1.0
                    ));
                }
            }
            _ => unreachable!(),
        }

        let valid_value = if target == "Midplane" {
            "true"
        } else if target == "AlongSketchNormal" {
            "false"
        } else {
            "true"
        };
        let valid_property = format!(
            r#"<Property name="{target}" type="App::PropertyBool"><Bool value="{valid_value}"/></Property>"#
        );
        let valid = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&flag_document(target, Some(&valid_property)))),
                &DecodeOptions::default(),
            )
            .expect("valid extrusion flag");
        for name in ["Pad", "Pocket"] {
            assert_typed(&valid, name);
        }
        match target {
            "Midplane" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&valid, name),
                        FeatureDefinition::Extrude {
                            extent: ExtrudeExtent::Symmetric { .. },
                            ..
                        }
                    ));
                }
            }
            "AlongSketchNormal" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&valid, name),
                        FeatureDefinition::Extrude {
                            length_along_profile_normal: Some(false),
                            ..
                        }
                    ));
                }
            }
            "AllowMultiFace" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&valid, name),
                        FeatureDefinition::Extrude {
                            allow_multi_profile_faces: Some(true),
                            ..
                        }
                    ));
                }
            }
            "Reversed" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&valid, name),
                        FeatureDefinition::Extrude {
                            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                                vector: direction,
                                ..
                            },
                            ..
                        } if direction.x == -1.0
                    ));
                }
            }
            "UseCustomVector" => {
                for name in ["Pad", "Pocket"] {
                    assert!(matches!(
                        feature_definition(&valid, name),
                        FeatureDefinition::Extrude {
                            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
                                vector: direction,
                                ..
                            },
                            ..
                        } if direction.x == 1.0
                    ));
                }
            }
            _ => unreachable!(),
        }

        for malformed in malformed_values {
            let replacement = malformed.replace("TARGET", target);
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&flag_document(target, Some(&replacement)))),
                    &DecodeOptions::default(),
                )
                .expect("malformed extrusion flag");
            for name in ["Pad", "Pocket"] {
                assert!(matches!(
                    feature_definition(&result, name),
                    FeatureDefinition::Native { kind, .. } if kind == &format!("PartDesign::{name}")
                ));
            }
            assert_eq!(result.report().losses.len(), 2);
            assert_valid_document(result.ir());
        }
    }
}

#[test]
fn transfers_sketch_pad_and_pocket_design_history() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4" Dependencies="1">
  <ObjectDeps Name="Body" Count="0"/>
  <ObjectDeps Name="Sketch" Count="0"/>
  <ObjectDeps Name="Pad" Count="1"><Dep Name="Sketch"/></ObjectDeps>
  <ObjectDeps Name="Pocket" Count="2"><Dep Name="Pad"/><Dep Name="Sketch"/></ObjectDeps>
  <Object type="PartDesign::Body" name="Body" id="1"/>
  <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
  <Object type="PartDesign::Pad" name="Pad" id="2"/>
  <Object type="PartDesign::Pocket" name="Pocket" id="3"/>
</Objects>
<ObjectData Count="4">
  <Object name="Body"><Properties Count="2">
    <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="Sketch"/><Link value="Pad"/><Link value="Pocket"/></LinkList></Property>
    <Property name="Tip" type="App::PropertyLink"><Link value="Pocket"/></Property>
  </Properties></Object>
  <Object name="Sketch"><Properties Count="3">
    <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="4">
      <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="10" EndY="0"/><Construction value="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><LineSegment StartX="10" StartY="0" EndX="10" EndY="5"/><Construction value="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><LineSegment StartX="10" StartY="5" EndX="0" EndY="5"/><Construction value="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="5" EndX="0" EndY="0"/><Construction value="0"/></Geometry>
    </GeometryList></Property>
    <Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="2">
      <Constrain Type="2" First="0" FirstPos="0"/>
      <Constrain Name="Width" Type="7" Value="10" IsDriving="1" First="0" FirstPos="1" Second="1" SecondPos="1"/>
    </ConstraintList></Property>
    <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0.7071067811865476" Q1="0" Q2="0" Q3="0.7071067811865476"/></Property>
  </Properties></Object>
  <Object name="Pad"><Properties Count="2">
    <Property name="Sketch" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  </Properties></Object>
  <Object name="Pocket"><Properties Count="4">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="2.5"/></Property>
    <Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Length" expression="Pad.Length / 4"/></ExpressionEngine></Property>
  </Properties></Object>
</ObjectData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("design history");
    assert_eq!(result.ir().model.sketches.len(), 1);
    assert_eq!(result.ir().model.sketch_entities.len(), 4);
    assert_eq!(result.ir().model.sketches[0].profiles.len(), 1);
    assert_eq!(result.ir().model.sketches[0].profiles[0].len(), 4);
    let (origin, normal, _) = result.ir().model.sketches[0]
        .resolved_placement()
        .expect("resolved sketch placement");
    assert_eq!(origin.x, 1.0);
    assert!((normal.y + 1.0).abs() < 1.0e-12);
    assert_eq!(result.ir().model.features.len(), 4);
    assert_eq!(result.ir().model.sketch_constraints.len(), 2);
    assert_eq!(result.ir().model.parameters.len(), 3);
    assert!(result
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                cadmpeg_ir::sketches::SketchConstraintDefinition::Horizontal { .. }
            )
        }));
    assert!(result
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                cadmpeg_ir::sketches::SketchConstraintDefinition::HorizontalDistance { .. }
            )
        }));
    let pad = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .expect("pad");
    let pocket = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pocket"))
        .expect("pocket");
    let body = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body"))
        .expect("body");
    assert_eq!(result.ir().model.feature_parent(&pad.id), Some(&body.id));
    assert_eq!(result.ir().model.feature_parent(&pocket.id), Some(&body.id));
    assert_eq!(
        body.source_properties.get("Tip").map(String::as_str),
        Some("fcstd:native:object#Pocket")
    );
    assert_eq!(pocket.suppressed, Some(true));
    assert_eq!(
        pocket
            .source_properties
            .get("Suppressed")
            .map(String::as_str),
        Some("true")
    );
    let pocket_length = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&pocket.id) && parameter.name == "Length"
        })
        .expect("pocket length");
    assert_eq!(pocket_length.expression, "Pad.Length / 4");
    assert_eq!(pocket_length.dependencies.len(), 1);
    assert!(matches!(
        pad.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Sketch(_),
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: cadmpeg_ir::features::Length(10.0)
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
    assert!(matches!(
        pocket.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: cadmpeg_ir::features::Length(2.5)
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
    let native_findings = crate::validate_native(result.ir());
    assert!(native_findings.is_empty(), "{native_findings:#?}");
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    let design_findings = validation
        .findings
        .iter()
        .filter(|finding| {
            finding
                .entity
                .as_deref()
                .is_some_and(|entity| entity.starts_with("fcstd:design:"))
        })
        .collect::<Vec<_>>();
    assert!(design_findings.is_empty(), "{design_findings:#?}");
}
