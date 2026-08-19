// SPDX-License-Identifier: Apache-2.0
//! Design sketches transfer unit tests.
#![allow(unused_imports)]

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::{Angle, Length};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn transfers_application_saved_rotated_conics_and_profile_chain() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/sketch_conics.FCStd"
    ));
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("application-saved conic fixture");
    let entities = &result.ir().model.sketch_entities;
    assert_eq!(entities.len(), 7);
    assert!(matches!(
        entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Arc {
            start_angle: cadmpeg_ir::features::Angle(start),
            end_angle: cadmpeg_ir::features::Angle(end),
            ..
        } if (start - 0.65).abs() < 1.0e-12 && (end - 1.83).abs() < 1.0e-12
    ));
    assert!(matches!(
        entities[3].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Ellipse {
            major_angle: cadmpeg_ir::features::Angle(angle),
            start_angle: Some(cadmpeg_ir::features::Angle(start)),
            end_angle: Some(cadmpeg_ir::features::Angle(end)),
            ..
        } if (angle - 0.53).abs() < 1.0e-12
            && (start - (std::f64::consts::TAU - 0.42)).abs() < 1.0e-12
            && (end - (std::f64::consts::TAU + 1.37)).abs() < 1.0e-12
    ));
    assert!(matches!(
        entities[4].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Ellipse {
            major_angle: cadmpeg_ir::features::Angle(angle),
            start_angle: None,
            end_angle: None,
            ..
        } if (angle - 0.71).abs() < 1.0e-12
    ));
    assert!(matches!(
        entities[5].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Hyperbola {
            major_angle: cadmpeg_ir::features::Angle(angle),
            start_parameter: Some(start),
            end_parameter: Some(end),
            ..
        } if (angle - 0.47).abs() < 1.0e-12
            && (start + 0.63).abs() < 1.0e-12
            && (end - 0.88).abs() < 1.0e-12
    ));
    assert!(matches!(
        entities[6].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Parabola {
            axis_angle: cadmpeg_ir::features::Angle(angle),
            start_parameter: Some(start),
            end_parameter: Some(end),
            ..
        } if (angle - 0.67).abs() < 1.0e-12
            && (start + 2.1).abs() < 1.0e-12
            && (end - 2.4).abs() < 1.0e-12
    ));
    assert!(result
        .ir()
        .model
        .shells
        .iter()
        .any(|shell| shell.wire_edges.len() == 3));
    assert_valid_document(result.ir());
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_malformed_sketch_record_counts() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="2">
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("count mismatch");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn rejects_nested_and_duplicate_sketch_value_roots() {
    for (property_name, type_name, root) in [
        ("Geometry", "Part::PropertyGeometryList", "GeometryList"),
        (
            "Constraints",
            "Sketcher::PropertyConstraintList",
            "ConstraintList",
        ),
    ] {
        for value in [
            format!("<Wrapper><{root} count=\"0\"/></Wrapper>"),
            format!("<{root} count=\"0\"/><{root} count=\"0\"/>"),
        ] {
            let document = format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="{property_name}" type="{type_name}">{value}</Property>
</Properties></Object></ObjectData></Document>"#
            );
            let error = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&document)),
                    &DecodeOptions::default(),
                )
                .expect_err("misframed sketch value root");
            assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
        }
    }
}

#[test]
fn rejects_declared_geometry_with_the_wrong_carrier_tag() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
<Geometry type="Part::GeomLineSegment"><Circle CenterX="0" CenterY="0" Radius="1"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("carrier mismatch");
    assert!(error.to_string().contains("declares Part::GeomLineSegment"));
    assert!(error.to_string().contains("expected <LineSegment>"));
}

#[test]
fn transfers_geom_point_carrier() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
<Geometry type="Part::GeomPoint"><GeomPoint X="1.25" Y="-2.5" Z="3.75"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("point carrier");
    assert!(matches!(
        result.ir().model.sketch_entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Point { position }
            if position == cadmpeg_ir::math::Point2::new(1.25, -2.5)
    ));
}

