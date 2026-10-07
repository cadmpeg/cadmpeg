// SPDX-License-Identifier: Apache-2.0
//! Admission limits for historical state indexes and change chains.

#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use crate::history_records::AsmDeltaState;

fn complete_record_binding_fixture() -> (Vec<u8>, Vec<AsmDeltaState>) {
    use crate::history_records::{AsmHistoryRecord, AsmHistoryRecordFraming};
    let mut bytes = crate::test_support::smbh_header_test::smbh_header_prefix();
    let start = bytes.len();
    crate::test_support::tokens_test::t_ident(&mut bytes, "body");
    crate::test_support::tokens_test::t_end(&mut bytes);
    let framed = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let record = &framed[0];
    let limit = record.offset.checked_add(record.len).unwrap();
    let record_bytes = bytes[record.offset..limit].to_vec();
    let state_id = "complete-state".to_owned();
    let state = AsmDeltaState {
        id: state_id.clone(),
        parent: "history".to_owned(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 1,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: vec![AsmHistoryRecord {
            id: "record".to_owned(),
            parent: state_id,
            revision_id: Some(1),
            byte_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            framing: AsmHistoryRecordFraming::Framed {
                index: cadmpeg_core::decode::u64_from_index(record.index),
                name: record.name.clone(),
                entity_references: Vec::new(),
            },
            raw_bytes: record_bytes,
        }],
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    (bytes, vec![state])
}

fn complete_record_binding_refusal(operation: &str) -> cadmpeg_core::CodecError {
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let (bytes, mut states) = complete_record_binding_fixture();
            crate::history::archive::bind_complete_record_tables(
                ctx,
                &mut states,
                &bytes,
                cadmpeg_asm::kernel_header::RefWidth::Eight,
            )
        },
    )
}

#[test]
fn complete_record_state_scan_refuses_work() {
    let operation = "scan F3D complete record states";
    let error = complete_record_binding_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn complete_state_record_scan_refuses_work() {
    let operation = "scan F3D complete state records";
    let error = complete_record_binding_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn complete_record_byte_comparison_refuses_work() {
    let operation = "compare F3D archived record bytes";
    let error = complete_record_binding_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn complete_record_name_comparison_refuses_work() {
    let operation = "compare F3D archived record names";
    let error = complete_record_binding_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
