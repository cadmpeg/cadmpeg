// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, NurbsSurface,
    Pcurve, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, PcurveUse, Point, Sense, Vertex,
};
use cadmpeg_ir::AnnotationBuilder;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn active_body_selection_accepts_a_complete_singleton_membership() {
    let first = BodyId("nx:test:body#first".into());
    let second = BodyId("nx:test:body#second".into());
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.bodies.extend([
        Body {
            id: first.clone(),
            kind: BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
        Body {
            id: second.clone(),
            kind: BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    ]);
    ir.source = Some(cadmpeg_ir::document::SourceMeta::default());
    let body_node_ids = BTreeMap::from([
        (first.clone(), BTreeSet::from([7])),
        (second, BTreeSet::from([8])),
    ]);

    assert!(super::select_active_body(&mut ir, &body_node_ids, &[7]));
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.bodies[0].id, first);
    assert_eq!(
        ir.source
            .as_ref()
            .and_then(|source| source.attributes.get("rmfastload_hits"))
            .map(String::as_str),
        Some("1")
    );
}

#[test]
fn rmfastload_preselection_keeps_only_streams_with_selected_body_images() {
    let first = BodyId("nx:s3:body#first".into());
    let second = BodyId("nx:s8:body#second".into());
    let body_node_ids = BTreeMap::from([
        (first.clone(), BTreeSet::from([7, 8])),
        (second, BTreeSet::from([8, 9])),
    ]);

    let selected = super::rmfastload_selected_bodies(&body_node_ids, &[7, 8]);
    assert_eq!(selected, BTreeSet::from([first]));
    assert_eq!(
        super::rmfastload_stream_indices(&selected),
        Some(BTreeSet::from([3]))
    );
}

#[test]
fn analytic_closed_isocurves_retain_the_native_full_turn() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let cone = SurfaceId("nx:test:cone".into());
    let sphere = SurfaceId("nx:test:sphere".into());
    let torus = SurfaceId("nx:test:torus".into());
    ir.model.surfaces.extend([
        Surface {
            id: cone.clone(),
            geometry: SurfaceGeometry::Cone {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
                ratio: 0.5,
                half_angle: 0.25_f64.atan(),
            },
            source_object: None,
        },
        Surface {
            id: sphere.clone(),
            geometry: SurfaceGeometry::Sphere {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: torus.clone(),
            geometry: SurfaceGeometry::Torus {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 3.0,
                minor_radius: 1.0,
            },
            source_object: None,
        },
    ]);
    let plane = SurfaceId("nx:test:plane".into());
    ir.model.surfaces.push(Surface {
        id: plane.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 1.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let cone_ellipse = CurveId("nx:test:cone-ellipse".into());
    let sphere_circle = CurveId("nx:test:sphere-circle".into());
    let torus_circle = CurveId("nx:test:torus-circle".into());
    ir.model.curves.extend([
        Curve {
            id: cone_ellipse.clone(),
            geometry: CurveGeometry::Ellipse {
                center: Point3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                major_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 2.25,
                minor_radius: 1.125,
            },
            source_object: None,
        },
        Curve {
            id: sphere_circle.clone(),
            geometry: CurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 3.0_f64.sqrt(),
            },
            source_object: None,
        },
        Curve {
            id: torus_circle.clone(),
            geometry: CurveGeometry::Circle {
                center: Point3::new(3.0, 0.0, 0.0),
                axis: Vector3::new(0.0, -1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
            },
            source_object: None,
        },
    ]);

    let range = [0.0, std::f64::consts::TAU];
    let cone_pcurve = crate::decode::pcurves::exact_analytic_isocurve_pcurve(
        &ir,
        &cone_ellipse,
        &cone,
        range,
        1.0e-12,
    )
    .expect("cone ellipse");
    let sphere_pcurve = crate::decode::pcurves::exact_analytic_isocurve_pcurve(
        &ir,
        &sphere_circle,
        &sphere,
        range,
        1.0e-12,
    )
    .expect("sphere parallel");
    let torus_pcurve = crate::decode::pcurves::exact_analytic_isocurve_pcurve(
        &ir,
        &torus_circle,
        &torus,
        range,
        1.0e-12,
    )
    .expect("torus meridian");
    assert!(matches!(
        sphere_pcurve,
        PcurveGeometry::Line { origin, direction }
            if (origin.v - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12
                && direction.u == 1.0
                && direction.v == 0.0
    ));
    assert!(matches!(
        torus_pcurve,
        PcurveGeometry::Line { origin, direction }
            if origin.u.abs() < 1.0e-12
                && direction.u == 0.0
                && direction.v == 1.0
    ));
    assert!(matches!(
        cone_pcurve,
        PcurveGeometry::Line { origin, direction }
            if (origin.v - 1.0).abs() < 1.0e-12
                && direction.u == 1.0
                && direction.v == 0.0
    ));
    for parameter in [0.0, 1.0, 3.0, 5.0, std::f64::consts::TAU] {
        for (curve, surface, pcurve) in [
            (&cone_ellipse, &cone, &cone_pcurve),
            (&sphere_circle, &sphere, &sphere_pcurve),
            (&torus_circle, &torus, &torus_pcurve),
        ] {
            let curve = ir
                .model
                .curves
                .iter()
                .find(|candidate| &candidate.id == curve)
                .unwrap();
            let expected = cadmpeg_ir::eval::curve_point(&curve.geometry, parameter).unwrap();
            let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter).unwrap();
            let actual = cadmpeg_ir::eval::model_surface_point_by_id(
                &cadmpeg_ir::index::ModelIndex::new(&ir),
                surface,
                uv.u,
                uv.v,
            )
            .unwrap();
            assert!(super::point_distance(expected, actual) < 1.0e-12);
        }
    }

    let construction = ProceduralCurveId("nx:test:closed-intersection".into());
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: sphere_circle.clone(),
        definition: ProceduralCurveDefinition::TolerantIntersection {
            supports: [sphere, plane],
            endpoints: [
                Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
                Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
            ],
            tolerance: 1.0e-8,
            parameterization: None,
        },
        cache_fit_tolerance: None,
    });
    let point = PointId("nx:test:closed-point".into());
    let vertex = VertexId("nx:test:closed-vertex".into());
    ir.model.points.push(Point {
        id: point.clone(),
        position: Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
        source_object: None,
    });
    ir.model.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: Some(1.0e-8),
    });
    ir.model.edges.push(Edge {
        id: EdgeId("nx:test:closed-edge".into()),
        curve: Some(sphere_circle),
        start: vertex.clone(),
        end: vertex,
        param_range: None,
        tolerance: Some(1.0e-8),
    });

    let procedural_start = ir.model.procedural_curves.len();
    let mut annotations = AnnotationBuilder::new();
    let transfer_budget = WorkBudget::new(usize::MAX);
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(usize::MAX);
    crate::decode::pcurves::complete_exact_boundary_intersection_pcurves_with_budget(
        &mut ir,
        &mut annotations,
        procedural_start,
        &transfer_budget,
        &geometry_budget,
    );
    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization, ..
    } = &ir.model.procedural_curves[0].definition
    else {
        panic!("closed intersection construction");
    };
    assert!(parameterization.is_none());
    crate::decode::pcurves::complete_exact_boundary_intersection_pcurves_with_budget(
        &mut ir,
        &mut annotations,
        0,
        &transfer_budget,
        &geometry_budget,
    );
    let ProceduralCurveDefinition::TolerantIntersection {
        supports,
        parameterization: Some(parameterization),
        ..
    } = &ir.model.procedural_curves[0].definition
    else {
        panic!("closed intersection parameterization");
    };
    assert_eq!(parameterization.parameter_range, range);
    assert_eq!(ir.model.edges[0].param_range, Some(range));
    assert!(parameterization
        .pcurves
        .iter()
        .enumerate()
        .all(|(side, pcurve)| {
            for parameter in [0.0, 1.0, 3.0, 5.0, std::f64::consts::TAU] {
                let Some(uv) = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter) else {
                    return false;
                };
                let Some(point) = cadmpeg_ir::eval::model_surface_point_by_id(
                    &cadmpeg_ir::index::ModelIndex::new(&ir),
                    &supports[side],
                    uv.u,
                    uv.v,
                ) else {
                    return false;
                };
                if (point.z - 1.0).abs() > 1.0e-8 {
                    return false;
                }
            }
            true
        }));
    for parameter in [0.0, 1.0, 3.0, 5.0, std::f64::consts::TAU] {
        let curve = &ir.model.procedural_curves[0].curve;
        let point = cadmpeg_ir::eval::model_curve_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            curve,
            parameter,
        )
        .expect("closed intersection evaluates");
        let inverse =
            cadmpeg_ir::eval::model_curve_parameter_near_point(&ir, curve, point, parameter)
                .unwrap_or_else(|| panic!("closed intersection inverts at parameter {parameter}"));
        assert!((inverse - parameter).abs() < 1.0e-10);
    }
}

