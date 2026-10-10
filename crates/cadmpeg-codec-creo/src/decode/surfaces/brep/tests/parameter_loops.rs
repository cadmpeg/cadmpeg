// SPDX-License-Identifier: Apache-2.0

use super::*;

fn native_triangle_collection_error(operation: &'static str, ordered: bool) -> CodecError {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let lp = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        (10..13)
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    );
    let polygon = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
    let bindings = lp
        .half_edges()
        .iter()
        .copied()
        .enumerate()
        .map(
            |(index, half_edge)| crate::topology::HalfEdgeVertexIncidence {
                half_edge,
                start_vertex_id: std::num::NonZeroU32::new(
                    u32::try_from(index + 1).expect("three vertices"),
                )
                .expect("one-based vertex fixture"),
                end_vertex_id: std::num::NonZeroU32::new(
                    u32::try_from((index + 1) % 3 + 1).expect("three vertices"),
                ),
            },
        )
        .collect::<Vec<_>>();
    let incidence = bindings
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices = polygon
        .iter()
        .enumerate()
        .map(|(index, point)| {
            (
                u32::try_from(index + 1).expect("three vertices"),
                [point[0], point[1], 0.0],
            )
        })
        .collect::<BTreeMap<_, _>>();
    let native_pcurves = lp
        .half_edges()
        .iter()
        .enumerate()
        .map(|(index, half_edge)| {
            (
                (half_edge.curve_id, 5),
                vec![([polygon[index], polygon[(index + 1) % 3]], 0)],
            )
        })
        .collect::<super::super::NativePcurveCandidates>();
    crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        operation,
        |ctx| {
            let typed = BTreeSet::new();
            let result = if ordered {
                ordered_native_parameter_face_loops(
                    ctx,
                    &[&lp],
                    (5, &surface),
                    &incidence,
                    &solved_vertices,
                    &native_pcurves,
                    NativeCurveEvidence {
                        typed_nonlinear_curve_ids: &typed,
                        model_curves: &[],
                        source_carriers:
                            &crate::decode::source_carriers::SourceUnitCarriers::default(),
                    },
                )
                .map(|_| ())
            } else {
                native_parameter_loop_polygon(
                    ctx,
                    &lp,
                    (5, &surface),
                    &incidence,
                    &solved_vertices,
                    &native_pcurves,
                    &typed,
                )
                .map(|_| ())
            };
            result
        },
    )
}

fn assert_native_collection_refusal(error: &CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

#[test]
fn native_parameter_loop_polygon_refuses_pcurve_segments() {
    assert_native_collection_refusal(
        &native_triangle_collection_error("creo native loop pcurve segments", false),
        "creo native loop pcurve segments",
    );
}

#[test]
fn native_parameter_loop_polygon_refuses_polygon_points() {
    assert_native_collection_refusal(
        &native_triangle_collection_error("creo native loop polygon points", false),
        "creo native loop polygon points",
    );
}

#[test]
fn ordered_native_parameter_face_loops_refuses_polygon_collection() {
    assert_native_collection_refusal(
        &native_triangle_collection_error("creo native face loop polygons", true),
        "creo native face loop polygons",
    );
}

#[test]
fn ordered_native_parameter_face_loops_refuses_loop_references() {
    assert_native_collection_refusal(
        &native_triangle_collection_error("creo native face loop references", true),
        "creo native face loop references",
    );
}

fn circle_order_collection_error(operation: &'static str) -> CodecError {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let make_loop = |base: u32| {
        crate::test_support::closed_loop(
            std::num::NonZeroU32::new(5),
            [base, base + 1]
                .into_iter()
                .map(|curve_id| crate::topology::HalfEdgeId {
                    curve_id,
                    side: crate::topology::Side::Zero,
                })
                .collect(),
        )
    };
    let outer = make_loop(10);
    let inner = make_loop(20);
    let make_circle = |id: u32, radius| Curve {
        id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .expect("valid CircleCurve fixture"),
        )),
        source_object: None,
    };
    let curves = vec![
        make_circle(10, 2.0),
        make_circle(11, 2.0),
        make_circle(20, 1.0),
        make_circle(21, 1.0),
    ];
    let polygons = vec![vec![[2.0, 0.0], [-2.0, 0.0]], vec![[1.0, 0.0], [-1.0, 0.0]]];
    crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        operation,
        |ctx| {
            super::super::ordered_two_edge_circle_loops(
                ctx,
                &[&outer, &inner],
                &polygons,
                &surface,
                &curves,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    )
}

