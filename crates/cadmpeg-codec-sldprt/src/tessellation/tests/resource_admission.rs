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
