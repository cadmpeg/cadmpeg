// SPDX-License-Identifier: Apache-2.0
mod admission;

const EPS_EXPECTED_PARAMETER: f64 = 1.0e-12;
const SPHERE_POLE_ROUNDING_OFFSET: f64 = 1.0e-9;
const EPS_INTERSECTION_FIT_MM: f64 = 1.0e-9;
const EPS_EXPECTED_INTERSECTION_PARAMETER: f64 = 1.0e-10;
const EPS_EXPECTED_BILINEAR_PARAMETER: f64 = 1.0e-8;

use super::sphere_latitude;
use super::unique_face_colors;
use crate::brep::entity;
use crate::brep::topology::{Bridge, Coedge, EdgeReferences, EdgeUse, Loop, Tables};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::topology::Sense;

fn with_test_context<T>(f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    f(&ctx)
}

fn intersection_support_pcurve(
    support_data: &crate::brep::intersection::IntersectionSupportData,
    chart: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    surface_attr: u16,
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    edge_endpoints: [cadmpeg_ir::math::Point3; 2],
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<
    Option<(
        super::PcurveGeometry,
        [f64; 2],
        super::IntersectionPcurveSource,
    )>,
    cadmpeg_core::CodecError,
> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("empty test root fits service policy");
    super::intersection_support_pcurve(
        &ctx,
        support_data,
        chart,
        surface_attr,
        surface,
        edge_endpoints,
        refusal,
    )
}

fn test_nurbs_curve(
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<cadmpeg_ir::math::Point3>,
    weights: Option<Vec<f64>>,
) -> cadmpeg_ir::geometry::nurbs::NurbsCurve {
    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        degree,
        knots,
        control_points,
        weights,
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid test NURBS curve")
}

// The fixture helper states each independent NURBS grid parameter explicitly.
#[allow(clippy::too_many_arguments)]
fn test_nurbs_surface(
    u_degree: u32,
    v_degree: u32,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    _u_count: u32,
    v_count: u32,
    control_points: &[cadmpeg_ir::math::Point3],
    weights: Option<Vec<f64>>,
) -> cadmpeg_ir::geometry::nurbs::NurbsSurface {
    cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
            control_points
                .chunks(cadmpeg_core::decode::index_from_u32(v_count))
                .map(<[_]>::to_vec)
                .collect(),
            weights.map(|values| {
                values
                    .chunks(cadmpeg_core::decode::index_from_u32(v_count))
                    .map(<[_]>::to_vec)
                    .collect()
            }),
        ),
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid test NURBS surface")
}

#[test]
fn sphere_latitude_refuses_a_circle_plane_beyond_the_pole() {
    use std::f64::consts::FRAC_PI_2;
    // Planes the sphere carries: the equator and the pole.
    assert_eq!(sphere_latitude(0.0, 2.0), Some(0.0));
    assert_eq!(sphere_latitude(2.0, 2.0), Some(FRAC_PI_2));
    // The rounding the radius match admits reaches the pole and no further.
    assert_eq!(
        sphere_latitude(2.0 + SPHERE_POLE_ROUNDING_OFFSET, 2.0),
        Some(FRAC_PI_2)
    );
    // A plane beyond that band states a circle on another sphere. The
    // radius match alone admits it, because a circle radius near zero
    // matches the saturated `sqrt(radius^2 - height^2)`.
    assert_eq!(sphere_latitude(2.1, 2.0), None);
    // A signed radius keeps the signed ratio.
    assert_eq!(sphere_latitude(2.0, -2.0), Some(-FRAC_PI_2));
}

#[test]
fn line_edge_parameters_convert_from_metres_to_millimetres() {
    let carrier = crate::brep::CurveCarrier {
        attr: 1,
        offset: 0,
        end: 0,
        geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 17.5, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0),
            )
            .unwrap(),
        )),
        parameter_range: Some(
            cadmpeg_ir::units::FiniteVector::new([-0.014, 0.0165]).expect("finite fixture range"),
        ),
    };

    let endpoints = [
        cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
        cadmpeg_ir::math::Point3::new(0.0, 31.5, 0.0),
    ];
    assert_eq!(
        super::edge_parameter_range(
            &cadmpeg_test_support::service_decode_context(),
            &carrier,
            Some(endpoints)
        )
        .unwrap(),
        Some(([-14.0, 16.5], true))
    );
}

fn bridge_record(attr: u16, refs: [u16; 5]) -> Bridge {
    Bridge {
        attr,
        sequence: 0,
        refs,
        sense: Sense::Forward,
        owner: None,
        offset: 0,
    }
}

fn loop_record(attr: u16, refs: [u16; 4]) -> Loop {
    Loop {
        attr,
        refs,
        offset: 0,
    }
}

fn coedge_record(attr: u16, refs: [u16; 9]) -> Coedge {
    Coedge {
        attr,
        refs,
        sense: Sense::Forward,
        offset: 0,
    }
}