#[test]
fn boundary_pcurve_requires_an_affine_carrier_witness() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let curve = CurveId("nx:test:bowed-boundary-curve".into());
    let surface = SurfaceId("nx:test:boundary-plane".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(5.0, 5.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
            ],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });

    assert!(super::exact_boundary_pcurve(
        &ir,
        &curve,
        &surface,
        [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
        [0.0, 1.0],
        1.0e-8,
    )
    .is_none());

    ir.model.curves[0].geometry = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(10.0, 0.0, 0.0),
    };
    assert!(matches!(
        super::exact_boundary_pcurve(
            &ir,
            &curve,
            &surface,
            [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            [0.0, 1.0],
            1.0e-8,
        ),
        Some(PcurveGeometry::Line { .. })
    ));
}

#[test]
fn boundary_pcurve_accepts_a_certified_affine_nurbs_boundary() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let curve = CurveId("nx:test:affine-nurbs-boundary-curve".into());
    let surface = SurfaceId("nx:test:affine-nurbs-boundary-surface".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(3.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: affine_nurbs_surface(0.0),
        source_object: None,
    });

    assert!(matches!(
        super::exact_boundary_pcurve(
            &ir,
            &curve,
            &surface,
            [Point3::new(0.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)],
            [0.0, 1.0],
            1.0e-8,
        ),
        Some(PcurveGeometry::Line { origin, direction })
            if origin.v == 0.0 && direction.u == 1.0 && direction.v == 0.0
    ));
}

fn affine_nurbs_surface(z: f64) -> SurfaceGeometry {
    SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, z),
            Point3::new(0.0, 2.0, z),
            Point3::new(3.0, 0.0, z),
            Point3::new(3.0, 2.0, z),
        ],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    })
}

fn quadratic_translation_surface(z: f64) -> SurfaceGeometry {
    SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 2,
        v_degree: 2,
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        u_count: 3,
        v_count: 3,
        control_points: [0.0, 1.0, 3.0]
            .into_iter()
            .flat_map(|x| {
                [0.0, 2.0, 5.0]
                    .into_iter()
                    .map(move |y| Point3::new(x, y, z))
            })
            .collect(),
        weights: Some(vec![2.0; 9]),
        u_periodic: false,
        v_periodic: false,
    })
}

fn degree_elevated_affine_surface(z: f64) -> SurfaceGeometry {
    SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 2,
        v_degree: 2,
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        u_count: 3,
        v_count: 3,
        control_points: [0.0, 1.5, 3.0]
            .into_iter()
            .flat_map(|x| {
                [0.0, 1.0, 2.0]
                    .into_iter()
                    .map(move |y| Point3::new(x, y, z))
            })
            .collect(),
        weights: None,
        u_periodic: false,
        v_periodic: false,
    })
}

fn quadratic_paraboloid_surface() -> SurfaceGeometry {
    let coordinates = [0.0, 0.5, 1.0];
    let square_controls = [0.0, 0.0, 1.0];
    SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 2,
        v_degree: 2,
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        u_count: 3,
        v_count: 3,
        control_points: (0..3)
            .flat_map(|u| {
                (0..3).map(move |v| {
                    Point3::new(
                        coordinates[u],
                        coordinates[v],
                        square_controls[u] + square_controls[v],
                    )
                })
            })
            .collect(),
        weights: None,
        u_periodic: false,
        v_periodic: false,
    })
}

