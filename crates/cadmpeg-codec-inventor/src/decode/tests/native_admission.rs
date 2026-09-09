// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::codec::{Codec, DecodeOptions, Decoded};

use crate::container::InventorContainer;
use crate::decode::{admit_assembly_placement, decode_container};
use crate::external_reference::{
    InventorEmbeddedReference, InventorExternalReference, UfrxDocument, UfrxModelState,
    UfrxOccurrence, UfrxRepresentationState, UfrxState,
};
use crate::native::ufrx::UfrxRecord;
use crate::native::{AssemblyPlacementRecordWire, StructuralIssueRecord};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::{fixture_with_ufrx, primary_envelope_fixture};
use crate::InventorCodec;

#[test]
fn empty_external_identity_does_not_fail_file_decode() {
    let bytes = fixture_with_ufrx(&external_references_stream());
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("invalid external identity must not fail file decode");
    assert_ufrx_issue(decoded.ir(), "ufrx-external-reference-0", "document_id");
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::InventorLossCode::UfrxTableMalformed.kind()));
    let namespace = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("native namespace");
    let ufrx = UfrxRecord::read(namespace).expect("admitted UFRx arenas agree");
    assert_eq!(ufrx.external_references().len(), 1);
    assert_eq!(ufrx.external_references()[0].ordinal, 1);
    assert_eq!(ufrx.external_references()[0].reference_id, 8);
}

#[test]
fn rejected_model_state_does_not_fail_decode() {
    let decoded = decode_ufrx(|document| document.model_states[0].name.clear());
    assert_ufrx_issue(&decoded.ir, "ufrx-model-state-0", "name");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let ufrx = UfrxRecord::read(namespace).expect("admitted UFRx arenas agree");
    assert!(ufrx.model_states().is_empty());
    assert_eq!(ufrx.external_references().len(), 1);
}

#[test]
fn rejected_representation_does_not_fail_decode() {
    let decoded = decode_ufrx(|document| {
        document
            .representation
            .as_mut()
            .expect("representation fixture")
            .active_model_state
            .clear();
    });
    assert_ufrx_issue(&decoded.ir, "ufrx-representation", "active_model_state");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    assert!(matches!(
        UfrxRecord::read(namespace).expect("admitted UFRx arenas agree"),
        UfrxRecord::ParsedPrefix {
            representation: None,
            ..
        }
    ));
}

#[test]
fn rejected_embedded_reference_does_not_fail_decode() {
    let decoded = decode_ufrx(|document| {
        document.embedded_references[0].source = document.unparsed_tail;
    });
    assert_ufrx_issue(&decoded.ir, "ufrx-embedded-reference-0", "record_len");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let ufrx = UfrxRecord::read(namespace).expect("admitted UFRx arenas agree");
    assert!(ufrx.embedded_references().is_empty());
    assert_eq!(ufrx.external_references().len(), 1);
}

#[test]
fn rejected_occurrence_does_not_fail_decode() {
    let decoded = decode_ufrx(|document| document.occurrences[0].header_padding_words = 9);
    assert_ufrx_issue(&decoded.ir, "ufrx-occurrence-0", "header_padding_words");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let ufrx = UfrxRecord::read(namespace).expect("admitted UFRx arenas agree");
    assert!(ufrx.occurrences().is_empty());
    assert_eq!(ufrx.external_references().len(), 1);
}

#[test]
fn nonfinite_assembly_placement_transform_is_rejected_at_parse() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("container fixture");
    let mut container = InventorContainer::open(&ctx, root).expect("container fixture");
    let mut payload = vec![0; 15];
    payload.extend_from_slice(&0_u16.to_le_bytes());
    payload.extend_from_slice(&0_u16.to_le_bytes());
    payload.extend_from_slice(&f64::INFINITY.to_le_bytes());
    let segment = &mut container.rse.segments[0];
    segment.kind = SegmentKind::AmGraphics;
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("framed fixture");
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("record fixture");
    };
    table.records[0].type_id = [
        0xa2, 0x63, 0x71, 0xca, 0xd0, 0x11, 0xb2, 0xd3, 0x00, 0x08, 0xbf, 0xbb, 0x21, 0xed, 0xdc,
        0x09,
    ];
    table.records[0].payload = View::over_retained(&payload);
    let decoded =
        decode_container(&ctx, &container).expect("invalid placement must not fail decode");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    let issues = namespace
        .arena_as::<RecordIssue>("assembly_record_issues")
        .expect("assembly issues");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].family, RecordIssueFamily::Assembly);
    assert!(issues[0].detail.contains("finite"), "{:?}", issues[0]);
    assert!(namespace
        .arena_as::<serde_json::Value>("assembly_placements")
        .expect("placements")
        .is_empty());
    assert!(crate::validate::validate_native(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains(&issues[0].detail)));
}