#[test]
fn face_walk_rejects_a_loop_owned_by_another_bridge() {
    let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
    let mut tables = Tables::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    tables
        .insert_loop(&ctx, loop_record(20, [0, 40, 11, 0]))
        .expect("loop");
    tables
        .insert_coedge(&ctx, coedge_record(40, [0, 0, 0, 40, 0, 0, 0, 0, 0]))
        .expect("coedge");

    let face = super::walk_face(&ctx, &bridge, &tables).expect("face walk");

    assert!(face.loops.is_empty());
}

#[test]
fn face_walk_rejects_a_ring_owned_by_another_loop() {
    let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
    let mut tables = Tables::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    tables
        .insert_loop(&ctx, loop_record(20, [0, 40, 10, 0]))
        .expect("loop");
    tables
        .insert_coedge(&ctx, coedge_record(40, [0, 21, 0, 40, 0, 0, 0, 0, 0]))
        .expect("coedge");

    let face = super::walk_face(&ctx, &bridge, &tables).expect("face walk");

    assert!(face.loops.is_empty());
}

#[test]
fn face_walk_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
    assert!(matches!(
        super::walk_face(&ctx, &bridge, &Tables::default()),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn face_walk_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let bridge = bridge_record(10, [0, 0, 20, 0, 30]);
    assert!(matches!(
        super::walk_face(&ctx, &bridge, &Tables::default()),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

fn face_color(face_attr: u16, color_attr: u16, face_seq: u32, rgb: [f32; 3]) -> entity::FaceColor {
    entity::FaceColor {
        face_attr,
        color_attr,
        face_seq,
        stream_order: 0,
        color: Color::new(rgb[0], rgb[1], rgb[2], 1.0).expect("valid color"),
        offset: usize::from(face_attr),
        target: None,
    }
}

fn face_color_version(face_attr: u16, seq: u32, stream_order: usize) -> entity::FaceColorVersion {
    entity::FaceColorVersion {
        face_attr,
        seq,
        stream_order,
    }
}

#[test]
fn current_uncolored_face_version_removes_an_older_color() {
    let colors = vec![face_color(700, 900, 1, [0.25, 0.5, 0.75])];

    let (resolved, unresolved) = with_test_context(|ctx| {
        unique_face_colors(
            ctx,
            colors,
            vec![face_color_version(700, 1, 0), face_color_version(700, 2, 0)],
        )
        .expect("face colors")
    });

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 0);
}

#[test]
fn later_stream_replaces_an_equal_sequence_face_color() {
    let old = face_color(700, 900, 2, [0.25, 0.5, 0.75]);
    let mut current = face_color(700, 901, 2, [0.75, 0.5, 0.25]);
    current.stream_order = 1;

    let (resolved, unresolved) = with_test_context(|ctx| {
        unique_face_colors(
            ctx,
            vec![old, current],
            vec![face_color_version(700, 2, 0), face_color_version(700, 2, 1)],
        )
        .expect("face colors")
    });

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].color_attr, 901);
    assert_eq!(unresolved, 0);
}

#[test]
fn conflicting_current_face_colors_remain_unresolved() {
    let colors = vec![
        face_color(700, 900, 2, [0.25, 0.5, 0.75]),
        face_color(700, 901, 2, [0.75, 0.5, 0.25]),
    ];

    let (resolved, unresolved) = with_test_context(|ctx| {
        unique_face_colors(
            ctx,
            colors,
            vec![face_color_version(700, 2, 0), face_color_version(700, 2, 0)],
        )
        .expect("face colors")
    });

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 1);
}

#[test]
fn conflicting_reuse_of_one_color_identity_remains_unresolved() {
    let colors = vec![
        face_color(700, 900, 2, [0.25, 0.5, 0.75]),
        face_color(701, 900, 2, [0.75, 0.5, 0.25]),
    ];

    let (resolved, unresolved) = with_test_context(|ctx| {
        unique_face_colors(
            ctx,
            colors,
            vec![face_color_version(700, 2, 0), face_color_version(701, 2, 0)],
        )
        .expect("face colors")
    });

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 2);
}

