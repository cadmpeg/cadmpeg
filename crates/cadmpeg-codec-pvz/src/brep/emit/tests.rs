// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::{BodyKind, FaceLoops, Sense};

use cadmpeg_ir::ids::BodyId;

use super::{emit, emit_with_work, Sink};
use crate::brep::model::ExactModel;
use crate::loss::PvzLossCode;
use crate::test_support::exact::{cube_stream, Stream, CUBE_SIZE};

fn emit_stream(stream: Stream) -> (CadIr, Vec<LossNote>, Vec<BodyId>) {
    emit_stream_with(stream, None)
}

/// Emits `stream`, with the derived-search work limit `work` when given.
fn emit_stream_with(stream: Stream, work: Option<usize>) -> (CadIr, Vec<LossNote>, Vec<BodyId>) {
    emit_edited(stream, work, |_| {})
}

/// Emits `stream` after `edit` changes its parsed records.
fn emit_edited(
    stream: Stream,
    work: Option<usize>,
    edit: impl FnOnce(&mut ExactModel),
) -> (CadIr, Vec<LossNote>, Vec<BodyId>) {
    let id = stream.id();
    let bytes = stream.finish();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut model = crate::brep::parse(&ctx, id, root).unwrap();
    edit(&mut model);
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let mut losses = Vec::new();
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("part.ol"));
    let scope = IdentityKey::encode_segment("part.ol");
    let mut sink = Sink {
        model: &mut ir.model,
        annotations: &mut annotations,
        losses: &mut losses,
        stream: &stream,
    };
    let body = match work {
        Some(work) => emit_with_work(&ctx, &model, &scope, &mut sink, work),
        None => emit(&ctx, &model, &scope, &mut sink),
    }
    .unwrap();
    ir.model.finalize();
    (ir, losses, body)
}

#[test]
fn cube_becomes_a_valid_outward_solid() {
    let (ir, losses, body) = emit_stream(cube_stream(
        false,
        [true, false, false, true, false, true],
        [false, true, false, false, true, true],
    ));
    assert!(losses.is_empty(), "{losses:?}");
    let model = &ir.model;
    assert_eq!(body, [model.bodies[0].id.clone()]);
    assert_eq!(model.bodies[0].kind, BodyKind::Solid);
    assert_eq!((model.faces.len(), model.shells.len()), (6, 1));
    assert_eq!(
        (model.edges.len(), model.vertices.len(), model.coedges.len()),
        (12, 8, 24)
    );
    let report = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).unwrap();
    let serious: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| finding.severity >= Severity::Warning)
        .collect();
    assert!(serious.is_empty(), "{serious:?}");
    let centre_mm = CUBE_SIZE * 1000.0 / 2.0;
    for face in &model.faces {
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .unwrap();
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
            panic!("plane carrier");
        };
        let normal = plane.frame().axis().as_raw();
        let side = if face.sense == Sense::Forward {
            1.0
        } else {
            -1.0
        };
        let origin = plane.origin().get();
        let outward = (origin.x - centre_mm) * normal.x
            + (origin.y - centre_mm) * normal.y
            + (origin.z - centre_mm) * normal.z;
        assert!(outward * side > 0.0, "{}", face.id);
    }
}

#[test]
fn opaque_records_are_reported() {
    let mut stream = cube_stream(true, [false; 6], [false; 6]);
    stream.record(0x0a, &[0; 12]);
    let (_, losses, body) = emit_stream(stream);
    assert_eq!(body.len(), 1);
    assert!(losses
        .iter()
        .any(|note| note.code == PvzLossCode::ExactRecordUninterpreted.kind()));
}

#[test]
fn faces_without_loops_are_left_out() {
    let stream = cube_stream(true, [false; 6], [false; 6]);
    let (ir, losses, body) = emit_edited(stream, None, |model| {
        let mut face = model.faces[0].clone();
        face.loops.clear();
        model.faces.push(face);
    });
    assert_eq!(body.len(), 1);
    assert_eq!(ir.model.bodies[0].kind, BodyKind::Solid);
    assert_eq!((ir.model.faces.len(), ir.model.surfaces.len()), (6, 6));
    assert!(losses
        .iter()
        .any(|note| note.code == PvzLossCode::ExactFaceUnboundedOmitted.kind()));
    let mut lone = Stream::new(true);
    lone.plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let (ir, _, body) = emit_stream(lone);
    assert!(body.is_empty());
    assert!(ir.model.faces.is_empty());
}

