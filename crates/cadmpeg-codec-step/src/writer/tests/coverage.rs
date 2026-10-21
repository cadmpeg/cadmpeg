// SPDX-License-Identifier: Apache-2.0
//! Coverage distinguishes complete geometry from omitted typed native content.

use cadmpeg_ir::codec::write::{target::TargetRequest, ArenaDisposition, EncodeInput, Encoder};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::schema::EntityKind;

use crate::StepCodec;

#[test]
fn complete_geometry_retains_written_coverage() {
    let ir = unit_cube().expect("unit cube fixture is admitted");
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("supported geometry writes");
    for arena in [
        EntityKind::Body,
        EntityKind::Region,
        EntityKind::Shell,
        EntityKind::Face,
        EntityKind::Loop,
        EntityKind::Coedge,
        EntityKind::Edge,
        EntityKind::Vertex,
        EntityKind::Point,
        EntityKind::Surface,
        EntityKind::Curve,
    ] {
        assert_eq!(
            plan.report().coverage.disposition(arena),
            ArenaDisposition::Written,
            "{arena:?}: {:?}",
            plan.report().losses
        );
    }
}

#[test]
fn own_native_drawing_records_are_omitted_and_reported() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let mut fields = serde_json::Map::new();
    fields.insert("source_id".into(), serde_json::json!("#100"));
    fields.insert("source_type".into(), serde_json::json!("DRAUGHTING_MODEL"));
    ir.native.namespace_mut("step").arenas_mut().insert(
        "drawing_targets".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new("step:test:drawing-target#0")
                .expect("valid native identity"),
            fields,
        )
        .expect("valid native record")],
    );
    ir.finalize();
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("native omission does not refuse supported geometry");
    assert_eq!(
        plan.report()
            .native_dispositions
            .disposition("step", "drawing_targets"),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.local_code() == "native.records-omitted"
            && loss.severity == cadmpeg_ir::report::Severity::Info
            && loss.message == "the step write omits 1 native record(s) from `step.drawing_targets`"
    }));
    assert_eq!(
        plan.report().coverage.disposition(EntityKind::Surface),
        ArenaDisposition::Written
    );
}

#[test]
fn omitted_solid_face_keeps_remaining_geometry_as_an_open_surface_model() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::report::Severity;
    use cadmpeg_ir::topology::BodyKind;
    use std::io::Cursor;

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let codec = StepCodec::default();
    let plan = codec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("missing carrier reduces its face");
    assert_eq!(
        plan.report().coverage.disposition(EntityKind::Body),
        ArenaDisposition::Reported
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Error
            && loss.code == crate::loss::StepLossCode::UnknownSurfaceFaceOmitted.kind()));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("surface model writes");
    let text = std::str::from_utf8(&bytes).expect("Part 21 text");
    assert!(text.contains("OPEN_SHELL("));
    assert!(
        !text.contains("MANIFOLD_SOLID_BREP("),
        "an open shell is never declared a solid"
    );
    let decoded = codec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("structurally valid partial output decodes");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len() - 1);
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("neutral validation")
        .is_ok());
}