#[test]
fn intersection_uv_converts_length_parameters_and_exact_endpoints() {
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    let endpoints = [
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            0.0,
            3.0,
        )
        .expect("cylinder start")
        .get(),
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            0.5,
            2.0,
        )
        .expect("cylinder end")
        .get(),
    ];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
    let support_data = super::super::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: 0.2,
        support_uv: Some([
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0029),
                cadmpeg_ir::math::Point2::new(0.5, 0.0018),
            ],
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ],
        ]),
    };
    let (geometry, range, source) = intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .expect("support parameterization");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected solved UV NURBS");
    };
    assert_eq!(
        nurbs.control_points()[0],
        cadmpeg_ir::math::Point2::new(0.0, 3.0)
    );
    assert_eq!(
        nurbs.control_points()[1],
        cadmpeg_ir::math::Point2::new(0.5, 2.0)
    );
    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(source, super::IntersectionPcurveSource::StoredCache);

    let ambiguous = super::super::intersection::IntersectionSupportData {
        supports: [10, 10],
        ..support_data.clone()
    };
    assert!(intersection_support_pcurve(
        &ambiguous,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .expect("resource allocation did not fail")
    .is_none());

    let malformed = super::super::intersection::IntersectionSupportData {
        support_uv: Some([Vec::new(), Vec::new()]),
        ..support_data
    };
    assert!(intersection_support_pcurve(
        &malformed,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .expect("resource allocation did not fail")
    .is_none());
}

#[test]
fn intersection_support_pcurve_refuses_collection_limit() {
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid cylinder"),
    ));
    let endpoints = [
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            0.0,
            3.0,
        )
        .expect("start")
        .get(),
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            0.5,
            2.0,
        )
        .expect("end")
        .get(),
    ];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
    let support_data = super::super::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: 0.2,
        support_uv: Some([
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0029),
                cadmpeg_ir::math::Point2::new(0.5, 0.0018),
            ],
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ],
        ]),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits policy");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = super::intersection_support_pcurve(
        &ctx,
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    ) else {
        panic!("two UV controls exceed one collection item")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert!(intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service policy")
    .is_some());
}

#[test]
fn analytic_intersection_chart_derives_continuous_uv_without_cache() {
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    let model_points = [(3.0, 1.0), (3.2, 2.0), (3.4, 3.0)]
        .map(|(u, v)| {
            cadmpeg_ir::eval::decode::surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &surface,
                u,
                v,
            )
            .expect("cylinder point")
            .get()
        })
        .to_vec();
    let endpoints = [model_points[0], model_points[2]];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
    let support_data = super::super::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: 0.011,
        support_uv: None,
    };

    let (geometry, _, source) = intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .expect("analytic support inversion");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected solved UV NURBS");
    };
    for (point, expected) in nurbs
        .control_points()
        .iter()
        .zip([(3.0, 1.0), (3.2, 2.0), (3.4, 3.0)])
    {
        assert!((point.u - expected.0).abs() < EPS_EXPECTED_PARAMETER);
        assert!((point.v - expected.1).abs() < EPS_EXPECTED_PARAMETER);
    }
    assert_eq!(source, super::IntersectionPcurveSource::AnalyticInverse);

    let under_toleranced = super::super::intersection::IntersectionSupportData {
        fit_tolerance_mm: 0.009,
        ..support_data
    };
    assert!(intersection_support_pcurve(
        &under_toleranced,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .expect("resource allocation did not fail")
    .is_none());
}

#[test]
fn analytic_torus_chart_unwraps_both_periodic_parameters() {
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            5.0,
            1.0,
        )
        .unwrap(),
    ));
    let expected = [(3.0, 3.0), (3.2, 3.2), (3.4, 3.4)];
    let model_points = expected
        .map(|(u, v)| {
            cadmpeg_ir::eval::decode::surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &surface,
                u,
                v,
            )
            .expect("torus point")
            .get()
        })
        .to_vec();
    let endpoints = [model_points[0], model_points[2]];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
    let support_data = super::super::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: 0.08,
        support_uv: None,
    };

    let (geometry, _, _) = intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .expect("torus support inversion");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected solved UV NURBS");
    };
    for (point, expected) in nurbs.control_points().iter().zip(expected) {
        assert!((point.u - expected.0).abs() < EPS_EXPECTED_PARAMETER);
        assert!((point.v - expected.1).abs() < EPS_EXPECTED_PARAMETER);
    }
}

#[test]
fn nurbs_intersection_chart_inverts_with_continuation_seeds() {
    let nurbs = test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let surface =
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs.clone()));
    let expected = [(0.2, 0.1), (0.5, 0.4), (0.8, 0.7)];
    let model_points = expected
        .map(|(u, v)| {
            cadmpeg_ir::eval::decode::nurbs_surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &nurbs,
                u,
                v,
            )
            .expect("surface point")
            .get()
        })
        .to_vec();
    let endpoints = [model_points[0], model_points[2]];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
    let support_data = super::super::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: EPS_INTERSECTION_FIT_MM,
        support_uv: None,
    };

    let (geometry, _, source) = intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .expect("NURBS support inversion");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected solved UV NURBS");
    };
    for (point, expected) in nurbs.control_points().iter().zip(expected) {
        assert!((point.u - expected.0).abs() < EPS_EXPECTED_INTERSECTION_PARAMETER);
        assert!((point.v - expected.1).abs() < EPS_EXPECTED_INTERSECTION_PARAMETER);
    }
    assert_eq!(source, super::IntersectionPcurveSource::NurbsInverse);
}

