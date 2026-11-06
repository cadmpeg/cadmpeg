// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::FcstdCodec;

const CARRIER_TOLERANCE: f64 = 1.0e-9;
const INTERPOLATION_TOLERANCE: f64 = 1.0e-6;

#[test]
fn authored_geometry_topology_keeps_source_objects_body_topology_and_curves() {
    let bytes = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/geometry_topology.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = decoded.ir();
    let model = &ir.model;
    let mut features = model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    let source_shapes = [
        ("Solid", BodyKind::Solid, 1),
        ("Void", BodyKind::Solid, 1),
        ("Seam", BodyKind::Solid, 1),
        ("Degenerate", BodyKind::Solid, 1),
        ("Sheet", BodyKind::Sheet, 1),
        ("Wire", BodyKind::Wire, 1),
        ("Compound", BodyKind::Solid, 2),
        ("MultiShell", BodyKind::Sheet, 2),
        ("CurveCarriers", BodyKind::Wire, 5),
        ("FreeformSurface", BodyKind::Sheet, 1),
        ("Revolved", BodyKind::Solid, 1),
        ("Swept", BodyKind::Solid, 1),
    ];
    assert_eq!(features.len(), source_shapes.len());
    assert_eq!(model.bodies.len(), source_shapes.len());

    let native_objects = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let mut output_ids = BTreeSet::new();
    for (ordinal, (feature, (name, expected_kind, expected_regions))) in
        features.iter().zip(source_shapes).enumerate()
    {
        assert_eq!(
            feature.ordinal,
            u64::try_from(ordinal).unwrap() + 1,
            "{name}: source ordinal"
        );
        assert_eq!(feature.name.as_deref(), Some(name));
        assert_eq!(feature.source_tag.as_deref(), Some("Part::Feature"));
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        ));

        let native_object = native_objects
            .iter()
            .find(|object| object.name() == name)
            .unwrap_or_else(|| panic!("{name}: native object"));
        assert_eq!(native_object.type_name, "Part::Feature", "{name}");
        assert_eq!(
            feature.native_ref.as_deref(),
            Some(native_object.id().as_str()),
            "{name}: native source object"
        );

        let [body_id] = feature.evaluation.outputs().as_slice() else {
            panic!("{name}: expected one output body")
        };
        assert!(
            output_ids.insert(body_id.as_str().to_owned()),
            "{name}: duplicate output"
        );
        let body = model
            .bodies
            .iter()
            .find(|body| &body.id == body_id)
            .unwrap_or_else(|| panic!("{name}: output body resolves"));
        assert_eq!(body.kind, expected_kind, "{name}: body kind");
        assert_eq!(body.regions.len(), expected_regions, "{name}: regions");

        for region_id in &body.regions {
            let region = model
                .regions
                .iter()
                .find(|region| &region.id == region_id)
                .unwrap_or_else(|| panic!("{name}: region resolves"));
            assert_eq!(region.body, body.id, "{name}: region owner");
            assert!(!region.shells.is_empty(), "{name}: region shells");
            for shell_id in &region.shells {
                let shell = model
                    .shells
                    .iter()
                    .find(|shell| &shell.id == shell_id)
                    .unwrap_or_else(|| panic!("{name}: shell resolves"));
                assert_eq!(shell.region, region.id, "{name}: shell owner");
            }
        }
    }
    assert_eq!(output_ids.len(), source_shapes.len());

    let feature = |name: &str| {
        model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    let [carrier_body_id] = feature("CurveCarriers").evaluation.outputs().as_slice() else {
        panic!("CurveCarriers: expected one output body")
    };
    let carrier_body = model
        .bodies
        .iter()
        .find(|body| &body.id == carrier_body_id)
        .unwrap();
    let mut carrier_curves = Vec::new();
    for region_id in &carrier_body.regions {
        let region = model
            .regions
            .iter()
            .find(|region| &region.id == region_id)
            .unwrap();
        for shell_id in &region.shells {
            let shell = model
                .shells
                .iter()
                .find(|shell| &shell.id == shell_id)
                .unwrap();
            for edge_id in shell.wire_edges() {
                let edge = model.edges.iter().find(|edge| &edge.id == edge_id).unwrap();
                let curve_id = edge.curve().expect("CurveCarriers edge has a curve");
                let curve = model
                    .curves
                    .iter()
                    .find(|curve| &curve.id == curve_id)
                    .unwrap();
                assert_eq!(
                    curve
                        .source_object
                        .as_ref()
                        .map(|source| source.object_id.as_str()),
                    feature("CurveCarriers").native_ref.as_deref(),
                    "curve source object"
                );
                carrier_curves.push(curve);
            }
        }
    }
    assert_eq!(carrier_curves.len(), 5);
    assert_eq!(
        carrier_curves
            .iter()
            .filter(|curve| matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Line(_))))
            .count(),
        1
    );
    assert_eq!(
        carrier_curves
            .iter()
            .filter(|curve| matches!(
                curve.geometry.solved(),
                Some(SolvedCurveGeometry::Circle(_))
            ))
            .count(),
        1
    );
    assert_eq!(
        carrier_curves
            .iter()
            .filter(|curve| matches!(
                curve.geometry.solved(),
                Some(SolvedCurveGeometry::Ellipse(_))
            ))
            .count(),
        1
    );
    let nurbs_curves = carrier_curves
        .iter()
        .filter_map(|curve| match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => Some(nurbs),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(nurbs_curves.len(), 2);

    let line = carrier_curves
        .iter()
        .find_map(|curve| match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Line(line)) => Some(line),
            _ => None,
        })
        .unwrap();
    assert_point(line.origin().get(), [0.0, 0.0, 0.0], CARRIER_TOLERANCE);
    let line_direction = *line.direction().as_raw();
    let line_length = 113.0_f64.sqrt();
    assert_near(line_direction.x, 10.0 / line_length, CARRIER_TOLERANCE);
    assert_near(line_direction.y, 3.0 / line_length, CARRIER_TOLERANCE);
    assert_near(line_direction.z, 2.0 / line_length, CARRIER_TOLERANCE);

    let circle = carrier_curves
        .iter()
        .find_map(|curve| match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Circle(circle)) => Some(circle),
            _ => None,
        })
        .unwrap();
    assert_point(circle.center().get(), [0.0, 0.0, 0.0], CARRIER_TOLERANCE);
    assert_near(circle.radius().get(), 5.0, CARRIER_TOLERANCE);

    let ellipse = carrier_curves
        .iter()
        .find_map(|curve| match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Ellipse(ellipse)) => Some(ellipse),
            _ => None,
        })
        .unwrap();
    assert_point(ellipse.center().get(), [0.0, 0.0, 0.0], CARRIER_TOLERANCE);
    assert_near(ellipse.major_radius().get(), 8.0, CARRIER_TOLERANCE);
    assert_near(ellipse.minor_radius().get(), 3.0, CARRIER_TOLERANCE);

    let bezier_poles = [
        [0.0, 0.0, 0.0],
        [3.0, 8.0, 0.0],
        [8.0, -2.0, 0.0],
        [12.0, 4.0, 0.0],
    ];
    let bezier_matches = nurbs_curves
        .iter()
        .filter(|curve| points_match(&curve.control_points(), &bezier_poles))
        .collect::<Vec<_>>();
    assert_eq!(bezier_matches.len(), 1, "Bezier source poles");
    assert_eq!(bezier_matches[0].degree(), 3);
    let interpolated_curve = nurbs_curves
        .iter()
        .find(|curve| !points_match(&curve.control_points(), &bezier_poles))
        .expect("the other NURBS carrier is the interpolated spline");
    assert_curve_interpolation_points(interpolated_curve);

    let [surface_body_id] = feature("FreeformSurface").evaluation.outputs().as_slice() else {
        panic!("FreeformSurface: expected one output body")
    };
    let surface_body = model
        .bodies
        .iter()
        .find(|body| &body.id == surface_body_id)
        .unwrap();
    let [region_id] = surface_body.regions.as_slice() else {
        panic!("FreeformSurface: expected one region")
    };
    let region = model
        .regions
        .iter()
        .find(|region| &region.id == region_id)
        .unwrap();
    let [shell_id] = region.shells.as_slice() else {
        panic!("FreeformSurface: expected one shell")
    };
    let shell = model
        .shells
        .iter()
        .find(|shell| &shell.id == shell_id)
        .unwrap();
    let [face_id] = shell.faces() else {
        panic!("FreeformSurface: expected one face")
    };
    let face = model.faces.iter().find(|face| &face.id == face_id).unwrap();
    let surface = model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap();
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        feature("FreeformSurface").native_ref.as_deref()
    );
    let Some(SolvedSurfaceGeometry::Nurbs(nurbs_surface)) = surface.geometry.solved() else {
        panic!("FreeformSurface: expected an interpolated NURBS surface")
    };
    assert_surface_interpolation_points(nurbs_surface);
}

