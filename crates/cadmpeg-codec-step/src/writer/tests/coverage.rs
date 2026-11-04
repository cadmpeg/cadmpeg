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
fn drawing_revision_can_precede_its_definition() {
    let source = crate::test_support::exchange::decode_inline(
        "#2=DRAWING_REVISION('A',#1,'rev');\n#1=DRAWING_DEFINITION('Main','detail');",
    );
    let plan = StepCodec::default()
        .plan(EncodeInput::new(source.ir(), None), TargetRequest::Inherit)
        .expect("forward drawing reference can be written");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write STEP");
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode drawing records");
    let revision = decoded
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAWING_REVISION")
        .expect("revision");
    let definition = decoded
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAWING_DEFINITION")
        .expect("definition");
    assert_eq!(revision.order, 0);
    assert_eq!(definition.order, 1);
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
fn unbound_appearance_survives_on_written_surface() {
    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let mut ir = unit_cube().expect("cube");
    let color = Color::new(0.125, 0.25, 0.5, 1.0).expect("color");
    ir.model.appearances.push(Appearance {
        id: AppearanceId::mint("test:model:appearance#unbound").expect("identity"),
        name: Some("material".into()),
        base_color: Some(color),
        native_ref: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        properties: Default::default(),
        textures: Vec::new(),
    });
    let mut other = ir.model.appearances[0].clone();
    other.id = AppearanceId::mint("test:model:appearance#other").expect("identity");
    other.name = Some("other material".into());
    other.base_color = Some(Color::new(0.75, 0.25, 0.125, 1.0).expect("color"));
    ir.model.appearances.push(other);
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("write appearance");
    assert!(plan.report().losses.iter().all(|loss| {
        loss.code != crate::loss::StepLossCode::AppearanceReducedToBaseColor.kind()
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write STEP");
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode STEP");
    assert!(decoded.ir().model.appearances.iter().any(|appearance| {
        appearance.name.as_deref() == Some("material") && appearance.base_color == Some(color)
    }));
    assert!(decoded.ir().model.appearances.iter().any(|appearance| {
        appearance.name.as_deref() == Some("other material")
            && appearance.base_color == Color::new(0.75, 0.25, 0.125, 1.0)
    }));

    ir.model.appearances[0].category = Some("unrepresented category".into());
    let reduced = StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("write reduced appearance");
    assert!(reduced.report().losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::AppearanceReducedToBaseColor.kind()
            && loss.message.contains("test:model:appearance#unbound")
    }));

    let mut point_ir = cadmpeg_ir::CadIr::empty();
    point_ir.model.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint("test:model:point#one").expect("identity"),
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
            .expect("point"),
        None,
    ));
    let mut point_appearance = ir.model.appearances[0].clone();
    point_appearance.category = None;
    point_appearance.base_color = Some(Color::new(0.125, 0.25, 0.5, 0.5).expect("color"));
    point_ir.model.appearances.push(point_appearance);
    let point_plan = StepCodec::default()
        .plan(EncodeInput::new(&point_ir, None), TargetRequest::Inherit)
        .expect("write point appearance");
    assert!(point_plan.report().losses.iter().any(|loss| {
        loss.code == crate::loss::StepLossCode::AppearanceReducedToBaseColor.kind()
            && loss.message.contains("test:model:appearance#unbound")
    }));
}

#[test]
fn native_drawing_targets_without_geometry_are_reported() {
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
        .expect("write surviving geometry");
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "step",
            "drawing_targets",
            "step:test:drawing-target#0"
        ),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    for id in ["step:test:drawing-target#0", "step:test:drawing#0"] {
        assert!(plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(id)));
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write STEP");
    let text = std::str::from_utf8(&bytes).expect("Part 21 text");
    assert!(!text.contains("ANNOTATION_PLANE("));
    assert!(!text.contains("DRAUGHTING_MODEL("));
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode surviving geometry");
    assert!(decoded.ir().model.drawings.is_empty());
}

#[test]
fn drawing_context_is_written_from_document_units() {
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ap242_presentation_pmi.p21"
    ));
    let source = StepCodec::default()
        .decode(&mut std::io::Cursor::new(input), &DecodeOptions::default())
        .expect("decode presentation");
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(source.ir(), None),
            TargetRequest::Explicit("step:ap242-e3"),
        )
        .expect("write presentation");
    let context = "step:data:geometric_representation_context_global_unit_assigned_context_representation_context#2";
    assert_eq!(
        plan.report()
            .native_dispositions
            .record_disposition("step", "drawing_targets", context),
        cadmpeg_ir::codec::write::NativeDisposition::Written,
        "{:?}",
        plan.report().losses
    );
    let before = serde_json::to_value(source.ir()).expect("source JSON");
    let report = serde_json::to_value(plan.report()).expect("report JSON");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write STEP");
    let decoded = StepCodec::default()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written presentation");
    let drawing = decoded
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.runtime_type == "DRAUGHTING_MODEL")
        .expect("drawing model");
    assert_eq!(drawing.relationships["presentation_context"].len(), 1);
    assert_eq!(drawing.relationships["items"].len(), 1);
    let after = serde_json::to_value(decoded.ir()).expect("output JSON");
    let allowances = cadmpeg_test_support::comparison::Allowances::export(&before, &report)
        .expect("export allowances");
    cadmpeg_test_support::comparison::agree(
        &before,
        &after,
        &allowances,
        cadmpeg_test_support::comparison::Mode::Same,
    )
    .expect("same-format document content survives");
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
    for arena in &registry {
        assert_eq!(
            plan.report().native_dispositions.disposition("step", arena),
            cadmpeg_ir::codec::write::NativeDisposition::Dropped,
            "an empty step.{arena} keeps the dropped default"
        );
    }
}
