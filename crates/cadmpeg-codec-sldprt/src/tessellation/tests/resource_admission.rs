// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use super::super::{CircularHole, PlanarOuter, PlanarTrim, PlaneFrame};

#[test]
fn polygon_simplicity_and_ear_search_refuse_work_limits() {
    let polygon = [[0.,0.],[2.,0.],[2.,2.],[1.,1.],[0.,2.]].map(|p| Point2::new(p[0],p[1]));
    for (work, operation) in [(0, "test SLDPRT polygon simplicity"), (35, "triangulate SLDPRT planar polygon")] {
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = work;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::super::triangulate_polygon(&ctx, &polygon, super::EPS_DISPLAY_QUANTIZATION).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn circular_trim_pair_comparisons_refuse_zero_work() {
    let circles = [CircularHole { center: Point2::new(0.,0.), radius: 2. }, CircularHole { center: Point2::new(0.,0.), radius: 1. }];
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(super::super::circular_outer_and_holes(&ctx, &circles, super::EPS_DISPLAY_QUANTIZATION), Err(CodecError::ResourceLimit(limit)) if limit.operation == "compare SLDPRT circular trim boundaries"));
}

#[test]
fn planar_mesh_boundary_queries_refuse_work_before_comparison() {
    let boundary = vec![Point2::new(0.,0.), Point2::new(2.,0.), Point2::new(2.,2.), Point2::new(0.,2.)];
    let trim = PlanarTrim { frame: PlaneFrame::new(cadmpeg_ir::features::FinitePoint3::ZERO, Vector3::new(0.,0.,1.), Vector3::new(1.,0.,0.)).unwrap(), outer: Some(PlanarOuter::Polygon(boundary)), holes: Vec::new(), boundary_tolerance: 0. };
    let mesh = cadmpeg_ir::tessellation::Tessellation::new(cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#boundary").unwrap(), cadmpeg_ir::tessellation::TessellationMesh::List { vertices: vec![Point3::new(0.,0.,0.),Point3::new(1.,0.,0.),Point3::new(0.,1.,0.)], triangles: vec![[0,1,2]] }, Vec::new()).unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 3;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(trim.contains_mesh(&ctx, &mesh, cadmpeg_ir::transform::Transform::identity(), super::EPS_DISPLAY_QUANTIZATION), Err(CodecError::ResourceLimit(limit)) if limit.operation == "test SLDPRT planar trim points"));
}

#[test]
fn display_class_discovery_refuses_both_source_scans() {
    for (payload, work, operation) in [(vec![0; 64], 0, "scan SLDPRT display class declarations"), ([super::super::CLASS_MARKER, &[1, 0], b"x", &[0; 64]].concat(), 71, "scan SLDPRT display class sources")] {
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = work;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::super::class_intervals(&ctx, &payload), Err(CodecError::ResourceLimit(limit)) if limit.operation == operation));
    }
}

#[test]
fn sole_surface_owner_respects_its_planar_trim() {
    let mut model = super::model_with_body();
    let face = super::add_square_face(&mut model, "bounded", 0.);
    super::set_shell_faces(&mut model, vec![face]);
    model.tessellations.push(cadmpeg_ir::tessellation::Tessellation::new(cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#outside").unwrap(), cadmpeg_ir::tessellation::TessellationMesh::List { vertices: vec![Point3::new(10.,10.,0.),Point3::new(11.,10.,0.),Point3::new(10.,11.,0.)], triangles: vec![[0,1,2]] }, Vec::new()).unwrap());
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(super::super::assign_unique_surface_owners(&ctx, &mut model).unwrap().is_empty());
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
}