#[test]
fn nurbs_intersection_chart_requires_a_complete_chord_certificate() {
    let nurbs = test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
        ],
        None,
    );
    let surface =
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs));
    let endpoints = [
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
    ];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
    let support_data = |fit_tolerance_mm| super::super::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm,
        support_uv: None,
    };

    assert!(intersection_support_pcurve(
        &support_data(0.3),
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .is_none());
    assert!(intersection_support_pcurve(
        &support_data(0.34),
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .is_some());
}

#[test]
fn canonical_edge_direction_uses_explicit_or_unique_forward_coedge() {
    use std::collections::BTreeMap;

    let record = |attr, refs, sense| Coedge {
        attr,
        refs,
        sense,
        offset: 0,
    };
    let edge = |attr, references| EdgeUse {
        attr,
        references,
        sequence: 0,
        offset: 0,
    };
    let mut coedges = BTreeMap::from([
        (
            10,
            record(10, [0, 0, 0, 0, 101, 0, 7, 0, 0], Sense::Reversed),
        ),
        (
            11,
            record(11, [0, 0, 0, 0, 102, 10, 7, 0, 0], Sense::Forward),
        ),
    ]);
    let prefixed_edge = edge(7, EdgeReferences::Compact { curve: 300 });
    let ctx = cadmpeg_test_support::service_decode_context();
    let canonical = |edge_use: &EdgeUse, coedges: &BTreeMap<u16, Coedge>| {
        let forward = super::forward_coedges_by_edge(&ctx, coedges).expect("forward coedge index");
        super::canonical_coedge_attr(&ctx, 7, Some(edge_use), coedges, &forward)
            .expect("canonical coedge lookup")
    };

    assert_eq!(canonical(&prefixed_edge, &coedges), Some(11));

    let bare_edge = edge(7, EdgeReferences::Bare([11, 0, 0, 300, 0, 0]));
    assert_eq!(canonical(&bare_edge, &coedges), Some(11));

    let sentinel_edge = edge(7, EdgeReferences::Bare([1, 0, 0, 300, 0, 0]));
    assert_eq!(canonical(&sentinel_edge, &coedges), Some(11));

    let reversed_edge = edge(7, EdgeReferences::Bare([10, 0, 0, 300, 0, 0]));
    assert_eq!(canonical(&reversed_edge, &coedges), None);

    coedges.insert(
        12,
        record(12, [0, 0, 0, 0, 103, 0, 7, 0, 0], Sense::Forward),
    );
    assert_eq!(canonical(&prefixed_edge, &coedges), None);
}

#[test]
fn boundary_coedge_uses_ring_endpoint_but_reciprocal_twin_supplies_edge_end() {
    use std::collections::BTreeMap;

    let record = |attr, refs| Coedge {
        attr,
        refs,
        sense: Sense::Forward,
        offset: 0,
    };
    let boundary = BTreeMap::from([(10, record(10, [0, 0, 0, 0, 101, 10, 7, 0, 0]))]);
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::edge_end_vuse(&ctx, 10, 102, &boundary).expect("edge end lookup"),
        102
    );

    let reciprocal = BTreeMap::from([
        (10, record(10, [0, 0, 0, 0, 101, 11, 7, 0, 0])),
        (11, record(11, [0, 0, 0, 0, 102, 10, 7, 0, 0])),
    ]);
    assert_eq!(
        super::edge_end_vuse(&ctx, 10, 103, &reciprocal).expect("edge end lookup"),
        102
    );
}

#[test]
fn normalized_surface_parameter_reversal_toggles_face_sense() {
    use cadmpeg_ir::topology::Sense;

    assert_eq!(super::surface_sense(Sense::Forward, false), Sense::Forward);
    assert_eq!(
        super::surface_sense(Sense::Reversed, false),
        Sense::Reversed
    );
    assert_eq!(super::surface_sense(Sense::Forward, true), Sense::Reversed);
    assert_eq!(super::surface_sense(Sense::Reversed, true), Sense::Forward);
}

#[test]
fn shared_edge_coedge_parity_orients_connected_faces() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Coedge, Face, Loop, Sense};

    let face = |id: &str, lp: &str| Face {
        id: FaceId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: SurfaceId::mint(format!("test:model:entity#surface-{id}"))
            .expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(format!(
            "test:model:entity#{lp}"
        ))
        .expect("identity grammar")]),
        name: None,
        color: None,
        tolerance: None,
    };
    let lp = |id: &str, face: &str, coedge: &str| Loop {
        id: LoopId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        face: FaceId::mint(format!("test:model:entity#{face}")).expect("identity grammar"),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![CoedgeId::mint(format!("test:model:entity#{coedge}"))
                    .expect("identity grammar")],
                Vec::new(),
            )
            .expect("fixture ring admission")
            .expect("valid loop ring"),
        ),
    };
    let coedge = |id: &str, lp: &str, radial: &str, sense| Coedge {
        id: CoedgeId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        owner_loop: LoopId::mint(format!("test:model:entity#{lp}")).expect("identity grammar"),
        edge: EdgeId::mint("test:model:entity#edge").expect("identity grammar"),
        radial_next: CoedgeId::mint(format!("test:model:entity#{radial}"))
            .expect("identity grammar"),
        sense,
        use_curve: None,
        pcurves: Vec::new(),
    };
    let mut brep = super::Brep {
        faces: vec![face("face-a", "loop-a"), face("face-b", "loop-b")],
        loops: vec![
            lp("loop-a", "face-a", "coedge-a"),
            lp("loop-b", "face-b", "coedge-b"),
        ],
        coedges: vec![
            coedge("coedge-a", "loop-a", "coedge-b", Sense::Forward),
            coedge("coedge-b", "loop-b", "coedge-a", Sense::Forward),
        ],
        ..Default::default()
    };

    with_test_context(|ctx| super::solve_face_orientation(ctx, &mut brep))
        .expect("orient first face pair");
    assert_eq!(brep.faces[0].sense, Sense::Forward);
    assert_eq!(brep.faces[1].sense, Sense::Reversed);

    brep.faces[1].sense = Sense::Reversed;
    brep.coedges[1].sense = Sense::Reversed;
    with_test_context(|ctx| super::solve_face_orientation(ctx, &mut brep))
        .expect("orient reversed face pair");
    assert_eq!(brep.faces[0].sense, Sense::Forward);
    assert_eq!(brep.faces[1].sense, Sense::Forward);
}

