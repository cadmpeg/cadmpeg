// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeFailure;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::CompositeCurveSegment;
use cadmpeg_ir::geometry::CompositeCurveTransition;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::ids::VertexId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::test_curves_and_surfaces::composite_curve_file;
use crate::test_support::test_curves_and_surfaces::composite_curve_with_join_gap;
use crate::test_support::test_curves_and_surfaces::heterogeneous_composite_curve_file;
use crate::test_support::test_curves_and_surfaces::mixed_analytic_composite_curve_file;
use crate::test_support::test_curves_and_surfaces::mixed_degree_composite_pcurve_file;
use crate::test_support::test_curves_and_surfaces::parametric_spline_composite_curve_file;
use crate::test_support::test_owned::owned_test_file;
use crate::test_support::test_owned::OwnedTestEntity;

use super::super::bounded_nurbs_for_curve;
use super::super::bounded_nurbs_for_curve_with_tolerance;
use super::super::concatenate_nurbs;
use super::super::elevate_nurbs_to_degree;
use super::super::reverse_nurbs;
use super::super::trim_nurbs_to_interval;
use super::test_nurbs;
use crate::IgesCodec;

#[test]
fn trimming_active_nurbs_subranges_preserves_a_rational_curve() {
    const EPS_TRIMMED_NURBS: f64 = 1.0e-9;
    let curve = test_nurbs(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, -1.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 2.0, 1.0]),
    );
    let interval = [0.25, 1.5];
    let trimmed = trim_nurbs_to_interval(None, &curve, interval)
        .expect("carrier lanes pair")
        .expect("a bounded active interval has an exact NURBS subrange");

    assert_eq!(trimmed.knots().first(), Some(&interval[0]));
    assert_eq!(trimmed.knots().last(), Some(&interval[1]));
    assert_eq!(
        trimmed.weights().map(|weights| weights.len()),
        Some(trimmed.pole_count())
    );
    for parameter in [0.25, 0.5, 1.0, 1.5] {
        let before = cadmpeg_ir::eval::nurbs_curve_point_at(&curve, parameter)
            .expect("source NURBS evaluates");
        let after = cadmpeg_ir::eval::nurbs_curve_point_at(&trimmed, parameter)
            .expect("trimmed NURBS evaluates");
        assert!(before.distance(after.get()) <= EPS_TRIMMED_NURBS);
    }
}

#[test]
fn concatenation_accepts_exact_active_nurbs_subranges() {
    const EPS_TRIMMED_NURBS: f64 = 1.0e-9;
    let curve = test_nurbs(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, -1.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 2.0, 1.0]),
    );
    let first = trim_nurbs_to_interval(None, &curve, [0.0, 1.0])
        .expect("carrier lanes pair")
        .expect("first active NURBS interval is exact");
    let second = trim_nurbs_to_interval(None, &curve, [1.0, 2.0])
        .expect("carrier lanes pair")
        .expect("second active NURBS interval is exact");
    let concatenated = concatenate_nurbs(
        None,
        vec![(first, [0.0, 1.0], ()), (second, [1.0, 2.0], ())],
        None,
    )
    .expect("carrier lanes pair")
    .expect("evaluated active endpoints join exactly");

    for parameter in [0.25, 0.75, 1.25, 1.75] {
        let before = cadmpeg_ir::eval::nurbs_curve_point_at(&curve, parameter)
            .expect("source NURBS evaluates");
        let after = cadmpeg_ir::eval::nurbs_curve_point_at(&concatenated.nurbs, parameter)
            .expect("concatenated NURBS evaluates");
        assert!(before.distance(after.get()) <= EPS_TRIMMED_NURBS);
    }
}

#[test]
fn trimming_supports_degree_zero_and_nonclamped_nurbs() {
    const EPS_TRIMMED_NURBS: f64 = 1.0e-9;
    let piecewise_constant = test_nurbs(
        0,
        vec![0.0, 1.0, 2.0],
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        None,
    );
    let nonclamped = test_nurbs(
        2,
        vec![0.0, 0.5, 1.0, 2.0, 3.0, 4.0, 5.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, -1.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
        ],
        None,
    );

    for (curve, interval, parameters) in [
        (piecewise_constant, [0.5, 1.5], vec![0.75, 1.25]),
        (nonclamped, [1.0, 3.0], vec![1.25, 2.0, 2.75]),
    ] {
        let trimmed = trim_nurbs_to_interval(None, &curve, interval)
            .expect("carrier lanes pair")
            .expect("a valid active interval has an exact NURBS subrange");
        for parameter in parameters {
            let before = cadmpeg_ir::eval::nurbs_curve_point_at(&curve, parameter)
                .expect("source NURBS evaluates");
            let after = cadmpeg_ir::eval::nurbs_curve_point_at(&trimmed, parameter)
                .expect("trimmed NURBS evaluates");
            assert!(before.distance(after.get()) <= EPS_TRIMMED_NURBS);
        }
    }
}

