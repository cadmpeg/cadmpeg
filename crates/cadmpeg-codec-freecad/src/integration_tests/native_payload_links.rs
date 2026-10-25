// SPDX-License-Identifier: Apache-2.0
//! Neutral body identities refer to the source shape payload they project.

use crate::test_support::test_archive::{CORE_DESIGN_PRODUCT, GEOMETRY};
use crate::FcstdCodec;
mod geometry_reads;
mod partial_write;
mod pcurve_reads;
mod polygons;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn every_shape_body_links_its_own_typed_shape_payload() {
    let binary = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
    ));
    for source in [GEOMETRY, CORE_DESIGN_PRODUCT, binary.as_slice()] {
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let payloads = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
            .unwrap();
        assert!(!decoded.ir().model.bodies.is_empty());
        for body in &decoded.ir().model.bodies {
            let native = body.native_ref.as_ref().unwrap();
            let payload = payloads
                .iter()
                .find(|payload| &payload.id == native)
                .unwrap();
            let prefix = crate::native::model_id("body", &payload.id, "");
            assert!(body.id.as_str().starts_with(&prefix));
        }
        crate::test_support::test_archive::assert_valid_document(decoded.ir());
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    }
}

#[test]
fn every_decoded_native_row_and_unlinked_neutral_record_has_its_actual_record_layout() {
    let gui = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
    ));
    let mut missing = Vec::new();
    for (name, bytes) in [
        ("geometry", GEOMETRY),
        ("core", CORE_DESIGN_PRODUCT),
        ("gui", gui.as_slice()),
    ] {
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let layouts = decoded.source_fidelity().record_layout();
        let mut native_ids = std::collections::BTreeSet::new();
        for (namespace, arenas) in &decoded.ir().native.0 {
            for (arena, rows) in arenas.arenas() {
                for row in rows {
                    native_ids.insert(row.id());
                    if !layouts.contains_key(row.id()) {
                        missing.push(format!("{name} native {namespace}/{arena} {}", row.id()));
                    }
                }
            }
        }
        decoded
            .ir()
            .visit_neutral_provenance(&mut |record| {
                match record.native_ref() {
                    Some(native) if !native_ids.contains(native) => missing.push(format!(
                        "{name} neutral {:?} {} has absent native ref {native}",
                        record.kind(),
                        record.id()
                    )),
                    None if !layouts.contains_key(record.id()) => missing.push(format!(
                        "{name} unlinked neutral {:?} {}",
                        record.kind(),
                        record.id()
                    )),
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
    }
    assert!(
        missing.is_empty(),
        "missing real producer layouts:\n{}",
        missing.join("\n")
    );
}

#[test]
fn actual_brep_row_links_and_envelope_references_survive_identity_rewriting_and_row_deletion() {
    use crate::brep::records::polygons::{Polygon3dRecord, PolygonOnTriangulationRecord};
    use cadmpeg_ir::schema::rewrite::identities;
    let ctx = cadmpeg_test_support::service_decode_context();
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(GEOMETRY), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let envelope = payloads
        .iter()
        .find(|payload| {
            crate::brep::Tables::from_payload(payload).is_some_and(|tables| {
                tables
                    .curves
                    .iter()
                    .any(|curve| matches!(curve, crate::brep::TextCurve::Line { .. }))
            })
        })
        .expect("fixture has a canonical payload containing a native line");
    let curve = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| {
            curve.native_ref.as_deref() == Some(envelope.id.as_str())
                && matches!(
                    curve.geometry.solved(),
                    Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Line(_))
                )
        })
        .expect("the native line has its actual neutral counterpart");
    let rename = |id: &str| {
        Ok(if id == envelope.id {
            "authored:geometry:payload#current".into()
        } else {
            id.to_owned()
        })
    };
    let rewritten_curve = identities(&ctx, curve.clone(), rename).unwrap();
    let rewritten_envelope = identities(&ctx, envelope.clone(), rename).unwrap();
    assert_eq!(
        rewritten_curve.native_ref.as_deref(),
        Some(rewritten_envelope.id.as_str())
    );
    assert_eq!(rewritten_envelope.id, "authored:geometry:payload#current");
    assert_eq!(rewritten_envelope.property, envelope.property);
    assert_eq!(rewritten_envelope.entry, envelope.entry);
    assert_eq!(rewritten_envelope.payload, envelope.payload);
    let mut renamed = cadmpeg_ir::native::NativeNamespace::default();
    let ids = envelope
        .polygon_references()
        .collect::<std::collections::BTreeSet<_>>();
    let polygons = namespace
        .arena_as::<Polygon3dRecord>("polygons3d")
        .unwrap()
        .into_iter()
        .filter(|row| ids.contains(row.id.as_str()))
        .map(|row| identities(&ctx, row, rename).unwrap())
        .collect::<Vec<_>>();
    renamed.set_arena(&ctx, "polygons3d", &polygons).unwrap();
    let indexed_polygons = namespace
        .arena_as::<PolygonOnTriangulationRecord>("polygons_on_triangulations")
        .unwrap()
        .into_iter()
        .filter(|row| ids.contains(row.id.as_str()))
        .map(|row| identities(&ctx, row, rename).unwrap())
        .collect::<Vec<_>>();
    renamed
        .set_arena(&ctx, "polygons_on_triangulations", &indexed_polygons)
        .unwrap();
    renamed
        .set_arena(&ctx, "shape_payloads", &[rewritten_envelope.clone()])
        .unwrap();
    let materialized = crate::brep::records::payloads_untracked(&renamed).unwrap();
    // Appendix C's canonical nonpolygon native home is the whole shape payload.
    assert_eq!(
        materialized[0].row_identity(2, 1),
        Some(rewritten_envelope.id.as_str())
    );
    let before = crate::brep::Tables::from_payload(&materialized[0])
        .unwrap()
        .curves
        .len();
    let tables = crate::brep::Tables::from_payload(&rewritten_envelope).unwrap();
    let origin = tables
        .curves
        .iter()
        .find_map(|curve| match curve {
            crate::brep::TextCurve::Line { origin, .. } => Some(origin),
            _ => None,
        })
        .expect("fixture has a native line");
    let old = origin.get();
    let changed = cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
        old.x + 0.25,
        old.y,
        old.z,
    ))
    .unwrap();
    let mut wire = serde_json::to_value(&rewritten_envelope).unwrap();
    let facts = if wire["text"].is_object() {
        &mut wire["text"]
    } else {
        &mut wire["binary"]
    };
    let line = facts["curves"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|curve| curve["kind"] == "line")
        .unwrap();
    line["origin"] = serde_json::to_value(changed).unwrap();
    let edited: crate::brep::ShapePayloadRecord = serde_json::from_value(wire).unwrap();
    renamed
        .set_arena(&ctx, "shape_payloads", &[edited])
        .unwrap();
    let after = crate::brep::records::payloads_untracked(&renamed).unwrap();
    assert_eq!(
        crate::brep::Tables::from_payload(&after[0])
            .unwrap()
            .curves
            .len(),
        before
    );
    assert!(crate::brep::Tables::from_payload(&after[0])
        .unwrap()
        .curves
        .iter()
        .any(|curve| matches!(curve, crate::brep::TextCurve::Line { origin, .. } if origin.get() == changed.get())));
    assert_ne!(after[0].payload, envelope.payload);
    renamed
        .set_arena(
            &ctx,
            "shape_payloads",
            &Vec::<crate::brep::ShapePayloadRecord>::new(),
        )
        .unwrap();
    assert!(crate::brep::records::payloads_untracked(&renamed)
        .unwrap()
        .is_empty());
    assert_eq!(
        renamed.arena_as::<Polygon3dRecord>("polygons3d").unwrap(),
        polygons
    );
    assert_eq!(
        renamed
            .arena_as::<PolygonOnTriangulationRecord>("polygons_on_triangulations")
            .unwrap(),
        indexed_polygons
    );
}

