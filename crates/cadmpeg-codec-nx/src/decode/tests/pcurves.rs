// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

const TOLERANT_INTERSECTION_FIT: f64 = 1.0e-8;
const EPS_TOPOLOGY_TOLERANCE: f64 = 1.0e-8;

use crate::decode::blend::{
    bezier_spans, closest_nurbs_curve_parameter, closest_pcurve_parameters,
    homogeneous_residual_distance, real_polynomial_roots, surface_contact_direction,
    surface_offset_lineage,
};
use crate::decode::build::{
    rmfastload_selected_bodies, rmfastload_stream_indices, select_active_body,
};
use crate::decode::emit::orient_edge_range;
use crate::decode::offset::{
    certified_offset_cache_fit, point_distance, subdivide_offset_rectangle, translation_net_normal,
};
use crate::decode::pcurves::{
    coincident_pcurve_pair, complete_tolerant_intersection_pcurves_from_serialized_branches,
    exact_boundary_pcurve, orient_tolerant_intersection_pcurve, pcurve_matches_edge,
};

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

const EPS_PCURVE_PARAMETERS: f64 = 1.0e-12;
const EPS_BOUNDARY_FIT: f64 = 1.0e-8;

#[test]
fn active_body_selection_accepts_a_complete_singleton_membership() {
    let first = BodyId::mint("nx:test:body#first").expect("identity grammar");
    let second = BodyId::mint("nx:test:body#second").expect("identity grammar");
    let mut ir = CadIr::empty();
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
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            crate::dialect::NxDialect::Splmsstr.id(),
        )),
        BTreeMap::new(),
    ));
    let body_node_ids = BTreeMap::from([
        (first.clone(), BTreeSet::from([7])),
        (second, BTreeSet::from([8])),
    ]);

    assert!(select_active_body(&mut ir, &body_node_ids, &[7]));
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
    let first = BodyId::mint("nx:s3:body#first").expect("identity grammar");
    let second = BodyId::mint("nx:s8:body#second").expect("identity grammar");
    let body_node_ids = BTreeMap::from([
        (first.clone(), BTreeSet::from([7, 8])),
        (second, BTreeSet::from([8, 9])),
    ]);

    let selected = rmfastload_selected_bodies(&body_node_ids, &[7, 8]);
    assert_eq!(selected, BTreeSet::from([first]));
    assert_eq!(
        rmfastload_stream_indices(&selected),
        Some(BTreeSet::from([3]))
    );
}

