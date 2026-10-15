// SPDX-License-Identifier: Apache-2.0
//! face evidence tests.

use super::{
    attach_free_vertices, native_support_circle_param_range, resolve_standard_endpoint_pairs,
    standard_native_support_endpoint_pair, AnnotationBuilder, CadIr, PcurveGeometry, Point, Point2,
    Point3, PointId, SolvedSurfaceGeometry, StandardCurveGeometry, StandardCurveSupport,
    StandardEdgeSupport, Surface, SurfaceGeometry, SurfaceId, Vector3, Vertex, VertexId,
};

#[test]
fn standard_empty_vertex_population_creates_no_owner_or_annotations() {
    let mut ir = CadIr::empty();
    let before = ir.clone();
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        attach_free_vertices(ctx, &mut ir, &mut annotations, &mut admission)
    })
    .expect("service profile admits free vertex owner");
    assert_eq!(ir, before);
    assert_eq!(annotations.build(), AnnotationBuilder::new().build());
}

#[test]
fn standard_surface_entity_limit_refuses_before_first_surface_append() {
    let bytes = crate::test_support::test_container::standard_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("standard fixture fits the input-byte limit");
    let result = crate::families::standard::decode::try_decode_standard(
        &ctx,
        &scan,
        &mut crate::nurbs::LaneRefusals::new(),
    );
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("first standard surface must exceed the entity limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(limit.used, 1);
    assert_eq!(limit.operation, "admit CATIA family model entity");
}

#[test]
fn standard_free_vertex_owner_limit_refuses_before_shell_creation() {
    let mut ir = CadIr::empty();
    ir.model.vertices.push(Vertex {
        id: VertexId::mint("catia:test:vertex#v").expect("identity grammar"),
        point: PointId::mint("catia:test:point#p").expect("identity grammar"),
        tolerance: None,
    });
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_entity_limit(0, |ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            attach_free_vertices(ctx, &mut ir, &mut annotations, &mut admission)
        else {
            panic!("free-vertex owner must exceed the entity limit");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::Entities
        );
        assert_eq!(limit.operation, "admit CATIA family model entity");
    });
    assert!(ir.model.bodies.is_empty());
    assert!(ir.model.regions.is_empty());
    assert!(ir.model.shells.is_empty());
}

#[test]
fn standard_unbound_vertices_receive_one_free_vertex_owner() {
    let mut ir = CadIr::empty();
    ir.model.vertices.push(Vertex {
        id: VertexId::mint("catia:test:vertex#v".to_string()).expect("identity grammar"),
        point: PointId::mint("catia:test:point#p".to_string()).expect("identity grammar"),
        tolerance: None,
    });
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
        attach_free_vertices(ctx, &mut ir, &mut annotations, &mut admission)
    })
    .expect("service profile admits free vertex owner");
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.regions.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(
        ir.model.shells[0].free_vertices(),
        [VertexId::mint("catia:test:vertex#v".to_string()).expect("identity grammar")]
    );
}

#[test]
fn standard_spline_retains_complete_surface_incidence_pair_domain() {
    let mut ir = CadIr::empty();
    for index in 0..138 {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(f64::from(index), 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ));
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#s{index}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
    }
    let bindings = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            true,
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            true,
            0,
        ),
    ];
    let indices = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            1,
        ),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let choices = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(
            ctx,
            &ir,
            &bindings,
            &indices,
            &[support],
            &[(0..138).collect()],
        )
    })
    .expect("service budget")
    .expect("endpoint option pass");
    assert_eq!(choices[0].len(), 9_453);
    assert_eq!(choices[0].first(), Some(&[0, 1]));
    assert_eq!(choices[0].last(), Some(&[136, 137]));
}

#[test]
fn standard_planar_intersection_spline_uses_the_common_line_domain() {
    let mut ir = CadIr::empty();
    for (index, position) in [
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
    }
    for (index, normal) in [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#s{index}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    normal,
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }
    let bindings = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            true,
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            true,
            0,
        ),
    ];
    let indices = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            1,
        ),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };

    let choices = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(
            ctx,
            &ir,
            &bindings,
            &indices,
            &[support],
            &[vec![0, 1, 2, 3]],
        )
    })
    .expect("service budget")
    .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 1]]]);
}

#[test]
fn standard_antipodal_circle_candidates_admit_full_circle_seams() {
    let mut ir = CadIr::empty();
    for (index, position) in [Point3::new(5.0, 0.0, 0.0), Point3::new(-5.0, 0.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#s{index}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }
    let bindings = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            true,
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            true,
            0,
        ),
    ];
    let indices = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            1,
        ),
    ]
    .into_iter()
    .collect();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: super::super::checked_circle(Point3::new(0.0, 0.0, 0.0), 5.0),
    };

    let choices = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(ctx, &ir, &bindings, &indices, &[support], &[vec![0, 1]])
    })
    .expect("service budget")
    .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 0], [0, 1], [1, 1]]]);
}