#[test]
fn source_less_polygon_deletion_keeps_source_census_and_writes_surviving_canonical_row() {
    use crate::brep::records::polygons::Polygon3dRecord;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let xml = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='Shape' id='1'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 2\n1 0 0 1 0 0\n1 0 0 2 0 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*\n";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml.as_slice()),
        ("Shape.brp", brep.as_slice()),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    let namespace = ir.native.namespace_mut("fcstd");
    let rows = namespace.arena_as::<Polygon3dRecord>("polygons3d").unwrap();
    assert_eq!(rows.len(), 2);
    let first_id = rows[0].id.clone();
    let kept = rows
        .into_iter()
        .filter(|row| row.id != first_id)
        .collect::<Vec<_>>();
    let census = namespace
        .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
        .unwrap();
    namespace.set_arena(&ctx, "polygons3d", &kept).unwrap();
    let findings = crate::test_support::validate_native(&ir);
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains(&first_id)));
    assert!(!findings
        .iter()
        .any(|finding| finding.message.contains("carrier census")));
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&first_id)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let output = reread
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<Polygon3dRecord>("polygons3d")
        .unwrap();
    assert_eq!(output.len(), 1);
    let polygon = &output[0].value;
    assert_eq!(
        polygon.nodes[0].get(),
        cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0)
    );
    assert_eq!(
        ir.native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
            .unwrap(),
        census
    );
}

#[test]
fn source_less_faceted_body_uses_current_face_mesh_and_does_not_restore_a_deleted_mesh() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let xml=b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='MeshShape' id='1'/></Objects><ObjectData Count='1'><Object name='MeshShape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let brep=b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 3\nFa 0 0.001 0 0 2 1 1001000 *\nSh 1001000 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml.as_slice()),
        ("Shape.brp", brep.as_slice()),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.tessellations.len(), 1);
    ir.model.tessellations[0]
        .edit_vertices(|point| {
            point.x += 42.0;
            Ok(())
        })
        .unwrap();
    let write = |ir: &cadmpeg_ir::CadIr| {
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(ir, None),
                TargetRequest::Explicit("fcstd:schema-4"),
            )
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap()
    };
    let current = write(&ir);
    assert_eq!(current.ir().model.bodies.len(), 1);
    assert_eq!(current.ir().model.tessellations.len(), 1);
    assert_eq!(
        current.ir().model.tessellations[0].vertices(),
        ir.model.tessellations[0].vertices()
    );
    assert_eq!(current.ir().model.tessellations[0].faces.len(), 1);
    assert_eq!(
        current.ir().model.tessellations[0].faces[0],
        current.ir().model.faces[0].id
    );
    crate::test_support::test_archive::assert_valid_document(current.ir());
    ir.model.tessellations.clear();
    let deleted = write(&ir);
    assert!(deleted.ir().model.tessellations.is_empty());
    assert!(deleted.ir().model.bodies.is_empty());
}