#[test]
fn analytic_closed_isocurves_retain_the_native_full_turn() {
    let mut ir = CadIr::empty();
    let cone = SurfaceId::mint("test:model:entity#nx:test:cone").expect("identity grammar");
    let sphere = SurfaceId::mint("test:model:entity#nx:test:sphere").expect("identity grammar");
    let torus = SurfaceId::mint("test:model:entity#nx:test:torus").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: cone.clone(),
            geometry: SurfaceGeometry::Cone(
                cadmpeg_ir::geometry::ConeSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                    0.5,
                    0.25_f64.atan(),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: sphere.clone(),
            geometry: SurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::SphereSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: torus.clone(),
            geometry: SurfaceGeometry::Torus(
                cadmpeg_ir::geometry::TorusSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    3.0,
                    1.0,
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let plane = SurfaceId::mint("test:model:entity#nx:test:plane").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: plane.clone(),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let cone_ellipse =
        CurveId::mint("test:model:entity#nx:test:cone-ellipse").expect("identity grammar");
    let sphere_circle =
        CurveId::mint("test:model:entity#nx:test:sphere-circle").expect("identity grammar");
    let torus_circle =
        CurveId::mint("test:model:entity#nx:test:torus-circle").expect("identity grammar");
    ir.model.curves.extend([
        Curve {
            id: cone_ellipse.clone(),
            geometry: CurveGeometry::Ellipse(
                cadmpeg_ir::geometry::EllipseCurve::try_new(
                    Point3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.25,
                    1.125,
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Curve {
            id: sphere_circle.clone(),
            geometry: CurveGeometry::Circle(
                cadmpeg_ir::geometry::CircleCurve::try_new(
                    Point3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    3.0_f64.sqrt(),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Curve {
            id: torus_circle.clone(),
            geometry: CurveGeometry::Circle(
                cadmpeg_ir::geometry::CircleCurve::try_new(
                    Point3::new(3.0, 0.0, 0.0),
                    Vector3::new(0.0, -1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    1.0,
                )
                .unwrap(),
            ),
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
    assert!(matches!(sphere_pcurve, PcurveGeometry::Line(line_pcurve)
        if {
            let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
            (origin.v - std::f64::consts::FRAC_PI_6).abs() < EPS_PCURVE_PARAMETERS
                && direction.u == 1.0
                && direction.v == 0.0
        }));
    assert!(matches!(torus_pcurve, PcurveGeometry::Line(line_pcurve)
        if {
            let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
            origin.u.abs() < EPS_PCURVE_PARAMETERS && direction.u == 0.0 && direction.v == 1.0
        }));
    assert!(matches!(cone_pcurve, PcurveGeometry::Line(line_pcurve)
        if {
            let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
            (origin.v - 1.0).abs() < EPS_PCURVE_PARAMETERS && direction.u == 1.0 && direction.v == 0.0
        }));
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
            assert!(point_distance(expected, actual) < 1.0e-12);
        }
    }

    let construction = ProceduralCurveId::mint("test:model:entity#nx:test:closed-intersection")
        .expect("identity grammar");
    let _attached = ir.model.add_procedural_curve(
        sphere_circle.clone(),
        ProceduralCurve::new(
            construction,
            ProceduralCurveDefinition::TolerantIntersection {
                construction: cadmpeg_ir::geometry::TolerantIntersectionConstruction::try_new(
                    [sphere, plane],
                    [
                        Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
                        Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
                    ],
                    TOLERANT_INTERSECTION_FIT,
                )
                .unwrap(),
                parameterization: None,
            },
        )
        .unwrap(),
    );
    let point = PointId::mint("test:model:entity#nx:test:closed-point").expect("identity grammar");
    let vertex =
        VertexId::mint("test:model:entity#nx:test:closed-vertex").expect("identity grammar");
    ir.model.points.push(Point {
        id: point.clone(),
        position: Point3::new(3.0_f64.sqrt(), 0.0, 1.0),
        source_object: None,
    });
    ir.model.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: Some(
            cadmpeg_ir::units::PositiveScalar::new(EPS_TOPOLOGY_TOLERANCE)
                .expect("positive finite tolerance"),
        ),
    });
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:entity#nx:test:closed-edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(sphere_circle)),
        start: vertex.clone(),
        end: vertex,
        tolerance: Some(
            cadmpeg_ir::units::PositiveScalar::new(EPS_TOPOLOGY_TOLERANCE)
                .expect("positive finite tolerance"),
        ),
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
    )
    .expect("valid exactness fields");
    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization, ..
    } = ir.model.procedural_curves[0].definition()
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
    )
    .expect("valid exactness fields");
    let ProceduralCurveDefinition::TolerantIntersection {
        construction: intersection,
        parameterization: Some(parameterization),
        ..
    } = ir.model.procedural_curves[0].definition()
    else {
        panic!("closed intersection parameterization");
    };
    let supports = intersection.supports();

    assert_eq!(parameterization.parameter_range(), range);
    assert_eq!(ir.model.edges[0].param_range(), Some(range));
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
        let curve = ir
            .model
            .procedural_curve_owner(&ir.model.procedural_curves[0].id)
            .expect("closed intersection owner");
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
    let mut ir = CadIr::empty();
    let curve =
        CurveId::mint("test:model:entity#nx:test:bowed-boundary-curve").expect("identity grammar");
    let surface =
        SurfaceId::mint("test:model:entity#nx:test:boundary-plane").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(
            NurbsCurve::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(5.0, 5.0, 0.0),
                    Point3::new(10.0, 0.0, 0.0),
                ],
                None,
                false,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });

    assert!(exact_boundary_pcurve(
        &ir,
        &curve,
        &surface,
        [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
        [0.0, 1.0],
        1.0e-8,
    )
    .is_none());

    ir.model.curves[0].geometry = CurveGeometry::Nurbs(
        cadmpeg_ir::geometry::NurbsCurve::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            None,
            false,
        )
        .unwrap(),
    );
    assert!(matches!(
        exact_boundary_pcurve(
            &ir,
            &curve,
            &surface,
            [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            [0.0, 1.0],
            EPS_BOUNDARY_FIT,
        ),
        Some(PcurveGeometry::Line(_))
    ));
}

#[test]
fn boundary_pcurve_accepts_a_certified_affine_nurbs_boundary() {
    let mut ir = CadIr::empty();
    let curve = CurveId::mint("test:model:entity#nx:test:affine-nurbs-boundary-curve")
        .expect("identity grammar");
    let surface = SurfaceId::mint("test:model:entity#nx:test:affine-nurbs-boundary-surface")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: affine_nurbs_surface(0.0),
        source_object: None,
    });

    assert!(matches!(exact_boundary_pcurve(
            &ir,
            &curve,
            &surface,
            [Point3::new(0.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)],
            [0.0, 1.0],
            EPS_BOUNDARY_FIT,
        ), Some(PcurveGeometry::Line(line_pcurve))
                if {
                    let origin = line_pcurve.origin();
    let direction = line_pcurve.direction();
                    origin.v == 0.0 && direction.u == 1.0 && direction.v == 0.0
                }));
}

fn affine_nurbs_surface(z: f64) -> SurfaceGeometry {
    SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                vec![Point3::new(0.0, 0.0, z), Point3::new(0.0, 2.0, z)],
                vec![Point3::new(3.0, 0.0, z), Point3::new(3.0, 2.0, z)],
            ],
            None,
            false,
            false,
            false,
        )
        .unwrap(),
    )
}