#[test]
fn planar_offset_cache_fit_is_certified_over_the_control_net() {
    let support = affine_nurbs_surface(0.0);
    let mut candidate = affine_nurbs_surface(4.0);
    let SurfaceGeometry::Nurbs(candidate) = &mut candidate else {
        unreachable!();
    };
    candidate.control_points[3].z += 0.000_5;

    let fit = super::certified_offset_cache_fit(
        &support,
        &SurfaceGeometry::Nurbs(candidate.clone()),
        4.0,
        0.001,
    )
    .expect("whole-patch fit");
    assert!((fit - 0.000_5).abs() < 1.0e-12);
    assert!(super::certified_offset_cache_fit(
        &support,
        &SurfaceGeometry::Nurbs(candidate.clone()),
        4.0,
        0.000_4
    )
    .is_none());
}

#[test]
fn adaptive_offset_certification_fails_closed_when_the_work_slice_is_empty() {
    let support = quadratic_paraboloid_surface();
    let SurfaceGeometry::Nurbs(support) = &support else {
        unreachable!();
    };
    let budget = crate::decode::geometry_work::GeometryWorkBudget::new(0);

    assert!(
        crate::decode::offset::certified_curved_offset_cache_fit_with_budget(
            support, support, 0.01, 0.02, true, &budget,
        )
        .is_none()
    );
    assert!(budget.exhausted());
}

#[test]
fn adaptive_bezier_root_isolation_fails_closed_when_the_work_slice_is_empty() {
    let budget = crate::decode::geometry_work::GeometryWorkBudget::new(0);
    let span = crate::decode::blend::ScalarBezierSpan {
        domain: [0.0, 1.0],
        controls: vec![-1.0, 1.0],
    };

    assert!(crate::decode::blend::scalar_bezier_roots_with_budget(span, &budget).is_none());
    assert!(budget.exhausted());
}

#[test]
fn pcurve_edge_admission_fails_closed_when_the_geometry_slice_is_empty() {
    let surface = SurfaceId("nx:test:budget-plane".into());
    let start_point = PointId("nx:test:budget-start-point".into());
    let end_point = PointId("nx:test:budget-end-point".into());
    let start_vertex = VertexId("nx:test:budget-start-vertex".into());
    let end_vertex = VertexId("nx:test:budget-end-vertex".into());
    let edge = EdgeId("nx:test:budget-edge".into());
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.points.extend([
        Point {
            id: start_point.clone(),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: end_point.clone(),
            position: Point3::new(1.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: start_vertex.clone(),
            point: start_point,
            tolerance: Some(0.0),
        },
        Vertex {
            id: end_vertex.clone(),
            point: end_point,
            tolerance: Some(0.0),
        },
    ]);
    ir.model.edges.push(Edge {
        id: edge.clone(),
        curve: None,
        start: start_vertex,
        end: end_vertex,
        param_range: None,
        tolerance: Some(0.0),
    });
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let budget = crate::decode::geometry_work::GeometryWorkBudget::new(0);
    let pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };

    assert!(
        !crate::decode::pcurves::pcurve_matches_edge_range_with_index_and_budget(
            &index,
            &edge,
            &surface,
            &pcurve,
            Some([0.0, 1.0]),
            None,
            &budget,
        )
    );
    assert!(budget.exhausted());
}

#[test]
fn offset_cache_fit_accepts_higher_degree_translation_nets() {
    assert_eq!(
        super::certified_offset_cache_fit(
            &quadratic_translation_surface(0.0),
            &quadratic_translation_surface(4.0),
            4.0,
            0.0
        ),
        Some(0.0)
    );
}

#[test]
fn periodic_offset_cache_fit_covers_the_complete_active_domain() {
    let mut support = quadratic_paraboloid_surface();
    let mut candidate = support.clone();
    let SurfaceGeometry::Nurbs(support_surface) = &mut support else {
        unreachable!();
    };
    let SurfaceGeometry::Nurbs(candidate_surface) = &mut candidate else {
        unreachable!();
    };
    support_surface.u_periodic = true;
    candidate_surface.u_periodic = true;

    assert_eq!(
        super::certified_offset_cache_fit(&support, &candidate, 0.0, 0.0),
        Some(0.0)
    );
}

#[test]
fn offset_cache_fit_certifies_differing_bases_on_one_parameter_domain() {
    let bound = super::certified_offset_cache_fit(
        &affine_nurbs_surface(0.0),
        &degree_elevated_affine_surface(4.0),
        4.0,
        0.1,
    )
    .expect("degree-elevated cache fit");
    assert!(bound <= 0.1);
}

#[test]
fn curved_offset_cache_fit_uses_span_local_derivative_bounds() {
    let support = quadratic_paraboloid_surface();
    assert_eq!(
        super::certified_offset_cache_fit(&support, &support, 0.0, 0.0),
        Some(0.0)
    );
    let bound = super::certified_offset_cache_fit(&support, &support, 0.01, 0.02)
        .expect("nonzero curved offset certified");
    assert!((0.01..=0.02).contains(&bound));
}

#[test]
fn offset_cache_fit_decouples_distant_knot_span_scale() {
    let x = [0.0, 0.25, 0.5, 1.0e9 + 0.5];
    let z = [0.0, 0.0, 0.1, 0.2];
    let support = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 2,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 4,
        v_count: 2,
        control_points: (0..4)
            .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
            .collect(),
        weights: None,
        u_periodic: false,
        v_periodic: false,
    });

    let bound = super::certified_offset_cache_fit(&support, &support, 0.01, 0.02)
        .expect("each regular knot span certifies independently");
    assert!((0.01..=0.02).contains(&bound));
}

#[test]
fn offset_cache_fit_certifies_regular_c0_knot_spans() {
    let x = [0.0, 0.25, 0.5, 1.0, 1.5];
    let z = [0.0, 0.0, 0.1, 0.1, 0.2];
    let support = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 2,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points: (0..5)
            .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
            .collect(),
        weights: None,
        u_periodic: false,
        v_periodic: false,
    });

    let bound = super::certified_offset_cache_fit(&support, &support, 0.01, 0.02)
        .expect("regular spans certify across the C0 knot break");
    assert!((0.01..=0.02).contains(&bound));
}

#[test]
fn curved_offset_cache_fit_rejects_an_uncertified_fold() {
    let mut support = quadratic_paraboloid_surface();
    let SurfaceGeometry::Nurbs(surface) = &mut support else {
        unreachable!();
    };
    for v in 0..3 {
        surface.control_points[2 * 3 + v] = surface.control_points[3 + v];
    }
    assert!(super::certified_offset_cache_fit(&support, &support, 0.0, 1.0).is_none());
}

