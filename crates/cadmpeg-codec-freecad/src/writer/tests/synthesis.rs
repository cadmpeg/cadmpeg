// SPDX-License-Identifier: Apache-2.0
//! Content authored from CADIR never depends on retained archive bytes.

use crate::test_support::test_archive::{CORE_DESIGN_PRODUCT, GEOMETRY};
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn synthesized(source: &[u8], target: &str) -> cadmpeg_ir::codec::DecodeResult {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(target),
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, decoded.ir(), output.ir());
    crate::test_support::test_archive::assert_valid_document(output.ir());
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
    output
}

#[test]
fn triangulated_face_survives_synthesis() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects><ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 3\nFa 0 0.001 0 0 2 1 1001000 *\nSh 1001000 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.faces.len(), 1);
    assert_eq!(input.ir().model.tessellations.len(), 1);
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.faces.len(), 1);
    assert_eq!(output.ir().model.tessellations.len(), 1);
    assert_eq!(
        output.ir().model.tessellations[0].vertices(),
        input.ir().model.tessellations[0].vertices()
    );
    assert_eq!(
        output.ir().model.tessellations[0].triangles(),
        input.ir().model.tessellations[0].triangles()
    );
    assert_eq!(
        output.ir().model.tessellations[0].chordal_deflection(),
        input.ir().model.tessellations[0].chordal_deflection()
    );
    assert_eq!(
        output.ir().model.tessellations[0].faces,
        vec![output.ir().model.faces[0].id.clone()]
    );
}

#[test]
fn triangulated_face_with_polygon_edge_survives_synthesis() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects><ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 1\n2 1 2 p 0.01 1 0 1\nSurfaces 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 7\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 6 1 1 0 0 1001000 +7 0 -6 0 *\nWi 1001000 +5 0 *\nFa 0 0.001 0 0 2 1 1001000 +4 0 *\nSh 1001000 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        (
            input.ir().model.faces.len(),
            input.ir().model.edges.len(),
            input.ir().model.tessellations.len()
        ),
        (1, 1, 1)
    );
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        (
            output.ir().model.faces.len(),
            output.ir().model.edges.len(),
            output.ir().model.tessellations.len()
        ),
        (1, 1, 1)
    );
    assert_eq!(
        output.ir().model.tessellations[0].triangles(),
        input.ir().model.tessellations[0].triangles()
    );
}

#[test]
fn unowned_triangulation_survives_synthesis() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects><ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 0\n*";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.tessellations.len(), 1);
    assert!(input.ir().model.tessellations[0].faces.is_empty());
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.tessellations.len(), 1);
    assert!(output.ir().model.tessellations[0].faces.is_empty());
    assert_eq!(
        output.ir().model.tessellations[0].vertices(),
        input.ir().model.tessellations[0].vertices()
    );
    assert_eq!(
        output.ir().model.tessellations[0].triangles(),
        input.ir().model.tessellations[0].triangles()
    );
}

#[test]
fn wire_body_with_only_a_vertex_redecodes() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 2\nVe 0.001 1 2 3 0 0 1001000 *\nCo 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        (
            input.ir().model.bodies.len(),
            input.ir().model.vertices.len()
        ),
        (1, 1)
    );
    let mut ir = input.ir().clone();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        (
            output.ir().model.bodies.len(),
            output.ir().model.vertices.len()
        ),
        (1, 1)
    );
}

#[test]
fn unsupported_face_does_not_remove_a_body_with_an_encodable_sibling() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 2\n1 0 0 0 0 0 1 1 0 0 0 1 0\n1 0 0 1 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 4\nFa 0 0.001 1 0 1001000 *\nFa 0 0.001 2 0 1001000 *\nSh 1001000 +4 0 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        (input.ir().model.bodies.len(), input.ir().model.faces.len()),
        (1, 2)
    );
    let mut ir = input.ir().clone();
    let lost_face = ir.model.faces[0].id.to_string();
    let surface = ir.model.faces[0].surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .unwrap()
        .geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
    );
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&lost_face)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        (
            output.ir().model.bodies.len(),
            output.ir().model.faces.len()
        ),
        (1, 1)
    );
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
}