#[test]
fn geometry_free_stream_does_not_report_synthetic_body_grouping() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let decoded = super::decode_body(&ctx, &[], &cadmpeg_ir::stream_name!("empty"))
        .expect("valid exactness fields");

    assert!(decoded.faces.is_empty());
    assert!(!decoded.stats.synthetic_body_grouping);
}

#[test]
fn native_brep_carrier_collection_refuses_before_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let body = crate::test_support::parasolid::triangle_body();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "index SLDPRT surface carriers",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy)?;
            super::decode_body(
                &ctx,
                &body,
                &cadmpeg_ir::stream_name!("candidate-admission"),
            )
        },
    );
    assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index SLDPRT surface carriers"));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&body, &arena, &DecodePolicy::service()).expect("root");
    super::decode_body(
        &ctx,
        &body,
        &cadmpeg_ir::stream_name!("candidate-admission"),
    )
    .expect("service profile admits native B-rep candidates");
}

#[test]
fn typed_body_records_preserve_stored_sheet_kind_and_links() {
    use crate::brep::typed::{BodyNode, FaceNode, Facts, RegionNode, ShellNode};
    use cadmpeg_ir::topology::BodyKind;
    use std::collections::BTreeSet;

    let facts = Facts {
        bodies: vec![BodyNode {
            attr: 3,
            node_id: 7,
            topology_refs: [7, 8, 9, 10, 1, 12, 11],
            ownership_refs: Vec::new(),
            kind: BodyKind::Sheet,
            offset: 1,
            end: 2,
        }],
        shells: vec![ShellNode {
            attr: 7,
            node_id: 814,
            refs: [1, 3, 1, 38, 1, 1, 39, 1],
            offset: 3,
            end: 4,
        }],
        regions: vec![
            RegionNode {
                attr: 11,
                node_id: 244,
                refs: [1, 3, 39, 1, 44],
                offset: 5,
                end: 6,
            },
            RegionNode {
                attr: 39,
                node_id: 815,
                refs: [1, 3, 1, 11, 7],
                offset: 7,
                end: 8,
            },
        ],
        faces: vec![FaceNode {
            attr: 100,
            node_id: 900,
            refs: [1, 1, 49, 7, 8],
            sense: Sense::Forward,
            offset: 9,
            end: 10,
        }],
    };
    let mut tables = Tables::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    tables
        .insert_bridge(
            &ctx,
            Bridge {
                attr: 100,
                sequence: 0,
                refs: [1, 1, 49, 7, 8],
                sense: Sense::Forward,
                owner: None,
                offset: 11,
            },
        )
        .expect("bridge");

    let records = super::typed_body_records(&ctx, &facts, &tables)
        .expect("body record allocation")
        .expect("typed body records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, BodyKind::Sheet);
    assert_eq!(records[0].regions.len(), 2);
    assert!(records[0].refs.contains(&100));
    assert_eq!(records[0].regions[1].shells[0].refs, vec![100]);
    assert_eq!(
        facts
            .hierarchies(&ctx, &BTreeSet::from([100]))
            .expect("hierarchy allocation")
            .expect("typed hierarchy")[0]
            .body
            .kind,
        BodyKind::Sheet
    );
}

#[test]
fn ambiguous_face_owner_stats_survive_when_all_uses_are_withheld() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let bridge = |attr, surface, offset| Bridge {
        attr,
        sequence: 0,
        refs: [0, 0, 0, 0, surface],
        sense: Sense::Forward,
        owner: Some(700),
        offset,
    };
    let mut tables = super::topology::Tables::default();
    tables
        .insert_bridge(&ctx, bridge(10, 100, 20))
        .expect("bridge");
    tables
        .insert_bridge(&ctx, bridge(11, 200, 10))
        .expect("bridge");
    let decoded = super::decode_graph(
        &ctx,
        &crate::brep::index::CarrierIndex::default(),
        &tables,
        super::entity::Facts {
            entity_count: 1,
            ..Default::default()
        },
        &super::typed::Facts::default(),
        &cadmpeg_ir::stream_name!("empty"),
    )
    .expect("valid exactness fields");

    assert!(decoded.faces.is_empty());
    assert_eq!(decoded.stats.ambiguous_face_owners, 1);
}