fn assert_curve_interpolation_points(curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve) {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(4.0, 5.0, 1.0),
        Point3::new(9.0, 1.0, 2.0),
        Point3::new(14.0, 6.0, 0.0),
    ];
    let seeds = curve
        .knots()
        .as_slice()
        .windows(2)
        .filter(|span| span[1] > span[0])
        .map(|span| (span[0] + span[1]) * 0.5)
        .collect::<Vec<_>>();
    let context = cadmpeg_test_support::service_decode_context();
    for point in points {
        let parameter = seeds
            .iter()
            .find_map(|seed| {
                cadmpeg_ir::eval::nurbs_curve_parameter_near_point(
                    &context,
                    curve,
                    point,
                    INTERPOLATION_TOLERANCE,
                    *seed,
                )
                .expect("find an interpolated curve point")
                .map(cadmpeg_ir::scalar::FiniteReal::get)
            })
            .expect("source interpolation point lies on the curve");
        let mapped = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            curve,
            parameter,
        )
        .expect("evaluate the interpolated curve point");
        assert!(mapped.get().distance(point) <= INTERPOLATION_TOLERANCE);
    }
}

fn assert_surface_interpolation_points(surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface) {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 5.0, 2.0),
        Point3::new(0.0, 10.0, 0.0),
        Point3::new(5.0, 0.0, 1.0),
        Point3::new(5.0, 5.0, 4.0),
        Point3::new(5.0, 10.0, 1.0),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(10.0, 5.0, 2.0),
        Point3::new(10.0, 10.0, 0.0),
    ];
    let context = cadmpeg_test_support::service_decode_context();
    for point in points {
        let parameters = cadmpeg_ir::eval::nurbs_surface_closest_parameter_with_budget(
            &context,
            surface,
            point,
            None,
            &cadmpeg_core::decode::WorkBudget::new(1_000_000),
        )
        .expect("find a surface parameter for an interpolation point")
        .expect("source interpolation point lies on the surface");
        let mapped = cadmpeg_ir::eval::decode::nurbs_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            surface,
            parameters.u,
            parameters.v,
        )
        .expect("evaluate the interpolated surface point");
        assert!(mapped.get().distance(point) <= INTERPOLATION_TOLERANCE);
    }
}

fn points_match(points: &[cadmpeg_ir::features::FinitePoint3], expected: &[[f64; 3]]) -> bool {
    points.len() == expected.len()
        && points.iter().zip(expected).all(|(actual, expected)| {
            let actual = actual.get();
            (actual.x - expected[0]).abs() <= CARRIER_TOLERANCE
                && (actual.y - expected[1]).abs() <= CARRIER_TOLERANCE
                && (actual.z - expected[2]).abs() <= CARRIER_TOLERANCE
        })
}

fn assert_point(actual: Point3, expected: [f64; 3], tolerance: f64) {
    assert_near(actual.x, expected[0], tolerance);
    assert_near(actual.y, expected[1], tolerance);
    assert_near(actual.z, expected[2], tolerance);
}

fn assert_near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected}"
    );
}