#[test]
fn unsupported_face_does_not_remove_an_edge_shared_with_a_supported_face() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 2\n1 0 0 1 0\n1 0 0 1 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 2\n1 0 0 0 0 0 1 1 0 0 0 1 0\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 9\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 2 2 2 0 0 1 0 1001000 +9 0 -8 0 *\nWi 1001000 +7 0 *\nWi 1001000 -7 0 *\nFa 0 0.001 1 0 1001000 +6 0 *\nFa 0 0.001 2 0 1001000 +5 0 *\nSh 1001000 +4 0 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        (
            input.ir().model.bodies.len(),
            input.ir().model.faces.len(),
            input.ir().model.edges.len()
        ),
        (1, 2, 1)
    );
    let mut ir = input.ir().clone();
    let surface = ir.model.faces[0].surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .unwrap()
        .geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
    );
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        (
            output.ir().model.bodies.len(),
            output.ir().model.faces.len(),
            output.ir().model.edges.len()
        ),
        (1, 1, 1)
    );
}

#[test]
fn source_less_synthesis_keeps_neutral_brep_and_history() {
    let baseline = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let output = synthesized(CORE_DESIGN_PRODUCT, "fcstd:schema-4");
    let before = &baseline.ir().model;
    let after = &output.ir().model;
    assert_eq!(after.bodies.len(), before.bodies.len(), "body count");
    assert_eq!(after.faces.len(), before.faces.len(), "face count");
    assert_eq!(after.edges.len(), before.edges.len(), "edge count");
    assert_eq!(after.vertices.len(), before.vertices.len(), "vertex count");
    assert_eq!(after.features.len(), before.features.len(), "feature count");
    assert_eq!(
        after.sketch_entities.len(),
        before.sketch_entities.len(),
        "sketch geometry count"
    );
    assert_eq!(
        after.sketch_constraints.len(),
        before.sketch_constraints.len(),
        "constraint count"
    );
}

#[test]
fn source_less_tolerance_modes_keep_the_sixteen_authored_history_parameters() {
    use cadmpeg_ir::features::{FeatureDefinition, FuzzyTolerance};
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
    ));
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.parameters.len(), 16);
    for mode in [FuzzyTolerance::KernelDefault, FuzzyTolerance::Automatic] {
        let mut current = decoded.ir().clone();
        let mut changed = Vec::new();
        for feature in &mut current.model.features {
            if let FeatureDefinition::PostProcess {
                operation, refine, ..
            } = feature.definition()
            {
                let definition = FeatureDefinition::PostProcess {
                    operation: operation.clone(),
                    refine: *refine,
                    fuzzy_tolerance: mode.clone(),
                };
                feature.evaluation.set_definition(definition);
                changed.push(feature.id.clone());
            }
        }
        assert_eq!(changed.len(), 3);
        let plan = FcstdCodec
            .plan(EncodeInput::new(&current, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(output.ir().model.parameters.len(), 16);
        assert!(!output
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "FuzzyTolerance"));
        for id in &changed {
            let feature = output
                .ir()
                .model
                .features
                .iter()
                .find(|feature| &feature.id == id)
                .unwrap();
            assert!(
                matches!(feature.definition(), FeatureDefinition::PostProcess { fuzzy_tolerance, .. } if fuzzy_tolerance == &mode)
            );
        }
    }
}

#[test]
fn forbidden_xml_label_characters_reduce_and_report_instead_of_refusing() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.features[0].name = Some("Label\0\u{1f}\t\nwith literal &#xFFFD;".into());
    let body =
        super::super::synthesis::write(&cadmpeg_test_support::service_decode_context(), &ir, "4")
            .unwrap();
    assert_eq!(
        body.coverage
            .disposition(cadmpeg_ir::schema::EntityKind::Feature),
        cadmpeg_ir::codec::write::ArenaDisposition::Reported
    );
    let notes: Vec<_> = body
        .losses
        .iter()
        .filter(|loss| loss.message.contains("XML-forbidden"))
        .collect();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].message.contains("Property[Label]/String: 2"));
    assert_eq!(notes[0].code.local_code(), "arena.not-transferred");
    let output = FcstdCodec
        .decode(&mut Cursor::new(body.bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| {
        feature.name.as_deref() == Some("Label\u{fffd}\u{fffd}\t\nwith literal &#xFFFD;")
    }));
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
}