#[test]
fn open_shells_form_a_sheet_body_beside_the_solid() {
    let stream = cube_stream(true, [false; 6], [false; 6]);
    let (ir, _, body) = emit_edited(stream, None, |model| {
        let mut face = model.faces[0].clone();
        let mut record = model.loops[face.loops[0]].clone();
        record.face = model.faces.len();
        record.coedges.truncate(1);
        record.coedges[0].edge = Some(model.edges.len());
        model.edges.push(model.edges[0].clone());
        face.loops = vec![model.loops.len()];
        model.loops.push(record);
        model.faces.push(face);
    });
    assert_eq!(body.len(), 2);
    let kinds: Vec<BodyKind> = ir.model.bodies.iter().map(|body| body.kind).collect();
    assert_eq!(kinds, [BodyKind::Solid, BodyKind::Sheet]);
    assert_eq!(ir.model.shells.len(), 2);
}

#[test]
fn stream_without_faces_writes_nothing() {
    let (ir, losses, body) = emit_stream(Stream::new(false));
    assert!(body.is_empty());
    assert!(losses.is_empty());
    assert!(ir.model.bodies.is_empty());
}

/// Line edges and line pcurves of a plane stream, written after its faces.
#[derive(Default)]
struct PlaneLoops {
    edges: Vec<([f64; 3], [f64; 3])>,
    pcurves: Vec<([f64; 2], [f64; 2])>,
}

impl PlaneLoops {
    /// A loop around the square `[low, low + size]²` of the plane `z = 0`,
    /// counter-clockwise unless `clockwise`, with loop `marker`.
    fn square(
        &mut self,
        stream: &mut Stream,
        low: [f64; 2],
        size: f64,
        marker: i32,
        clockwise: bool,
    ) {
        let mut corners = [
            low,
            [low[0] + size, low[1]],
            [low[0] + size, low[1] + size],
            [low[0], low[1] + size],
        ];
        if clockwise {
            corners.reverse();
        }
        stream.loop_record(marker);
        for position in 0..4 {
            let (a, b) = (corners[position], corners[(position + 1) % 4]);
            let direction = [b[0] - a[0], b[1] - a[1]];
            let edge = u32::try_from(self.edges.len()).unwrap();
            let pcurve = u32::try_from(self.pcurves.len()).unwrap();
            self.edges
                .push(([a[0], a[1], 0.0], [direction[0], direction[1], 0.0]));
            self.pcurves.push((a, direction));
            stream.coedge(edge, 1, pcurve, crate::test_support::exact::NONE);
        }
        stream.terminator();
    }

    fn finish(self, stream: &mut Stream) {
        for (origin, direction) in self.edges {
            stream.line_edge(origin, direction);
        }
        for (origin, direction) in self.pcurves {
            stream.line_pcurve(origin, direction);
        }
    }
}

/// One plane face with two outer squares and a hole in the second.
fn two_region_plane() -> Stream {
    let mut stream = Stream::new(true);
    stream.plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let mut loops = PlaneLoops::default();
    loops.square(&mut stream, [0.0, 0.0], 0.01, -2, false);
    loops.square(&mut stream, [0.02, 0.0], 0.01, -2, false);
    loops.square(&mut stream, [0.0225, 0.0025], 0.005, 0, true);
    loops.finish(&mut stream);
    stream
}

#[test]
fn inner_loop_joins_the_outer_loop_that_contains_it() {
    let (ir, losses, _) = emit_stream(two_region_plane());
    assert_eq!(ir.model.faces.len(), 2);
    let FaceLoops::Classified { inner, .. } = &ir.model.faces[1].loops else {
        panic!("classified loops");
    };
    assert_eq!(inner.len(), 1);
    assert!(!losses
        .iter()
        .any(|note| note.code == PvzLossCode::ExactTopologyWorkExhausted.kind()));
}

#[test]
fn exhausted_nesting_is_reported() {
    let (ir, losses, _) = emit_stream_with(two_region_plane(), Some(1));
    assert_eq!(ir.model.faces.len(), 1);
    assert!(losses
        .iter()
        .any(|note| note.code == PvzLossCode::ExactTopologyWorkExhausted.kind()));
}

/// Warnings and worse of the neutral validation of `ir`.
fn serious_findings(ir: &CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
    cadmpeg_ir::validate::validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.severity >= Severity::Warning)
        .collect()
}

#[test]
fn edges_stored_once_per_use_become_shared_edges() {
    for reverse in [false, true] {
        let (ir, losses, _) =
            emit_edited(cube_stream(true, [false; 6], [false; 6]), None, |model| {
                crate::test_support::exact::split_shared_edges(model, reverse);
            });
        let model = &ir.model;
        assert_eq!(model.bodies[0].kind, BodyKind::Solid);
        assert_eq!(model.shells.len(), 1);
        assert_eq!((model.edges.len(), model.vertices.len()), (12, 8));
        assert!(model.edges.iter().all(|edge| edge.tolerance.is_some()));
        let findings = serious_findings(&ir);
        assert!(findings.is_empty(), "{findings:?}");
        assert!(losses
            .iter()
            .any(|note| note.code == PvzLossCode::ExactEdgeUsesMerged.kind()));
    }
}