#[test]
fn rejected_placement_digest_records_its_source_and_keeps_later_placements() {
    let wire = serde_json::json!({
        "id": "inventor:assembly:placement#segment-1", "segment_token": "segment", "record_ordinal": 1,
        "header_id": 0, "owner_reference": 0, "attribute_reference": 0, "state": 0,
        "transform_prefix": false, "transform_encoding": [0, 0],
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "branch": 0, "graphics_state": 0, "occurrence_id": 1, "graphics_index": 0,
        "object_reference": 0, "suffix_len": 48, "suffix_sha256": "invalid"
    });
    let mut issues = Vec::new();
    let bad: AssemblyPlacementRecordWire =
        serde_json::from_value(wire.clone()).expect("wire fixture");
    assert!(admit_assembly_placement(bad, &mut issues).is_none());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].segment_token, "segment");
    assert_eq!(issues[0].record_ordinal, 1);
    assert!(issues[0].detail.contains("suffix_sha256"));
    let mut wire = wire;
    wire["suffix_sha256"] = serde_json::json!("0".repeat(64));
    let good = serde_json::from_value(wire).expect("wire fixture");
    assert!(admit_assembly_placement(good, &mut issues).is_some());
    assert_eq!(issues.len(), 1);
}

fn assert_ufrx_issue(ir: &cadmpeg_ir::document::CadIr, scope: &str, field: &str) {
    let namespace = ir.native.namespace("inventor").expect("native namespace");
    let issues = namespace
        .arena_as::<StructuralIssueRecord>("structural_issues")
        .expect("structural issues");
    let issue = issues
        .iter()
        .find(|issue| issue.scope == scope)
        .expect("rejected record has an issue");
    assert!(issue.detail.contains(field), "{}", issue.detail);
    assert!(crate::validate::validate_native(ir)
        .iter()
        .any(|finding| finding.message.contains(scope) && finding.message.contains(field)));
}

fn decode_ufrx(edit: impl FnOnce(&mut UfrxDocument<'_>)) -> Decoded {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("container fixture");
    let mut container = InventorContainer::open(&ctx, root).expect("container fixture");
    let source = root.child(0, 77).expect("source bytes");
    let mut document = UfrxDocument {
        stream: container
            .snapshot
            .stream("RSeStorage/RSeSegInfo")
            .expect("validated fixture stream")
            .id(),
        schema: 15,
        section_versions: vec![1],
        original_file_name: "part.ipt".into(),
        caption: "part".into(),
        representation: Some(UfrxRepresentationState {
            prefix: 0,
            active_representation: None,
            secondary_active_lod_state: [0; 2],
            active_model_state: "Primary".into(),
            active_model_state_state: [0; 2],
        }),
        model_states: vec![UfrxModelState {
            prefix: 0,
            name: "Primary".into(),
            state: [0; 2],
            prefix_count: 0,
            parameters: vec![],
            suffix: source,
        }],
        references: vec![InventorExternalReference {
            path: "part.ipt".into(),
            library_id: 0,
            library_name: String::new(),
            display_name: String::new(),
            state_groups: vec![],
            state: [0; 2],
            document_id: [0; 16],
            database_id: [0; 16],
            reference_id: 7,
            occurrence_count: 1,
            version: 0,
            flags: 0,
        }],
        embedded_references: vec![InventorEmbeddedReference {
            value_0: 0,
            filetime: 0,
            value_1: 0,
            extended_value: None,
            value_2: 0,
            path: String::new(),
            library_id: 0,
            library_name: String::new(),
            state: 0,
            display_name: String::new(),
            state_values: [0; 8],
            source,
        }],
        occurrences: vec![UfrxOccurrence {
            end_string_flag: 0,
            file_reference_id: 7,
            occurrence_id: 42,
            header_value: 0,
            title: None,
            header_padding_words: 0,
            source,
        }],
        unparsed_tail: root.child(0, 0).expect("empty tail"),
    };
    edit(&mut document);
    container.ufrx = UfrxState::Parsed(Box::new(document));
    let decoded =
        decode_container(&ctx, &container).expect("rejected native record must not fail decode");
    assert!(decoded
        .body
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::InventorLossCode::UfrxTableMalformed.kind()));
    decoded
}