#[test]
fn every_catalog_schema_synthesizes_and_validates_a_dumb_solid() {
    let baseline = FcstdCodec
        .decode(&mut Cursor::new(GEOMETRY), &DecodeOptions::default())
        .unwrap();
    for target in ["fcstd:schema-2", "fcstd:schema-3", "fcstd:schema-4"] {
        let output = synthesized(GEOMETRY, target);
        assert_eq!(
            output.ir().model.bodies.len(),
            baseline.ir().model.bodies.len(),
            "{target}"
        );
        assert_eq!(
            output
                .ir()
                .source
                .as_ref()
                .unwrap()
                .dialect()
                .unwrap()
                .dialect()
                .as_str(),
            target
        );
    }
}

#[test]
fn effective_appearance_preserves_independent_body_style() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    // The independent style uses exactly representable channels: the native
    // `ShapeColor` home packs RGB to 8 bits and `Transparency` to an integer
    // percent, so the test asserts the home with values that survive both
    // exactly (#238 §5.5).
    let fallback =
        cadmpeg_ir::topology::Color::new(33.0 / 255.0, 69.0 / 255.0, 156.0 / 255.0, 0.39).unwrap();
    let effective = cadmpeg_ir::topology::Color::from_rgba8(48, 96, 144, 255);
    let mut ir = decoded.ir().clone();
    ir.model.bodies[0].color = Some(fallback);
    ir.model.bodies[0].visible = Some(false);
    let mut value = serde_json::to_value(&ir).unwrap();
    let body = value["model"]["bodies"][0]["id"].clone();
    // A bound appearance needs its full material facts to travel in
    // `ShapeMaterial`; a bare color has no valid material encoding and is
    // omitted with a loss instead (#238 §5.5).
    value["model"]["appearances"].as_array_mut().unwrap().push(serde_json::json!({"id":"test:model:appearance#bound", "name":"Authored display material", "base_color": effective, "properties":{"shininess": 32.0, "material_transparency": 0.0}}));
    value["model"]["appearance_bindings"].as_array_mut().unwrap().push(serde_json::json!({"id":"test:model:appearance-binding#bound", "target":{"kind":"body", "id":body}, "appearance":"test:model:appearance#bound"}));
    let ir = cadmpeg_ir::CadIr::from_json(&value.to_string()).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.color == Some(fallback) && body.visible == Some(false)));
    assert!(output
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.base_color == Some(effective)));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "appearance.not-transferred"));
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &ir, output.ir());
}

#[test]
fn deleting_neutral_product_and_gui_state_does_not_replay_native_records() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.product_definitions.clear();
    ir.model.occurrences.clear();
    ir.model.assembly_joints.clear();
    ir.model.view_presentations.clear();
    ir.model.presentation_documents.clear();
    ir.model.appearances.clear();
    ir.model.appearance_bindings.clear();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.product_definitions.is_empty());
    assert!(output.ir().model.occurrences.is_empty());
    assert!(output.ir().model.assembly_joints.is_empty());
    assert!(output
        .ir()
        .model
        .presentation_documents
        .iter()
        .all(|document| document.states().is_empty()));
    assert_eq!(output.ir().model.bodies.len(), ir.model.bodies.len());
}

#[test]
fn standalone_authored_product_preserves_absent_label_and_has_no_invented_occurrence() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.product_definitions.push(serde_json::from_value(serde_json::json!({"id":"authored:model:product_definition#Prototype","kind":"part","native_ref":"fcstd:native:object#Prototype"})).unwrap());
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let _report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.product_definitions.len(), 1);
    // A `None` label is written as the object Name and a missing source name
    // as the object name; both re-decode as `Some`, an addition (#238 §5.5).
    assert_eq!(
        output.ir().model.product_definitions[0].source_name,
        Some("Product0".into())
    );
    assert_eq!(
        output.ir().model.product_definitions[0].label,
        Some("Product0".into())
    );
    assert!(output.ir().model.occurrences.is_empty());
    // No strict arena agreement: the `None`-to-`Some` label and source name
    // are the documented addition, so the re-decoded definition differs by
    // design (#238 §5.5). The assertions above cover the whole authored
    // document: one definition with its native homes and no occurrence.
}