#[test]
fn topology_pruning_retains_a_procedural_blend_spine() {
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, Curve, CurveGeometry, ProceduralSurface,
        ProceduralSurfaceDefinition, SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId};

    let spine = CurveId::mint("test:model:entity#spine").expect("identity grammar");
    let mut brep = super::Brep {
        curves: vec![Curve {
            id: spine.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        }],
        procedural_surfaces: vec![ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:entity#blend").expect("identity grammar"),
            ProceduralSurfaceDefinition::Blend(
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    [None, None],
                    Some(spine.clone()),
                    BlendRadiusLaw::constant(0.5).unwrap(),
                    BlendCrossSection::Circular,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .unwrap(),
            ),
            None,
        )],
        ..Default::default()
    };

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    super::prune_rejected_topology(&ctx, &mut brep).expect("prune topology");
    assert_eq!(brep.curves.first().map(|curve| &curve.id), Some(&spine));
}

#[test]
fn homogeneous_quadratic_identity_proves_constant_radius() {
    let radius = 2.0;
    let checked_radius = cadmpeg_ir::scalar::PositiveLength::new(radius).expect("positive radius");
    let controls = [
        cadmpeg_ir::math::Point2::new(radius, 0.0),
        cadmpeg_ir::math::Point2::new(radius, radius),
        cadmpeg_ir::math::Point2::new(0.0, radius),
    ];
    let weights = [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0];
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    assert!(super::quadratic_nurbs_has_constant_radius(
        &cadmpeg_test_support::service_decode_context(),
        &controls,
        Some(&weights),
        &knots,
        checked_radius,
    )
    .unwrap());

    let mut invalid = controls;
    invalid[1].u += 0.01;
    assert!(!super::quadratic_nurbs_has_constant_radius(
        &cadmpeg_test_support::service_decode_context(),
        &invalid,
        Some(&weights),
        &knots,
        checked_radius,
    )
    .unwrap());
}

#[test]
fn interior_ruled_surface_line_has_affine_isoparametric_inverse() {
    let surface = test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 1.0, 0.0),
        ],
        None,
    );
    let geometry = match with_test_context(|ctx| {
        super::ruled_surface_line_pcurve(
            ctx,
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
            cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
    })
    .expect("ruling evaluation")
    {
        super::InverseResolution::Unique(geometry) => geometry,
        super::InverseResolution::NoMatch => panic!("interior ruling did not match"),
        super::InverseResolution::Ambiguous => panic!("interior ruling was ambiguous"),
    };
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected affine line pcurve");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!(origin.u.abs() < EPS_EXPECTED_PARAMETER);
    assert!((origin.v - 0.5).abs() < EPS_EXPECTED_PARAMETER);
    assert!((direction.u - 2.0 / 3.0).abs() < EPS_EXPECTED_PARAMETER);
    assert!(direction.v.abs() < EPS_EXPECTED_PARAMETER);
}

#[test]
fn interior_linear_axis_rational_nurbs_isocurve_has_exact_pcurve() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![-0.1, -0.1, 0.9, 0.9],
        3,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, -1.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 3.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, -1.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 3.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, -1.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, 3.0),
        ],
        Some(vec![1.0, 1.0, 2.0, 2.0, 1.0, 1.0]),
    );
    let curve = test_nurbs_curve(
        2,
        surface.u_knots().to_vec(),
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 2.0, 1.0]),
    );
    let geometry =
        match super::nurbs_isocurve_pcurve(&ctx, &surface, &curve).expect("isocurve lanes pair") {
            super::InverseResolution::Unique(geometry) => geometry,
            super::InverseResolution::NoMatch => panic!("interior isocurve did not match"),
            super::InverseResolution::Ambiguous => panic!("interior isocurve was ambiguous"),
        };
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected isoparametric line pcurve");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!(origin.u.abs() < EPS_EXPECTED_PARAMETER);
    assert!((origin.v - 0.15).abs() < EPS_EXPECTED_PARAMETER);
    assert!((direction.u - 1.0).abs() < EPS_EXPECTED_PARAMETER);
    assert!(direction.v.abs() < EPS_EXPECTED_PARAMETER);
}

