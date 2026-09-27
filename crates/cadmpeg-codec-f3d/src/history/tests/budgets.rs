// SPDX-License-Identifier: Apache-2.0
//! History resource-budget unit tests.
#![allow(clippy::unwrap_used)]

use crate::history::{
    history_topology_work_budget_exceeded, HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY,
};

fn one_delta_state() -> Vec<u8> {
    let mut bytes = super::super::DELTA.to_vec();
    for (tag, value) in [(0x04, 1_i32), (0x04, 1), (0x04, 0),
        (0x0c, -1), (0x0c, -1), (0x0c, 0), (0x0c, -1), (0x0c, 0)] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0x0b, 0x11]);
    bytes
}

fn one_board_state() -> Vec<u8> {
    let mut bytes = one_delta_state();
    bytes.pop();
    for (tag, value) in [(0x04, 1_i32), (0x0c, 0), (0x04, 1),
        (0x04, 1), (0x0c, -1), (0x0c, 2), (0x04, 0), (0x04, 0)] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    bytes
}

fn history_id_lengths() -> (u64, u64, u64, u64) {
    let history = crate::ids::native_scoped_id("history", "asm-history", format_args!("{:010}", 0));
    let state = crate::ids::native_scoped_id("history", "asm-delta-state", format_args!("{:010}", 0));
    let board = crate::ids::native_scoped_id("history", "asm-bulletin-board", "0000000000:000000");
    let change = crate::ids::native_scoped_id("history", "asm-entity-change", "0000000000:000000:000000");
    (history.len() as u64, state.len() as u64, board.len() as u64, change.len() as u64)
}

fn history_record_with_limits(
    bytes: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy).unwrap();
    super::super::decode_history_records(
        &ctx,
        bytes,
        0,
        None,
        "history",
        "state",
        cadmpeg_asm::kernel_header::RefWidth::Four,
    )
    .unwrap_err()
}

fn one_framed_history_record() -> Vec<u8> {
    let mut bytes = b"\x0d\x01x\x0c".to_vec();
    bytes.extend_from_slice(&3_i32.to_le_bytes());
    bytes.push(0x11);
    bytes
}

fn one_state_history() -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmDeltaState, AsmHistory, AsmTopologyCache};

    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "state".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 0,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Absent,
            transition: None,
        }],
    }
}

#[test]
fn history_graph_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::graph_is_coherent_charged(&ctx, &one_state_history()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ASM history states"));
}

#[test]
fn history_graph_visit_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::graph_is_coherent_charged(&ctx, &one_state_history()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "visit F3D ASM history state"));
}

#[test]
fn history_record_references_refuse_collection_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let error = history_record_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "frame F3D history references"));
}

#[test]
fn history_record_vector_refuses_collection_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let error = history_record_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "frame F3D history record"));
}

#[test]
fn history_record_id_refuses_retained_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1 + std::mem::size_of::<cadmpeg_asm::sab::Token>() as u64
        + bytes.len() as u64;
    let error = history_record_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID"));
}

#[test]
fn history_record_parent_refuses_retained_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let id = crate::ids::native_scoped_id("history", "asm-history-record", format_args!("{:010}", 0));
    policy.limits.max_retained_bytes = 1 + std::mem::size_of::<cadmpeg_asm::sab::Token>() as u64
        + bytes.len() as u64 + id.len() as u64;
    let error = history_record_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D history record parent"));
}

#[test]
fn opaque_history_record_id_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID"));
}

#[test]
fn opaque_history_record_parent_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let id = crate::ids::native_scoped_id("history", "asm-history-record", format_args!("{:010}", 0));
    policy.limits.max_retained_bytes = 1 + id.len() as u64;
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy opaque F3D history record parent"));
}

#[test]
fn opaque_history_error_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let id = crate::ids::native_scoped_id("history", "asm-history-record", format_args!("{:010}", 0));
    policy.limits.max_retained_bytes = 1 + id.len() as u64 + "state".len() as u64;
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain opaque F3D history error"));
}

#[test]
fn history_board_id_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let (history, state, _, _) = history_id_lengths();
    policy.limits.max_retained_bytes = history + state;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID"));
}

#[test]
fn history_change_vector_refuses_collection_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM entity change"));
}

#[test]
fn history_change_id_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let (history, state, board, _) = history_id_lengths();
    policy.limits.max_retained_bytes = history + state + board;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID"));
}

#[test]
fn history_change_parent_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let (history, state, board, change) = history_id_lengths();
    policy.limits.max_retained_bytes = history + state + board + change;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ASM change parent"));
}

#[test]
fn history_board_vector_refuses_collection_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM bulletin board"));
}

#[test]
fn history_board_parent_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let (history, state, board, change) = history_id_lengths();
    policy.limits.max_retained_bytes = history + state + board + change + board;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ASM board parent"));
}

fn decode_with_limits(bytes: &[u8], policy: &cadmpeg_core::decode::DecodePolicy) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy).unwrap();
    super::super::decode(
        &ctx,
        bytes,
        "history",
        cadmpeg_asm::kernel_header::RefWidth::Four,
        &policy.limits,
    ).unwrap_err()
}

#[test]
fn history_id_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = decode_with_limits(&[], &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID"));
}

#[test]
fn history_state_id_refuses_retained_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let history_id = crate::ids::native_scoped_id("history", "asm-history", format_args!("{:010}", 0));
    policy.limits.max_retained_bytes = history_id.len() as u64;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID"));
}

#[test]
fn history_state_vector_refuses_collection_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM delta state"));
}

#[test]
fn history_parent_copy_refuses_retained_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let history_id = crate::ids::native_scoped_id("history", "asm-history", format_args!("{:010}", 0));
    let state_id = crate::ids::native_scoped_id("history", "asm-delta-state", format_args!("{:010}", 0));
    policy.limits.max_retained_bytes = (history_id.len() + state_id.len()) as u64;
    let error = decode_with_limits(&bytes, &policy);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ASM history parent"));
}

#[test]
fn history_delta_offsets_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = [super::super::DELTA, super::super::DELTA].concat();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = super::super::decode(
        &ctx,
        &bytes,
        "history",
        cadmpeg_asm::kernel_header::RefWidth::Four,
        &policy.limits,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "f3d history delta offsets"
    ));
}

#[test]
fn history_binding_work_budget_charges_state_record_cross_product() {
    let desktop = cadmpeg_core::decode::ResourceLimits::desktop();
    let desktop_entries = desktop.max_work_units / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY;
    assert!(!history_topology_work_budget_exceeded(
        [usize::try_from(desktop_entries).expect("desktop entry budget fits usize")],
        &desktop
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::try_from(desktop_entries + 1).expect("desktop entry overflow fits usize")],
        &desktop
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::MAX, 1],
        &desktop
    ));

    let service = cadmpeg_core::decode::ResourceLimits::service();
    let service_entries = service.max_work_units / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY;
    assert!(!history_topology_work_budget_exceeded(
        [usize::try_from(service_entries).expect("service entry budget fits usize")],
        &service
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::try_from(service_entries + 1).expect("service entry overflow fits usize")],
        &service
    ));
}