#[test]
fn solved_profile_directions_and_parameter_attribution_survive_synthesis() {
    synthesized(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
        )),
        "fcstd:schema-4",
    );
}

#[test]
fn native_feature_dependency_edits_replace_stored_object_dependencies() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="3" Dependencies="1">
<ObjectDeps Name="A" Count="0"/>
<ObjectDeps Name="B" Count="1"><Dep Name="A"/></ObjectDeps>
<ObjectDeps Name="C" Count="0"/>
<Object type="Vendor::Node" name="A"/>
<Object type="Vendor::Node" name="B"/>
<Object type="Vendor::Node" name="C"/>
</Objects><ObjectData Count="3">
<Object name="A"><Properties Count="0"/></Object>
<Object name="B"><Properties Count="0"/></Object>
<Object name="C"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let input = crate::test_support::test_archive::archive(document);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    let feature_id = |name: &str| {
        decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
            .id
            .clone()
    };
    let original = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id == feature_id("B"))
        .unwrap();
    assert_eq!(original.dependencies.as_slice(), &[feature_id("A")]);
    for (owner, dependencies) in [("B", vec![]), ("B", vec!["C"]), ("A", vec!["C"])] {
        let mut current = decoded.ir().clone();
        let feature = current
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == feature_id(owner))
            .unwrap();
        assert!(
            matches!(feature.definition().operation(), cadmpeg_ir::features::FeatureOperation::Native { kind, .. } if kind.as_str() == "Vendor::Node")
        );
        feature.dependencies = dependencies
            .iter()
            .map(|name| feature_id(name))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let native_before = current.native.clone();
        let plan = FcstdCodec
            .plan(EncodeInput::new(&current, None), TargetRequest::Inherit)
            .unwrap();
        assert!(matches!(
            plan.report().write_path(),
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let objects = output
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::ObjectRecord>("objects")
            .unwrap();
        let object = objects
            .iter()
            .find(|object| object.name() == owner)
            .unwrap();
        assert_eq!(
            object.dependencies,
            dependencies
                .iter()
                .map(|name| crate::native::native_id("object", name))
                .collect::<Vec<_>>()
        );
        let feature = output
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(owner))
            .unwrap();
        let expected = dependencies
            .iter()
            .map(|name| {
                output
                    .ir()
                    .model
                    .features
                    .iter()
                    .find(|feature| feature.name.as_deref() == Some(name))
                    .unwrap()
                    .id
                    .clone()
            })
            .collect::<Vec<_>>();
        assert_eq!(feature.dependencies.as_slice(), expected);
        assert_eq!(current.native, native_before);
        assert!(crate::test_support::validate_native(output.ir()).is_empty());
        crate::test_support::test_archive::assert_valid_document(output.ir());
    }
}