#[test]
fn curved_offset_cache_fit_accepts_a_regular_turning_control_net() {
    let mut support = quadratic_paraboloid_surface();
    let SurfaceGeometry::Nurbs(surface) = &mut support else {
        unreachable!();
    };
    for v in 0..3 {
        surface.control_points[2 * 3 + v].x = 0.0;
    }
    assert_eq!(
        super::certified_offset_cache_fit(&support, &support, 0.0, 0.0),
        Some(0.0)
    );
}

#[test]
fn curved_offset_cache_fit_certifies_deeply_localized_regularity() {
    let epsilon = 2.0_f64.powi(-100);
    let x = [0.0, epsilon / 3.0, 2.0 * epsilon / 3.0, 1.0 + epsilon];
    let z = [0.0, 0.0, 1.0 / 3.0, 1.0];
    let support = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 3,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 4,
        v_count: 2,
        control_points: (0..4)
            .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
            .collect(),
        weights: None,
        u_periodic: false,
        v_periodic: false,
    });
    let SurfaceGeometry::Nurbs(surface) = &support else {
        unreachable!();
    };

    assert!(super::translation_net_normal(surface).is_none());
    assert_eq!(
        super::certified_offset_cache_fit(&support, &support, 0.0, 0.0),
        Some(0.0)
    );
}

#[test]
fn offset_cache_subdivision_uses_the_remaining_divisible_axis() {
    let u0 = 1.0_f64;
    let u1 = f64::from_bits(u0.to_bits() + 1);
    let u = u0 + (u1 - u0) * 0.5;
    let mut rectangles = Vec::new();

    assert!(super::subdivide_offset_rectangle(
        &mut rectangles,
        [u0, u1, 0.0, 1.0],
        [u, 0.5],
        true,
    ));
    assert_eq!(rectangles, vec![[u0, u1, 0.0, 0.5], [u0, u1, 0.5, 1.0]]);
}

#[test]
fn curved_offset_cache_fit_certifies_varying_positive_weights() {
    let mut support = quadratic_paraboloid_surface();
    let SurfaceGeometry::Nurbs(surface) = &mut support else {
        unreachable!();
    };
    let axis_weights = [1.0, 1.01, 1.02];
    surface.weights = Some(
        (0..3)
            .flat_map(|u| (0..3).map(move |v| axis_weights[u] * axis_weights[v]))
            .collect(),
    );

    assert_eq!(
        super::certified_offset_cache_fit(&support, &support, 0.0, 0.0),
        Some(0.0)
    );
    assert!(super::certified_offset_cache_fit(&support, &support, 0.01, 0.02).is_some());
}

#[test]
fn rational_offset_cache_bounds_are_translation_invariant() {
    let mut support = quadratic_paraboloid_surface();
    let SurfaceGeometry::Nurbs(surface) = &mut support else {
        unreachable!();
    };
    for point in &mut surface.control_points {
        point.x += 1.0e12;
        point.y -= 2.0e12;
        point.z += 3.0e12;
    }
    let axis_weights = [1.0, 1.01, 1.02];
    surface.weights = Some(
        (0..3)
            .flat_map(|u| (0..3).map(move |v| axis_weights[u] * axis_weights[v]))
            .collect(),
    );

    let bound = super::certified_offset_cache_fit(&support, &support, 0.01, 0.02)
        .expect("absolute placement does not widen rational derivative bounds");
    assert!(bound <= 0.02);
}

#[test]
fn nurbs_surface_fit_uses_the_declared_geometric_tolerance() {
    let SurfaceGeometry::Nurbs(surface) = quadratic_paraboloid_surface() else {
        unreachable!();
    };
    let mut point = cadmpeg_ir::eval::nurbs_surface_point(&surface, 0.4, 0.6).unwrap();
    point.z += 0.001;

    let parameters =
        cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance(&surface, point, None, 0.01)
            .unwrap();
    let mapped =
        cadmpeg_ir::eval::nurbs_surface_point(&surface, parameters.u, parameters.v).unwrap();

    assert!(super::point_distance(mapped, point) <= 0.01);
}

#[test]
fn nurbs_blend_contact_requires_the_declared_radius_shell() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let surface = SurfaceId("nx:test:contact-support".into());
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: affine_nurbs_surface(0.0),
        source_object: None,
    });
    let center = Point3::new(1.2, 0.7, 2.0);

    let direction = super::surface_contact_direction(&ir, &surface, center, 2.0, 0)
        .expect("the support contains one contact at the blend radius");
    assert!((direction - Vector3::new(0.0, 0.0, -1.0)).norm() < 1.0e-10);
    assert!(super::surface_contact_direction(&ir, &surface, center, 1.0, 0).is_none());
}

#[test]
fn saved_offset_cache_retains_its_procedural_lineage() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let support = SurfaceId("nx:test:support".into());
    let cache = SurfaceId("nx:test:cache".into());
    ir.model.surfaces.extend([
        Surface {
            id: support.clone(),
            geometry: affine_nurbs_surface(0.0),
            source_object: None,
        },
        Surface {
            id: cache.clone(),
            geometry: affine_nurbs_surface(4.0),
            source_object: None,
        },
    ]);
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("nx:test:offset".into()),
        surface: cache.clone(),
        definition: ProceduralSurfaceDefinition::Offset {
            support: support.clone(),
            distance: 4.0,
            u_sense: Some(0),
            v_sense: Some(0),
            support_extension: None,
            extension_flags: Vec::new(),
            revision_form: None,
        },
        cache_fit_tolerance: Some(0.0),
        record_bounds: None,
    });

    assert_eq!(
        super::surface_offset_lineage(&ir, &cache, 0),
        Some((support, 4.0))
    );
}

