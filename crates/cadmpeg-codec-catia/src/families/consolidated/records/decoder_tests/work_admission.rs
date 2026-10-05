// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

fn assert_source_work_refusal(
    operation: &'static str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<usize, CodecError>,
) {
    assert_eq!(
        crate::test_support::with_service_context(|ctx| run(ctx))
            .expect("service source fixture produces one run"),
        1
    );
    let result = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = run(ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
    assert!(matches!(result,
        Err(CodecError::ResourceLimit(limit)) if limit.operation == operation));
}

fn assert_topology_index_refusal(operation: &'static str) {
    let bytes = crate::test_support::test_b2::b2_topology_edge_run_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(operation, |ctx| {
        super::super::consolidated_topology_edge_runs_from_records(ctx, &bytes, &records)
            .map(|runs| runs.len())
    });
}

#[test]
fn consolidated_topology_edge_owned_source_preserves_work_refusal() {
    assert_topology_index_refusal("catia_consolidated_topology_edges");
}

#[test]
fn consolidated_topology_use_owned_source_preserves_work_refusal() {
    assert_topology_index_refusal("catia_consolidated_topology_uses");
}

fn record(class: u8, token: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xb2,
        0x03,
        class,
        u8::try_from(payload.len()).expect("fixture payload fits u8"),
        token,
    ];
    bytes.extend_from_slice(payload);
    bytes
}

fn analytic_circle_run() -> Vec<u8> {
    let mut parameter = vec![0x05, 0x00];
    parameter.extend_from_slice(&12.0_f64.to_le_bytes());
    parameter.extend_from_slice(&34.0_f64.to_le_bytes());
    let mut circle = vec![0x05];
    for value in [12.0_f64, 34.0, 5.0, 0.0, 10.0] {
        circle.extend_from_slice(&value.to_le_bytes());
    }
    circle.push(0x01);
    circle.extend_from_slice(&0.0_f64.to_le_bytes());
    let mut definition = vec![0x82, 0x05, 0x09, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 10.0, 0.001, 4.0, 9.0, 1.0, -2.0, 0.001] {
        definition.extend_from_slice(&value.to_le_bytes());
    }
    let mut bytes = record(0x18, 0x15, &parameter);
    bytes.extend_from_slice(&record(0x19, 0x05, &circle));
    bytes.extend_from_slice(&record(0x23, 0x05, &definition));
    bytes.extend_from_slice(
        &crate::test_support::test_a5a8::a5_native_edge_identity_stream(6, 139, 142),
    );
    bytes
}

#[test]
fn analytic_circle_use_owned_source_preserves_work_refusal() {
    let bytes = analytic_circle_run();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal("catia_analytic_circle_use_runs", |ctx| {
        super::super::consolidated_analytic_circle_edge_runs_from_records(ctx, &bytes, &records)
            .map(|runs| runs.len())
    });
}

#[test]
fn analytic_circle_carrier_record_scan_propagates_work_refusal() {
    let bytes = analytic_circle_run();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal("catia_b2_family_record_scan", |ctx| {
        super::super::consolidated_analytic_circle_edge_runs_from_records(ctx, &bytes, &records)
            .map(|runs| runs.len())
    });
}

fn class25_run() -> Vec<u8> {
    let mut descriptor = vec![0x08, 0x34, 0x12, 0x02];
    descriptor.extend_from_slice(&3.0_f64.to_le_bytes());
    descriptor.extend_from_slice(&7.0_f64.to_le_bytes());
    let mut definition = vec![0x82, 0x05, 0xe7, 0x0a, 0x87, 0x0d];
    for value in [1.0_f64, 2.0, 0.001, 3.0, 4.0, 1.0, 5.0, 0.001] {
        definition.extend_from_slice(&value.to_le_bytes());
    }
    let mut bytes = record(0x18, 0x05, &descriptor);
    bytes.extend_from_slice(&record(0x25, 0x05, &definition));
    bytes.extend_from_slice(
        &crate::test_support::test_a5a8::a5_native_edge_identity_stream(6, 139, 142),
    );
    bytes
}

fn assert_class25_index_refusal(operation: &'static str) {
    let bytes = class25_run();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal(operation, |ctx| {
        super::super::consolidated_class25_edge_runs_from_records(ctx, &bytes, &records)
            .map(|runs| runs.len())
    });
}

#[test]
fn class25_descriptor_owned_source_preserves_work_refusal() {
    assert_class25_index_refusal("catia_class25_edge_descriptors");
}

#[test]
fn class25_use_owned_source_preserves_work_refusal() {
    assert_class25_index_refusal("catia_class25_edge_use_runs");
}

#[test]
fn edge_use_metadata_owned_source_preserves_work_refusal() {
    let mut bytes = vec![0xb2, 0x03, 0x24, 0x04, 0x05, 0x81, 0x05, 0x0f, 0x87];
    bytes.extend_from_slice(
        &crate::test_support::test_a5a8::a5_native_edge_identity_stream(6, 139, 142),
    );
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal("catia_edge_use_metadata_index", |ctx| {
        super::super::consolidated_edge_use_runs_from_records(ctx, &bytes, &records)
            .map(|runs| runs.len())
    });
}

#[test]
fn resolved_edge_block_owned_source_preserves_work_refusal() {
    let bytes = crate::test_support::test_a5_bound::a5_cylinder_bound_edge_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal("catia_resolved_edge_blocks", |ctx| {
        super::super::resolve_consolidated_edge_blocks_from_records(
            ctx,
            &bytes,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .map(|runs| runs.len())
    });
}

#[test]
fn resolved_circle_carrier_record_scan_propagates_work_refusal() {
    let mut bytes = crate::test_support::test_a5_bound::a5_cylinder_bound_edge_stream();
    bytes.extend_from_slice(&crate::test_support::test_b2::b2_circle_stream());
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_source_work_refusal("catia_resolved_circle_record_scan", |ctx| {
        super::super::resolve_consolidated_edge_blocks_from_records(
            ctx,
            &bytes,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .map(|runs| runs.len())
    });
}
