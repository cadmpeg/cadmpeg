// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::{BodyKind, Sense};

use cadmpeg_ir::ids::BodyId;

use super::{emit, Sink};
use crate::loss::PvzLossCode;
use crate::test_support::exact::{cube_stream, Stream, CUBE_SIZE};

fn emit_stream(stream: Stream) -> (CadIr, Vec<LossNote>, Option<BodyId>) {
    let id = stream.id();
    let bytes = stream.finish();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let model = crate::brep::parse(&ctx, id, root).unwrap();
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let mut losses = Vec::new();
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("part.ol"));
    let scope = IdentityKey::encode_segment("part.ol");
    let body = emit(
        &ctx,
        &model,
        &scope,
        &mut Sink {
            model: &mut ir.model,
            annotations: &mut annotations,
            losses: &mut losses,
            stream: &stream,
        },
    )
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
    assert_eq!(model.bodies[0].id, body.unwrap());
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
    let mut stream = Stream::new(true);
    stream.plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    stream.record(0x0a, &[0; 12]);
    let (_, losses, body) = emit_stream(stream);
    assert!(body.is_some());
    assert!(losses
        .iter()
        .any(|note| note.code == PvzLossCode::ExactRecordUninterpreted.kind()));
}

#[test]
fn stream_without_faces_writes_nothing() {
    let (ir, losses, body) = emit_stream(Stream::new(false));
    assert!(body.is_none());
    assert!(losses.is_empty());
    assert!(ir.model.bodies.is_empty());
}