#[test]
fn serialized_surface_curves_select_a_terminal_intersection_branch() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let surfaces = [
        SurfaceId("nx:test:surface#0".into()),
        SurfaceId("nx:test:surface#1".into()),
    ];
    for surface in &surfaces {
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let curve = CurveId("nx:test:curve".into());
    let procedural = ProceduralCurveId("nx:test:intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: procedural.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: procedural,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::TolerantIntersection {
            supports: surfaces.clone(),
            endpoints: [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            tolerance: 0.01,
            parameterization: None,
        },
        cache_fit_tolerance: None,
    });
    let points = [
        PointId("nx:test:point#0".into()),
        PointId("nx:test:point#1".into()),
    ];
    let vertices = [
        VertexId("nx:test:vertex#0".into()),
        VertexId("nx:test:vertex#1".into()),
    ];
    for index in 0..2 {
        ir.model.points.push(Point {
            id: points[index].clone(),
            position: Point3::new(0.005 + 9.99 * index as f64, 0.0, 0.0),
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertices[index].clone(),
            point: points[index].clone(),
            tolerance: None,
        });
    }
    let edge = EdgeId("nx:test:edge".into());
    ir.model.edges.push(Edge {
        id: edge.clone(),
        curve: Some(curve),
        start: vertices[0].clone(),
        end: vertices[1].clone(),
        param_range: None,
        tolerance: Some(0.03),
    });
    let pcurves = [
        PcurveId("nx:test:pcurve#0".into()),
        PcurveId("nx:test:pcurve#1".into()),
    ];
    let faces = [
        FaceId("nx:test:face#0".into()),
        FaceId("nx:test:face#1".into()),
    ];
    let loops = [
        LoopId("nx:test:loop#0".into()),
        LoopId("nx:test:loop#1".into()),
    ];
    let coedges = [
        CoedgeId("nx:test:coedge#0".into()),
        CoedgeId("nx:test:coedge#1".into()),
    ];
    for index in 0..2 {
        ir.model.pcurves.push(Pcurve {
            id: pcurves[index].clone(),
            geometry: PcurveGeometry::Line {
                origin: Point2::new(0.0, 0.0),
                direction: Point2::new(1.0, 0.0),
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some([0.0, 10.0]),
            fit_tolerance: Some(0.02),
        });
        ir.model.faces.push(Face {
            id: faces[index].clone(),
            shell: ShellId("nx:test:shell".into()),
            surface: surfaces[index].clone(),
            sense: Sense::Forward,
            loops: vec![loops[index].clone()],
            name: None,
            color: None,
            tolerance: Some(0.03),
        });
        ir.model.loops.push(Loop {
            id: loops[index].clone(),
            face: faces[index].clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: vec![coedges[index].clone()],
            vertex_uses: Vec::new(),
        });
        ir.model.coedges.push(Coedge {
            id: coedges[index].clone(),
            owner_loop: loops[index].clone(),
            edge: edge.clone(),
            next: coedges[index].clone(),
            previous: coedges[index].clone(),
            radial_next: coedges[1 - index].clone(),
            sense: Sense::Forward,
            pcurves: vec![PcurveUse {
                pcurve: pcurves[index].clone(),
                isoparametric: None,
                parameter_range: Some([0.0, 10.0]),
            }],
            use_curve: None,
            use_curve_parameter_range: None,
        });
    }
    let serialized = [0, 1]
        .map(|index| {
            (
                ir.model.procedural_curves[0].curve.clone(),
                surfaces[index].clone(),
                pcurves[index].clone(),
            )
        })
        .into_iter()
        .collect();
    super::complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    assert!(matches!(
        ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::TolerantIntersection {
            parameterization: None,
            ..
        }
    ));
    for pcurve in &mut ir.model.pcurves {
        pcurve.fit_tolerance = Some(0.01);
    }
    super::complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );

    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = &ir.model.procedural_curves[0].definition
    else {
        panic!("serialized branch transferred");
    };
    assert_eq!(parameterization.parameter_range, [0.0, 10.0]);
    assert_eq!(ir.model.edges[0].param_range, Some([0.0, 10.0]));
    assert_eq!(
        cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &surfaces[0],
            5.0,
            0.0
        ),
        cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &surfaces[1],
            5.0,
            0.0
        )
    );

    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization, ..
    } = &mut ir.model.procedural_curves[0].definition
    else {
        unreachable!();
    };
    *parameterization = None;
    let edge = &mut ir.model.edges[0];
    edge.param_range = None;
    std::mem::swap(&mut edge.start, &mut edge.end);
    for pcurve in &mut ir.model.pcurves {
        pcurve.geometry = PcurveGeometry::Line {
            origin: Point2::new(10.0, 0.0),
            direction: Point2::new(-1.0, 0.0),
        };
    }
    super::complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = &ir.model.procedural_curves[0].definition
    else {
        panic!("reversed serialized branch transferred");
    };
    assert!(parameterization.pcurves.iter().all(|pcurve| matches!(
        pcurve,
        PcurveGeometry::Line { origin, direction }
            if origin.u == 0.0 && direction.u == 1.0
    )));
    assert_eq!(ir.model.edges[0].start, vertices[0]);
    assert_eq!(ir.model.edges[0].end, vertices[1]);

    let range = [-1.5, 1.5];
    let canonical = PcurveGeometry::Ellipse {
        center: Point2::new(5.0, 0.0),
        x_axis: Point2::new(1.0, 0.0),
        y_axis: Point2::new(0.0, 1.0),
        major_radius: 4.0,
        minor_radius: 2.0,
    };
    let endpoints = range.map(|parameter| {
        let uv = cadmpeg_ir::eval::pcurve_uv(&canonical, parameter).unwrap();
        Point3::new(uv.u, uv.v, 0.0)
    });
    for (point, position) in ir.model.points.iter_mut().zip(endpoints) {
        point.position = position;
    }
    let ProceduralCurveDefinition::TolerantIntersection {
        endpoints: stored_endpoints,
        parameterization,
        ..
    } = &mut ir.model.procedural_curves[0].definition
    else {
        unreachable!();
    };
    *stored_endpoints = endpoints;
    *parameterization = None;
    ir.model.edges[0].param_range = None;
    for coedge in &mut ir.model.coedges {
        coedge.pcurves[0].parameter_range = Some(range);
    }
    for pcurve in &mut ir.model.pcurves {
        pcurve.parameter_range = Some(range);
        pcurve.geometry = PcurveGeometry::Ellipse {
            center: Point2::new(5.0, 0.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, -1.0),
            major_radius: 4.0,
            minor_radius: 2.0,
        };
    }
    super::complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = &ir.model.procedural_curves[0].definition
    else {
        panic!("reversed symmetric conic branches transferred");
    };
    assert_eq!(parameterization.parameter_range, range);
    assert!(parameterization.pcurves.iter().all(|pcurve| matches!(
        pcurve,
        PcurveGeometry::Ellipse { y_axis, .. } if y_axis.v == 1.0
    )));

    let ProceduralCurveDefinition::TolerantIntersection {
        tolerance,
        parameterization,
        ..
    } = &mut ir.model.procedural_curves[0].definition
    else {
        unreachable!();
    };
    *tolerance = 10.0;
    *parameterization = None;
    ir.model.edges[0].param_range = None;
    super::complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    assert!(matches!(
        ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::TolerantIntersection {
            parameterization: Some(_),
            ..
        }
    ));
}

