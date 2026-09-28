// SPDX-License-Identifier: Apache-2.0
//! Decode-owner external-reference projection tests.

use super::super::{
    report_xref_parse_loss, report_xref_placement_failures, report_xref_placement_overrides,
};
use crate::loss::F3dLossCode;

fn placement_table() -> crate::xref::XrefTable {
    crate::xref::XrefTable {
        designs: Vec::new(),
        references: vec![crate::records::xref::XrefReference {
            id: "f3d:xref:reference#4-occurrence-0".into(),
            ordinal: 4,
            occurrence_ordinal: 0,
            from: "root.f3d".into(),
            relative_path: "part.f3d".into(),
            neutron_role: "role-guid".into(),
            neutron_data: "data-guid".into(),
            transform: None,
        }],
        placement_failures: Vec::new(),
        placement_overrides: vec![crate::xref::PlacementOverride {
            ordinal: 4,
            count: 2,
        }],
    }
}

#[test]
fn superseded_xref_placements_have_a_distinct_loss_note() {
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    let table = placement_table();

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    report_xref_placement_overrides(&ctx, &mut report, &table).unwrap();

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::XrefPlacementSuperseded.kind())
        .expect("superseded placement loss");
    assert_eq!(loss.message, "2 structured placement record(s) for external occurrence part.f3d and role role-guid were superseded by scope-bound Component Insert carrier(s)");
}

#[test]
fn xref_placement_override_loss_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = report_xref_placement_overrides(&ctx, &mut report, &placement_table()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D xref placement losses"));
}

#[test]
fn xref_placement_failure_loss_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let mut table = placement_table();
    table.placement_failures.push(4);
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = report_xref_placement_failures(&ctx, &mut report, &table).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D xref placement losses"));
}

#[test]
fn xref_parse_loss_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = report_xref_parse_loss(
        &ctx,
        &mut report,
        &cadmpeg_core::CodecError::malformed("invalid xref"),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D xref parse loss"));
}

#[test]
fn assembly_property_note_preserves_role_and_data_text() {
    let table = placement_table();
    assert_eq!(
        super::super::XrefPropertyNote(&table.references[0]).to_string(),
        "neutronRole role-guid, neutronData data-guid"
    );
}

#[test]
fn assembly_note_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let table = placement_table();
    let error = super::super::push_decode_note(
        &ctx,
        &mut report,
        format_args!("xref {}", super::super::XrefPropertyNote(&table.references[0])),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D decode notes"));
}