#[test]
fn concatenation_preserves_degree_zero_spans() {
    let point = Point3::new(1.0, 2.0, 3.0);
    let first = (
        test_nurbs(0, vec![0.0, 1.0, 2.0], vec![point, point], None),
        [0.0, 2.0],
    );
    let second = (test_nurbs(0, vec![0.0, 1.0], vec![point], None), [0.0, 1.0]);
    let concatenated = concatenate_nurbs(
        None,
        vec![(first.0, first.1, ()), (second.0, second.1, ())],
        None,
    )
    .expect("carrier lanes pair")
    .expect("degree-zero spans with an exact join concatenate");

    assert_eq!(concatenated.nurbs.degree(), 0);
    assert_eq!(
        concatenated.nurbs.knots().as_slice(),
        vec![0.0, 1.0, 2.0, 3.0]
    );
    assert_eq!(
        concatenated.nurbs.control_points(),
        vec![point, point, point]
    );
    for parameter in [0.5, 1.5, 2.5] {
        assert_eq!(
            cadmpeg_ir::eval::nurbs_curve_point_at(&concatenated.nurbs, parameter)
                .ok()
                .map(cadmpeg_ir::features::FinitePoint3::get),
            Some(point)
        );
    }
}

#[test]
fn multi_span_linear_degree_elevation_preserves_a_degenerate_curve() {
    let mut curve = test_nurbs(
        1,
        vec![0.5, 0.5, 1.5, 2.5, 2.5],
        vec![
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(1.0, 2.0, 3.0),
        ],
        None,
    );
    let before = cadmpeg_ir::eval::nurbs_curve_point_at(&curve, 2.0)
        .expect("valid multi-span linear NURBS evaluates before degree elevation");
    elevate_nurbs_to_degree(None, &mut curve, [0.5, 2.5], 3, None).expect("elevation lanes pair");
    let after = cadmpeg_ir::eval::nurbs_curve_point_at(&curve, 2.0)
        .expect("valid multi-span linear NURBS evaluates after degree elevation");
    assert_eq!(curve.degree(), 3);
    assert!(before.distance(after.get()) <= 1.0e-12);
}

#[test]
fn multi_span_degree_zero_elevation_preserves_the_curve() {
    let point = Point3::new(1.0, 2.0, 3.0);
    let source = test_nurbs(0, vec![0.0, 1.0, 2.0], vec![point; 2], None);
    let mut elevated = source.clone();
    elevate_nurbs_to_degree(None, &mut elevated, [0.0, 2.0], 2, None)
        .expect("elevation lanes pair");
    assert_eq!(elevated.degree(), 2);
    for parameter in [0.25, 0.75, 1.25, 1.75] {
        let before = cadmpeg_ir::eval::nurbs_curve_point_at(&source, parameter).unwrap();
        let after = cadmpeg_ir::eval::nurbs_curve_point_at(&elevated, parameter).unwrap();
        assert_eq!(before, after);
    }
}

#[test]
fn multi_span_rational_degree_elevation_preserves_the_curve() {
    const EPS_DEGREE_ELEVATION: f64 = 1.0e-9;
    let source = test_nurbs(
        2,
        vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, -1.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 2.0, 1.0, 3.0]),
    );
    let mut elevated = source.clone();
    elevate_nurbs_to_degree(None, &mut elevated, [0.0, 1.0], 3, None)
        .expect("elevation lanes pair");
    assert_eq!(elevated.degree(), 3);
    assert_eq!(elevated.weights().map(|weights| weights.len()), Some(7));
    for parameter in [0.0, 0.125, 0.5, 0.75, 1.0] {
        let before = cadmpeg_ir::eval::nurbs_curve_point_at(&source, parameter).unwrap();
        let after = cadmpeg_ir::eval::nurbs_curve_point_at(&elevated, parameter).unwrap();
        assert!(before.distance(after.get()) <= EPS_DEGREE_ELEVATION);
    }
}

