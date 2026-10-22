// SPDX-License-Identifier: Apache-2.0
//! Actual path fields, source-coordinate meaning and admission rollback.

use super::{identity_qualified_path_bytes, path_link, push_guid};
use crate::records::feature::assembly::{
    DesignAssemblyOperandPath, TYPED_OCCURRENCE_PATH_TYPE_GUID,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::BTreeMap;

fn paired_path_bytes() -> Vec<u8> {
    let plain = identity_qualified_path_bytes(*b"366", true);
    let mut bytes = plain[..plain.len() - 8].to_vec();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for unit in "43".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    push_guid(&mut bytes, "66666666-6666-6666-6666-666666666666");
    bytes
}

#[test]
fn source_pointers_keep_bulk_coordinates_and_reads_keep_actual_parent_coordinates() {
    let bytes = paired_path_bytes();
    let mut enclosing = vec![0; 37];
    enclosing.extend_from_slice(&bytes);
    enclosing.extend_from_slice(b"after-path");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&enclosing, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(37, 37 + bytes.len()).unwrap();
    let parsed = super::super::exact_assembly_operand_path(
        source,
        0,
        65,
        bytes.len(),
        path_link(),
        Some(TYPED_OCCURRENCE_PATH_TYPE_GUID),
    )
    .unwrap()
    .unwrap();
    let mut pointers = BTreeMap::new();
    ctx.record("scope", source, || {
        parsed
            .source
            .read(&ctx, &parsed.content, 10, "path", &mut pointers)
    })
    .unwrap();
    assert_eq!(
        pointers,
        BTreeMap::from([
            ("path.byte_offset".to_owned(), 0),
            ("path.occurrence_guid_offsets.0".to_owned(), 29),
            ("path.identity_guid_offsets.0".to_owned(), 105),
            ("path.identity_guid_offsets.1".to_owned(), 181),
            ("path.identity_guid_offsets.2".to_owned(), 265),
            ("path.identity_guid_offsets.3".to_owned(), 341),
            ("path.paired_occurrence_key_offsets.0".to_owned(), 433),
            ("path.paired_occurrence_key_offsets.1".to_owned(), 441),
        ])
    );
    let capture = ctx.capture_part_extent(source).unwrap();
    assert_eq!(capture.into_parts(), ("file".into(), 37, 513, None));
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "scope");
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![37..52, 62..290, 298..450, 466..550]),])
    );
}

#[test]
fn source_pair_content_mismatch_rolls_back_the_record_capture() {
    let bytes = paired_path_bytes();
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let parsed = super::super::exact_assembly_operand_path(
        source,
        0,
        65,
        bytes.len(),
        path_link(),
        Some(TYPED_OCCURRENCE_PATH_TYPE_GUID),
    )
    .unwrap()
    .unwrap();
    let mut wire = serde_json::to_value(&parsed.content).unwrap();
    wire["paired_occurrence_keys"][0] = serde_json::json!("99");
    let changed: DesignAssemblyOperandPath = serde_json::from_value(wire).unwrap();
    let error = ctx
        .record("scope", source, || {
            parsed
                .source
                .read(&ctx, &changed, 10, "path", &mut BTreeMap::new())
        })
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(message) if message == "assembly path key differs from content")
    );
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn equal_foreign_source_bytes_do_not_admit_a_path_draft() {
    let bytes = paired_path_bytes();
    let arena = DecodeArena::new();
    let (_source_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let parsed = super::super::exact_assembly_operand_path(
        source,
        0,
        65,
        bytes.len(),
        path_link(),
        Some(TYPED_OCCURRENCE_PATH_TYPE_GUID),
    )
    .unwrap()
    .unwrap();
    let other_arena = DecodeArena::new();
    let (ctx, owner) =
        DecodeContext::from_root_bytes(&bytes, &other_arena, &DecodePolicy::service()).unwrap();
    assert!(ctx
        .record("scope", owner, || parsed.source.read(
            &ctx,
            &parsed.content,
            10,
            "path",
            &mut BTreeMap::new()
        ))
        .is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn admitted_locator_wrapper_and_path_links_keep_their_actual_source_slots() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

    let mut bytes = vec![1];
    bytes.extend_from_slice(&64_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&super::envelope_bytes(10));
    let scope = DesignParameterScope::empty("scope", DesignFeatureKind::Assemble, 10);
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let parsed = super::super::exact_assembly_operand_path_envelope(
        source,
        &scope,
        &std::collections::HashMap::new(),
        64,
        1,
        11,
    )
    .unwrap()
    .unwrap();
    let mut pointers = BTreeMap::new();
    ctx.record("scope", source.child(0, 11).unwrap(), || {
        parsed
            .source
            .read(&ctx, &parsed.content, 10, "path", &mut pointers)
    })
    .unwrap();
    assert_eq!(
        pointers,
        BTreeMap::from([
            ("path.byte_offset".to_owned(), 201),
            ("path.link.locator_reference_offset".to_owned(), 1),
            ("path.link.locator_byte_offset".to_owned(), 11),
            ("path.link.locator_scope_reference_offset".to_owned(), 174),
            ("path.link.wrapper_reference_offset".to_owned(), 185),
            ("path.link.wrapper_byte_offset".to_owned(), 302),
            ("path.link.path_reference_offset".to_owned(), 329),
            ("path.occurrence_guid_offsets.0".to_owned(), 230),
        ])
    );
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (0, 11));
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([(
            "file".to_owned(),
            vec![
                1..5,
                11..22,
                174..178,
                185..189,
                201..216,
                226..313,
                329..333
            ]
        ),])
    );
}