fn quadratic_translation_surface(z: f64) -> SurfaceGeometry {
    SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            2,
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            [0.0, 1.0, 3.0]
                .into_iter()
                .map(|x| {
                    [0.0, 2.0, 5.0]
                        .into_iter()
                        .map(|y| Point3::new(x, y, z))
                        .collect()
                })
                .collect(),
            Some(vec![2.0; 9]).map(|values| values.chunks(3 as usize).map(<[_]>::to_vec).collect()),
            false,
            false,
            false,
        )
        .unwrap(),
    )
}

fn degree_elevated_affine_surface(z: f64) -> SurfaceGeometry {
    SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            2,
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            [0.0, 1.5, 3.0]
                .into_iter()
                .map(|x| {
                    [0.0, 1.0, 2.0]
                        .into_iter()
                        .map(|y| Point3::new(x, y, z))
                        .collect()
                })
                .collect(),
            None,
            false,
            false,
            false,
        )
        .unwrap(),
    )
}

fn quadratic_paraboloid_surface() -> SurfaceGeometry {
    let coordinates = [0.0, 0.5, 1.0];
    let square_controls = [0.0, 0.0, 1.0];
    SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            2,
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            (0..3)
                .map(|u| {
                    (0..3)
                        .map(|v| {
                            Point3::new(
                                coordinates[u],
                                coordinates[v],
                                square_controls[u] + square_controls[v],
                            )
                        })
                        .collect()
                })
                .collect(),
            None,
            false,
            false,
            false,
        )
        .unwrap(),
    )
}

#[test]
fn planar_offset_cache_fit_is_certified_over_the_control_net() {
    let support = affine_nurbs_surface(0.0);
    let mut candidate = affine_nurbs_surface(4.0);
    let SurfaceGeometry::Nurbs(candidate) = &mut candidate else {
        unreachable!();
    };
    candidate
        .edit_control_points(|rows| rows[1][0].z += 0.000_5)
        .unwrap();

    let fit = certified_offset_cache_fit(
        &support,
        &SurfaceGeometry::Nurbs(candidate.clone()),
        4.0,
        0.001,
    )
    .expect("whole-patch fit");
    assert!((fit - 0.000_5).abs() < 1.0e-12);
    assert!(certified_offset_cache_fit(
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
    let surface =
        SurfaceId::mint("test:model:entity#nx:test:budget-plane").expect("identity grammar");
    let start_point =
        PointId::mint("test:model:entity#nx:test:budget-start-point").expect("identity grammar");
    let end_point =
        PointId::mint("test:model:entity#nx:test:budget-end-point").expect("identity grammar");
    let start_vertex =
        VertexId::mint("test:model:entity#nx:test:budget-start-vertex").expect("identity grammar");
    let end_vertex =
        VertexId::mint("test:model:entity#nx:test:budget-end-vertex").expect("identity grammar");
    let edge = EdgeId::mint("test:model:entity#nx:test:budget-edge").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
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
            tolerance: None,
        },
        Vertex {
            id: end_vertex.clone(),
            point: end_point,
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: edge.clone(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
        start: start_vertex,
        end: end_vertex,
        tolerance: None,
    });
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let budget = crate::decode::geometry_work::GeometryWorkBudget::new(0);
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
            .unwrap(),
    );

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
        certified_offset_cache_fit(
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
    support_surface.set_u_periodic(true);
    candidate_surface.set_u_periodic(true);

    assert_eq!(
        certified_offset_cache_fit(&support, &candidate, 0.0, 0.0),
        Some(0.0)
    );
}

#[test]
fn offset_cache_fit_certifies_differing_bases_on_one_parameter_domain() {
    let bound = certified_offset_cache_fit(
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
        certified_offset_cache_fit(&support, &support, 0.0, 0.0),
        Some(0.0)
    );
    let bound = certified_offset_cache_fit(&support, &support, 0.01, 0.02)
        .expect("nonzero curved offset certified");
    assert!((0.01..=0.02).contains(&bound));
}

#[test]
fn offset_cache_fit_decouples_distant_knot_span_scale() {
    let x = [0.0, 0.25, 0.5, 1.0e9 + 0.5];
    let z = [0.0, 0.0, 0.1, 0.2];
    let support = SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            2,
            1,
            vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            (0..4)
                .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
                .collect::<Vec<_>>()
                .chunks(2 as usize)
                .map(<[_]>::to_vec)
                .collect(),
            None,
            false,
            false,
            false,
        )
        .unwrap(),
    );

    let bound = certified_offset_cache_fit(&support, &support, 0.01, 0.02)
        .expect("each regular knot span certifies independently");
    assert!((0.01..=0.02).contains(&bound));
}