#[test]
fn mixed_degree_composition_accepts_a_multi_span_linear_child() {
    let point = |x, y| Point3::new(x, y, 0.0);
    let line = |start, end| test_nurbs(1, vec![0.0, 0.0, 1.0, 1.0], vec![start, end], None);
    let constant = |position| test_nurbs(1, vec![0.0, 0.0, 1.0, 2.0, 2.0], vec![position; 3], None);
    let cubic = test_nurbs(
        3,
        vec![0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0],
        vec![
            point(1.0, 1.0),
            point(1.666_666_666_666_666_7, 0.666_666_666_666_666_6),
            point(2.333_333_333_333_333_5, 0.333_333_333_333_333_3),
            point(3.0, 0.0),
        ],
        None,
    );
    let mut children = vec![
        (line(point(3.0, 0.0), point(2.0, 0.0)), [0.0, 1.0]),
        (constant(point(2.0, 0.0)), [0.0, 2.0]),
        (line(point(2.0, 0.0), point(1.0, 0.0)), [0.0, 1.0]),
        (line(point(1.0, 0.0), point(1.0, 1.0)), [0.0, 1.0]),
        (cubic, [0.0, 2.0]),
        (line(point(3.0, 0.0), point(3.0, 0.0)), [0.0, 1.0]),
    ];
    for (index, (curve, interval)) in children.iter_mut().enumerate() {
        if curve.degree() < 3 {
            elevate_nurbs_to_degree(None, curve, *interval, 3, None)
                .unwrap_or_else(|error| panic!("child {index} should elevate: {error}"));
        }
    }
    let concatenated = concatenate_nurbs(
        None,
        children
            .into_iter()
            .map(|(curve, range)| (curve, range, ()))
            .collect(),
        None,
    )
    .expect("carrier lanes pair")
    .expect("mixed-degree composite should have an exact NURBS carrier");
    assert_eq!(concatenated.nurbs.degree(), 3);
    assert_eq!(
        std::iter::once(0.0)
            .chain(concatenated.segments.into_iter().map(|segment| segment.end))
            .collect::<Vec<_>>(),
        vec![0.0, 1.0, 3.0, 4.0, 5.0, 7.0, 8.0]
    );
}

#[test]
fn concatenated_range_is_exactly_the_canonical_knot_domain() {
    let line = |start: f64, end: f64, x: f64| {
        (
            test_nurbs(
                1,
                vec![start, start, end, end],
                vec![Point3::new(x, 0.0, 0.0), Point3::new(x + 1.0, 0.0, 0.0)],
                None,
            ),
            [start, end],
        )
    };
    let first = line(0.0, 0.3, 0.0);
    let second = line(1.0e9, 1.0e9 + 0.1, 1.0);

    let concatenated = concatenate_nurbs(
        None,
        vec![(first.0, first.1, ()), (second.0, second.1, ())],
        None,
    )
    .expect("carrier lanes pair")
    .expect("joined lines should concatenate");

    assert_eq!(
        Some(&concatenated.segments.end()),
        concatenated.nurbs.knots().last()
    );
}

#[test]
fn tolerance_allows_a_bounded_carrier_join_within_resolution() {
    let first_id = CurveId::mint("test:model:curve#first").expect("identity grammar");
    let second_id = CurveId::mint("test:model:curve#second").expect("identity grammar");
    let composite_id = CurveId::mint("test:model:curve#composite").expect("identity grammar");
    let first_end = Point3::new(1.0, 0.0, 0.0);
    let mut ir = CadIr::empty();
    ir.model.curves.extend([
        Curve {
            id: first_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(test_nurbs(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), first_end],
                None,
            ))),
            source_object: None,
        },
        Curve {
            id: second_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(test_nurbs(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(1.0005, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                None,
            ))),
            source_object: None,
        },
        Curve {
            id: composite_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments: cadmpeg_ir::geometry::CompositeCurveSegments::try_from(vec![
                    CompositeCurveSegment {
                        curve: first_id.clone(),
                        same_sense: true,
                        transition: CompositeCurveTransition::Continuous,
                    },
                    CompositeCurveSegment {
                        curve: second_id.clone(),
                        same_sense: true,
                        transition: CompositeCurveTransition::Continuous,
                    },
                ])
                .unwrap(),
                self_intersect: None,
            }),
            source_object: None,
        },
    ]);
    for (index, curve) in [first_id, second_id].into_iter().enumerate() {
        ir.model.edges.push(Edge {
            id: EdgeId::mint(format!("test:model:edge#edge-{index}")).expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve), Some([0.0, 1.0])).unwrap(),
            start: VertexId::mint(format!("test:model:vertex#start-{index}"))
                .expect("identity grammar"),
            end: VertexId::mint(format!("test:model:vertex#end-{index}"))
                .expect("identity grammar"),
            tolerance: None,
        });
    }
    assert!(bounded_nurbs_for_curve(&ir, &composite_id, None, None)
        .expect("carrier lanes pair")
        .is_none());
    let (carrier, range) =
        bounded_nurbs_for_curve_with_tolerance(&ir, &composite_id, Some(0.001), None, None)
            .expect("carrier lanes pair")
            .expect("carrier join within the global resolution should project");
    assert_eq!(range, [0.0, 2.0]);
    assert_eq!(carrier.control_points()[0], Point3::new(0.0, 0.0, 0.0));
}