#[test]
fn ordered_two_edge_circle_loops_refuses_geometry_collection() {
    assert_native_collection_refusal(
        &circle_order_collection_error("creo native circle loop geometry"),
        "creo native circle loop geometry",
    );
}

#[test]
fn ordered_two_edge_circle_loops_refuses_order_collection() {
    assert_native_collection_refusal(
        &circle_order_collection_error("creo native circle loop order"),
        "creo native circle loop order",
    );
}

#[test]
fn ordered_two_edge_circle_loops_refuses_ordered_output() {
    assert_native_collection_refusal(
        &circle_order_collection_error("creo native ordered circle loops"),
        "creo native ordered circle loops",
    );
}

fn native_parameter_loop_polygon_service(
    lp: &crate::topology::Loop,
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<crate::topology::HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &super::super::NativePcurveCandidates,
    typed_nonlinear_curve_ids: &BTreeSet<u32>,
) -> Option<Vec<[f64; 2]>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        native_parameter_loop_polygon(
            ctx,
            lp,
            (face_id, surface),
            incidence,
            solved_vertices,
            native_pcurves,
            typed_nonlinear_curve_ids,
        )
    })
    .expect("service native loop polygon")
}

fn ordered_native_parameter_face_loops_service<'a>(
    loops: &[&'a crate::topology::Loop],
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<crate::topology::HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &super::super::NativePcurveCandidates,
    curve_evidence: NativeCurveEvidence<'_>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        ordered_native_parameter_face_loops(
            ctx,
            loops,
            (face_id, surface),
            incidence,
            solved_vertices,
            native_pcurves,
            curve_evidence,
        )
    })
    .expect("service native face loop ordering")
}

#[test]
fn native_parameter_loops_order_non_planar_cylindrical_face() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let make_loop = |first_curve| {
        crate::test_support::closed_loop(
            std::num::NonZeroU32::new(5),
            (0_u32..4)
                .map(|index| crate::topology::HalfEdgeId {
                    curve_id: first_curve + index,
                    side: crate::topology::Side::Zero,
                })
                .collect(),
        )
    };
    let outer = make_loop(10);
    let inner = make_loop(20);
    let outer_polygon = [[0.0, 0.0], [1.0, 0.0], [1.0, 4.0], [0.0, 4.0]];
    let inner_polygon = [[0.25, 1.0], [0.75, 1.0], [0.75, 3.0], [0.25, 3.0]];
    let mut bindings = Vec::new();
    let mut solved_vertices = BTreeMap::new();
    let mut native_pcurves = BTreeMap::<(u32, u32), Vec<([[f64; 2]; 2], usize)>>::new();
    for (base_vertex, (lp, polygon)) in [
        (1_u32, (&outer, outer_polygon)),
        (5_u32, (&inner, inner_polygon)),
    ] {
        for index in 0..4 {
            let half_edge = lp.half_edges()[index];
            let offset = u32::try_from(index).expect("four boundary edges");
            let next_offset = u32::try_from((index + 1) % 4).expect("four boundary edges");
            let start_vertex_id = base_vertex + offset;
            let end_vertex_id = base_vertex + next_offset;
            let start_uv = polygon[index];
            let end_uv = polygon[(index + 1) % 4];
            let point = cadmpeg_ir::eval::decode::surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &surface,
                start_uv[0],
                start_uv[1],
            )
            .expect("analytic cylinder endpoint");
            solved_vertices.insert(start_vertex_id, [point.x, point.y, point.z]);
            bindings.push(crate::topology::HalfEdgeVertexIncidence {
                half_edge,
                start_vertex_id: std::num::NonZeroU32::new(start_vertex_id)
                    .expect("one-based vertex fixture"),
                end_vertex_id: std::num::NonZeroU32::new(end_vertex_id),
            });
            native_pcurves
                .entry((half_edge.curve_id, 5))
                .or_default()
                .push(([start_uv, end_uv], 0));
        }
    }
    let incidence = bindings
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(
        native_parameter_loop_polygon_service(
            &outer,
            5,
            &surface,
            &incidence,
            &solved_vertices,
            &native_pcurves,
            &BTreeSet::new(),
        ),
        Some(outer_polygon.into_iter().collect())
    );
    let ordered = ordered_native_parameter_face_loops_service(
        &[&inner, &outer],
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        NativeCurveEvidence {
            typed_nonlinear_curve_ids: &BTreeSet::new(),
            model_curves: &[],
            source_carriers: &crate::decode::source_carriers::SourceUnitCarriers::default(),
        },
    )
    .expect("one parameter-space outer loop");
    assert_eq!(ordered[0].half_edges()[0].curve_id, 10);
    assert_eq!(ordered[1].half_edges()[0].curve_id, 20);
}

