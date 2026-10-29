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
    crate::neutral_projection::project(&ctx, &scan, Vec::new(), Vec::new(), &mut ir).unwrap();
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

#[test]
fn document_metadata_projection_keeps_typed_values_order_and_native_owners() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};

    let archive = ArchiveVersion::V8;
    let mut revision = vec![0x10];
    revision.extend(bytes::utf16_bytes("Source author"));
    revision.extend(
        [1_i32, 2, 3, 4, 5, 126, 6, 127]
            .into_iter()
            .flat_map(i32::to_le_bytes),
    );
    revision.extend(bytes::utf16_bytes("Source editor"));
    revision.extend(
        [8_i32, 9, 10, 11, 5, 126, 4, 158]
            .into_iter()
            .flat_map(i32::to_le_bytes),
    );
    revision.extend(7_i32.to_le_bytes());
    let mut notes = vec![0x11];
    notes.extend(1_i32.to_le_bytes());
    notes.extend(bytes::utf16_bytes("Source notes"));
    notes.extend(1_i32.to_le_bytes());
    notes.extend(
        [10_i32, 20, 300, 400]
            .into_iter()
            .flat_map(i32::to_le_bytes),
    );
    notes.push(1);
    let mut application = vec![0x10];
    for value in [
        "Source application",
        "https://example.invalid/source",
        "Source application details",
    ] {
        application.extend(bytes::utf16_bytes(value));
    }
    let source = bytes::minimal_document(
        "80",
        &[
            bytes::table(
                archive,
                0x1000_0014,
                &[
                    bytes::crc_chunk(archive, 0x2000_8021, &revision),
                    bytes::crc_chunk(archive, 0x2000_8022, &notes),
                    bytes::crc_chunk(archive, 0x2000_8024, &application),
                ],
            ),
            bytes::table(archive, 0x1000_0015, &[bytes::units_record(archive, 2)]),
            bytes::table(archive, 0x1000_0013, &[]),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, &source).unwrap();
    let mut ir = CadIr::empty();
    crate::document_data::install(&ctx, &scan, &mut ir).unwrap();
    let before = ir.native.clone();
    assert!(
        crate::neutral_projection::project(&ctx, &scan, Vec::new(), Vec::new(), &mut ir)
            .unwrap()
            .is_empty()
    );
    assert_eq!(ir.native, before);
    let expected = [
        (
            "document_notes",
            "rhino:document:notes#current",
            "html",
            AttributeValue::Boolean(true),
        ),
        (
            "document_notes",
            "rhino:document:notes#current",
            "text",
            AttributeValue::String("Source notes".into()),
        ),
        (
            "document_notes",
            "rhino:document:notes#current",
            "visible",
            AttributeValue::Boolean(true),
        ),
        (
            "document_notes",
            "rhino:document:notes#current",
            "window_rectangle",
            AttributeValue::vector([10.0, 20.0, 300.0, 400.0]).unwrap(),
        ),
        (
            "document_notes",
            "rhino:document:notes#current",
            "locked",
            AttributeValue::Boolean(true),
        ),
        (
            "revisions",
            "rhino:document:revision#current",
            "created_by",
            AttributeValue::String("Source author".into()),
        ),
        (
            "revisions",
            "rhino:document:revision#current",
            "created_utc_fields",
            AttributeValue::vector([1.0, 2.0, 3.0, 4.0, 5.0, 126.0, 6.0, 127.0]).unwrap(),
        ),
        (
            "revisions",
            "rhino:document:revision#current",
            "last_edited_by",
            AttributeValue::String("Source editor".into()),
        ),
        (
            "revisions",
            "rhino:document:revision#current",
            "last_edited_utc_fields",
            AttributeValue::vector([8.0, 9.0, 10.0, 11.0, 5.0, 126.0, 4.0, 158.0]).unwrap(),
        ),
        (
            "revisions",
            "rhino:document:revision#current",
            "revision_count",
            AttributeValue::Integer(7),
        ),
        (
            "applications",
            "rhino:document:application#writer",
            "name",
            AttributeValue::String("Source application".into()),
        ),
        (
            "applications",
            "rhino:document:application#writer",
            "url",
            AttributeValue::String("https://example.invalid/source".into()),
        ),
        (
            "applications",
            "rhino:document:application#writer",
            "details",
            AttributeValue::String("Source application details".into()),
        ),
    ];
    assert_eq!(ir.model.attributes.len(), expected.len());
    for (attribute, (arena, native_id, field, value)) in ir.model.attributes.iter().zip(expected) {
        assert_eq!(attribute.name, format!("rhino.{arena}.{field}"));
        assert_eq!(attribute.target, AttributeTarget::Document);
        assert_eq!(attribute.native_ref.as_deref(), Some(native_id));
        assert_eq!(attribute.values, [Some(value)]);
        let records = &ir.native.namespace("rhino").unwrap().arenas()[arena];
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id(), native_id);
        assert_eq!(records[0].field("neutral_attributes"), None);
    }
}