#[test]
fn reversed_nurbs_pcurve_preserves_the_selected_interval() {
    let pcurve = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 2.0),
            Point2::new(3.0, 1.0),
        ],
        weights: Some(vec![1.0, 2.0, 1.5]),
        periodic: false,
    };
    let range = [0.25, 1.75];
    let reversed =
        super::reverse_pcurve_over_range(&pcurve, range).expect("reversible NURBS pcurve");
    for parameter in [range[0], 0.5, 1.0, 1.5, range[1]] {
        let expected =
            cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1.0e-12);
        assert!((actual.v - expected.v).abs() < 1.0e-12);
    }
}

#[test]
fn reversed_symmetric_analytic_pcurves_preserve_the_selected_interval() {
    let carriers = [
        PcurveGeometry::Ellipse {
            center: Point2::new(2.0, 3.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            major_radius: 4.0,
            minor_radius: 2.0,
        },
        PcurveGeometry::Parabola {
            vertex: Point2::new(2.0, 3.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            focal_distance: 0.75,
        },
        PcurveGeometry::Hyperbola {
            center: Point2::new(2.0, 3.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            major_radius: 4.0,
            minor_radius: 2.0,
        },
    ];
    let range = [-1.5, 1.5];
    for carrier in carriers {
        let reversed = super::reverse_pcurve_over_range(&carrier, range)
            .expect("symmetric analytic pcurve is exactly reversible");
        for parameter in [-1.5, -0.75, 0.0, 0.75, 1.5] {
            let expected = cadmpeg_ir::eval::pcurve_uv(&carrier, -parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }
    }
}

#[test]
fn reversed_analytic_conics_preserve_arbitrary_selected_intervals() {
    let carriers = [
        PcurveGeometry::Ellipse {
            center: Point2::new(2.0, 3.0),
            x_axis: Point2::new(0.6, 0.8),
            y_axis: Point2::new(-0.8, 0.6),
            major_radius: 4.0,
            minor_radius: 2.0,
        },
        PcurveGeometry::Hyperbola {
            center: Point2::new(-3.0, 5.0),
            x_axis: Point2::new(0.8, -0.6),
            y_axis: Point2::new(0.6, 0.8),
            major_radius: 2.5,
            minor_radius: 1.25,
        },
    ];
    let range = [0.25, 1.75];
    for carrier in carriers {
        let reversed = super::reverse_pcurve_over_range(&carrier, range)
            .expect("a finite conic interval has an exact coefficient reflection");
        assert!(matches!(
            (&carrier, &reversed),
            (
                PcurveGeometry::Ellipse { .. },
                PcurveGeometry::Harmonic { .. }
            ) | (
                PcurveGeometry::Hyperbola { .. },
                PcurveGeometry::Hyperbolic { .. }
            )
        ));
        for parameter in [0.25, 0.5, 1.0, 1.5, 1.75] {
            let expected =
                cadmpeg_ir::eval::pcurve_uv(&carrier, range[0] + range[1] - parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }

        let reflected_twice = super::reverse_pcurve_over_range(&reversed, range)
            .expect("general conic coefficients remain exactly reversible");
        for parameter in [0.25, 0.75, 1.25, 1.75] {
            let expected = cadmpeg_ir::eval::pcurve_uv(&carrier, parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reflected_twice, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }
    }
}

#[test]
fn reversed_parabola_preserves_an_arbitrary_selected_interval() {
    let pcurve = PcurveGeometry::Parabola {
        vertex: Point2::new(2.0, 3.0),
        x_axis: Point2::new(0.6, 0.8),
        y_axis: Point2::new(-0.8, 0.6),
        focal_distance: 0.75,
    };
    let range = [0.25, 2.75];
    let reversed = super::reverse_pcurve_over_range(&pcurve, range)
        .expect("a finite parabola interval has an exact quadratic reflection");
    assert!(matches!(
        &reversed,
        PcurveGeometry::Nurbs {
            degree: 2,
            weights: None,
            periodic: false,
            ..
        }
    ));
    for parameter in [0.25, 0.5, 1.0, 1.75, 2.5, 2.75] {
        let expected =
            cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1e-12);
        assert!((actual.v - expected.v).abs() < 1e-12);
    }

    let offset = PcurveGeometry::Offset {
        distance: 1.25,
        basis: Box::new(pcurve.clone()),
    };
    let PcurveGeometry::Offset { distance, basis } =
        super::reverse_pcurve_over_range(&offset, range)
            .expect("offset parabola reflection closes recursively")
    else {
        panic!("reversed offset parabola");
    };
    assert_eq!(distance, -1.25);
    for parameter in [0.25, 1.0, 2.0, 2.75] {
        let expected =
            cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&basis, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1e-12);
        assert!((actual.v - expected.v).abs() < 1e-12);
    }
}

#[test]
fn reversed_offset_pcurve_reverses_its_basis_and_signed_side() {
    let pcurve = PcurveGeometry::Offset {
        distance: 2.5,
        basis: Box::new(PcurveGeometry::Line {
            origin: Point2::new(1.0, 3.0),
            direction: Point2::new(2.0, -1.0),
        }),
    };
    let reversed = super::reverse_pcurve_over_range(&pcurve, [2.0, 6.0])
        .expect("offset construction is exactly reversible");
    let PcurveGeometry::Offset { distance, basis } = &reversed else {
        panic!("reversed offset");
    };
    assert_eq!(*distance, -2.5);
    for parameter in [2.0, 3.0, 5.0, 6.0] {
        let expected_basis = cadmpeg_ir::eval::pcurve_uv(
            match &pcurve {
                PcurveGeometry::Offset { basis, .. } => basis,
                _ => unreachable!(),
            },
            8.0 - parameter,
        )
        .unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(basis, parameter).unwrap();
        assert_eq!(actual, expected_basis);
        let expected = cadmpeg_ir::eval::pcurve_uv(&pcurve, 8.0 - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1e-12);
        assert!((actual.v - expected.v).abs() < 1e-12);
    }

    let support = SurfaceId("nx:test:offset-orientation-support".into());
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.surfaces.push(Surface {
        id: support.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 2.0).unwrap();
    let second = cadmpeg_ir::eval::pcurve_uv(&pcurve, 6.0).unwrap();
    let oriented = super::orient_tolerant_intersection_pcurve(
        &ir,
        &CurveId("nx:test:unused-orientation-curve".into()),
        &support,
        &pcurve,
        [2.0, 6.0],
        [
            Point3::new(second.u, second.v, 0.0),
            Point3::new(first.u, first.v, 0.0),
        ],
        1e-12,
    )
    .expect("offset endpoints select the reversed terminal branch");
    for parameter in [2.0, 3.0, 5.0, 6.0] {
        let expected = cadmpeg_ir::eval::pcurve_uv(&pcurve, 8.0 - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&oriented, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1e-12);
        assert!((actual.v - expected.v).abs() < 1e-12);
    }
}

#[test]
fn closed_serialized_pcurve_uses_carrier_tangent_for_orientation() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let curve = CurveId("nx:test:closed-orientation-curve".into());
    let support = SurfaceId("nx:test:closed-orientation-support".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: support.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let pcurve = PcurveGeometry::Circle {
        center: Point2::new(0.0, 0.0),
        x_axis: Point2::new(1.0, 0.0),
        y_axis: Point2::new(0.0, 1.0),
        radius: 2.0,
    };
    let endpoint = Point3::new(2.0, 0.0, 0.0);

    let oriented = super::orient_tolerant_intersection_pcurve(
        &ir,
        &curve,
        &support,
        &pcurve,
        [0.0, std::f64::consts::TAU],
        [endpoint, endpoint],
        1.0e-12,
    )
    .expect("carrier tangent selects one closed-branch orientation");
    let uv = cadmpeg_ir::eval::pcurve_uv(&oriented, std::f64::consts::FRAC_PI_2).unwrap();
    assert!((uv.u - 0.0).abs() < 1.0e-12);
    assert!((uv.v - 2.0).abs() < 1.0e-12);
}

#[test]
fn edge_incidence_uses_only_declared_tolerances_at_large_scale() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let curve_id = CurveId("nx:test:curve#0".into());
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("nx:test:intersection#0".into()),
        curve: curve_id.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                        pcurve_parameter_range: None,
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                        pcurve_parameter_range: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: Some(2.0),
    });

    let start_point = PointId("nx:test:point#0".into());
    let end_point = PointId("nx:test:point#1".into());
    ir.model.points.extend([
        Point {
            id: start_point.clone(),
            position: Point3::new(0.0, 0.0, 1.0),
            source_object: None,
        },
        Point {
            id: end_point.clone(),
            position: Point3::new(1.0, 0.005, 1.0),
            source_object: None,
        },
    ]);
    let start = VertexId("nx:test:vertex#0".into());
    let end = VertexId("nx:test:vertex#1".into());
    ir.model.vertices.extend([
        Vertex {
            id: start.clone(),
            point: start_point,
            tolerance: None,
        },
        Vertex {
            id: end.clone(),
            point: end_point,
            tolerance: None,
        },
    ]);
    let edge = EdgeId("nx:test:edge#0".into());
    ir.model.edges.push(Edge {
        id: edge.clone(),
        curve: Some(curve_id.clone()),
        start: start.clone(),
        end: end.clone(),
        param_range: None,
        tolerance: None,
    });
    let support = SurfaceId("nx:test:surface-support#0".into());
    let surface = SurfaceId("nx:test:surface#0".into());
    let construction = ProceduralSurfaceId("nx:test:surface-offset#0".into());
    ir.model.surfaces.extend([
        Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        },
    ]);
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: construction,
        surface: surface.clone(),
        definition: ProceduralSurfaceDefinition::Offset {
            support,
            distance: 1.0,
            u_sense: Some(0),
            v_sense: Some(0),
            support_extension: None,
            extension_flags: Vec::new(),
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: None,
        periodic: false,
    };

    assert!(super::orient_edge_range(&ir, &curve_id, [0.0, 1.0], &start, &end, None).is_none());
    assert!(!super::pcurve_matches_edge(
        &ir, &edge, &surface, &pcurve, None,
    ));
    assert!(super::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve,
        Some(0.01),
    ));
    let large_distance = super::point_distance(
        Point3::new(1.0e200, 1.0e200, 1.0e200),
        Point3::new(0.0, 0.0, 0.0),
    );
    assert!(large_distance.is_finite());
    assert!((large_distance / 1.0e200 - 3.0_f64.sqrt()).abs() < 1.0e-15);
}