#[test]
fn standard_parallel_line_rows_retain_domains_independent_of_allocation_order() {
    let mut ir = CadIr::empty();
    for (index, position) in [
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(-2.0, 0.0, 1.0),
        Point3::new(2.0, 0.0, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#p{index}")).expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position)
                .expect("a finite position is a point"),
            None,
        ));
    }
    for index in 0..2 {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("catia:test:surface#s{index}")).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .expect("valid CylinderSurface fixture"),
            )),
            source_object: None,
        });
    }
    let bindings = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            true,
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            true,
            0,
        ),
    ];
    let indices = [
        (
            SurfaceId::mint("catia:test:surface#s0".to_string()).expect("identity grammar"),
            0,
        ),
        (
            SurfaceId::mint("catia:test:surface#s1".to_string()).expect("identity grammar"),
            1,
        ),
    ]
    .into_iter()
    .collect();
    let supports = [(90, 900), (10, 100)].map(|(pos, tag)| StandardCurveSupport {
        pos,
        tag,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    });

    let choices = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(
            ctx,
            &ir,
            &bindings,
            &indices,
            &supports,
            &[vec![0, 1, 2, 3], vec![0, 1, 2, 3]],
        )
    })
    .expect("service budget")
    .expect("endpoint option pass");

    assert_eq!(choices, [vec![[0, 2], [1, 3]], vec![[0, 2], [1, 3]]]);
}

#[test]
fn line_pair_constraint_rejects_pairs_beyond_edge_roles() {
    let constraint = super::super::super::StandardLinePairConstraint::new(&[], &[], &[]);
    assert!(constraint.edge_pairs(&[None]).is_none());
}

/// A unit-radius cone about +Z whose cross-section radius overflows at
/// v = 1e308, with the pcurve that lifts onto its unit circle and the pcurve
/// that lifts to points without finite coordinates.
fn overflowing_cone_support(parameter_range: [f64; 2]) -> StandardEdgeSupport {
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            1.0,
            1.5,
        )
        .expect("valid ConeSurface fixture"),
    ));
    let line = |origin_v: f64| {
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, origin_v),
                Point2::new(1.0, 0.0),
            )
            .expect("valid LinePcurve fixture"),
        )
    };
    StandardEdgeSupport {
        surface_object_ids: [20, 21],
        carriers: [
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cone.clone()),
            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(cone),
        ],
        pcurves: [line(0.0), line(1.0e308)],
        parameter_range,
    }
}

#[test]
fn a_native_circle_range_reads_from_the_finite_support_when_its_partner_overflows() {
    let native = overflowing_cone_support([0.0, 1.5 * std::f64::consts::PI]);
    assert_eq!(
        native_support_circle_param_range(
            &cadmpeg_test_support::service_decode_context(),
            &native,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, -1.0, 0.0)],
        ),
        Ok(Some([0.0, 1.5 * std::f64::consts::PI]))
    );
}

#[test]
fn a_native_endpoint_pair_reads_from_the_finite_support_when_its_partner_overflows() {
    let native = overflowing_cone_support([1.0, 4.0]);
    let points = [1.0_f64, 4.0]
        .into_iter()
        .enumerate()
        .map(|(index, angle)| {
            Point::new(
                PointId::mint(format!("catia:test:point#overflow-{index}"))
                    .expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(angle.cos(), angle.sin(), 0.0))
                    .expect("a finite position is a point"),
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        standard_native_support_endpoint_pair(
            &cadmpeg_test_support::service_decode_context(),
            &native,
            &points,
            &[0, 1],
            None
        ),
        Ok(Some([0, 1]))
    );
}

#[test]
fn a_native_endpoint_pair_reads_from_the_finite_support_when_its_placed_partner_overflows() {
    let mut native = overflowing_cone_support([1.0, 4.0]);
    native.carriers = native.carriers.map(|carrier| match carrier {
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
            SurfaceGeometry::Solved(cone),
        ) => crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
                cadmpeg_ir::geometry::PlacedSurface::try_new(
                    Box::new(cone),
                    cadmpeg_ir::transform::Transform::identity(),
                )
                .expect("valid PlacedSurface fixture"),
            )),
        ),
        carrier => carrier,
    });
    let points = [1.0_f64, 4.0]
        .into_iter()
        .enumerate()
        .map(|(index, angle)| {
            Point::new(
                PointId::mint(format!("catia:test:point#placed-overflow-{index}"))
                    .expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(angle.cos(), angle.sin(), 0.0))
                    .expect("a finite position is a point"),
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        standard_native_support_endpoint_pair(
            &cadmpeg_test_support::service_decode_context(),
            &native,
            &points,
            &[0, 1],
            None
        ),
        Ok(Some([0, 1]))
    );
}
