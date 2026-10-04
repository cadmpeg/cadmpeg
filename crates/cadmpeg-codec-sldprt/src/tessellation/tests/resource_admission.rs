// SPDX-License-Identifier: Apache-2.0
use super::super::{CircularHole, PlanarOuter, PlanarTrim, PlaneFrame};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

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
            // 71 payload bytes, one class-name validation byte and one copied byte.
            73,
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
    // Three projections, one constraint, three points, three holes, eighteen boundary visits, one triangle and one hole.
    policy.limits.max_work_units = 30;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(trim.contains_mesh(&ctx, &mesh, cadmpeg_ir::transform::Transform::identity(), super::EPS_DISPLAY_QUANTIZATION), Err(CodecError::ResourceLimit(limit)) if limit.operation == "test SLDPRT planar trim triangles")
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
    // 128 work units plus 1027 bytes hashed by one loop and 28 boundary-record lookups.
    policy.limits.max_work_units = 1155;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(super::super::planar_trim(&ctx, face, surface, &topology), Err(CodecError::ResourceLimit(limit)) if limit.operation == "compare SLDPRT planar trim boundaries")
    );
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