#[test]
fn reversing_a_subrange_reflects_the_active_nurbs_domain() {
    let curve = test_nurbs(
        1,
        vec![0.0, 0.0, 10.0, 10.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
        None,
    );
    let (reversed, range) =
        reverse_nurbs(curve, [2.0, 5.0]).expect("a bounded subrange reverses exactly");
    assert_eq!(range, [5.0, 8.0]);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point_at(&reversed, range[0])
            .ok()
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Some(Point3::new(5.0, 0.0, 0.0))
    );
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point_at(&reversed, range[1])
            .ok()
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Some(Point3::new(2.0, 0.0, 0.0))
    );
}

#[test]
fn reversing_a_range_outside_the_active_nurbs_domain_is_rejected() {
    let curve = test_nurbs(
        1,
        vec![0.0, 0.0, 10.0, 10.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
        None,
    );
    let error = reverse_nurbs(curve, [-1.0, 5.0])
        .expect_err("an interval outside the child's own domain is refused")
        .to_string();
    assert!(
        error.contains("outside its domain [0, 10]"),
        "the refusal names the interval and the domain: {error}"
    );
}

#[test]
fn decode_concatenates_ordered_composite_curve_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
        .unwrap();
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = composite.geometry.solved() else {
        panic!("expected a concatenated NURBS cache");
    };
    assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(nurbs.control_points().len(), 3);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, 1.5)
            .ok()
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn composite_join_uses_global_resolution_and_reports_degradation() {
    let within_resolution = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.000_999)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let within_curve = within_resolution
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
        .expect("Type 102 curve within the Global resolution");
    assert!(matches!(
        within_curve.geometry.solved(),
        Some(SolvedCurveGeometry::Nurbs(_))
    ));
    assert!(within_resolution.report().losses.is_empty());

    let outside_resolution = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_001)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let outside_curve = outside_resolution
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
        .expect("degraded Type 102 curve");
    let Some(SolvedCurveGeometry::Composite { segments, .. }) = outside_curve.geometry.solved()
    else {
        panic!("expected retained native Type 102 carrier")
    };
    assert_eq!(
        segments[1].transition,
        cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous
    );
    assert!(outside_resolution.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::CompositeCarrierDegraded.kind()
            && loss.message.contains("Global minimum resolution")
    }));
    let validation = cadmpeg_ir::validate_neutral(
        outside_resolution.ir(),
        outside_resolution.report().losses.clone(),
    )
    .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let at_or_beyond_resolution = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_000_000_000_000_2)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let at_or_beyond_resolution_curve = at_or_beyond_resolution
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
        .expect("Type 102 curve at the Global resolution");
    assert!(matches!(
        at_or_beyond_resolution_curve.geometry.solved(),
        Some(SolvedCurveGeometry::Composite { .. })
    ));
    assert_eq!(
        at_or_beyond_resolution
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::CompositeCarrierDegraded.kind())
            .count(),
        1
    );
}