fn material_texture(uuid: u8) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut body = vec![uuid; 16];
    body.extend(7_u32.to_le_bytes());
    body.extend(bytes::utf16_bytes("texture.png"));
    body.push(1);
    for value in 1..=7_u32 {
        body.extend(value.to_le_bytes());
    }
    for index in 0..16 {
        body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
    }
    body.extend([1, 2, 3, 4]);
    body.extend([5, 6, 7, 8]);
    body.extend([0x22; 16]);
    for value in [0.25_f64, 1.25, 0.1, 0.2, 0.3, 0.4, 0.5] {
        body.extend(value.to_le_bytes());
    }
    body.extend([9, 10, 11, 12]);
    for value in [0.6_f64, 0.7, 0.8, 0.9] {
        body.extend(value.to_le_bytes());
    }
    body.extend(4_i32.to_le_bytes());
    let texture_uuid = crate::wire::Uuid::from_canonical([
        0xd6, 0xff, 0x10, 0x6d, 0x32, 0x9b, 0x4f, 0x29, 0x97, 0xe2, 0xfd, 0x28, 0x2a, 0x61, 0x80,
        0x20,
    ])
    .to_wire();
    bytes::long_chunk(
        archive,
        0x0002_7ffa,
        &[
            bytes::crc_chunk(archive, 0x0002_fffb, &texture_uuid),
            bytes::nested_crc_chunk(
                archive,
                0x0002_fffc,
                &bytes::anonymous_chunk(archive, 0, &body),
            ),
            bytes::short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    )
}

