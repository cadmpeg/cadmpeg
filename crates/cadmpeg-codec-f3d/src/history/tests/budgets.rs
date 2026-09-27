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
