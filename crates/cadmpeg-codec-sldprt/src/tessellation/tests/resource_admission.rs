// SPDX-License-Identifier: Apache-2.0
use super::super::{CircularHole, PlanarOuter, PlanarTrim, PlaneFrame};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

const EPS_AREA_SUM: f64 = 1e-10;

#[test]
fn polygon_simplicity_and_ear_search_refuse_work_limits() {
    let polygon =
        [[0., 0.], [2., 0.], [2., 2.], [1., 1.], [0., 2.]].map(|p| Point2::new(p[0], p[1]));
    for (work, operation) in [
        (0, "test SLDPRT polygon simplicity"),
        (35, "triangulate SLDPRT planar polygon"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error =
            super::super::triangulate_polygon(&ctx, &polygon, super::EPS_DISPLAY_QUANTIZATION)
                .unwrap_err();
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(trim.contains_mesh(&ctx, &mesh, cadmpeg_ir::transform::Transform::identity(), super::EPS_DISPLAY_QUANTIZATION), Err(CodecError::ResourceLimit(limit)) if limit.operation == "test SLDPRT planar trim points")
    );
}

#[test]
fn display_class_discovery_refuses_both_source_scans() {
    for (payload, work, operation) in [
        (vec![0; 64], 0, "scan SLDPRT display class declarations"),
        (
            [super::super::CLASS_MARKER, &[1, 0], b"x", &[0; 64]].concat(),
            72,
            "scan SLDPRT display class sources",
        ),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(super::super::class_intervals(&ctx, &payload), Err(CodecError::ResourceLimit(limit)) if limit.operation == operation)
        );
    }
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
            &mesh,
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
    let hole = super::super::PlanarHole::polygon(
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 31;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(trim.contains_mesh(&ctx, &mesh, cadmpeg_ir::transform::Transform::identity(), super::EPS_DISPLAY_QUANTIZATION), Err(CodecError::ResourceLimit(limit)) if limit.operation == "test SLDPRT planar trim triangles")
    );
}

#[test]
fn single_planar_polygon_does_not_bill_boundary_pairs() {
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
    };
    let face = model.faces.iter().find(|face| face.id == face_id).unwrap();
    let surface = &model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap()
        .geometry;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 128;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::super::planar_trim(&ctx, face, surface, &topology)
        .unwrap()
        .is_some());
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn polygon_triangle_check_refuses_a_self_crossing_outer() {
    let boundary = [[0., 0.], [2., 0.], [0., 2.], [2., 2.]].map(|p| Point2::new(p[0], p[1]));
    assert!(!super::super::polygon_contains_triangle(
        &cadmpeg_test_support::service_decode_context(),
        &boundary,
        [boundary[0], boundary[1], boundary[2]],
        super::EPS_DISPLAY_QUANTIZATION
    )
    .unwrap());
}

#[test]
fn convex_polygon_ear_search_bills_only_visited_candidates() {
    let polygon = (0..256)
        .map(|i| {
            let angle = f64::from(i) * std::f64::consts::TAU / 256.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1_000_000;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let triangles =
        super::super::triangulate_polygon(&ctx, &polygon, super::EPS_DISPLAY_QUANTIZATION)
            .unwrap()
            .unwrap();
    assert_eq!(triangles.len(), polygon.len() - 2);
    let area = triangles
        .iter()
        .map(|triangle| super::super::signed_area_twice(triangle[0], triangle[1], triangle[2]))
        .sum::<f64>();
    let expected =
        super::super::simple_polygon_area_twice(&polygon, super::EPS_DISPLAY_QUANTIZATION)
            .unwrap()
            .get();
    assert!((area - expected).abs() <= EPS_AREA_SUM);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn checked_convex_boundary_is_reused_for_every_triangle() {
    let polygon = (0..256)
        .map(|i| {
            let angle = f64::from(i) * std::f64::consts::TAU / 256.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let boundary =
        super::super::CheckedPlanarPolygon::new(&ctx, &polygon, super::EPS_DISPLAY_QUANTIZATION)
            .unwrap()
            .unwrap();
    for index in 1..polygon.len() - 1 {
        assert!(boundary
            .contains_triangle(&ctx, [polygon[0], polygon[index], polygon[index + 1]])
            .unwrap());
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn planar_boundary_pairs_bill_only_visited_comparisons() {
    let polygon = (0..4096)
        .map(|i| {
            let angle = f64::from(i) * std::f64::consts::TAU / 4096.0;
            Point2::new(10.0 + angle.cos(), 10.0 + angle.sin())
        })
        .collect::<Vec<_>>();
    let outside = [
        Point2::new(-2.0, -2.0),
        Point2::new(2.0, -2.0),
        Point2::new(0.0, 2.0),
    ];
    let shared_edge = [polygon[0], polygon[1], Point2::new(10.0, 10.0)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One three-edge containment probe and one shared-edge intersection.
    policy.limits.max_work_units = 128;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!super::super::polygon_inside_polygon(
        &ctx,
        &polygon,
        &outside,
        super::EPS_DISPLAY_QUANTIZATION,
    )
    .unwrap());
    assert!(super::super::polygons_overlap(
        &ctx,
        &polygon,
        &shared_edge,
        super::EPS_DISPLAY_QUANTIZATION,
    )
    .unwrap());
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn disjoint_planar_boundaries_use_linear_bounds_work() {
    let polygon = |center: f64| {
        (0u32..4096)
            .map(|index| {
                let angle = f64::from(index) * std::f64::consts::TAU / 4096.0;
                Point2::new(center + angle.cos(), angle.sin())
            })
            .collect::<Vec<_>>()
    };
    let first = polygon(0.0);
    let second = polygon(4.0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 100_000;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!super::super::polygons_overlap(
        &ctx,
        &first,
        &second,
        super::EPS_DISPLAY_QUANTIZATION
    )
    .unwrap());
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn convex_outer_box_proves_inner_boundary_containment() {
    let polygon = |radius: f64| {
        (0u32..4096)
            .map(|index| {
                let angle = f64::from(index) * std::f64::consts::TAU / 4096.0;
                Point2::new(radius * angle.cos(), radius * angle.sin())
            })
            .collect::<Vec<_>>()
    };
    let inner = polygon(1.0);
    let outer = polygon(4.0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2_000_000;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::super::polygon_inside_polygon(
        &ctx,
        &inner,
        &outer,
        super::EPS_DISPLAY_QUANTIZATION
    )
    .unwrap());
    assert!(ctx.resource_refusal().is_none());
}