fn external_references_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [
        11, 23, 31, 19, 12, 18, 1, 2, 4, 2, 1, 3, 1, 2, 6, 2, 2, 5, 0, 1, 2, 0, 0, 0, 0,
    ] {
        u16_le(&mut bytes, value);
    }
    bytes.extend_from_slice(&[0; 32]);
    utf16(&mut bytes, "");
    bytes.extend_from_slice(&[0; 48]);
    u32_le(&mut bytes, 0);
    bytes.extend_from_slice(&[0; 16]);
    utf16(&mut bytes, "assembly.iam");
    u16_le(&mut bytes, 0);
    u32_le(&mut bytes, 0);
    u32_le(&mut bytes, 0);
    bytes.extend_from_slice(&[0; 4]);
    utf16(&mut bytes, "Default");
    bytes.extend_from_slice(&[0; 4]);
    u16_le(&mut bytes, 0);
    u32_le(&mut bytes, 3);
    u16_le(&mut bytes, 0);
    u16_le(&mut bytes, 1);
    u32_le(&mut bytes, 1);
    u32_le(&mut bytes, 0);
    u32_le(&mut bytes, 0);
    u32_le(&mut bytes, 2);
    utf16(&mut bytes, "References");
    u32_le(&mut bytes, 0);
    for (path, id) in [("", 7), ("part.ipt", 8)] {
        utf16(&mut bytes, path);
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        utf16(&mut bytes, "");
        u16_le(&mut bytes, 0);
        utf16(&mut bytes, "");
        u32_le(&mut bytes, 0);
        u16_le(&mut bytes, 0);
        u16_le(&mut bytes, 0);
        bytes.extend_from_slice(&[0; 32]);
        for value in [id, 0, 12, 4] {
            u32_le(&mut bytes, value);
        }
    }
    u32_le(&mut bytes, 0);
    u32_le(&mut bytes, 0);
    bytes
}

fn u16_le(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn u32_le(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn utf16(bytes: &mut Vec<u8>, value: &str) {
    u32_le(bytes, value.encode_utf16().count() as u32);
    for unit in value.encode_utf16() {
        u16_le(bytes, unit);
    }
}

#[test]
fn protein_admission_keeps_later_assets_and_rejections() {
    let mut issues = Vec::new();
    let assets = ["bad.bin", "assets/InstanceProperties.bin"]
        .into_iter()
        .filter_map(|entry_name| {
            let wire = serde_json::from_value(serde_json::json!({
                "id": "asset", "entry_name": entry_name, "ordinal": 3,
                "asset": { "ordinal": 3, "logical_offset": 0, "schema": "GenericSchema",
                    "guid": "asset-guid", "base": "", "asset_lib_id": "", "properties": {} }
            }))
            .expect("Protein asset wire fixture");
            crate::decode::admit_protein_asset(wire, &mut issues)
        })
        .collect::<Vec<_>>();
    assert_eq!(assets.len(), 1);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].scope, "asset");
    assert!(issues[0].detail.contains("entry_name"));
    let rejections = ["bad.bin", "assets/InstanceProperties.bin"]
        .into_iter()
        .filter_map(|entry_name| {
            crate::decode::admit_protein_rejection(
                crate::native::protein::ProteinRejectionRecordWire {
                    id: "rejection".into(),
                    entry_name: entry_name.into(),
                    ordinal: 4,
                    detail: "invalid record".into(),
                },
                &mut issues,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rejections.len(), 1);
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[1].scope, "rejection");
    assert!(issues[1].detail.contains("entry_name"));
}
