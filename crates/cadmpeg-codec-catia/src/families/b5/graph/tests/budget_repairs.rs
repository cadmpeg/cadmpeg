// SPDX-License-Identifier: Apache-2.0
//! B5 budget and indexing regressions.

#[test]
fn b5_topology_visitor_admits_only_the_next_candidate() {
    let triangle = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut bytes = Vec::new();
    for _ in 0..4 {
        bytes.extend_from_slice(&triangle);
        bytes.extend([0; 16]);
    }
    let refusal =
        crate::test_support::with_work_refusal("catia_b5_topology_candidate_scan", |ctx| {
            super::super::visit_topology_runs(
                ctx,
                &bytes,
                &mut crate::nurbs::LaneRefusals::new(),
                |_, _| Ok(false),
            )
        });
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = refusal else {
        panic!("work refusal")
    };
    assert_eq!(limit.additional, 1);
    let mut visited = 0;
    crate::test_support::with_service_context(|ctx| {
        super::super::visit_topology_runs(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
            |_, graph| {
                assert!(!graph.faces.is_empty());
                visited += 1;
                Ok(false)
            },
        )
    })
    .expect("closed candidate graph");
    assert_eq!(visited, 1);
}