#[test]
fn extended_nurbs_isocurve_clamps_the_carrier_before_matching() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let curve = test_nurbs_curve(
        1,
        vec![-1.0, -1.0, 2.0, 2.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.5, -1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 2.0, 0.0),
        ],
        None,
    );
    let resolution = super::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.2, 0.8])
        .expect("isocurve lanes pair");
    let super::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
        line_pcurve,
    )) = resolution
    else {
        panic!("extended isocurve was not certified");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!((origin.u - 0.5).abs() < EPS_EXPECTED_PARAMETER);
    assert!(origin.v.abs() < EPS_EXPECTED_PARAMETER);
    assert!(direction.u.abs() < EPS_EXPECTED_PARAMETER);
    assert!((direction.v - 1.0).abs() < EPS_EXPECTED_PARAMETER);
    let clamped = super::clamp_nurbs_curve_to_domain(&ctx, &curve, [0.0, 1.0])
        .expect("the clamped lanes are a curve")
        .expect("clamped segment");
    let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
        &ctx,
        &surface,
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        0.5,
    )
    .expect("resource allocation did not fail")
    .expect("surface isocurve");
    assert!(super::nurbs_representation_matches(&ctx, &expected, &clamped).unwrap());
}

#[test]
fn extended_quadratic_isocurve_preserves_the_inserted_homogeneous_segment() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        1,
        2,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        2,
        3,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let curve = test_nurbs_curve(
        2,
        vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.5, -1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 0.5, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 2.0, 0.0),
        ],
        None,
    );
    let resolution = super::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.1, 0.9])
        .expect("isocurve lanes pair");
    assert!(
        matches!(resolution, super::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        line_pcurve,
                    )) if {
                        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
                        (origin.u - 0.5).abs() <= f64::EPSILON * 64.0
                            && origin.v.abs() <= f64::EPSILON * 64.0
                            && direction.u.abs() <= f64::EPSILON * 64.0
                            && (direction.v - 1.0).abs() <= f64::EPSILON * 64.0
                    })
    );
    let clamped = super::clamp_nurbs_curve_to_domain(&ctx, &curve, [0.0, 1.0])
        .expect("the clamped lanes are a curve")
        .expect("clamped quadratic segment");
    let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
        &ctx,
        &surface,
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        0.5,
    )
    .expect("resource allocation did not fail")
    .expect("quadratic surface isocurve");
    assert!(super::nurbs_representation_matches(&ctx, &expected, &clamped).unwrap());
}

#[test]
fn extended_rational_isocurve_compares_weights_after_homogeneous_clamping() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        Some(vec![1.0, 1.2, 1.0, 1.2]),
    );
    let curve = test_nurbs_curve(
        1,
        vec![-1.0, -1.0, 2.0, 2.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.5, -1.5, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 2.4 / 1.4, 0.0),
        ],
        Some(vec![0.8, 1.4]),
    );
    let resolution = super::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.2, 0.8])
        .expect("isocurve lanes pair");
    assert!(
        matches!(resolution, super::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        line_pcurve,
                    )) if {
                        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
                        (origin.u - 0.5).abs() <= f64::EPSILON * 64.0
                            && origin.v.abs() <= f64::EPSILON * 64.0
                            && direction.u.abs() <= f64::EPSILON * 64.0
                            && (direction.v - 1.0).abs() <= f64::EPSILON * 64.0
                    })
    );
    let clamped = super::clamp_nurbs_curve_to_domain(&ctx, &curve, [0.0, 1.0])
        .expect("the clamped lanes are a curve")
        .expect("clamped rational segment");
    let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
        &ctx,
        &surface,
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        0.5,
    )
    .expect("resource allocation did not fail")
    .expect("rational surface isocurve");
    assert!(super::nurbs_representation_matches(&ctx, &expected, &clamped).unwrap());
}

#[test]
fn degree_one_nurbs_cache_pcurve_keeps_measured_chordal_error() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        3,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0),
            cadmpeg_ir::math::Point3::new(0.5, 1.0, 1.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let curve = test_nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
    );
    let resolution = super::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.0, 1.0])
        .expect("isocurve lanes pair");
    let super::NurbsPcurveResolution::Cache {
        geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs },
        fit_tolerance,
    } = resolution
    else {
        panic!("degree-one cache was not accepted");
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots(), curve.knots());
    assert_eq!(nurbs.control_points().len(), 2);
    assert!(nurbs.weights().is_none());
    assert!(!nurbs.periodic());
    assert!(fit_tolerance > 0.4);
    assert!(fit_tolerance < 0.6);
}

#[test]
fn off_surface_nurbs_edge_is_classified_before_cache_inversion() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let curve = test_nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 10.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 10.0),
        ],
        None,
    );
    assert!(matches!(
        super::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.0, 1.0])
            .expect("isocurve lanes pair"),
        super::NurbsPcurveResolution::OffSurface
    ));
}