fn material_record(uuid: u8, name: &str, index: i32, fresnel: bool, textures: &[u8]) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut body = vec![uuid; 16];
    body.extend(index.to_le_bytes());
    body.extend(bytes::utf16_bytes(name));
    body.extend([0xaa; 16]);
    for color in [
        [1, 2, 3, 4],
        [10, 20, 30, 40],
        [5, 6, 7, 8],
        [9, 10, 11, 12],
        [13, 14, 15, 16],
        [17, 18, 19, 20],
    ] {
        body.extend(color);
    }
    for value in [1.25_f64, 0.125, 2.5, 0.25] {
        body.extend(value.to_le_bytes());
    }
    let texture_start = body.len();
    let mut texture_array = i32::try_from(textures.len())
        .unwrap()
        .to_le_bytes()
        .to_vec();
    let mut children = Vec::new();
    for identity in textures {
        let start = texture_array.len();
        texture_array.extend(material_texture(*identity));
        children.push(start + 8..texture_array.len() + 8);
    }
    let mut texture_fields = 1_i32.to_le_bytes().to_vec();
    texture_fields.extend(0_i32.to_le_bytes());
    texture_fields.extend(texture_array);
    body.extend(bytes::crc_chunk_excluding(
        archive,
        0x4000_8000,
        &texture_fields,
        &children,
    ));
    let texture_end = body.len();
    body.extend(bytes::utf16_bytes(""));
    body.extend(0_i32.to_le_bytes());
    body.extend([1, 0]);
    if fresnel {
        body.push(1);
        for value in [0.375_f64, 0.625, 1.5] {
            body.extend(value.to_le_bytes());
        }
        body.extend([0xbb; 16]);
        body.push(1);
    }
    let mut fields = 1_i32.to_le_bytes().to_vec();
    fields.extend((if fresnel { 7_i32 } else { 3 }).to_le_bytes());
    fields.extend(body);
    let inner = bytes::crc_chunk_excluding(
        archive,
        0x4000_8000,
        &fields,
        &[std::ops::Range {
            start: texture_start + 8,
            end: texture_end + 8,
        }],
    );
    let mut payload = vec![0x20];
    payload.extend(inner);
    let material_uuid = crate::wire::Uuid::from_canonical([
        0x60, 0xb5, 0xdb, 0xbc, 0xe6, 0x60, 0x11, 0xd3, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ])
    .to_wire();
    let class = bytes::long_chunk(
        archive,
        0x0002_7ffa,
        &[
            bytes::crc_chunk(archive, 0x0002_fffb, &material_uuid),
            bytes::crc_chunk_excluding(
                archive,
                0x0002_fffc,
                &payload,
                &[std::ops::Range {
                    start: 1,
                    end: payload.len(),
                }],
            ),
            bytes::short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    bytes::nested_crc_chunk(archive, 0x2000_8040, &class)
}

#[test]
fn material_typed_handoff_keeps_native_order_values_and_optional_field_absence() {
    let archive = bytes::minimal_document(
        "80",
        &[
            bytes::table(ArchiveVersion::V8, 0x1000_0015, &[]),
            bytes::table(
                ArchiveVersion::V8,
                0x1000_0010,
                &[
                    material_record(0x99, "Last identity", 7, true, &[0x44, 0x11]),
                    material_record(0x11, "First equal identity", 7, true, &[]),
                    material_record(0x11, "  ", -2, false, &[]),
                ],
            ),
            bytes::table(ArchiveVersion::V8, 0x1000_0013, &[]),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&archive, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, &archive).unwrap();
    let mut ir = CadIr::empty();
    let (installed, materials) = crate::presentation::install(&ctx, view, &scan, &mut ir).unwrap();
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    assert!(installed.opaque_records.is_empty());
    let before = ir.native.clone();
    let losses =
        crate::neutral_projection::project(&ctx, &scan, materials, Vec::new(), &mut ir).unwrap();
    assert_eq!(losses.len(), 2);
    for (loss, (order, uuid)) in losses.iter().zip([
        (0, "44444444-4444-4444-4444-444444444444"),
        (1, "11111111-1111-1111-1111-111111111111"),
    ]) {
        assert_eq!(
            loss.code,
            crate::loss::RhinoLossCode::NativeRecordUnprojected.kind()
        );
        assert_eq!(loss.message, format!("rhino:presentation:material#99999999-9999-9999-9999-999999999999 texture {order} (Some(\"{uuid}\")): the full UVW transform and texture interpretation are retained natively without a neutral texture mapping"));
    }
    assert_eq!(ir.native, before);
    let native = &ir.native.namespace("rhino").unwrap().arenas()["materials"];
    assert_eq!(ir.model.appearances.len(), 3);
    for (ordinal, (name, uuid, count)) in [
        (
            Some("First equal identity"),
            "11111111-1111-1111-1111-111111111111",
            7,
        ),
        (None, "11111111-1111-1111-1111-111111111111", 4),
        (
            Some("Last identity"),
            "99999999-9999-9999-9999-999999999999",
            7,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let appearance = &ir.model.appearances[ordinal];
        assert_eq!(
            appearance.id.as_str(),
            format!("rhino:model:material#{ordinal:06}")
        );
        assert_eq!(appearance.native_ref.as_deref(), Some(native[ordinal].id()));
        assert_eq!(appearance.name.as_deref(), name);
        assert_eq!(appearance.asset_guid.as_deref(), Some(uuid));
        assert_eq!(
            native[ordinal].field("source_uuid"),
            Some(serde_json::json!(uuid))
        );
        assert_eq!(
            appearance.base_color.unwrap(),
            cadmpeg_ir::topology::Color::from_rgba8(10, 20, 30, 215)
        );
        assert!(appearance.textures.is_empty());
        for (name, value) in [
            ("index_of_refraction", 1.25),
            ("reflectivity", 0.125),
            ("shine", 2.5),
            ("transparency", 0.25),
        ] {
            assert_eq!(appearance.properties[name].get(), value);
        }
        assert_eq!(appearance.properties.len(), count);
        for (name, value) in [
            ("reflection_glossiness", 0.375),
            ("refraction_glossiness", 0.625),
            ("fresnel_index_of_refraction", 1.5),
        ] {
            assert_eq!(
                appearance.properties.get(name).copied(),
                (count == 7).then(|| crate::test_support::finite(value))
            );
        }
    }
    assert_eq!(
        native[0].field("name"),
        Some(serde_json::json!("First equal identity"))
    );
    assert_eq!(native[1].field("name"), Some(serde_json::json!("  ")));
    assert_eq!(
        native[2].field("name"),
        Some(serde_json::json!("Last identity"))
    );
    assert_eq!(native[0].field("archive_index"), Some(serde_json::json!(7)));
    assert_eq!(
        native[1].field("archive_index"),
        Some(serde_json::json!(-2))
    );
    assert_eq!(native[2].field("archive_index"), Some(serde_json::json!(7)));
}

fn saved_view_source(name: &str, location: [f64; 3]) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut viewport = vec![0x15];
    for value in [1_i32, 1, 1, 2] {
        viewport.extend(value.to_le_bytes());
    }
    for vector in [
        location,
        [0.0, 0.0, -1.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ] {
        bytes::point(&mut viewport, vector);
    }
    for value in [-2.0_f64, 2.0, -1.0, 1.0, 0.1, 100.0] {
        viewport.extend(value.to_le_bytes());
    }
    for value in [0_i32, 1920, 1080, 0, 0, 1] {
        viewport.extend(value.to_le_bytes());
    }
    viewport.extend([0x11; 16]);
    viewport.extend([1, 0, 1, 0, 1]);
    bytes::point(&mut viewport, [4.0, 5.0, 6.0]);
    viewport.push(1);
    for value in [1.0_f64, 2.0, 3.0] {
        viewport.extend(value.to_le_bytes());
    }
    bytes::nested_crc_chunk(
        archive,
        0x2000_803b,
        &[
            bytes::crc_chunk(archive, 0x2000_8a3b, &bytes::utf16_bytes(name)),
            bytes::crc_chunk(archive, 0x2000_823b, &viewport),
            bytes::short_chunk(archive, 0xa000_033b, 0),
            bytes::short_chunk(archive, 0xa000_043b, 1),
            bytes::short_chunk(archive, 0xa000_053b, 0),
            bytes::short_chunk(archive, 0xa000_093b, 2),
            bytes::short_chunk(archive, 0xffff_ffff, 0),
        ]
        .concat(),
    )
}

#[test]
fn typed_view_handoff_orders_native_owners_without_reordering_camera_values() {
    let archive = ArchiveVersion::V8;
    let list = |typecode, view: Vec<u8>| {
        let mut payload = 1_i32.to_le_bytes().to_vec();
        payload.extend(view);
        bytes::crc_chunk_excluding(
            archive,
            typecode,
            &payload,
            &[std::ops::Range {
                start: 4,
                end: payload.len(),
            }],
        )
    };
    let source = bytes::minimal_document(
        "80",
        &[
            bytes::table(
                archive,
                0x1000_0015,
                &[
                    bytes::units_record(archive, 2),
                    list(
                        0x2000_8036,
                        saved_view_source("Named source", [1.25, -2.5, 3.75]),
                    ),
                    list(
                        0x2000_8037,
                        saved_view_source("Active source", [10.0, 20.0, 30.0]),
                    ),
                ],
            ),
            bytes::table(archive, 0x1000_0013, &[]),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, &source).unwrap();
    let mut ir = CadIr::empty();
    let (installed, views) = crate::views::install(&ctx, &scan, &mut ir).unwrap();
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    assert!(installed.opaque_records.is_empty());
    let native_before = ir.native.clone();
    let losses =
        crate::neutral_projection::project(&ctx, &scan, Vec::new(), views, &mut ir).unwrap();
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(ir.native, native_before);
    let records = &ir.native.namespace("rhino").unwrap().arenas()["views"];
    assert_eq!(records.len(), 2);
    assert_eq!(ir.model.view_presentations.len(), 2);
    assert_eq!(ir.model.presentation_documents.len(), 2);
    for (ordinal, (name, location)) in [
        ("Active source", [10.0, 20.0, 30.0]),
        ("Named source", [1.25, -2.5, 3.75]),
    ]
    .into_iter()
    .enumerate()
    {
        let view = &ir.model.view_presentations[ordinal];
        assert_eq!(view.native_ref.as_deref(), Some(records[ordinal].id()));
        assert_eq!(view.order, u32::try_from(ordinal).unwrap());
        assert_eq!(
            view.properties
                .iter()
                .find(|(key, _)| key.as_str() == "name")
                .unwrap()
                .1,
            name
        );
        assert_eq!(view.display_mode.as_deref(), Some("2"));
        let document = &ir.model.presentation_documents[ordinal];
        assert_eq!(document.native_ref.as_deref(), Some(records[ordinal].id()));
        let cadmpeg_ir::presentation::PresentationStateKind::Camera(camera) =
            &document.states()[0].kind
        else {
            panic!("camera")
        };
        assert_eq!(camera.position.unwrap().get(), location);
        assert_eq!(camera.orientation.unwrap().get(), [0.0, 0.0, 1.0, 0.0]);
    }
}