#[test]
fn offset_cache_fit_certifies_regular_c0_knot_spans() {
    let x = [0.0, 0.25, 0.5, 1.0, 1.5];
    let z = [0.0, 0.0, 0.1, 0.1, 0.2];
    let support = SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            2,
            1,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            (0..5)
                .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
                .collect::<Vec<_>>()
                .chunks(2 as usize)
                .map(<[_]>::to_vec)
                .collect(),
            None,
            false,
            false,
            false,
        )
        .unwrap(),
    );

    let bound = certified_offset_cache_fit(&support, &support, 0.01, 0.02)
        .expect("regular spans certify across the C0 knot break");
    assert!((0.01..=0.02).contains(&bound));
}

#[test]
fn curved_offset_cache_fit_rejects_an_uncertified_fold() {
    let mut support = quadratic_paraboloid_surface();
    let SurfaceGeometry::Nurbs(surface) = &mut support else {
        unreachable!();
    };
    let replacement = (0..3)
        .map(|v| surface.control_grid()[1][v])
        .collect::<Vec<_>>();
    surface
        .edit_control_points(|rows| {
            rows[2].copy_from_slice(&replacement);
        })
        .unwrap();
    assert!(certified_offset_cache_fit(&support, &support, 0.0, 1.0).is_none());
}

#[test]
fn curved_offset_cache_fit_accepts_a_regular_turning_control_net() {
    let mut support = quadratic_paraboloid_surface();
    let SurfaceGeometry::Nurbs(surface) = &mut support else {
        unreachable!();
    };
    surface
        .edit_control_points(|rows| {
            for v in 0..3 {
                rows[2][v].x = 0.0;
            }
        })
        .unwrap();
    assert_eq!(
        certified_offset_cache_fit(&support, &support, 0.0, 0.0),
        Some(0.0)
    );
}

#[test]
fn curved_offset_cache_fit_certifies_deeply_localized_regularity() {
    let epsilon = 2.0_f64.powi(-100);
    let x = [0.0, epsilon / 3.0, 2.0 * epsilon / 3.0, 1.0 + epsilon];
    let z = [0.0, 0.0, 1.0 / 3.0, 1.0];
    let support = SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            3,
            1,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            (0..4)
                .flat_map(|u| (0..2).map(move |v| Point3::new(x[u], v as f64, z[u])))
                .collect::<Vec<_>>()
                .chunks(2 as usize)
                .map(<[_]>::to_vec)
                .collect(),
            None,
            false,
            false,
            false,
        )
        .unwrap(),
    );
    let SurfaceGeometry::Nurbs(surface) = &support else {
        unreachable!();
    };

    assert!(translation_net_normal(surface).is_none());
    assert_eq!(
        certified_offset_cache_fit(&support, &support, 0.0, 0.0),
        Some(0.0)
    );
}

#[test]
fn offset_cache_subdivision_uses_the_remaining_divisible_axis() {
    let u0 = 1.0_f64;
    let u1 = f64::from_bits(u0.to_bits() + 1);
    let u = u0 + (u1 - u0) * 0.5;
    let mut rectangles = Vec::new();

    assert!(subdivide_offset_rectangle(
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
    surface
        .set_weights(Some(
            (0..3)
                .map(|u| (0..3).map(|v| axis_weights[u] * axis_weights[v]).collect())
                .collect(),
        ))
        .unwrap();

    assert_eq!(
        certified_offset_cache_fit(&support, &support, 0.0, 0.0),
        Some(0.0)
    );
    assert!(certified_offset_cache_fit(&support, &support, 0.01, 0.02).is_some());
}

#[test]
fn rational_offset_cache_bounds_are_translation_invariant() {
    let mut support = quadratic_paraboloid_surface();
    let SurfaceGeometry::Nurbs(surface) = &mut support else {
        unreachable!();
    };
    surface
        .edit_control_points(|rows| {
            for point in rows.iter_mut().flatten() {
                point.x += 1.0e12;
                point.y -= 2.0e12;
                point.z += 3.0e12;
            }
        })
        .unwrap();
    let axis_weights = [1.0, 1.01, 1.02];
    surface
        .set_weights(Some(
            (0..3)
                .map(|u| (0..3).map(|v| axis_weights[u] * axis_weights[v]).collect())
                .collect(),
        ))
        .unwrap();

    let bound = certified_offset_cache_fit(&support, &support, 0.01, 0.02)
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

    assert!(point_distance(mapped, point) <= 0.01);
}

#[test]
fn nurbs_blend_contact_requires_the_declared_radius_shell() {
    let mut ir = CadIr::empty();
    let surface =
        SurfaceId::mint("test:model:entity#nx:test:contact-support").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: affine_nurbs_surface(0.0),
        source_object: None,
    });
    let center = Point3::new(1.2, 0.7, 2.0);

    let direction = surface_contact_direction(&ir, &surface, center, 2.0, 0)
        .expect("the support contains one contact at the blend radius");
    assert!((direction - Vector3::new(0.0, 0.0, -1.0)).norm() < 1.0e-10);
    assert!(surface_contact_direction(&ir, &surface, center, 1.0, 0).is_none());
}

