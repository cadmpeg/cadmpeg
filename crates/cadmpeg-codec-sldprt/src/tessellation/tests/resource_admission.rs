// SPDX-License-Identifier: Apache-2.0
use super::super::{CircularHole, PlanarOuter, PlanarTrim, PlaneFrame};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

#[test]
fn polygon_simplicity_and_ear_search_refuse_work_limits() {
    let polygon =
        [[0., 0.], [2., 0.], [2., 2.], [1., 1.], [0., 2.]].map(|p| Point2::new(p[0], p[1]));
    for operation in [
        "test SLDPRT polygon simplicity",
        "triangulate SLDPRT planar polygon",
        "test SLDPRT polygon ear diagonal",
        "test SLDPRT polygon ear interior",
    ] {
        let error = crate::test_support::work_refusal_at(operation, |ctx| {
            super::geometry_predicates::triangulate_polygon(
                ctx,
                &polygon,
                super::EPS_DISPLAY_QUANTIZATION,
            )
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn circular_trim_pair_comparisons_refuse_zero_work() {
    let circles = [
        CircularHole {
            center: Point2::new(0., 0.),
            radius: 2.,
        },
        CircularHole {
            center: Point2::new(0., 0.),
            radius: 1.,
        },
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(super::super::circular_outer_and_holes(&ctx, &circles, super::EPS_DISPLAY_QUANTIZATION), Err(CodecError::ResourceLimit(limit)) if limit.operation == "compare SLDPRT circular trim boundaries")
    );
}

#[test]
fn planar_mesh_boundary_queries_refuse_work_before_comparison() {
    let boundary = vec![
        Point2::new(0., 0.),
        Point2::new(2., 0.),
        Point2::new(2., 2.),
        Point2::new(0., 2.),
    ];
    let trim = PlanarTrim {
        frame: PlaneFrame::new(
            cadmpeg_ir::features::FinitePoint3::ZERO,
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
        outer: Some(PlanarOuter::Polygon(boundary)),
        holes: Vec::new(),
        boundary_tolerance: 0.,
    };
    let mesh = cadmpeg_ir::tessellation::Tessellation::new(
        cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#boundary")
            .unwrap(),
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices: vec![
                Point3::new(0., 0., 0.),
                Point3::new(1., 0., 0.),
                Point3::new(0., 1., 0.),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap();
    let error = crate::test_support::work_refusal_at("test SLDPRT planar trim points", |ctx| {
        trim.contains_mesh(
            ctx,
            mesh.mesh(),
            cadmpeg_ir::transform::Transform::identity(),
            super::EPS_DISPLAY_QUANTIZATION,
        )
    });
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "test SLDPRT planar trim points")
    );
}

#[test]
fn display_class_discovery_refuses_both_source_scans() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(super::super::class_intervals(&ctx, &[0; 64]), Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan SLDPRT display class declarations")
    );

    let mut payload = Vec::new();
    super::class(&mut payload, "moAmbientLight_c", &[12]);
    let mut source = crate::test_support::container::outer_header();
    source.extend(crate::test_support::container::make_block(
        0x41,
        "Contents/DisplayLists",
        &payload,
    ));
    let error = crate::test_support::work_refusal_at("scan SLDPRT display class sources", |ctx| {
        let scan = crate::container::scan(ctx, cadmpeg_core::decode::View::over_retained(&source))?;
        super::super::scene_feature_classes(ctx, &scan)
    });
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "scan SLDPRT display class sources")
    );
}

#[test]
fn sole_surface_owner_respects_its_planar_trim() {
    let mut model = super::model_with_body();
    let face = super::add_square_face(&mut model, "bounded", 0.);
    super::set_shell_faces(&mut model, vec![face]);
    model.tessellations.push(
        cadmpeg_ir::tessellation::Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#outside")
                .unwrap(),
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(10., 10., 0.),
                    Point3::new(11., 10., 0.),
                    Point3::new(10., 11., 0.),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap(),
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(super::super::assign_unique_surface_owners(&ctx, &mut model)
        .unwrap()
        .is_empty());
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
}

#[test]
fn planar_trim_rejects_a_triangle_spanning_a_concave_outer() {
    let boundary = [
        [0., 0.],
        [3., 0.],
        [3., 3.],
        [2., 3.],
        [2., 1.],
        [1., 1.],
        [1., 3.],
        [0., 3.],
    ]
    .map(|p| Point2::new(p[0], p[1]))
    .to_vec();
    let trim = PlanarTrim {
        frame: PlaneFrame::new(
            cadmpeg_ir::features::FinitePoint3::ZERO,
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
        outer: Some(PlanarOuter::Polygon(boundary)),
        holes: Vec::new(),
        boundary_tolerance: 0.,
    };
    let mesh = cadmpeg_ir::tessellation::Tessellation::new(
        cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#concave")
            .unwrap(),
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices: vec![
                Point3::new(0.5, 2.5, 0.),
                Point3::new(2.5, 2.5, 0.),
                Point3::new(1.5, 0.5, 0.),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap();
    assert!(!trim
        .contains_mesh(
            &cadmpeg_test_support::service_decode_context(),
            mesh.mesh(),
            cadmpeg_ir::transform::Transform::identity(),
            super::EPS_DISPLAY_QUANTIZATION
        )
        .unwrap());
}

#[test]
fn polygon_triangle_check_accepts_edges_on_the_outer_boundary() {
    let boundary = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| Point2::new(p[0], p[1]));
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(super::super::polygon_contains_triangle(
        &ctx,
        &boundary,
        [boundary[0], boundary[1], boundary[2]],
        super::EPS_DISPLAY_QUANTIZATION
    )
    .unwrap());
}

#[test]
fn planar_mesh_hole_triangles_refuse_work_before_overlap() {
    let boundary = [[0., 0.], [1., 0.], [0., 1.]]
        .map(|p| Point2::new(p[0], p[1]))
        .to_vec();
    let hole = super::geometry_predicates::hole_polygon(
        &cadmpeg_test_support::service_decode_context(),
        boundary,
        super::EPS_DISPLAY_QUANTIZATION,
    )
    .unwrap()
    .unwrap();
    let trim = PlanarTrim {
        frame: PlaneFrame::new(
            cadmpeg_ir::features::FinitePoint3::ZERO,
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
        outer: None,
        holes: vec![hole],
        boundary_tolerance: 0.,
    };
    let mesh = cadmpeg_ir::tessellation::Tessellation::new(
        cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#overlap")
            .unwrap(),
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices: vec![
                Point3::new(-1., -1., 0.),
                Point3::new(2., -1., 0.),
                Point3::new(-1., 2., 0.),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap();
    let error = crate::test_support::work_refusal_at("test SLDPRT planar trim triangles", |ctx| {
        trim.contains_mesh(
            ctx,
            mesh.mesh(),
            cadmpeg_ir::transform::Transform::identity(),
            super::EPS_DISPLAY_QUANTIZATION,
        )
    });
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "test SLDPRT planar trim triangles")
    );
}

#[test]
fn planar_polygon_construction_refuses_boundary_comparison_work() {
    let mut model = super::model_with_body();
    let face_id = super::add_square_face(&mut model, "budget", 0.);
    let loops = model.loops.iter().map(|entry| (&entry.id, entry)).collect();
    let coedges = model
        .coedges
        .iter()
        .map(|entry| (&entry.id, entry))
        .collect();
    let edges = model.edges.iter().map(|entry| (&entry.id, entry)).collect();
    let vertices = model
        .vertices
        .iter()
        .map(|entry| (&entry.id, entry))
        .collect();
    let points = model
        .points
        .iter()
        .map(|entry| (&entry.id, entry.position().get()))
        .collect();
    let curves = model
        .curves
        .iter()
        .map(|entry| (&entry.id, &entry.geometry))
        .collect();
    let topology = super::super::TrimTopology {
        loops: &loops,
        coedges: &coedges,
        edges: &edges,
        vertices: &vertices,
        points: &points,
        curves: &curves,
        coordinate_scale: model
            .points
            .iter()
            .map(|entry| entry.position().get())
            .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
            .fold(1.0_f64, f64::max),
    };
    let face = model.faces.iter().find(|face| face.id == face_id).unwrap();
    let surface = &model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap()
        .geometry;
    let error =
        crate::test_support::work_refusal_at("compare SLDPRT planar trim boundaries", |ctx| {
            super::super::planar_trim(ctx, face, surface, &topology)
        });
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "compare SLDPRT planar trim boundaries")
    );
}

#[test]
fn planar_trim_refuses_a_triangle_inside_a_self_crossing_outer() {
    let boundary = [[0., 0.], [2., 0.], [0., 2.], [2., 2.]]
        .map(|p| Point2::new(p[0], p[1]))
        .to_vec();
    assert!(!super::geometry_predicates::is_simple_polygon(
        &boundary,
        super::EPS_DISPLAY_QUANTIZATION
    ));
    let trim = PlanarTrim {
        frame: PlaneFrame::new(
            cadmpeg_ir::features::FinitePoint3::ZERO,
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
        outer: Some(PlanarOuter::Polygon(boundary)),
        holes: Vec::new(),
        boundary_tolerance: 0.,
    };
    let mesh = cadmpeg_ir::tessellation::Tessellation::new(
        cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#crossing")
            .unwrap(),
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices: vec![
                Point3::new(0., 0., 0.),
                Point3::new(2., 0., 0.),
                Point3::new(0., 2., 0.),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap();
    assert!(!trim
        .contains_mesh(
            &cadmpeg_test_support::service_decode_context(),
            mesh.mesh(),
            cadmpeg_ir::transform::Transform::identity(),
            super::EPS_DISPLAY_QUANTIZATION
        )
        .unwrap());
}


fn boundary_sample_inputs() -> (
    cadmpeg_ir::geometry::CurveGeometry,
    cadmpeg_ir::geometry::SurfaceGeometry,
    PlaneFrame,
) {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0., 0., 0.), Vector3::new(0., 0., 1.), Vector3::new(1., 0., 0.),
        ).unwrap(),
    ));
    let frame = super::super::plane_frame(surface.solved().unwrap()).unwrap();
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0., 0., 0.), Vector3::new(1., 0., 0.),
        ).unwrap(),
    ));
    (curve, surface, frame)
}