#[test]
fn omitted_unreadable_feature_cannot_leave_a_dangling_dependency() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="Bad" Count="0"/><ObjectDeps Name="Pocket" Count="1"><Dep Name="Bad"/></ObjectDeps><Object type="Part::Box" name="Bad"/><Object type="PartDesign::Pocket" name="Pocket"/></Objects><ObjectData Count="2"><Object name="Bad"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="NaN"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object><Object name="Pocket"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let input = crate::test_support::test_archive::archive_entries(&[(
        "Document.xml",
        document.as_bytes(),
    )]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    let before_namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let before_objects = before_namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let before_bad = before_objects
        .iter()
        .find(|object| object.name() == "Bad")
        .unwrap();
    let before_properties = before_namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let length = before_properties
        .iter()
        .find(|property| property.owner == before_bad.id().as_str() && property.name == "Length")
        .unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(plan.report().losses.iter().any(|loss| loss.code
        == crate::loss::FreecadLossCode::ExportNativePropertyNotTransferred
            .note("")
            .code
        && loss.message.contains(&length.id)
        && loss.message.contains(&length.owner)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let objects = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let bad = objects
        .iter()
        .find(|object| object.name() == "Bad")
        .unwrap();
    assert_eq!(bad.type_name, "Part::Box");
    let pocket = objects
        .iter()
        .find(|object| object.name() == "Pocket")
        .unwrap();
    assert_eq!(pocket.type_name, "PartDesign::Pocket");
    assert_eq!(pocket.dependencies, [bad.id().as_str().to_owned()]);
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(!properties
        .iter()
        .any(|property| { property.owner == bad.id().as_str() && property.name == "Length" }));
    for name in ["Width", "Height"] {
        let property = properties
            .iter()
            .find(|property| property.owner == bad.id().as_str() && property.name == name)
            .unwrap();
        assert_eq!(property.type_name, "App::PropertyLength");
        assert_eq!(
            property.values()[0].attributes["value"]
                .parse::<f64>()
                .unwrap(),
            1.0
        );
    }
    assert!(objects
        .iter()
        .all(|object| object.dependencies.iter().all(|dependency| objects
            .iter()
            .any(|target| target.id().as_str() == dependency))));
}

#[test]
fn independent_linked_prototype_placement_is_baked_and_retains_its_decomposition() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let occurrence = ir
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.linked_prototype.is_some())
        .unwrap();
    let name = occurrence.name.clone();
    occurrence.linked_prototype = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 17.0],
            [0.0, 1.0, 0.0, 23.0],
            [0.0, 0.0, 1.0, 31.0],
        ])
        .unwrap(),
    );
    let expected = occurrence.clone();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let actual = output
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name == name)
        .unwrap();
    // The edited linked placement conflicts with the prototype's own
    // placement, so the effective placement is baked with `LinkTransform`
    // false and a fold loss; the neutral decomposition is not retained
    // (#238 §5.5).
    assert_eq!(
        actual.transform,
        Some(
            expected
                .effective_transform()
                .unwrap()
                .expect("authored placement")
        )
    );
    assert_eq!(actual.linked_prototype, None);
    assert!(report.losses.iter().any(|loss| loss.code
        == crate::loss::FreecadLossCode::ProductLinkedPlacementFolded
            .note("")
            .code));
    let native = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
        .unwrap();
    let native = native
        .iter()
        .find(|record| Some(record.object.as_str()) == actual.native_ref.as_deref())
        .unwrap();
    assert_eq!(native.link_transform(), Some(false));
    assert_eq!(
        native.local_transform().unwrap().transform(),
        expected
            .effective_transform()
            .unwrap()
            .expect("authored placement")
    );
    // No strict arena agreement: the folded occurrence is the documented
    // normalization (baked effective placement, cleared prototype link), so
    // the re-decoded occurrence differs by design (#238 §5.5). The
    // assertions above cover the fold: neutral baking, the fold loss, and
    // the native `LinkTransform=false` with the effective placement.
}