#[test]
fn saved_offset_cache_retains_its_procedural_lineage() {
    let mut ir = CadIr::empty();
    let support = SurfaceId::mint("test:model:entity#nx:test:support").expect("identity grammar");
    let cache = SurfaceId::mint("test:model:entity#nx:test:cache").expect("identity grammar");
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
    let procedural = ProceduralSurface::try_new(
        ProceduralSurfaceId::mint("test:model:entity#nx:test:offset").expect("identity grammar"),
        ProceduralSurfaceDefinition::Offset(
            cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                support.clone(),
                4.0,
                Some(0),
                Some(0),
                false,
                cadmpeg_ir::geometry::OffsetExtension::Legacy {
                    flags: cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                },
            )
            .unwrap(),
        ),
        Some(0.0),
        None,
    )
    .unwrap();
    ir.model
        .add_procedural_surface(cache.clone(), procedural)
        .unwrap();

    assert_eq!(surface_offset_lineage(&ir, &cache, 0), Some((support, 4.0)));
}

#[test]
fn serialized_surface_curves_select_a_terminal_intersection_branch() {
    let mut ir = CadIr::empty();
    let surfaces = [
        SurfaceId::mint("nx:test:surface#0").expect("identity grammar"),
        SurfaceId::mint("nx:test:surface#1").expect("identity grammar"),
    ];
    for surface in &surfaces {
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        });
    }
    let curve = CurveId::mint("test:model:entity#nx:test:curve").expect("identity grammar");
    let procedural = ProceduralCurveId::mint("test:model:entity#nx:test:intersection")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: procedural.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(
        ProceduralCurve::new(
            procedural,
            ProceduralCurveDefinition::TolerantIntersection {
                construction: cadmpeg_ir::geometry::TolerantIntersectionConstruction::try_new(
                    surfaces.clone(),
                    [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
                    0.01,
                )
                .unwrap(),
                parameterization: None,
            },
        )
        .unwrap(),
    );
    let points = [
        PointId::mint("nx:test:point#0").expect("identity grammar"),
        PointId::mint("nx:test:point#1").expect("identity grammar"),
    ];
    let vertices = [
        VertexId::mint("nx:test:vertex#0").expect("identity grammar"),
        VertexId::mint("nx:test:vertex#1").expect("identity grammar"),
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
    let edge = EdgeId::mint("test:model:entity#nx:test:edge").expect("identity grammar");
    ir.model.edges.push(Edge {
        id: edge.clone(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve)),
        start: vertices[0].clone(),
        end: vertices[1].clone(),
        tolerance: Some(
            cadmpeg_ir::units::PositiveScalar::new(0.03).expect("positive finite tolerance"),
        ),
    });
    let pcurves = [
        PcurveId::mint("nx:test:pcurve#0").expect("identity grammar"),
        PcurveId::mint("nx:test:pcurve#1").expect("identity grammar"),
    ];
    let faces = [
        FaceId::mint("nx:test:face#0").expect("identity grammar"),
        FaceId::mint("nx:test:face#1").expect("identity grammar"),
    ];
    let loops = [
        LoopId::mint("nx:test:loop#0").expect("identity grammar"),
        LoopId::mint("nx:test:loop#1").expect("identity grammar"),
    ];
    let coedges = [
        CoedgeId::mint("nx:test:coedge#0").expect("identity grammar"),
        CoedgeId::mint("nx:test:coedge#1").expect("identity grammar"),
    ];
    for index in 0..2 {
        ir.model.pcurves.push(Pcurve {
            id: pcurves[index].clone(),
            geometry: PcurveGeometry::Line(
                cadmpeg_ir::geometry::LinePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                )
                .unwrap(),
            ),
            metadata: cadmpeg_ir::geometry::PcurveMetadata::try_general(
                None,
                Some([0.0, 10.0]),
                Some(0.02),
            )
            .unwrap(),
        });
        ir.model.faces.push(Face {
            id: faces[index].clone(),
            shell: ShellId::mint("test:model:entity#nx:test:shell").expect("identity grammar"),
            surface: surfaces[index].clone(),
            sense: Sense::Forward,
            loops: vec![loops[index].clone()].into(),
            name: None,
            color: None,
            tolerance: Some(
                cadmpeg_ir::units::PositiveScalar::new(0.03).expect("positive finite tolerance"),
            ),
        });
        ir.model.loops.push(Loop {
            id: loops[index].clone(),
            face: faces[index].clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(vec![coedges[index].clone()], Vec::new())
                    .expect("valid loop ring"),
            ),
        });
        ir.model.coedges.push(Coedge {
            id: coedges[index].clone(),
            owner_loop: loops[index].clone(),
            edge: edge.clone(),
            radial_next: coedges[1 - index].clone(),
            sense: Sense::Forward,
            pcurves: vec![PcurveUse {
                pcurve: pcurves[index].clone(),
                isoparametric: None,
                parameter_range: Some(
                    cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 10.0]).unwrap(),
                ),
            }],
            use_curve: None,
        });
    }
    let serialized = [0, 1]
        .map(|index| {
            (
                ir.model
                    .procedural_curve_owner(&ir.model.procedural_curves[0].id)
                    .expect("intersection owner")
                    .clone(),
                surfaces[index].clone(),
                pcurves[index].clone(),
            )
        })
        .into_iter()
        .collect();
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    assert!(matches!(
        ir.model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::TolerantIntersection {
            parameterization: None,
            ..
        }
    ));
    for pcurve in &mut ir.model.pcurves {
        let cadmpeg_ir::geometry::PcurveMetadata::General { form: metadata } = &mut pcurve.metadata
        else {
            panic!("fixture uses general pcurve metadata")
        };
        metadata.set_fit_tolerance(Some(0.01)).unwrap();
    }
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );

    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = ir.model.procedural_curves[0].definition()
    else {
        panic!("serialized branch transferred");
    };
    assert_eq!(parameterization.parameter_range(), [0.0, 10.0]);
    assert_eq!(ir.model.edges[0].param_range(), Some([0.0, 10.0]));
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

    ir.model.procedural_curves[0]
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::TolerantIntersection {
                parameterization, ..
            } = definition
            else {
                unreachable!();
            };
            *parameterization = None;
        })
        .unwrap();
    let edge = &mut ir.model.edges[0];
    edge.set_param_range(None).unwrap();
    std::mem::swap(&mut edge.start, &mut edge.end);
    for pcurve in &mut ir.model.pcurves {
        pcurve.geometry = PcurveGeometry::Line(
            cadmpeg_ir::geometry::LinePcurve::try_new(
                Point2::new(10.0, 0.0),
                Point2::new(-1.0, 0.0),
            )
            .unwrap(),
        );
    }
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = ir.model.procedural_curves[0].definition()
    else {
        panic!("reversed serialized branch transferred");
    };
    assert!(parameterization.pcurves.iter().all(
        |pcurve| matches!(pcurve, PcurveGeometry::Line(line_pcurve)
                if {
                    let origin = line_pcurve.origin();
        let direction = line_pcurve.direction();
                    origin.u == 0.0 && direction.u == 1.0
                })
    ));
    assert_eq!(ir.model.edges[0].start, vertices[0]);
    assert_eq!(ir.model.edges[0].end, vertices[1]);

    let range = [-1.5, 1.5];
    let canonical = PcurveGeometry::Ellipse(
        cadmpeg_ir::geometry::EllipsePcurve::try_new(
            Point2::new(5.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            4.0,
            2.0,
        )
        .unwrap(),
    );
    let endpoints = range.map(|parameter| {
        let uv = cadmpeg_ir::eval::pcurve_uv(&canonical, parameter).unwrap();
        Point3::new(uv.u, uv.v, 0.0)
    });
    for (point, position) in ir.model.points.iter_mut().zip(endpoints) {
        point.position = position;
    }
    ir.model.procedural_curves[0]
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::TolerantIntersection {
                construction: intersection,
                parameterization,
                ..
            } = definition
            else {
                unreachable!();
            };
            let supports = intersection.supports();
            let tolerance = intersection.tolerance();
            *intersection = cadmpeg_ir::geometry::TolerantIntersectionConstruction::try_new(
                supports.clone(),
                endpoints,
                tolerance,
            )
            .unwrap();
            *parameterization = None;
        })
        .unwrap();
    ir.model.edges[0].set_param_range(None).unwrap();
    for coedge in &mut ir.model.coedges {
        coedge.pcurves[0].parameter_range =
            Some(cadmpeg_ir::geometry::DirectedParameterRange::new(range).unwrap());
    }
    for pcurve in &mut ir.model.pcurves {
        let cadmpeg_ir::geometry::PcurveMetadata::General { form: metadata } = &mut pcurve.metadata
        else {
            panic!("fixture uses general pcurve metadata")
        };
        metadata.set_parameter_range(Some(range)).unwrap();
        pcurve.geometry = PcurveGeometry::Ellipse(
            cadmpeg_ir::geometry::EllipsePcurve::try_new(
                Point2::new(5.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, -1.0),
                4.0,
                2.0,
            )
            .unwrap(),
        );
    }
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    let ProceduralCurveDefinition::TolerantIntersection {
        parameterization: Some(parameterization),
        ..
    } = ir.model.procedural_curves[0].definition()
    else {
        panic!("reversed symmetric conic branches transferred");
    };
    assert_eq!(parameterization.parameter_range(), range);
    assert!(parameterization.pcurves.iter().all(
        |pcurve| matches!(pcurve, PcurveGeometry::Ellipse(ellipse_pcurve)
        if {
            let y_axis = ellipse_pcurve.y_axis();
            y_axis.v == 1.0
        })
    ));

    ir.model.procedural_curves[0]
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::TolerantIntersection {
                construction: intersection,
                parameterization,
                ..
            } = definition
            else {
                unreachable!();
            };
            let supports = intersection.supports();
            let endpoints = intersection.endpoints();
            *intersection = cadmpeg_ir::geometry::TolerantIntersectionConstruction::try_new(
                supports.clone(),
                *endpoints,
                10.0,
            )
            .unwrap();
            *parameterization = None;
        })
        .unwrap();
    ir.model.edges[0].set_param_range(None).unwrap();
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        &mut ir,
        &serialized,
        &mut AnnotationBuilder::new(),
    );
    assert!(matches!(
        ir.model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::TolerantIntersection {
            parameterization: Some(_),
            ..
        }
    ));
}

