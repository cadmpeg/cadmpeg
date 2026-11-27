// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::test_curves_and_surfaces::conic_arc_file;
use crate::IgesCodec;
use crate::IgesVersion;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::Codec;
use std::io::Cursor;

#[test]
fn encode_emits_the_typed_ellipse_form_for_v5_0() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(conic_arc_file(0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;")),
            &DecodeOptions::default(),
        )
        .expect("the ellipse fixture decodes");
    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(IgesVersion::V5_0.descriptor().id.as_str()),
        )
        .expect("V5.0 admits a typed ellipse");
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).expect("V5.0 ellipse writes");
    assert!(!report.losses.iter().any(|loss| {
        loss.code.taxonomy() == cadmpeg_ir::report::loss::LossTaxonomy::GeometryNotTransferred
    }));

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("the V5.0 ellipse output decodes");
    let conic = round_trip
        .ir()
        .native
        .namespace("iges")
        .expect("the output has an IGES namespace")
        .arenas()["entities"]
        .iter()
        .find(|record| record.field("entity_type") == Some(104.into()))
        .expect("the output has a Type 104 entity");
    assert_eq!(conic.field("form"), Some(1.into()));
    assert!(round_trip.report().losses.is_empty());
    assert_eq!(
        round_trip.report().dialects().unwrap().primary().declared()["effective_version"],
        "5.0"
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn transformed_nurbs_overflow_is_refused_without_changing_the_source() {
    let nurbs = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            Point3::new(f64::MAX, 0.0, 0.0),
            Point3::new(f64::MAX, 1.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    let transform = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Nurbs(nurbs)),
            transform,
        )
        .expect("placed curve"),
    ));
    let before = geometry.clone();
    let error =
        crate::writer::flatten_curve(geometry.solved().expect("solved carrier")).unwrap_err();
    assert!(error.to_string().contains("non-finite"));
    assert_eq!(geometry, before);
}

/// The Type 126 closed flag (PROP2) states whether the curve starts and ends
/// at one point over the parameter range that the entity states.
#[test]
fn the_type_126_closed_flag_follows_the_stated_parameter_range() {
    let radius = cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("a radius");
    let circle = crate::entities::curve_conversion::elliptical_arc_nurbs(
        Point3::new(0.0, 0.0, 0.0),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius,
        radius,
        [0.0, std::f64::consts::TAU],
    )
    .expect("a rational circle")
    .expect("a rational circle over one turn");
    let degree = usize::try_from(circle.degree()).expect("a degree");
    let knots = circle
        .knots()
        .finite_knots()
        .map(cadmpeg_ir::scalar::FiniteReal::get)
        .collect::<Vec<_>>();
    let domain = [knots[degree], knots[circle.control_points().len()]];
    for (range, closed) in [
        (domain, "1"),
        ([domain[0], domain[0].midpoint(domain[1])], "0"),
        ([domain[0].midpoint(domain[1]), domain[1]], "0"),
    ] {
        let entity = crate::writer::encode_nurbs(
            &circle,
            cadmpeg_ir::units::FiniteVector::new(range).expect("a finite range"),
            "circle",
        )
        .expect("the curve is written");
        let parameters = String::from_utf8(entity.parameter_body).expect("ASCII parameters");
        assert_eq!(entity.type_code, 126);
        assert_eq!(
            parameters.split(',').nth(3),
            Some(closed),
            "range {range:?}: {parameters}"
        );
    }
}