#[test]
fn exact_editor_feature_expression_constraint_and_appearance_edits_survive() {
    use serde_json::json;
    for (source_index, source) in [
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
        ))
        .as_slice(),
        CORE_DESIGN_PRODUCT,
    ]
    .into_iter()
    .enumerate()
    {
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        if source_index == 0 {
            assert_eq!(decoded.ir().model.parameters.len(), 16);
        }
        let mut value = serde_json::to_value(decoded.ir()).unwrap();
        let features = value["model"]["features"].as_array_mut().unwrap();
        features
            .iter_mut()
            .find(|feature| {
                feature["id"]
                    .as_str()
                    .is_some_and(|id| id.ends_with("#Pad") || id.ends_with("#DesignedFeature"))
            })
            .unwrap()["name"] = json!("AuthoredPad");
        let parameters = value["model"]["parameters"].as_array_mut().unwrap();
        parameters
            .iter_mut()
            .find(|parameter| {
                parameter["id"].as_str().is_some_and(|id| {
                    id.ends_with("#Pad:Length") || id.ends_with("#DesignedFeature:Length")
                })
            })
            .unwrap()["expression"] = json!(cadmpeg_ir::features::Expression::text("37 mm"));
        parameters
            .iter_mut()
            .find(|parameter| {
                parameter["id"].as_str().is_some_and(|id| {
                    id.ends_with("#Pad:Length") || id.ends_with("#DesignedFeature:Length")
                })
            })
            .unwrap()["value"] = json!({"kind":"length","value":37.0});
        let dimensional = parameters
            .iter_mut()
            .find(|parameter| {
                parameter["id"]
                    .as_str()
                    .is_some_and(|id| id.contains(":constraint:"))
            })
            .unwrap();
        dimensional["expression"] = json!(cadmpeg_ir::features::Expression::text("31 mm"));
        dimensional["value"] = json!({"kind":"length","value":31.0});
        let body = value["model"]["bodies"][0]["id"].clone();
        let color = cadmpeg_ir::topology::Color::from_rgba8(48, 96, 144, 255);
        if let Some(appearance) = value["model"]["appearances"]
            .as_array_mut()
            .unwrap()
            .first_mut()
        {
            appearance["base_color"] = json!(color);
        } else {
            // A bound appearance needs its full material facts to travel in
            // `ShapeMaterial`; a bare color has no valid material encoding
            // and normalizes to an independent body style instead (#238 §5.5).
            value["model"]["appearances"].as_array_mut().unwrap().push(json!({"id":"cadir:model:appearance#authored","name":"Authored appearance","base_color":color,"properties":{"shininess":32.0,"material_transparency":0.0}}));
            value["model"]["appearance_bindings"].as_array_mut().unwrap().push(json!({"id":"cadir:model:appearance-binding#authored","target":{"kind":"body","id":body},"appearance":"cadir:model:appearance#authored"}));
        }
        let mut edited = cadmpeg_ir::CadIr::from_json(&value.to_string()).unwrap();
        let changed = edited
            .model
            .parameters
            .iter()
            .filter(|parameter| {
                matches!(
                    parameter.expression.literal_text().as_deref(),
                    Some("37 mm" | "31 mm")
                )
            })
            .map(|parameter| {
                (
                    parameter.id.clone(),
                    parameter.expression.clone(),
                    parameter.value.clone(),
                )
            })
            .collect::<Vec<_>>();
        for (id, expression, value) in changed {
            edited
                .set_parameter_expression(&id, expression, value)
                .unwrap();
        }
        let plan = FcstdCodec
            .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
            .unwrap();
        let report = plan.report().clone();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        if source_index == 0 {
            assert_eq!(output.ir().model.parameters.len(), 16);
            assert!(!output
                .ir()
                .model
                .parameters
                .iter()
                .any(|parameter| parameter.name == "FuzzyTolerance"));
        }
        for feature in &edited.model.features {
            if let cadmpeg_ir::features::FeatureDefinition::PostProcess {
                fuzzy_tolerance, ..
            } = feature.definition()
            {
                let written = output
                    .ir()
                    .model
                    .features
                    .iter()
                    .find(|written| written.id == feature.id)
                    .unwrap();
                assert!(
                    matches!(written.definition(), cadmpeg_ir::features::FeatureDefinition::PostProcess { fuzzy_tolerance: actual, .. } if actual == fuzzy_tolerance)
                );
            }
        }
        assert!(
            output.source_fidelity().unrepresented().is_empty(),
            "{:?}",
            output.report().losses
        );
        assert!(
            output.report().losses.is_empty(),
            "{:?}",
            output.report().losses
        );
        assert!(output
            .ir()
            .model
            .features
            .iter()
            .any(|feature| feature.name.as_deref() == Some("AuthoredPad")));
        assert!(output
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.expression.literal_text().as_deref() == Some("37 mm")));
        assert!(output.ir().model.parameters.iter().any(|parameter|parameter.expression.literal_text().as_deref()==Some("31 mm") && matches!(&parameter.value,Some(cadmpeg_ir::features::ParameterValue::Length(value)) if value.get()==31.0)));
        assert!(output
            .ir()
            .model
            .appearance_bindings
            .iter()
            .any(|binding| output
                .ir()
                .model
                .appearances
                .iter()
                .any(|appearance| appearance.id == binding.appearance
                    && appearance.base_color == Some(color))));
        for arena in [
            "features",
            "sketches",
            "sketch_entities",
            "sketch_constraints",
            "parameters",
            "bodies",
            "spreadsheets",
            "product_definitions",
            "occurrences",
        ] {
            assert_eq!(
                serde_json::to_value(&edited.model).unwrap()[arena]
                    .as_array()
                    .unwrap()
                    .len(),
                serde_json::to_value(&output.ir().model).unwrap()[arena]
                    .as_array()
                    .unwrap()
                    .len(),
                "{arena}"
            );
        }
        // The renamed feature shares its object with a product definition
        // whose label does not follow the rename: the feature name wins the
        // object Label with a documented loss, so the re-decoded definition
        // differs by design and the strict agreement below cannot hold
        // (#238 §5.5). The per-arena counts above and the targeted edit
        // assertions cover the surviving edits.
        let renamed = edited
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("AuthoredPad"))
            .unwrap();
        let shared = edited.model.product_definitions.iter().any(|definition| {
            definition.native_ref.is_some()
                && definition.native_ref == renamed.native_ref
                && definition.label.as_deref() != renamed.name.as_deref()
        });
        assert_eq!(
            report.losses.iter().any(|loss| loss.code
                == crate::loss::FreecadLossCode::ProductLabelNotTransferred
                    .note("")
                    .code),
            shared,
            "the label loss fires exactly when a shared definition diverges"
        );
        if !shared {
            cadmpeg_test_support::contract::assert_written_arenas_agree(
                &report,
                &edited,
                output.ir(),
            );
        }
    }
}

