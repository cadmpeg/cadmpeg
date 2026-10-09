// SPDX-License-Identifier: Apache-2.0
//! Per-face projection index lifetime boundaries.

use super::{project_brep, BrepSketchSource};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, PlaneSurface};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{SketchEntityId, SketchGeometryDefinition};
use cadmpeg_ir::topology::{Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense, Vertex};
use std::mem::{align_of, size_of};

fn two_face_brep() -> crate::brep::graph::Brep {
    let surface = SurfaceId::mint("test:model:surface#plane").unwrap();
    let curve = CurveId::mint("test:model:curve#circle").unwrap();
    let edge = EdgeId::mint("test:model:edge#circle").unwrap();
    let vertex = VertexId::mint("test:model:vertex#circle").unwrap();
    let point = PointId::mint("test:model:point#circle").unwrap();
    let faces = [FaceId::mint("test:model:face#first").unwrap(), FaceId::mint("test:model:face#second").unwrap()];
    let loops = [LoopId::mint("test:model:loop#first").unwrap(), LoopId::mint("test:model:loop#second").unwrap()];
    let coedges = [CoedgeId::mint("test:model:coedge#first").unwrap(), CoedgeId::mint("test:model:coedge#second").unwrap()];
    let mut brep = crate::brep::graph::Brep {
        surfaces: vec![Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0),
            ).unwrap())),
            source_object: None,
        }],
        curves: vec![Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 1.0,
            ).unwrap())),
            source_object: None,
        }],
        edges: vec![Edge {
            id: edge.clone(), carrier: EdgeCarrier::unbounded(Some(curve)), start: vertex.clone(), end: vertex.clone(), tolerance: None,
        }],
        vertices: vec![Vertex { id: vertex, point: point.clone(), tolerance: None }],
        points: vec![Point::new(point, FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).unwrap(), None)],
        ..Default::default()
    };
    for index in 0..2 {
        brep.faces.push(Face {
            id: faces[index].clone(), shell: ShellId::mint("test:model:shell#pair").unwrap(), surface: surface.clone(),
            sense: Sense::Forward, loops: FaceLoops::Classified { outer: loops[index].clone(), inner: Vec::new() },
            name: None, color: None, tolerance: None,
        });
        brep.loops.push(Loop {
            id: loops[index].clone(), face: faces[index].clone(), boundary: LoopBoundary::Ring(LoopRing::single(coedges[index].clone())),
        });
        brep.coedges.push(Coedge {
            id: coedges[index].clone(), owner_loop: loops[index].clone(), edge: edge.clone(), radial_next: coedges[1 - index].clone(),
            sense: if index == 0 { Sense::Forward } else { Sense::Reversed }, pcurves: Vec::new(), use_curve: None,
        });
    }
    brep
}

fn first_hash_table<T>() -> usize {
    4 * size_of::<T>() + 4 + 16 + align_of::<T>().max(16) - 1
}

fn projection_peak() -> u64 {
    let stream_indexes = first_hash_table::<(&SurfaceId, &SurfaceGeometry)>()
        + first_hash_table::<(&LoopId, &Loop)>()
        + first_hash_table::<(&CoedgeId, &Coedge)>()
        + first_hash_table::<(&EdgeId, &Edge)>()
        + first_hash_table::<(&VertexId, &PointId)>()
        + first_hash_table::<(&PointId, Point3)>()
        + first_hash_table::<(&CurveId, &CurveGeometry)>();
    let sketch_id = "sldprt:model:sketch#0:0:0".len();
    let entity_id = "sldprt:model:sketch-entity#0:0:0:0".len();
    let face_indexes = first_hash_table::<&VertexId>()
        + first_hash_table::<(&EdgeId, SketchEntityId)>() + entity_id;
    // Endpoint grouping starts at one exact slot. Adding its second occurrence grows
    // to four slots and admits overlap for the original slot before reallocation.
    type Locus<'a> = (bool, &'a SketchEntityId);
    type EndpointKey<'a> = &'a str;
    type EndpointValue<'a> = Vec<Locus<'a>>;
    let tree_alignment = align_of::<EndpointKey<'_>>()
        .max(align_of::<EndpointValue<'_>>()).max(align_of::<usize>());
    let endpoint_tree = 11 * (size_of::<EndpointKey<'_>>() + size_of::<EndpointValue<'_>>())
        + 16 * size_of::<usize>() + 2 * tree_alignment;
    let endpoint_peak = endpoint_tree + 5 * size_of::<Locus<'_>>();
    let face_peak = endpoint_peak.max(entity_id).max(2 * sketch_id);
    u64::try_from(stream_indexes + sketch_id + face_indexes + face_peak).unwrap()
}

#[test]
fn projected_face_indexes_expire_before_the_next_face() {
    let brep = two_face_brep();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = projection_peak();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut annotations = Annotations::default();
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    project_brep(&ctx, &brep, &BrepSketchSource {
        block_offset: 0, stream_ordinal: 0, stream_offset: 0,
        source_stream: &cadmpeg_ir::stream_name!("test:face-projection"),
        sketch_name: "circle", configuration: None, native_ref: "native:circle",
    }, &mut annotations, &mut sketches, &mut entities, &mut constraints).unwrap();
    assert_eq!(sketches.len(), 2);
    assert_eq!(entities.len(), 2);
    assert!(constraints.is_empty());
    for index in 0..2 {
        assert_eq!(sketches[index].id.as_str(), format!("sldprt:model:sketch#0:0:{index}"));
        let profile = sketches[index].profiles.as_slice();
        assert_eq!(profile.len(), 1);
        assert_eq!(profile[0].len(), 1);
        assert_eq!(&profile[0][0].entity, entities[index].id());
        assert_eq!(profile[0][0].reversed, index == 1);
        assert_eq!(entities[index].native_ref.as_deref(), Some("0:test:model:edge#circle"));
        assert!(matches!(entities[index].geometry.definition(), SketchGeometryDefinition::Circle { .. }));
    }
    let _reuse = ctx.reserve_scoped(projection_peak(), "reuse expired projected face indexes").unwrap();
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn projected_face_index_peak_preserves_original_sticky_refusal() {
    let brep = two_face_brep();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = projection_peak() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut annotations = Annotations::default();
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    let error = project_brep(&ctx, &brep, &BrepSketchSource {
        block_offset: 0, stream_ordinal: 0, stream_offset: 0,
        source_stream: &cadmpeg_ir::stream_name!("test:face-projection"),
        sketch_name: "circle", configuration: None, native_ref: "native:circle",
    }, &mut annotations, &mut sketches, &mut entities, &mut constraints).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("endpoint growth overlap must refuse"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SLDPRT shared sketch endpoint loci");
    let old_slot = u64::try_from(size_of::<(bool, &SketchEntityId)>()).unwrap();
    assert_eq!(limit.used, projection_peak() - old_slot);
    assert_eq!(limit.additional, old_slot);
    assert_eq!(limit.limit, projection_peak() - 1);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(replayed) = ctx.reserve_scoped(0, "replay projected face refusal").unwrap_err()
    else { panic!("the original resource refusal remains sticky"); };
    assert_eq!(replayed, limit);
}
