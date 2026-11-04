// SPDX-License-Identifier: Apache-2.0
//! Coverage distinguishes complete geometry from omitted typed native content.

use cadmpeg_ir::codec::write::{target::TargetRequest, ArenaDisposition, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::schema::EntityKind;

use crate::StepCodec;

#[test]
fn drawing_definition_retains_name_and_description() {
    let source =
        crate::test_support::exchange::decode_inline("#1=DRAWING_DEFINITION('Main','detail');");
    let plan = StepCodec::default()
        .plan(EncodeInput::new(source.ir(), None), TargetRequest::Inherit)
        .expect("drawing definition can be written");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write STEP");
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode drawing definition");
    let drawing = decoded
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAWING_DEFINITION")
        .expect("drawing definition survives");
    assert_eq!(drawing.parameters["name"], "Main");
    assert_eq!(drawing.parameters["description"], "detail");
}

#[test]
fn drawing_revision_retains_its_definition() {
    let source = crate::test_support::exchange::decode_inline(
        "#1=DRAWING_DEFINITION('Main','detail');\n#2=DRAWING_REVISION('A',#1,'rev');",
    );
    let plan = StepCodec::default()
        .plan(EncodeInput::new(source.ir(), None), TargetRequest::Inherit)
        .expect("drawing revision can be written");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write STEP");
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode drawing revision");
    let definition = decoded
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAWING_DEFINITION")
        .expect("drawing definition survives");
    let revision = decoded
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAWING_REVISION")
        .expect("drawing revision survives");
    assert_eq!(revision.parameters["name"], "A");
    assert_eq!(revision.parameters["description"], "rev");
    assert_eq!(
        revision.relationships["drawing"][0].local_target(),
        Some(definition.id.as_str())
    );
}

#[test]
fn drawing_definition_and_revision_keep_absent_optional_fields() {
    let source = crate::test_support::exchange::decode_inline(
        "#1=DRAWING_DEFINITION('Main',$);\n#2=DRAWING_REVISION('A',#1,$);",
    );
    let mut ir = source.ir().clone();
    for drawing in &mut ir.model.drawings {
        drawing.parameters.remove("description");
    }
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("optional drawing fields can be absent");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write STEP");
    let text = std::str::from_utf8(&bytes).expect("STEP text");
    assert!(text.contains("DRAWING_DEFINITION('Main',$)"));
    assert!(text
        .lines()
        .any(|line| line.contains("DRAWING_REVISION('A',") && line.ends_with(",$);")));
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode drawing records");
    for kind in ["DRAWING_DEFINITION", "DRAWING_REVISION"] {
        let drawing = decoded
            .ir()
            .model
            .drawings
            .iter()
            .find(|drawing| drawing.runtime_type == kind)
            .expect("drawing survives");
        assert_eq!(drawing.parameters["description"], "$");
    }
}

#[test]
fn drawing_sheet_retains_its_revision_and_item() {
    use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind};
    use cadmpeg_ir::references::{ReferenceSelection, ReferenceTarget};

    let mut source = unit_cube().expect("cube");
    let point = source.model.points[0].id.as_str().to_owned();
    let mut sheet = Drawing::new(
        DrawingId::mint("step:test:drawing#sheet").expect("identity"),
        "step:test:drawing#sheet".to_owned(),
        DrawingKind::Page,
        "DRAWING_SHEET_REVISION".to_owned(),
        0,
        None,
    );
    sheet.parameters.insert(
        cadmpeg_core::nonblank_literal!("source_type"),
        "DRAWING_SHEET_REVISION".to_owned(),
    );
    sheet
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("name"), "Sheet".to_owned());
    sheet
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("revision"), "A".to_owned());
    sheet.parameters.insert(
        cadmpeg_core::nonblank_literal!("custom"),
        "unrepresented value".to_owned(),
    );
    sheet.relationships.insert(
        cadmpeg_core::nonblank_literal!("items"),
        vec![ReferenceSelection::new(
            ReferenceTarget::Local(point),
            Vec::new(),
        )],
    );
    source.model.drawings.push(sheet);
    source.finalize();
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
        .expect("drawing sheet can be written");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::DrawingPresentationRecordsOmitted.kind()
            && loss.message.contains("step:test:drawing#sheet")
            && loss.message.contains("parameter custom")
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write STEP");
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode drawing sheet");
    let sheet = decoded
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAWING_SHEET_REVISION")
        .expect("drawing sheet survives");
    assert_eq!(sheet.parameters["name"], "Sheet");
    assert_eq!(sheet.parameters["revision"], "A");
    let target = sheet.relationships["items"][0]
        .local_target()
        .expect("item target");
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == target));
}

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
fn own_native_drawing_records_are_reemitted_and_reported() {
    use cadmpeg_ir::drawings::{Drawing, DrawingKind};
    use cadmpeg_ir::references::{ReferenceSelection, ReferenceTarget};

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let mut fields = serde_json::Map::new();
    fields.insert("source_id".into(), serde_json::json!("#100"));
    fields.insert("source_type".into(), serde_json::json!("ANNOTATION_PLANE"));
    let native_id = cadmpeg_ir::ids::Identity::new("step:test:drawing-target#0")
        .expect("valid native identity");
    ir.native.namespace_mut("step").arenas_mut().insert(
        "drawing_targets".into(),
        vec![cadmpeg_ir::NativeRecord::new(native_id, fields).expect("valid native record")],
    );
    let mut drawing = Drawing::new(
        cadmpeg_ir::drawings::DrawingId::mint("step:test:drawing#0")
            .expect("valid drawing identity"),
        "step:test:drawing#0".to_owned(),
        DrawingKind::Other,
        "DRAUGHTING_MODEL".to_owned(),
        0,
        None,
    );
    drawing.parameters.insert(
        cadmpeg_core::text::NonBlankString::new("source_type".to_owned()).expect("non-blank"),
        "DRAUGHTING_MODEL".to_owned(),
    );
    drawing.relationships.insert(
        cadmpeg_core::text::NonBlankString::new("items".to_owned()).expect("non-blank"),
        vec![ReferenceSelection::new(
            ReferenceTarget::Local("step:test:drawing-target#0".to_owned()),
            Vec::new(),
        )],
    );
    ir.model.drawings.push(drawing);
    ir.finalize();
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("drawing-target re-emission does not refuse supported geometry");
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "step",
            "drawing_targets",
            "step:test:drawing-target#0"
        ),
        cadmpeg_ir::codec::write::NativeDisposition::Written
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("drawing-target re-emission writes");
    let text = std::str::from_utf8(&bytes).expect("Part 21 text");
    assert!(
        text.contains("ANNOTATION_PLANE("),
        "the retained plane is re-emitted"
    );
    assert!(
        text.contains("DRAUGHTING_MODEL("),
        "the neutral drawing is written"
    );
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("structurally valid re-emission decodes");
    assert_eq!(
        decoded.ir().model.drawings.len(),
        1,
        "the re-decode holds the drawing"
    );
    let targets = decoded
        .ir()
        .native
        .namespace("step")
        .expect("step namespace is retained")
        .arenas()
        .get("drawing_targets")
        .cloned()
        .unwrap_or_default();
    assert!(
        targets
            .iter()
            .any(|record| record.fields().is_ok_and(|fields| fields
                .get("source_type")
                .and_then(|source| source.as_str())
                == Some("ANNOTATION_PLANE"))),
        "the re-decode holds the re-emitted plane target: {:?}",
        targets.iter().map(|record| record.id()).collect::<Vec<_>>()
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

#[test]
fn step_declares_no_reduced_or_capacity_fields() {
    assert!(crate::writer::REDUCED_FIELDS.is_empty());
    assert!(crate::writer::CAPACITY_FIELDS.is_empty());
}

#[test]
fn native_registry_equals_dispositions() {
    let ir = unit_cube().expect("unit cube fixture is admitted");
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("supported geometry writes");
    let declared: std::collections::BTreeSet<_> = plan
        .report()
        .native_dispositions
        .namespaces
        .get("step")
        .expect("step namespace is declared")
        .arenas
        .keys()
        .cloned()
        .collect();
    let registry: std::collections::BTreeSet<_> = crate::reader::NATIVE_ARENAS
        .iter()
        .map(|arena| (*arena).to_owned())
        .collect();
    assert_eq!(declared, registry);
    // The unit cube retains no native records, so every arena keeps the
    // namespace default. Per-record upgrades (written drawing targets) are
    // covered by `own_native_drawing_records_are_reemitted_and_reported`.
    for arena in &registry {
        assert_eq!(
            plan.report().native_dispositions.disposition("step", arena),
            cadmpeg_ir::codec::write::NativeDisposition::Dropped,
            "an empty step.{arena} keeps the dropped default"
        );
    }
}