#[test]
fn boundary_coincidence_is_certified_between_uniform_samples() {
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let surfaces = [
        SurfaceId("nx:test:surface#0".into()),
        SurfaceId("nx:test:surface#1".into()),
    ];
    let surface = || NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.01, 0.02, 1.0, 1.0],
        u_count: 2,
        v_count: 4,
        control_points: [0.0, 1.0]
            .into_iter()
            .flat_map(|y| {
                [0.0, 0.1, 0.2, 10.0]
                    .into_iter()
                    .map(move |x| Point3::new(x, y, 0.0))
            })
            .collect(),
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    ir.model.surfaces.extend([
        Surface {
            id: surfaces[0].clone(),
            geometry: SurfaceGeometry::Nurbs(surface()),
            source_object: None,
        },
        Surface {
            id: surfaces[1].clone(),
            geometry: SurfaceGeometry::Nurbs(surface()),
            source_object: None,
        },
    ]);
    let pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(0.0, 1.0),
    };
    assert!(super::coincident_pcurve_pair(
        &ir,
        [&surfaces[0], &surfaces[1]],
        [&pcurve, &pcurve],
        [0.0, 1.0],
        0.1,
    ));

    let SurfaceGeometry::Nurbs(second) = &mut ir.model.surfaces[1].geometry else {
        unreachable!()
    };
    second.control_points[1].z = 1.0;
    assert!(!super::coincident_pcurve_pair(
        &ir,
        [&surfaces[0], &surfaces[1]],
        [&pcurve, &pcurve],
        [0.0, 1.0],
        0.1,
    ));
}