fn boundary_sample_policy(bytes: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_collection_items = 1;
    policy.limits.max_work_units = 0;
    policy
}

fn boundary_sample_limits(sampling_tolerance: f64) -> super::super::BoundaryTolerance {
    super::super::BoundaryTolerance {
        tolerance: super::EPS_DISPLAY_QUANTIZATION,
        sampling_tolerance,
    }
}

fn boundary_test_arc() -> super::super::PlanarArc {
    super::super::PlanarArc {
        center: Point3::new(0., 0., 0.),
        first_direction: Vector3::new(1., 0., 0.),
        second_direction: Vector3::new(0., 1., 0.),
        first_radius: 1., second_radius: 1.,
    }
}

fn point2_bytes() -> u64 {
    u64::try_from(std::mem::size_of::<Point2>()).unwrap()
}

#[test]
fn planar_line_sample_has_exact_live_storage_and_free_fixed_work() {
    let (curve, surface, frame) = boundary_sample_inputs();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &boundary_sample_policy(point2_bytes())).unwrap();
    let (samples, tolerance) = super::super::planar_boundary_samples(
        &ctx, &curve, Point3::new(2., 3., 0.), Point3::new(4., 3., 0.),
        &surface, frame, boundary_sample_limits(1.),
    ).unwrap().unwrap();
    assert_eq!(&*samples, &[Point2::new(2., 3.)]);
    assert_eq!(samples.points.capacity(), 1);
    assert_eq!(tolerance, 0.);
    drop(samples);
    drop(ctx.reserve_scoped(point2_bytes(), "test complete line sample release").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn planar_line_sample_refuses_backing_before_allocation() {
    let (curve, surface, frame) = boundary_sample_inputs();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &boundary_sample_policy(point2_bytes() - 1)).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        super::super::planar_boundary_samples(&ctx, &curve, Point3::new(0., 0., 0.), Point3::new(1., 0., 0.), &surface, frame, boundary_sample_limits(1.))
    });
    let limit = match &result { Err(CodecError::ResourceLimit(limit)) => *limit, _ => panic!("line backing refusal") };
    drop(result);
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SLDPRT planar trim line samples");
    assert_eq!((limit.used, limit.additional, limit.limit), (0, point2_bytes(), point2_bytes() - 1));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn planar_arc_sample_owns_only_its_actual_amortized_backing() {
    let (_, surface, frame) = boundary_sample_inputs();
    let arena = DecodeArena::new();
    let mut policy = boundary_sample_policy(4 * point2_bytes());
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    // The quarter-circle's one segment fits a sampling tolerance of one.
    let (samples, tolerance) = boundary_test_arc().samples(&ctx, 0., std::f64::consts::FRAC_PI_2, &surface, frame, boundary_sample_limits(1.)).unwrap().unwrap();
    assert_eq!(&*samples, &[Point2::new(1., 0.)]);
    assert_eq!(samples.points.capacity(), 4);
    let sine = (std::f64::consts::FRAC_PI_2 / 4.).sin();
    assert_eq!(tolerance, sine * (2. * sine));
    drop(samples);
    drop(ctx.reserve_scoped(4 * point2_bytes(), "test complete arc sample release").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn planar_arc_sample_refuses_first_backing_before_allocation() {
    let (_, surface, frame) = boundary_sample_inputs();
    let arena = DecodeArena::new();
    let mut policy = boundary_sample_policy(4 * point2_bytes() - 1);
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| boundary_test_arc().samples(&ctx, 0., std::f64::consts::FRAC_PI_2, &surface, frame, boundary_sample_limits(1.)));
    let limit = match &result { Err(CodecError::ResourceLimit(limit)) => *limit, _ => panic!("arc backing refusal") };
    drop(result);
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SLDPRT planar trim arc samples");
    assert_eq!((limit.used, limit.additional, limit.limit), (0, 4 * point2_bytes(), 4 * point2_bytes() - 1));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn off_plane_arc_releases_its_sample_prefix_before_return() {
    let (_, surface, frame) = boundary_sample_inputs();
    let mut arc = boundary_test_arc();
    const SAMPLE_TILT: f64 = 1.0e-5;
    arc.second_direction = Vector3::new(0., SAMPLE_TILT.cos(), SAMPLE_TILT.sin());
    let arena = DecodeArena::new();
    let mut policy = boundary_sample_policy(4 * point2_bytes());
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    // tolerance=.2 chooses two segments. The first is on-plane; the second
    // has z=sin(tilt)*sin(pi/4) and fails the unchanged residual gate before insertion.
    let result = arc.samples(&ctx, 0., std::f64::consts::FRAC_PI_2, &surface, frame, boundary_sample_limits(0.2)).unwrap();
    assert!(result.is_none());
    drop(result);
    drop(ctx.reserve_scoped(4 * point2_bytes(), "test rejected arc prefix release").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn planar_sample_consumption_releases_source_and_retains_only_polygon() {
    let (curve, surface, frame) = boundary_sample_inputs();
    let arena = DecodeArena::new();
    let mut policy = boundary_sample_policy(point2_bytes());
    policy.limits.max_retained_bytes = 4 * point2_bytes();
    policy.limits.max_collection_items = 2;
    // append_vec admits one moved source item plus its inline bytes.
    policy.limits.max_work_units = 1 + point2_bytes();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (samples, _) = super::super::planar_boundary_samples(&ctx, &curve, Point3::new(2., 3., 0.), Point3::new(4., 3., 0.), &surface, frame, boundary_sample_limits(1.)).unwrap().unwrap();
    let mut polygon = Vec::new();
    samples.append_into(&ctx, &mut polygon).unwrap();
    assert_eq!(polygon, [Point2::new(2., 3.)]);
    assert_eq!(polygon.capacity(), 4);
    drop(ctx.reserve_scoped(point2_bytes(), "test consumed source sample release").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn planar_sample_backing_stays_live_through_polygon_growth_refusal() {
    let (curve, surface, frame) = boundary_sample_inputs();
    let arena = DecodeArena::new();
    let mut policy = boundary_sample_policy(4 * point2_bytes());
    policy.limits.max_retained_bytes = 8 * point2_bytes();
    policy.limits.max_collection_items = 6;
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut polygon = ctx.collection_vec(4, "test initial polygon backing").unwrap();
    let original = [Point2::new(0., 0.), Point2::new(1., 0.), Point2::new(1., 1.), Point2::new(0., 1.)];
    for point in original { polygon.push(point); }
    let (samples, _) = super::super::planar_boundary_samples(&ctx, &curve, Point3::new(2., 3., 0.), Point3::new(4., 3., 0.), &surface, frame, boundary_sample_limits(1.)).unwrap().unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| samples.append_into(&ctx, &mut polygon));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("polygon overlap refusal"); };
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SLDPRT planar trim polygon");
    assert_eq!((limit.used, limit.additional, limit.limit), (point2_bytes(), 4 * point2_bytes(), 4 * point2_bytes()));
    assert_eq!(polygon, original);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn planar_sample_preserves_original_refusal_without_allocation() {
    let (curve, surface, frame) = boundary_sample_inputs();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &boundary_sample_policy(point2_bytes())).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(2, "test original boundary sample refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| super::super::planar_boundary_samples(&ctx, &curve, Point3::new(0., 0., 0.), Point3::new(1., 0., 0.), &surface, frame, boundary_sample_limits(1.)));
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    drop(result);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
