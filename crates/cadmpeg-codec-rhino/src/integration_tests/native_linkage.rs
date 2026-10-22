// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::test_dump as bytes;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;

fn source() -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut dot = vec![0x11];
    for coordinate in [1.0_f64, 2.0, 3.0] {
        dot.extend(coordinate.to_le_bytes());
    }
    dot.extend(12_i32.to_le_bytes());
    dot.extend(bytes::utf16_bytes("Original annotation"));
    dot.extend(bytes::utf16_bytes("Arial"));
    dot.extend(0_i32.to_le_bytes());
    dot.extend(bytes::utf16_bytes("secondary"));
    let dot = bytes::object_record_with_payload(
        archive,
        0x2000,
        [
            0x02, 0x83, 0x19, 0x74, 0xf4, 0xcd, 0x95, 0x4f, 0x96, 0x09, 0x6d, 0x68, 0x4f, 0x22,
            0xab, 0x37,
        ],
        &dot,
    );
    let definition = bytes::definition_record(
        archive,
        &bytes::v6_definition_payload(archive, [0x51; 16], &[], 1, false, false),
    );
    let reference = bytes::object_record_with_payload(
        archive,
        0x1000,
        bytes::INSTANCE_REFERENCE_CLASS,
        &bytes::instance_reference_payload(
            [0x51; 16],
            cadmpeg_ir::transform::Transform::identity().rows(),
        ),
    );
    bytes::minimal_document(
        "80",
        &[
            bytes::table(archive, 0x1000_0014, &[]),
            bytes::table(archive, 0x1000_0015, &[bytes::units_record(archive, 2)]),
            bytes::table(archive, 0x1000_0021, &[definition]),
            bytes::table(archive, 0x1000_0013, &[dot, reference]),
        ],
    )
}

#[test]
fn annotation_projection_keeps_every_native_field_and_only_the_neutral_record_links_back() {
    let source = source();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, &source).unwrap();
    let mut ir = CadIr::empty();
    crate::annotations::install(&ctx, &scan, &mut ir).unwrap();
    let before = ir.native.clone();
    crate::neutral_projection::project(&ctx, &scan, &mut ir).unwrap();
    assert_eq!(ir.native, before);
    let [annotation] = ir.model.semantic_annotations.as_slice() else {
        panic!("one source dot")
    };
    assert_eq!(annotation.text, ["Original annotation", "secondary"]);
    assert_eq!(annotation.position.unwrap().get(), [1.0, 2.0, 3.0]);
    let records = &ir.native.namespace("rhino").unwrap().arenas()["text_dots"];
    assert_eq!(records.len(), 1);
    assert_eq!(annotation.native_ref, records[0].id());
    assert_eq!(
        records[0].field("primary_text"),
        Some(serde_json::json!("Original annotation"))
    );
    assert_eq!(
        records[0].field("font_face"),
        Some(serde_json::json!("Arial"))
    );
    assert_eq!(records[0].field("neutral_ref"), None);
}

#[test]
fn product_projection_keeps_every_native_definition_and_occurrence_field_without_reverse_links() {
    let source = source();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, &source).unwrap();
    let mut ir = CadIr::empty();
    crate::product::install(&ctx, &scan, &mut ir).unwrap();
    let before = ir.native.clone();
    crate::product::project(&ctx, &scan, &mut ir).unwrap();
    assert_eq!(ir.native, before);
    assert_eq!(ir.model.product_definitions.len(), 1);
    assert_eq!(ir.model.occurrences.len(), 1);
    let namespace = ir.native.namespace("rhino").unwrap();
    for (arena, link) in [
        (
            "product_definitions",
            ir.model.product_definitions[0]
                .native_ref
                .as_deref()
                .unwrap(),
        ),
        (
            "product_occurrences",
            ir.model.occurrences[0].native_ref.as_deref().unwrap(),
        ),
    ] {
        let [record] = namespace.arenas()[arena].as_slice() else {
            panic!("one source product record")
        };
        assert_eq!(record.id(), link);
        assert_eq!(record.field("neutral_refs"), None);
    }
    assert_eq!(
        ir.model.product_definitions[0].source_name.as_deref(),
        Some("modern definition")
    );
    assert_eq!(
        ir.model.occurrences[0].transform,
        Some(cadmpeg_ir::transform::Transform::identity())
    );
}

#[test]
fn source_free_writer_uses_current_neutral_annotations_and_products_through_native_refs() {
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source()), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    assert_eq!(ir.model.semantic_annotations.len(), 1);
    assert_eq!(ir.model.product_definitions.len(), 1);
    assert_eq!(ir.model.occurrences.len(), 1);
    let before = ir.native.clone();
    ir.model.semantic_annotations[0].text =
        vec!["Edited annotation".into(), "edited secondary".into()];
    ir.model.semantic_annotations[0].position = Some(FiniteVector::new([4.0, 5.0, 6.0]).unwrap());
    ir.model.product_definitions[0].source_name = Some("Edited definition".into());
    for target in [
        "rhino:archive-50",
        "rhino:archive-60",
        "rhino:archive-70",
        "rhino:archive-80",
    ] {
        let plan = RhinoCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit(target))
            .unwrap();
        assert!(matches!(
            plan.report().write_path(),
            WritePath::Synthesized { .. }
        ));
        let mut output = Vec::new();
        plan.write_to(&mut output).unwrap();
        assert_eq!(ir.native, before);
        let reopened = RhinoCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .unwrap();
        assert!(reopened
            .report()
            .losses
            .iter()
            .all(|loss| { loss.code != crate::loss::RhinoLossCode::IntegrityFailure.kind() }));
        let [annotation] = reopened.ir().model.semantic_annotations.as_slice() else {
            panic!("one current annotation")
        };
        assert_eq!(annotation.text, ["Edited annotation", "edited secondary"]);
        assert_eq!(annotation.position.unwrap().get(), [4.0, 5.0, 6.0]);
        let [definition] = reopened.ir().model.product_definitions.as_slice() else {
            panic!("one current definition")
        };
        assert_eq!(definition.source_name.as_deref(), Some("Edited definition"));
        assert_eq!(reopened.ir().model.occurrences.len(), 1);
        let namespace = reopened.ir().native.namespace("rhino").unwrap();
        for arena in [
            "annotations",
            "text_dots",
            "annotation_arrows",
            "product_definitions",
            "product_occurrences",
        ] {
            for record in namespace.arenas().get(arena).into_iter().flatten() {
                assert_eq!(record.field("neutral_ref"), None);
                assert_eq!(record.field("neutral_refs"), None);
            }
        }
    }
}
