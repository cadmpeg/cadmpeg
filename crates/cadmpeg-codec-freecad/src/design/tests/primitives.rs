// SPDX-License-Identifier: Apache-2.0
//! Design primitives transfer unit tests.
#![allow(unused_imports)]

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::{
    Angle, BooleanOp, FeatureDefinition, Length, PathRef, RevolveExtent, Termination,
};
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
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
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
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="0" z="1"/></Property>
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
                    termination: Termination::Angle { angle }
                }),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join
        } if (angle.0 - std::f64::consts::PI).abs() < 1e-12
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
        }] if (angle.0 - std::f64::consts::FRAC_PI_6).abs() < 1e-12)
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
  <Property name="Base" type="App::PropertyVector"><Vector x="1" y="2" z="3"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="2" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="2"/></Property>
 </Properties></Object>
 <Object name="ToFace"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="UpToFace" type="App::PropertyLinkSub"><LinkSub value="Standalone" count="1"><Sub value="Face1"/></LinkSub></Property>
 </Properties></Object>
 <Object name="TwoAngles"><Properties Count="6">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="4"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="120"/></Property>
  <Property name="Angle2" type="App::PropertyAngle"><Float value="30"/></Property>
 </Properties></Object>
 <Object name="Midplane"><Properties Count="10">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="3" z="0"/></Property>
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
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="Standalone"><Properties Count="8">
  <Property name="Source" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="0" z="4"/></Property>
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
                    termination: Termination::ToFirst
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
                    termination: Termination::ToFace { .. }
                }),
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        definition("TwoAngles"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { extent: Some(RevolveExtent::TwoSided { first: Termination::Angle { angle: first }, second: Termination::Angle { angle: second } }), .. }, .. }
            if (first.0 - 120_f64.to_radians()).abs() < 1e-12 && (second.0 - 30_f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        definition("Midplane"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { axis: Some(axis), extent: Some(RevolveExtent::Symmetric { termination: Termination::Angle { .. } }), axis_reference: Some(cadmpeg_ir::features::PathRef::Native(reference)), fuse_order: Some(cadmpeg_ir::features::RevolutionFuseOrder::FeatureFirst), solid: Some(true), allow_multi_profile_faces: Some(false), .. }, .. }
            if axis.direction.y == -1.0 && reference.ends_with(":ReferenceAxis")
    ));
    assert!(matches!(
        definition("ThroughAll"),
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::ThroughAll
                }),
                ..
            },
            op: BooleanOp::Cut
        }
    ));
    assert!(matches!(
        definition("Standalone"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { profile: Some(cadmpeg_ir::features::ProfileRef::Sketch(_)), axis: Some(axis), extent: Some(RevolveExtent::Symmetric { termination: Termination::Angle { .. } }), axis_reference: Some(cadmpeg_ir::features::PathRef::Native(reference)), solid: Some(true), face_maker_class: Some(face_maker), .. }, op: BooleanOp::NewBody }
            if axis.direction.z == 1.0 && reference.ends_with(":AxisLink")
                && face_maker == "Part::FaceMakerUnified"
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
        } if (angle - std::f64::consts::PI).abs() < 1e-12
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
  <Property name="SegmentLength" type="App::PropertyQuantity"><Float value="0.5"/></Property>
  <Property name="LocalCoord" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Style" type="App::PropertyEnumeration"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="Spiral"><Properties Count="4">
  <Property name="Growth" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Radius" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="Rotations" type="App::PropertyQuantity"><Float value="3.5"/></Property>
  <Property name="SegmentLength" type="App::PropertyQuantity"><Float value="0.25"/></Property>
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
            pitch: cadmpeg_ir::features::Length(4.0),
            revolutions: 5.0,
            clockwise: true,
            cone_angle: Some(cadmpeg_ir::features::Angle(angle)),
            segment_turns: Some(0.5),
            construction_style: Some(cadmpeg_ir::features::HelixConstructionStyle::Corrected),
            radial_growth: None,
            ..
        } if (*angle - 12_f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        definition("Spiral"),
        cadmpeg_ir::features::FeatureDefinition::Helix {
            radius: cadmpeg_ir::features::Length(5.0),
            pitch: cadmpeg_ir::features::Length(0.0),
            revolutions: 3.5,
            radial_growth: Some(cadmpeg_ir::features::Length(2.0)),
            cone_angle: None,
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
  <Property name="Base" type="App::PropertyVector"><Vector x="1" y="2" z="3"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="0" z="1"/></Property>
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
        && construction.tolerance.is_none())
    );
    assert!(result.report().losses.is_empty());
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