#[test]
fn closed_serialized_pcurve_uses_carrier_tangent_for_orientation() {
    let mut ir = CadIr::empty();
    let curve = CurveId::mint("test:model:entity#nx:test:closed-orientation-curve")
        .expect("identity grammar");
    let support = SurfaceId::mint("test:model:entity#nx:test:closed-orientation-support")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Circle(
            cadmpeg_ir::geometry::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: support.clone(),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let pcurve = PcurveGeometry::Circle(
        cadmpeg_ir::geometry::CirclePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            2.0,
        )
        .unwrap(),
    );
    let endpoint = Point3::new(2.0, 0.0, 0.0);

    let oriented = orient_tolerant_intersection_pcurve(
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
    let mut ir = CadIr::empty();
    let curve_id = CurveId::mint("nx:test:curve#0").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let procedural = ProceduralCurve::try_new(
        ProceduralCurveId::mint("nx:test:intersection#0").expect("identity grammar"),
        ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext::try_new(
                [
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                    },
                ],
                [0.0, 1.0],
                [Vec::new(), Vec::new(), Vec::new()],
            )
            .unwrap(),
            discontinuity_flag: false,
        },
        Some(2.0),
    )
    .unwrap();
    ir.model
        .add_procedural_curve(curve_id.clone(), procedural)
        .unwrap();

    let start_point = PointId::mint("nx:test:point#0").expect("identity grammar");
    let end_point = PointId::mint("nx:test:point#1").expect("identity grammar");
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
    let start = VertexId::mint("nx:test:vertex#0").expect("identity grammar");
    let end = VertexId::mint("nx:test:vertex#1").expect("identity grammar");
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
    let edge = EdgeId::mint("nx:test:edge#0").expect("identity grammar");
    ir.model.edges.push(Edge {
        id: edge.clone(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id.clone())),
        start: start.clone(),
        end: end.clone(),
        tolerance: None,
    });
    let support = SurfaceId::mint("nx:test:surface-support#0").expect("identity grammar");
    let surface = SurfaceId::mint("nx:test:surface#0").expect("identity grammar");
    let construction =
        ProceduralSurfaceId::mint("nx:test:surface-offset#0").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        },
    ]);
    ir.model.procedural_surfaces.push(
        ProceduralSurface::new(
            construction,
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    support,
                    1.0,
                    Some(0),
                    Some(0),
                    false,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy {
                        flags: cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                    },
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap(),
    );
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            None,
            false,
        )
        .unwrap(),
    };

    assert!(orient_edge_range(&ir, &curve_id, [0.0, 1.0], &start, &end, None).is_none());
    assert!(!pcurve_matches_edge(&ir, &edge, &surface, &pcurve, None,));
    assert!(pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve,
        Some(0.01),
    ));
    let large_distance = point_distance(
        Point3::new(1.0e200, 1.0e200, 1.0e200),
        Point3::new(0.0, 0.0, 0.0),
    );
    assert!(large_distance.is_finite());
    assert!((large_distance / 1.0e200 - 3.0_f64.sqrt()).abs() < 1.0e-15);
}