#[test]
fn strict_decode_refuses_a_degraded_composite_carrier_loss() {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;

    let error = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_001)),
            &options,
        )
        .unwrap_err();

    match error {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => {
            assert_eq!(
                rejection.loss().code.to_string(),
                IgesLossCode::CompositeCarrierDegraded.kind().to_string()
            );
        }
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn decode_concatenates_exact_circular_arc_and_line_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(mixed_analytic_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
        .unwrap();
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = composite.geometry.solved() else {
        panic!("expected an exact quadratic composite cache");
    };
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(nurbs.control_points().len(), 5);
    assert_eq!(
        nurbs.weights().unwrap()[1].get(),
        std::f64::consts::FRAC_1_SQRT_2
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn composite_analytic_child_refuses_arc_lane_before_projection() {
    let bytes = mixed_analytic_composite_curve_file();
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        match IgesCodec.decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        ) {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                if limit.operation == "iges analytic arc weighted poles" {
                    return;
                }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            other => panic!("expected analytic child collection refusal: {other:?}"),
        }
    }
    panic!("analytic child weighted-pole refusal was not reached");
}

#[test]
fn decode_converts_heterogeneous_composite_curve_children_to_an_exact_carrier() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(heterogeneous_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
        .unwrap();
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = composite.geometry.solved() else {
        panic!("expected an exact heterogeneous composite carrier");
    };
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(nurbs.control_points().len(), 5);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_mixed_degree_composite_pcurve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(mixed_degree_composite_pcurve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "iges:model:curve#D7")
        .unwrap();
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = curve.geometry.solved() else {
        panic!("expected an elevated cubic composite cache");
    };
    assert_eq!(nurbs.degree(), 3);
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge
                .curve()
                .is_some_and(|id| id.as_str() == "iges:model:curve#D7"))
            .and_then(cadmpeg_ir::topology::Edge::param_range)
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, 2.0])
    );
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D11")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(face.loops.len(), 1);
    assert_eq!(result.ir().model.pcurves.len(), 1);
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } =
        &result.ir().model.pcurves[0].geometry
    else {
        panic!("expected an elevated cubic composite pcurve");
    };
    assert_eq!(nurbs.degree(), 3);
    assert_eq!(result.ir().model.pcurves[0].fit_tolerance(), None);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_composite_curve_with_an_inconsistent_parametric_spline_child() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "iges:model:curve#D3")
        .expect("composite curve should be projected after its spline child");
    assert!(matches!(
        composite.geometry.solved(),
        Some(SolvedCurveGeometry::Nurbs(_))
    ));
    assert_eq!(result.report().losses.len(), 2);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("terminal derivative block disagrees with the last polynomial")
    }));
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
            .count(),
        1
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::SplineHeaderNotTransferred.kind())
            .count(),
        1
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_large_composite_batch_without_repeated_curve_scans() {
    const COMPOSITE_COUNT: usize = 2_000;
    let mut entities = vec![
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "CHILD1".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "CHILD2".into(),
            status: "00010000",
            parameters: "110,1,0,0,2,0,0;".into(),
        },
    ];
    entities.extend((0..COMPOSITE_COUNT).map(|index| OwnedTestEntity {
        entity_type: 102,
        form: 0,
        label: format!("C{index:06}"),
        status: "00000000",
        parameters: "102,2,1,3;".into(),
    }));

    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_curves.len(), COMPOSITE_COUNT);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

/// A child whose stated interval does not increase is not an endpoint-join
/// failure, and the refusal says so.
#[test]
fn a_reversed_child_interval_names_itself_not_the_endpoint_join() {
    let point = Point3::new(1.0, 2.0, 3.0);
    let first = (
        test_nurbs(0, vec![0.0, 1.0, 2.0], vec![point, point], None),
        [0.0, 2.0],
    );
    // The second child states the interval [1.0, 0.0] over an ordinary knot
    // vector: the stated interval, not the knots, runs backwards.
    let second = (test_nurbs(0, vec![0.0, 1.0], vec![point], None), [1.0, 0.0]);
    let error = concatenate_nurbs(
        None,
        vec![(first.0, first.1, ()), (second.0, second.1, ())],
        None,
    )
    .expect_err("a reversed child interval is refused by name")
    .to_string();
    assert!(
        error.contains("reversed interval"),
        "the refusal names the reversed interval: {error}"
    );
    assert!(
        !error.contains("endpoints"),
        "the refusal is not the endpoint join: {error}"
    );
}

/// A composite that states no child at all is refused by name.
#[test]
fn an_empty_child_list_names_itself() {
    let error = concatenate_nurbs(None, Vec::<(NurbsCurve, [f64; 2], ())>::new(), None)
        .expect_err("an empty child list is refused by name")
        .to_string();
    assert!(
        error.contains("no child curve"),
        "the refusal names the empty child list: {error}"
    );
}