#[test]
fn v_linear_surface_line_has_axis_symmetric_inverse() {
    let surface = test_nurbs_surface(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        3,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let geometry = match with_test_context(|ctx| {
        super::ruled_surface_line_pcurve(
            ctx,
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        )
    })
    .expect("ruling evaluation")
    {
        super::InverseResolution::Unique(geometry) => geometry,
        super::InverseResolution::NoMatch => panic!("transposed ruling did not match"),
        super::InverseResolution::Ambiguous => panic!("transposed ruling was ambiguous"),
    };
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected affine line pcurve");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!((origin.u - 0.5).abs() < EPS_EXPECTED_BILINEAR_PARAMETER);
    assert!(origin.v.abs() < EPS_EXPECTED_PARAMETER);
    assert!(direction.u.abs() < EPS_EXPECTED_PARAMETER);
    assert!((direction.v - 1.0).abs() < EPS_EXPECTED_PARAMETER);
}

#[test]
fn repeated_ruled_surface_line_candidates_are_ambiguous() {
    let surface = test_nurbs_surface(
        1,
        2,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        2,
        3,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
    );
    assert!(matches!(
        with_test_context(|ctx| super::ruled_surface_line_pcurve(
            ctx,
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ))
        .expect("ruling evaluation"),
        super::InverseResolution::Ambiguous
    ));
}

#[test]
fn repeated_nurbs_endpoint_candidates_are_ambiguous() {
    let curve = test_nurbs_curve(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        ],
        None,
    );
    assert!(matches!(
        with_test_context(|ctx| super::nurbs_parameter_at_point(
            ctx,
            &curve,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
        ))
        .expect("inverse evaluation"),
        super::InverseResolution::Ambiguous
    ));
}

#[test]
fn ambiguous_cylindrical_endpoint_withholds_the_derived_pcurve() {
    use cadmpeg_ir::annotations::AnnotationBuilder;
    use cadmpeg_ir::geometry::{Curve, Surface};
    use cadmpeg_ir::ids::{CurveId, EdgeId, FaceId, LoopId, PointId, SurfaceId, VertexId};
    use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();

    let surface_id = SurfaceId::mint("test:model:entity#surface").expect("identity grammar");
    let curve_id = CurveId::mint("test:model:entity#curve").expect("identity grammar");
    let loop_id = LoopId::mint("test:model:entity#loop").expect("identity grammar");
    let edge_id = EdgeId::mint("test:model:entity#edge").expect("identity grammar");
    let start_vertex = VertexId::mint("test:model:entity#start-vertex").expect("identity grammar");
    let end_vertex = VertexId::mint("test:model:entity#end-vertex").expect("identity grammar");
    let start_point = PointId::mint("test:model:entity#start-point").expect("identity grammar");
    let end_point = PointId::mint("test:model:entity#end-point").expect("identity grammar");
    let coedge_id =
        cadmpeg_ir::ids::CoedgeId::mint("test:model:entity#coedge").expect("identity grammar");
    let mut brep = super::Brep {
        surfaces: vec![Surface {
            id: surface_id.clone(),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                        1000.0,
                    )
                    .unwrap(),
                ),
            ),
            source_object: None,
        }],
        curves: vec![Curve {
            id: curve_id.clone(),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                test_nurbs_curve(
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 1000.0),
                        cadmpeg_ir::math::Point3::new(1000.0, 0.0, 0.0),
                    ],
                    None,
                ),
            )),
            source_object: None,
        }],
        faces: vec![Face {
            id: FaceId::mint("test:model:entity#face").expect("identity grammar"),
            shell: cadmpeg_ir::ids::ShellId::mint("test:model:entity#shell")
                .expect("identity grammar"),
            surface: surface_id,
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
            name: None,
            color: None,
            tolerance: None,
        }],
        loops: vec![Loop {
            id: loop_id.clone(),
            face: FaceId::mint("test:model:entity#face").expect("identity grammar"),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(
                    &cadmpeg_test_support::service_decode_context(),
                    vec![coedge_id.clone()],
                    Vec::new(),
                )
                .expect("fixture ring admission")
                .expect("valid loop ring"),
            ),
        }],
        coedges: vec![Coedge {
            id: coedge_id,
            owner_loop: loop_id,
            edge: edge_id.clone(),
            radial_next: cadmpeg_ir::ids::CoedgeId::mint("test:model:entity#coedge")
                .expect("identity grammar"),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        }],
        edges: vec![Edge {
            id: edge_id,
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
            start: start_vertex.clone(),
            end: end_vertex.clone(),
            tolerance: None,
        }],
        vertices: vec![
            Vertex {
                id: start_vertex,
                point: start_point.clone(),
                tolerance: None,
            },
            Vertex {
                id: end_vertex,
                point: end_point.clone(),
                tolerance: None,
            },
        ],
        points: vec![
            Point::new(
                start_point,
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    1000.0, 0.0, 0.0,
                ))
                .expect("a finite position is a point"),
                None,
            ),
            Point::new(
                end_point,
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    1000.0, 0.0, 0.0,
                ))
                .expect("a finite position is a point"),
                None,
            ),
        ],
        ..Default::default()
    };
    let mut annotations = AnnotationBuilder::new();
    let source_stream = cadmpeg_ir::annotations::StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::stream_name!("test"),
        "fixture stream handle",
    )
    .unwrap();
    super::derive_pcurves(&ctx, &mut brep, &mut annotations, &source_stream)
        .expect("cylindrical pcurve derivation");

    assert!(brep.pcurves.is_empty());
    assert_eq!(brep.stats.ambiguous_pcurve_parameters, 1);
}