#[test]
fn boundary_coincidence_is_certified_between_uniform_samples() {
    let mut ir = CadIr::empty();
    let surfaces = [
        SurfaceId::mint("nx:test:surface#0").expect("identity grammar"),
        SurfaceId::mint("nx:test:surface#1").expect("identity grammar"),
    ];
    let surface = || {
        NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.01, 0.02, 1.0, 1.0],
            [0.0, 1.0]
                .into_iter()
                .flat_map(|y| {
                    [0.0, 0.1, 0.2, 10.0]
                        .into_iter()
                        .map(move |x| Point3::new(x, y, 0.0))
                })
                .collect::<Vec<_>>()
                .chunks(4 as usize)
                .map(<[_]>::to_vec)
                .collect(),
            None,
            false,
            false,
            false,
        )
        .unwrap()
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
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(0.0, 1.0))
            .unwrap(),
    );
    assert!(coincident_pcurve_pair(
        &ir,
        [&surfaces[0], &surfaces[1]],
        [&pcurve, &pcurve],
        [0.0, 1.0],
        0.1,
    ));

    let SurfaceGeometry::Nurbs(second) = &mut ir.model.surfaces[1].geometry else {
        unreachable!()
    };
    second
        .edit_control_points(|rows| rows[0][1].z = 1.0)
        .unwrap();
    assert!(!coincident_pcurve_pair(
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
        nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
            4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            controls,
            Some(weights.to_vec()),
            false,
        )
        .unwrap(),
    };
    let roots = closest_pcurve_parameters(&pcurve, Point2::new(0.0, 0.0), Some(0.11))
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
        nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
            4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            control_points,
            Some(weights.to_vec()),
            false,
        )
        .unwrap(),
    };
    let parameters = closest_pcurve_parameters(&pcurve, Point2::new(0.0, 1.0e-4), Some(0.11))
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
    let curve = NurbsCurve::new(
        4,
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        control_points,
        Some(weights.to_vec()),
        false,
    )
    .unwrap();
    let point = Point3::new(0.0, 1.0e-4, 0.0);

    let first =
        closest_nurbs_curve_parameter(&curve, point, Some(0.099)).expect("first close branch");
    let second =
        closest_nurbs_curve_parameter(&curve, point, Some(0.101)).expect("second close branch");
    let remote =
        closest_nurbs_curve_parameter(&curve, point, Some(0.69)).expect("remote global branch");

    assert!((first - 0.1).abs() < 1.0e-8);
    assert!((second - 0.1001).abs() < 1.0e-8);
    assert!((remote - 0.7).abs() < 1.0e-8);
}