#[test]
fn product_metadata_does_not_replace_a_typed_vendor_construction_object() {
    // A faithful container/vendor pair: explicit placements keep both
    // occurrences placed and explicit labels keep both definitions named, so
    // the round trip conserves the document. Absent labels normalize to
    // object names (standalone coverage) and unplaced occurrences are
    // omitted with a loss (unknown-ancestor coverage); neither applies here
    // (#238 §5.5, #213).
    let document = r#"<Document SchemaVersion="4"><Objects Count="2"><Object name="Root" type="App::Part"/><Object name="Vendor" type="Vendor::PartDesign::PadLike"/></Objects><ObjectData Count="2"><Object name="Root"><Properties Count="3"><Property name="Label" type="App::PropertyString"><String value="Root"/></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Vendor"/></LinkList></Property></Properties></Object><Object name="Vendor"><Properties Count="3"><Property name="Label" type="App::PropertyString"><String value="Vendor"/></Property><Property name="Length" type="App::PropertyLength"><Float value="5"/></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object></ObjectData></Document>"#;
    let input = crate::test_support::test_archive::archive_entries(&[(
        "Document.xml",
        document.as_bytes(),
    )]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.source_fidelity().unrepresented().is_empty());
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let objects = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    assert!(objects
        .iter()
        .any(|object| object.type_name == "Vendor::PartDesign::PadLike"));
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(properties
        .iter()
        .all(|property| property.name != "ScaleVector"));
    let vendor = output
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Vendor"))
        .expect("vendor construction object keeps its neutral feature");
    assert!(
        matches!(
            vendor.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native { kind, .. }
            ) if kind.as_str() == "Vendor::PartDesign::PadLike"
        ),
        "the vendor construction object keeps its Native kind"
    );
    assert_eq!(output.ir().model.product_definitions.len(), 2);
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, decoded.ir(), output.ir());
}

#[test]
fn public_fixtures_synthesize_and_redecode_without_native_findings() {
    // Every public fixture synthesizes from CADIR and re-decodes with no
    // native finding: the writer writes FreeCAD's own state (#238).
    let dir = format!(
        "{}/../../corpus/freecad_fcstd/fixtures",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures dir")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "FCStd")
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "the fixture set is not empty");
    for path in &names {
        let source = std::fs::read(path).expect("fixture bytes");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .expect("fixture decodes");
        let plan = FcstdCodec
            .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
            .expect("fixture synthesizes");
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).expect("synthesis writes");
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("synthesis re-decodes");
        assert!(
            crate::test_support::validate_native(output.ir()).is_empty(),
            "{}: native findings on re-decode",
            path.display()
        );
    }
}
