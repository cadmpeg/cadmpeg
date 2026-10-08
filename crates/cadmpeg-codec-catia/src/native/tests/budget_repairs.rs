// SPDX-License-Identifier: Apache-2.0
//! Native projection storage and traversal admission.

use crate::test_support::{
    with_materialized_limit, with_retained_limit, with_service_context, with_work_limit,
};
use cadmpeg_core::{decode::ResourceDimension, CodecError};

#[test]
fn native_rejected_signature_releases_speculative_storage() {
    with_materialized_limit(4096, |ctx| -> Result<_, CodecError> {
        for source in [
            "(#1_ : #In Real,bad) : Real",
            "(#1_ : #In Real,#1_ : #In Real) : Real",
        ] {
            assert!(super::super::relation_type_signature_charged(ctx, None, source)?.is_none());
            let storage = ctx.reserve_scoped(4096, "test released signature")?;
            drop(storage);
        }
        Ok(())
    })
    .expect("rejected signatures release their input copies");
    with_retained_limit(0, |ctx| -> Result<_, CodecError> {
        assert!(super::super::relation_type_signature_charged(
            ctx,
            None,
            "(#1_ : #In Real,bad) : Real"
        )?
        .is_none());
        Ok(())
    })
    .expect("rejected signature retains no input copies");
}

#[test]
fn native_owner_packet_boxes_refuse_retained_storage() {
    use crate::test_support::test_b2::{
        b2_fixed_owner_boundary_cycle_stream, b2_owner_chart_stream,
    };
    for bytes in [
        b2_owner_chart_stream(0x28),
        b2_fixed_owner_boundary_cycle_stream().0,
    ] {
        let records = crate::wire::records::consolidated_records(&bytes);
        let refused = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "catia_native_owner_packet_box",
            |cap| {
                with_retained_limit(cap, |ctx| {
                    super::super::projection::consolidated_owner_packets(ctx, &bytes, &records)
                })
            },
        );
        assert!(
            matches!(refused, CodecError::ResourceLimit(limit) if limit.operation == "catia_native_owner_packet_box")
        );
        let packets = with_service_context(|ctx| {
            super::super::projection::consolidated_owner_packets(ctx, &bytes, &records)
        })
        .expect("owner packet boxes admitted");
        assert_eq!(packets.len(), 1);
    }
}

#[test]
fn native_vertex_reference_membership_preserves_order_with_linear_work() {
    let native = crate::native::CatiaNative::decode(
        &crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 14, 15),
    );
    let template = &native.consolidated_edge_nodes[0];
    let nodes: Vec<_> = (0..1024)
        .map(|identity| {
            let mut node = template.clone();
            node.endpoint_records = Some([100, 200]);
            node.vertex_refs = [identity, identity + 2000];
            node
        })
        .collect();
    // Two endpoint lookups and insertions, two incident-name comparisons,
    // and geometric table/vector relocation fit within 512 units per node.
    // Scanning the growing reference vectors requires more than 4 million units.
    let vertices = with_work_limit(1024 * 512, |ctx| {
        super::super::edge_node::consolidated_vertex_identities(ctx, &nodes)
    })
    .expect("membership work is linear in distinct distances");
    assert_eq!(vertices.len(), 2);
    assert_eq!(vertices[0].reference_values, (0..1024).collect::<Vec<_>>());
    assert_eq!(
        vertices[1].reference_values,
        (2000..3024).collect::<Vec<_>>()
    );
    let refused = crate::test_support::with_work_refusal(
        "catia_native_vertex_identity_reference_checks",
        |ctx| super::super::edge_node::consolidated_vertex_identities(ctx, &nodes),
    );
    assert!(
        matches!(refused, Err(CodecError::ResourceLimit(limit)) if limit.operation == "catia_native_vertex_identity_reference_checks")
    );
}