#[test]
fn rational_pcurve_incidence_isolates_close_branches() {
    let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
    let controls = [
        0.006_306_3,
        -0.029_213_45,
        0.095_295_133_333_333_34,
        -0.070_192_95,
        0.024_297_3,
    ]
    .into_iter()
    .zip(weights)
    .map(|(numerator, weight)| Point2::new(numerator / weight, 0.0))
    .collect::<Vec<_>>();
    let pcurve = PcurveGeometry::Nurbs {
        degree: 4,
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        control_points: controls,
        weights: Some(weights.to_vec()),
        periodic: false,
    };
    let roots = super::closest_pcurve_parameters(&pcurve, Point2::new(0.0, 0.0), Some(0.11))
        .expect("complete homogeneous root isolation");

    assert_eq!(roots.len(), 4);
    for (actual, expected) in roots.iter().zip([0.1001, 0.1, 0.7, 0.9]) {
        assert!((actual - expected).abs() < 1.0e-8);
    }
}

#[test]
fn rational_pcurve_closest_search_retains_close_global_branches() {
    let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
    let control_points = [
        0.006_306_3,
        -0.029_213_45,
        0.095_295_133_333_333_34,
        -0.070_192_95,
        0.024_297_3,
    ]
    .into_iter()
    .zip(weights)
    .map(|(numerator, weight)| Point2::new(numerator / weight, 0.0))
    .collect();
    let pcurve = PcurveGeometry::Nurbs {
        degree: 4,
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        control_points,
        weights: Some(weights.to_vec()),
        periodic: false,
    };
    let parameters =
        super::closest_pcurve_parameters(&pcurve, Point2::new(0.0, 1.0e-4), Some(0.11))
            .expect("complete global closest-point search");

    assert_eq!(parameters.len(), 4, "{parameters:?}");
    for (actual, expected) in parameters.iter().zip([0.1001, 0.1, 0.7, 0.9]) {
        assert!((actual - expected).abs() < 1.0e-8);
    }
}

#[test]
fn rational_spine_closest_search_resolves_close_global_branches() {
    let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
    let control_points = [
        0.006_306_3,
        -0.029_213_45,
        0.095_295_133_333_333_34,
        -0.070_192_95,
        0.024_297_3,
    ]
    .into_iter()
    .zip(weights)
    .map(|(numerator, weight)| Point3::new(numerator / weight, 0.0, 0.0))
    .collect();
    let curve = NurbsCurve {
        degree: 4,
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        control_points,
        weights: Some(weights.to_vec()),
        periodic: false,
    };
    let point = Point3::new(0.0, 1.0e-4, 0.0);

    let first = super::closest_nurbs_curve_parameter(&curve, point, Some(0.099))
        .expect("first close branch");
    let second = super::closest_nurbs_curve_parameter(&curve, point, Some(0.101))
        .expect("second close branch");
    let remote = super::closest_nurbs_curve_parameter(&curve, point, Some(0.69))
        .expect("remote global branch");

    assert!((first - 0.1).abs() < 1.0e-8);
    assert!((second - 0.1001).abs() < 1.0e-8);
    assert!((remote - 0.7).abs() < 1.0e-8);
}

#[test]
fn periodic_nurbs_inversion_lifts_the_continuation_phase() {
    let knots = vec![0.0, 0.0, 1.0, 2.0, 2.0];
    let pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: knots.clone(),
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 0.0),
        ],
        weights: None,
        periodic: true,
    };
    let curve = NurbsCurve {
        degree: 1,
        knots,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: true,
    };

    assert_eq!(
        super::closest_pcurve_parameters(&pcurve, Point2::new(0.0, 0.0), Some(4.1))
            .expect("periodic pcurve phase"),
        [4.0]
    );
    assert_eq!(
        super::closest_nurbs_curve_parameter(&curve, Point3::new(0.0, 0.0, 0.0), Some(4.1),)
            .expect("periodic curve phase"),
        4.0
    );
}

#[test]
fn polynomial_root_isolation_retains_repeated_real_roots() {
    let roots =
        super::real_polynomial_roots(&[-1.0, 3.5, -3.0, -0.5, 1.0]).expect("finite quartic roots");

    assert_eq!(roots.len(), 3);
    for (actual, expected) in roots.iter().zip([-2.0, 0.5, 1.0]) {
        assert!((actual - expected).abs() < 1.0e-10, "{actual}");
    }
}

#[test]
fn coincident_pcurve_interval_retains_seed_and_boundaries() {
    let pcurve = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![Point2::new(2.0, -3.0); 3],
        weights: None,
        periodic: false,
    };
    let roots = super::closest_pcurve_parameters(&pcurve, Point2::new(2.0, -3.0), Some(0.3))
        .expect("coincident interval");

    assert_eq!(roots, [0.3, 0.0, 1.0]);
}

#[test]
fn pcurve_bezier_extraction_preserves_rational_knot_spans() {
    let knots = [0.0, 0.0, 0.0, 0.25, 0.75, 1.0, 1.0, 1.0];
    let points = [
        Point2::new(-1.0, 0.0),
        Point2::new(0.0, 2.0),
        Point2::new(1.0, -1.0),
        Point2::new(2.0, 3.0),
        Point2::new(4.0, 0.0),
    ];
    let weights = [1.0, 1.5, 0.75, 2.0, 1.25];
    let controls = points
        .iter()
        .zip(weights)
        .map(|(point, weight)| [point.u * weight, point.v * weight, weight])
        .collect();
    let spans = super::bezier_spans(2, &knots, controls).expect("valid Bézier extraction");

    assert_eq!(spans.len(), 3);
    for span in spans {
        for fraction in [0.0, 0.5, 1.0] {
            let parameter = span.domain[0] + fraction * (span.domain[1] - span.domain[0]);
            let expected =
                cadmpeg_ir::eval::nurbs_pcurve_uv(2, &knots, &points, Some(&weights), parameter)
                    .expect("source NURBS evaluation");
            let actual =
                super::homogeneous_residual_distance(&span.controls, parameter, span.domain);
            assert!((actual - expected.u.hypot(expected.v)).abs() < 1.0e-12);
        }
    }
}
