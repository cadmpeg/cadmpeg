// SPDX-License-Identifier: Apache-2.0
//! Scratch candidates that have no complete edge-run consumer.

#[test]
fn unclaimed_pcurves_release_storage() {
    let bytes = crate::test_support::test_b2::b2_pcurve_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..64 {
            assert!(
                super::consolidated_edge_blocks_from_records(ctx, &bytes, &records)
                    .expect("unclaimed pcurve scratch")
                    .is_empty()
            );
        }
    });
}

#[test]
fn blocks_without_topology_runs_release_storage() {
    let bytes = crate::test_support::test_b2::b2_edge_block_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::consolidated_edge_blocks_from_records(ctx, &bytes, &records)
        })
        .expect("one complete block")
        .len(),
        1
    );
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..64 {
            assert!(
                super::consolidated_topology_edge_runs_from_records(ctx, &bytes, &records)
                    .expect("unclaimed block scratch")
                    .is_empty()
            );
        }
    });
}

#[test]
fn unconsumed_nurbs_carriers_release_storage() {
    let bytes = crate::test_support::test_a5a8::a5_surface_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..64 {
            assert!(super::resolve_consolidated_edge_blocks_from_records(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new()
            )
            .expect("carrier scratch")
            .is_empty());
        }
    });
}

#[test]
fn unconsumed_carriers_build_no_resolution_indexes() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let mut bytes = crate::test_support::test_b2::b2_circle_stream();
    bytes.extend(crate::test_support::test_a5a8::a5_surface_stream());
    let records = crate::wire::records::consolidated_records(&bytes);
    for operation in [
        "catia_resolved_carrier_identities",
        "catia_consolidated_carrier_positions",
    ] {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        assert!(crate::test_support::with_work_limit(u64::MAX, |ctx| {
            super::resolve_consolidated_edge_blocks_from_records(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("no resolution-index consumer")
        .is_empty());
    }
}