#[test]
fn periodic_nurbs_inversion_lifts_the_continuation_phase() {
    let knots = vec![0.0, 0.0, 1.0, 2.0, 2.0];
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
            1,
            knots.clone(),
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 0.0),
            ],
            None,
            true,
        )
        .unwrap(),
    };
    let curve = NurbsCurve::new(
        1,
        knots,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        None,
        true,
    )
    .unwrap();

    assert_eq!(
        closest_pcurve_parameters(&pcurve, Point2::new(0.0, 0.0), Some(4.1))
            .expect("periodic pcurve phase"),
        [4.0]
    );
    assert_eq!(
        closest_nurbs_curve_parameter(&curve, Point3::new(0.0, 0.0, 0.0), Some(4.1),)
            .expect("periodic curve phase"),
        4.0
    );
}

#[test]
fn polynomial_root_isolation_retains_repeated_real_roots() {
    let roots = real_polynomial_roots(&[-1.0, 3.5, -3.0, -0.5, 1.0]).expect("finite quartic roots");

    assert_eq!(roots.len(), 3);
    for (actual, expected) in roots.iter().zip([-2.0, 0.5, 1.0]) {
        assert!((actual - expected).abs() < 1.0e-10, "{actual}");
    }
}

#[test]
fn coincident_pcurve_interval_retains_seed_and_boundaries() {
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![Point2::new(2.0, -3.0); 3],
            None,
            false,
        )
        .unwrap(),
    };
    let roots = closest_pcurve_parameters(&pcurve, Point2::new(2.0, -3.0), Some(0.3))
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
    let spans = bezier_spans(2, &knots, controls).expect("valid Bézier extraction");

    assert_eq!(spans.len(), 3);
    for span in spans {
        for fraction in [0.0, 0.5, 1.0] {
            let parameter = span.domain[0] + fraction * (span.domain[1] - span.domain[0]);
            let expected =
                cadmpeg_ir::eval::nurbs_pcurve_uv(2, &knots, &points, Some(&weights), parameter)
                    .expect("source NURBS evaluation");
            let actual = homogeneous_residual_distance(&span.controls, parameter, span.domain);
            assert!((actual - expected.u.hypot(expected.v)).abs() < 1.0e-12);
        }
    }
}

mod reversal;