#[test]
fn rejects_point_alias_for_geom_point() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
<Geometry type="Part::GeomPoint"><Point X="1.25" Y="-2.5"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("unregistered point carrier");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn rejects_incomplete_present_sketch_placement() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="2">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("incomplete placement");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn follows_freecad_null_axis_fallback_for_sketch_placements() {
    for (angle, axis, expected_normal, expected_x_axis) in [
        (
            0.0,
            "Ox=\"0\" Oy=\"0\" Oz=\"0\"",
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ),
        (
            std::f64::consts::FRAC_PI_2,
            "Ox=\"0\" Oy=\"0\" Oz=\"0\"",
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        ),
        (
            std::f64::consts::FRAC_PI_2,
            "Ox=\"1e-20\" Oy=\"0\" Oz=\"0\"",
            cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ),
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="2">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0" Q1="1" Q2="0" Q3="0" A="{angle}" {axis}/></Property>
</Properties></Object></ObjectData></Document>"#
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect("null-axis sketch placement");
        let (origin, normal, x_axis) = result.ir().model.sketches[0]
            .resolved_placement()
            .expect("resolved sketch placement");
        assert_eq!(origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
        assert!((normal.x - expected_normal.x).abs() < f64::EPSILON * 16.0);
        assert!((normal.y - expected_normal.y).abs() < f64::EPSILON * 16.0);
        assert!((normal.z - expected_normal.z).abs() < f64::EPSILON * 16.0);
        assert!((x_axis.x - expected_x_axis.x).abs() < f64::EPSILON * 16.0);
        assert!((x_axis.y - expected_x_axis.y).abs() < f64::EPSILON * 16.0);
        assert!((x_axis.z - expected_x_axis.z).abs() < f64::EPSILON * 16.0);
        assert!(crate::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
    }
}

#[test]
fn rejects_malformed_constraint_operand_lists() {
    for document in [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="2">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1"><Geometry type="Part::GeomPoint"><GeomPoint X="0" Y="0" Z="0"/></Geometry></GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="1"><Constrain Type="20" ElementIds="0 bad" ElementPositions="0 0"/></ConstraintList></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="2">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1"><Geometry type="Part::GeomPoint"><GeomPoint X="0" Y="0" Z="0"/></Geometry></GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="1"><Constrain Type="20" First="0" FirstPos="invalid"/></ConstraintList></Property>
</Properties></Object></ObjectData></Document>"#,
    ] {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect_err("malformed constraint operand list");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn retains_unknown_and_ambiguous_sketch_carriers_as_native() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="2">
<Geometry type="Vendor::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="1" StartY="0" EndX="2" EndY="0"/><Circle CenterX="0" CenterY="0" Radius="1"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("unknown and ambiguous carriers");
    let entities = &result.ir().model.sketch_entities;
    assert_eq!(entities.len(), 2);
    assert!(matches!(
        &entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Native { native_kind }
            if native_kind == "Vendor::GeomLineSegment"
    ));
    assert!(matches!(
        &entities[1].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Native { native_kind }
            if native_kind == "Part::GeomLineSegment"
    ));
    assert_valid_document(result.ir());
}

#[test]
pub(crate) fn transfers_point_and_elliptical_sketch_geometry_without_fabricated_defaults() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="6">
 <Geometry type="Part::GeomPoint"><GeomPoint X="1" Y="2" Z="0"/></Geometry>
 <Geometry type="Part::GeomEllipse"><Ellipse CenterX="3" CenterY="4" MajorRadius="6" MinorRadius="2" MajorAxisX="0" MajorAxisY="1"/></Geometry>
 <Geometry type="Part::GeomArcOfEllipse"><ArcOfEllipse CenterX="0" CenterY="0" MajorRadius="5" MinorRadius="3" MajorAngle="0.25" FirstParameter="0.5" LastParameter="1.5"/></Geometry>
 <Geometry type="Part::GeomCircle"><Circle CenterX="9" CenterY="9"/></Geometry>
 <Geometry type="Part::GeomCircle"><UID value="41"/><Circle CenterX="7" CenterY="8" Radius="0"/></Geometry>
 <Geometry type="Part::GeomLineSegment"><UID value="42"/><LineSegment StartX="1" StartY="3" EndX="2" EndY="4"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("sketch geometry");
    let entities = &result.ir().model.sketch_entities;
    assert!(matches!(
        entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Point { position }
            if position == cadmpeg_ir::math::Point2::new(1.0, 2.0)
    ));
    assert!(matches!(
        entities[1].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Ellipse {
            major_angle: cadmpeg_ir::features::Angle(angle),
            start_angle: None,
            end_angle: None,
            ..
        } if (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
    assert!(matches!(
        entities[2].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Ellipse {
            start_angle: Some(cadmpeg_ir::features::Angle(0.5)),
            end_angle: Some(cadmpeg_ir::features::Angle(1.5)),
            ..
        }
    ));
    assert!(matches!(
        entities[3].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Native { .. }
    ));
    assert!(matches!(
        entities[4].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Point { position }
            if position == cadmpeg_ir::math::Point2::new(7.0, 8.0)
    ));
    assert!(matches!(
        entities[5].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Line { start, end }
            if start == cadmpeg_ir::math::Point2::new(1.0, 3.0)
                && end == cadmpeg_ir::math::Point2::new(2.0, 4.0)
    ));
}

#[test]
pub(crate) fn transfers_full_and_bounded_sketch_conics() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Conics" id="1"/></Objects>
<ObjectData Count="1"><Object name="Conics"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="6">
 <Geometry type="Part::GeomHyperbola"><Hyperbola CenterX="1" CenterY="2" AngleXU="0.25" MajorRadius="5" MinorRadius="3"/></Geometry>
 <Geometry type="Part::GeomArcOfHyperbola"><ArcOfHyperbola CenterX="2" CenterY="3" AngleXU="0.5" MajorRadius="7" MinorRadius="4" StartAngle="-1" EndAngle="1.5"/></Geometry>
 <Geometry type="Part::GeomParabola"><Parabola CenterX="3" CenterY="4" AngleXU="0.75" Focal="2"/></Geometry>
 <Geometry type="Part::GeomArcOfParabola"><ArcOfParabola CenterX="4" CenterY="5" AngleXU="1" Focal="2.5" StartAngle="-2" EndAngle="3"/></Geometry>
 <Geometry type="Part::GeomArcOfCircle"><ArcOfCircle CenterX="0" CenterY="0" Radius="4" AngleXU="0.6" StartAngle="0.2" EndAngle="1.2"/></Geometry>
 <Geometry type="Part::GeomArcOfEllipse"><ArcOfEllipse CenterX="0" CenterY="1" AngleXU="0.3" MajorRadius="6" MinorRadius="2" StartAngle="0.4" EndAngle="1.4"/></Geometry>
</GeometryList></Property></Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("sketch conics");
    let entities = &result.ir().model.sketch_entities;
    assert_eq!(entities.len(), 6);
    assert!(matches!(
        entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Hyperbola {
            start_parameter: None,
            end_parameter: None,
            ..
        }
    ));
    assert!(matches!(
        entities[1].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Hyperbola {
            start_parameter: Some(-1.0),
            end_parameter: Some(1.5),
            ..
        }
    ));
    assert!(matches!(
        entities[2].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Parabola {
            focal_length: cadmpeg_ir::features::Length(2.0),
            start_parameter: None,
            ..
        }
    ));
    assert!(matches!(
        entities[3].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Parabola {
            focal_length: cadmpeg_ir::features::Length(2.5),
            start_parameter: Some(-2.0),
            end_parameter: Some(3.0),
            ..
        }
    ));
    assert!(matches!(
        entities[4].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Arc {
            start_angle: cadmpeg_ir::features::Angle(start),
            end_angle: cadmpeg_ir::features::Angle(end),
            ..
        } if (start - 0.8).abs() < 1e-12 && (end - 1.8).abs() < 1e-12
    ));
    assert!(matches!(
        entities[5].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Ellipse {
            start_angle: Some(_),
            end_angle: Some(_),
            ..
        }
    ));
    assert!(entities.iter().all(|entity| !matches!(
        entity.geometry,
        cadmpeg_ir::sketches::SketchGeometry::Native { .. }
    )));
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());
}

#[test]
pub(crate) fn transfers_bounded_rational_sketch_nurbs() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
 <Geometry type="Part::GeomBSplineCurve"><BSplineCurve PolesCount="3" KnotsCount="2" Degree="2" IsPeriodic="0">
  <Pole X="0" Y="0" Z="0" Weight="1"/>
  <Pole X="1" Y="2" Z="0" Weight="0.5"/>
  <Pole X="3" Y="0" Z="0" Weight="1"/>
  <Knot Value="0" Mult="3"/>
  <Knot Value="1" Mult="3"/>
 </BSplineCurve></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("sketch NURBS");
    assert!(matches!(
        &result.ir().model.sketch_entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Nurbs {
            degree: 2,
            knots,
            control_points,
            weights: Some(weights),
            periodic: false,
        } if knots == &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
            && control_points.len() == 3
            && weights == &[1.0, 0.5, 1.0]
    ));
}

#[test]
pub(crate) fn neutralizes_symmetric_locus_distance_and_point_on_object_constraints() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/><Object type="Part::Feature" name="Source" id="2"/></Objects>
<ObjectData Count="2"><Object name="Sketch"><Properties Count="4">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="4">
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="1" EndX="1" EndY="1"/></Geometry>
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0.5" StartY="-1" EndX="0.5" EndY="2"/></Geometry>
<Geometry type="Part::GeomPoint"><GeomPoint X="2" Y="6" Z="0"/></Geometry>
</GeometryList></Property>
<Property name="ExternalGeometry" type="App::PropertyLinkSubList"><LinkSubList count="2"><Link obj="Source" sub="Edge1"/><Link obj="Source" sub="Edge2"/></LinkSubList></Property>
<Property name="ExternalGeo" type="Part::PropertyGeometryList"><GeometryList count="3">
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="0" EndY="1"/></Geometry>
 <Geometry type="Part::GeomCircle"><Circle CenterX="4" CenterY="5" Radius="2"/></Geometry>
</GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="17">
 <Constrain Type="14" First="0" FirstPos="1" Second="1" SecondPos="1" Third="2" ThirdPos="0"/>
 <Constrain Type="6" First="0" FirstPos="1" Second="1" SecondPos="2" Value="4" IsDriving="1"/>
 <Constrain Name="OnAxis" MetaData="reviewed" Type="13" Orientation="4" Value="0" LabelDistance="2.5" LabelPosition="0.25" IsDriving="0" IsInVirtualSpace="1" IsVisible="0" IsActive="1" First="0" FirstPos="1" Second="2" SecondPos="0"/>
 <Constrain Type="16" First="0" FirstPos="2" Second="1" SecondPos="1" Third="2" ThirdPos="0" Value="1.33" IsDriving="1"/>
 <Constrain Type="19" First="0" FirstPos="0" Value="0.75" IsDriving="1"/>
 <Constrain Type="15" InternalAlignmentType="9" InternalAlignmentIndex="2" First="0" FirstPos="0" Second="1" SecondPos="0"/>
 <Constrain Type="20" ElementIds="2 0 1" ElementPositions="0 0 0"/>
 <Constrain Type="21" MetaData="{&quot;text&quot;:&quot;R42&quot;,&quot;font&quot;:&quot;Mono&quot;,&quot;isTextHeight&quot;:false}" ElementIds="2 0" ElementPositions="0 0"/>
 <Constrain Type="0" IsActive="0"/>
 <Constrain Type="13" First="0" FirstPos="1" Second="-1" SecondPos="0"/>
 <Constrain Type="6" First="-1" FirstPos="1" Second="0" SecondPos="1" Value="2" IsDriving="1"/>
 <Constrain Type="13" First="0" FirstPos="2" Second="-3" SecondPos="0"/>
 <Constrain Type="7" First="-4" FirstPos="1" Second="0" SecondPos="1" Value="3" IsDriving="1"/>
 <Constrain Name="Repeated" Type="9" First="0" FirstPos="0" Value="0.5" IsDriving="1"/>
 <Constrain Name="Repeated" Type="8" First="3" FirstPos="1" Value="6" IsDriving="1"/>
 <Constrain Type="9" First="0" FirstPos="0" Second="-2" SecondPos="2" Value="1.5" IsDriving="1"/>
 <Constrain Type="7" First="-2" FirstPos="1" Second="3" SecondPos="1" Value="3.175" IsDriving="1"/>
</ConstraintList></Property>
</Properties></Object><Object name="Source"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("sketch constraints");
    let constraint = |index: usize| {
        result
            .ir()
            .model
            .sketch_constraints
            .iter()
            .find(|constraint| constraint.id.0.ends_with(&format!(":{index}")))
            .expect("constraint index")
    };
    assert!(matches!(
        constraint(1).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Symmetric { .. }
    ));
    let point_on_object = constraint(3);
    assert_eq!(point_on_object.name.as_deref(), Some("OnAxis"));
    assert_eq!(point_on_object.metadata.as_deref(), Some("reviewed"));
    assert_eq!(point_on_object.orientation, Some(4));
    assert_eq!(point_on_object.label_distance, Some(2.5));
    assert_eq!(point_on_object.label_position, Some(0.25));
    assert_eq!(point_on_object.driving, Some(false));
    assert_eq!(point_on_object.virtual_space, Some(true));
    assert_eq!(point_on_object.visible, Some(false));
    assert_eq!(point_on_object.active, Some(true));
    assert!(matches!(
        constraint(4).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::SnellsLaw { .. }
    ));
    assert!(matches!(
        constraint(5).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Weight { .. }
    ));
    assert!(matches!(
        result
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id.0.ends_with(":constraint:4"))
            .expect("Snell parameter")
            .value,
        Some(cadmpeg_ir::features::ParameterValue::Real(value)) if (value - 1.33).abs() < 1e-12
    ));
    assert!(matches!(
        result
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id.0.ends_with(":constraint:5"))
            .expect("weight parameter")
            .value,
        Some(cadmpeg_ir::features::ParameterValue::Real(value)) if (value - 0.75).abs() < 1e-12
    ));
    assert!(matches!(
        constraint(6).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::InternalAlignment {
            alignment: cadmpeg_ir::sketches::SketchInternalAlignment::BsplineControlPoint,
            index: Some(2),
            ..
        }
    ));
    assert!(matches!(
        constraint(7).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Group { ref elements }
            if elements.len() == 3
    ));
    assert!(matches!(
        constraint(8).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Text {
            ref text,
            font: Some(ref font),
            is_text_height: false,
            ..
        } if text == "R42" && font == "Mono"
    ));
    assert!(matches!(
        constraint(9).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Disabled
    ));
    assert!(matches!(
        constraint(10).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::PointOnObject { .. }
    ));
    assert!(matches!(
        constraint(11).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::DistanceLoci { .. }
    ));
    assert!(matches!(
        constraint(12).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::PointOnObject { .. }
    ));
    assert!(matches!(
        constraint(13).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::HorizontalDistance { .. }
    ));
    assert!(matches!(
        constraint(14).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::AngleToAxis {
            axis: cadmpeg_ir::sketches::SketchAxis::Horizontal,
            ..
        }
    ));
    assert!(matches!(
        constraint(15).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::VerticalDistance {
            ref first,
            ref second,
            ..
        } if matches!(first, cadmpeg_ir::sketches::SketchLocus::Entity(id) if id.0.ends_with(":reference-root-point"))
            && matches!(second, cadmpeg_ir::sketches::SketchLocus::Entity(id) if id.0.ends_with(":4"))
    ));
    let repeated_parameters = result
        .ir()
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter
                .properties
                .get("source_name")
                .is_some_and(|name| name == "Repeated")
        })
        .collect::<Vec<_>>();
    assert_eq!(repeated_parameters.len(), 2);
    assert_eq!(repeated_parameters[0].name, "Constraint14");
    assert_eq!(repeated_parameters[1].name, "Constraint15");
    assert!(matches!(
        constraint(16).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::AngleToAxis {
            axis: cadmpeg_ir::sketches::SketchAxis::Vertical,
            ..
        }
    ));
    assert!(matches!(
        constraint(17).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::HorizontalDistance {
            ref first,
            ref second,
            ..
        } if matches!(first, cadmpeg_ir::sketches::SketchLocus::Entity(id) if id.0.ends_with(":reference-root-point"))
            && matches!(second, cadmpeg_ir::sketches::SketchLocus::Entity(id) if id.0.ends_with(":4"))
    ));
    assert!(result.ir().model.sketch_entities.iter().any(|entity| {
        entity.id.0.ends_with(":reference-horizontal-axis")
            && matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::ReferenceLine { .. }
            )
    }));
    assert!(result
        .ir()
        .model
        .sketch_entities
        .iter()
        .any(|entity| entity.id.0.ends_with(":reference-root-point")));
    let external = result
        .ir()
        .model
        .sketch_entities
        .iter()
        .find(|entity| entity.id.0.ends_with(":external:0"))
        .expect("external geometry");
    assert!(matches!(
        external.geometry,
        cadmpeg_ir::sketches::SketchGeometry::Circle { .. }
    ));
    assert!(external
        .geometry_ref
        .as_deref()
        .is_some_and(|reference| reference.ends_with(":ExternalGeometry")));
    assert_eq!(external.endpoint_refs, ["Edge1"]);
    let unresolved_external = result
        .ir()
        .model
        .sketch_entities
        .iter()
        .find(|entity| entity.id.0.ends_with(":external:1"))
        .expect("link-only external geometry");
    assert!(matches!(
        &unresolved_external.geometry,
        cadmpeg_ir::sketches::SketchGeometry::ExternalReference {
            document: None,
            object,
            subelements,
        } if object.ends_with("Source") && subelements == &["Edge2"]
    ));
    assert!(matches!(
        constraint(2).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::DistanceLoci { .. }
    ));
    assert!(matches!(
        constraint(3).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::PointOnObject {
            point: cadmpeg_ir::sketches::SketchLocus::Start(_),
            ..
        }
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn neutralizes_line_midpoint_coincidence() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="2">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="2">
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="2" EndY="0"/></Geometry>
<Geometry type="Part::GeomPoint"><GeomPoint X="1" Y="0" Z="0"/></Geometry>
</GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="2">
<Constrain Type="1" First="0" FirstPos="3" Second="1" SecondPos="1"/>
<Constrain Type="1" First="0" FirstPos="2" Second="1" SecondPos="3"/>
</ConstraintList></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("midpoint constraint");

    assert!(matches!(
        result.ir().model.sketch_constraints[0].definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Midpoint { .. }
    ));
    assert_valid_document(result.ir());
}