#[test]
fn native_parameter_loops_admit_proven_two_edge_circles() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let outer = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        [10_u32, 11]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    );
    let inner = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        [20_u32, 21]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    );
    let bindings = [(10, 1, 2), (11, 2, 1), (20, 3, 4), (21, 4, 3)]
        .into_iter()
        .map(|(curve_id, start_vertex_id, end_vertex_id)| {
            crate::topology::HalfEdgeVertexIncidence {
                half_edge: crate::topology::HalfEdgeId {
                    curve_id,
                    side: crate::topology::Side::Zero,
                },
                start_vertex_id: std::num::NonZeroU32::new(start_vertex_id)
                    .expect("one-based vertex fixture"),
                end_vertex_id: std::num::NonZeroU32::new(end_vertex_id),
            }
        })
        .collect::<Vec<_>>();
    let incidence = bindings
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices = BTreeMap::from([
        (1, [2.0, 0.0, 0.0]),
        (2, [-2.0, 0.0, 0.0]),
        (3, [1.0, 0.0, 0.0]),
        (4, [-1.0, 0.0, 0.0]),
    ]);
    let native_pcurves = BTreeMap::from([
        ((10, 5), vec![([[2.0, 0.0], [-2.0, 0.0]], 0)]),
        ((11, 5), vec![([[-2.0, 0.0], [2.0, 0.0]], 0)]),
        ((20, 5), vec![([[1.0, 0.0], [-1.0, 0.0]], 0)]),
        ((21, 5), vec![([[-1.0, 0.0], [1.0, 0.0]], 0)]),
    ]);
    let circle = |id, radius| Curve {
        id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .expect("valid CircleCurve fixture"),
        )),
        source_object: None,
    };
    let model_curves = vec![
        circle(10, 2.0),
        circle(11, 2.0),
        circle(20, 1.0),
        circle(21, 1.0),
    ];
    let typed_nonlinear_curve_ids = BTreeSet::from([10, 11, 20, 21]);

    let operations = std::cell::RefCell::new(Vec::new());
    let polygon = crate::test_support::assert_refusal_order(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        &["creo typed nonlinear curve ids lookup"],
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("root");
            let result = super::super::native_parameter_loop_polygon(
                &ctx, &outer, (5, &surface), &incidence, &solved_vertices,
                &native_pcurves, &typed_nonlinear_curve_ids,
            );
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                operations.borrow_mut().push(refusal.operation);
            }
            result
        },
    );
    assert_eq!(polygon, Some(vec![[2.0, 0.0], [-2.0, 0.0]]));
    assert!(!operations.borrow().contains(&"creo B-rep nonlinear half edge search"));
    assert!(!operations.borrow().contains(&"creo B-rep parameter segment search"));

    assert_eq!(
        native_parameter_loop_polygon_service(
            &outer,
            5,
            &surface,
            &incidence,
            &solved_vertices,
            &native_pcurves,
            &typed_nonlinear_curve_ids,
        ),
        Some(vec![[2.0, 0.0], [-2.0, 0.0]])
    );
    assert!(native_parameter_loop_polygon_service(
        &outer,
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        &BTreeSet::new(),
    )
    .is_none());

    let ordered = ordered_native_parameter_face_loops_service(
        &[&inner, &outer],
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        NativeCurveEvidence {
            typed_nonlinear_curve_ids: &typed_nonlinear_curve_ids,
            model_curves: &model_curves,
            source_carriers: &crate::decode::source_carriers::SourceUnitCarriers::default(),
        },
    )
    .expect("concentric two-edge circles have a proven outer loop");
    assert_eq!(ordered[0].half_edges()[0].curve_id, 10);
    assert_eq!(ordered[1].half_edges()[0].curve_id, 20);
}
