#[test]
fn standard_topology_entity_limit_propagates_before_fallback() {
    let bytes = crate::test_support::test_container::tetrahedron_topology_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("scan the container");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // One retained carrier, four surfaces, four points, four vertices,
    // four faces, and one body, region, and shell precede the first edge.
    policy.limits.max_entities = 20;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("topology fixture fits the input-byte limit");
    let result = crate::families::standard::decode::try_decode_standard(
        &ctx,
        &scan,
        &mut crate::nurbs::LaneRefusals::new(),
    );
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("first topology entity must exceed the entity limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(limit.used, 20);
    assert_eq!(limit.operation, "admit CATIA family model entity");
}

/// Complete `duplicates` parallel edges between points 0 and 1, edge `i` known
/// on face `i`, beside one two-face edge whose endpoints no edge closes. Every
/// pairing of the duplicates with their faces closes points 0 and 1 and leaves
/// the last two faces open, so the search tries all `duplicates!` pairings.
fn unclosable_duplicate_face_completion(
    topology_budget: &cadmpeg_core::decode::WorkBudget,
    duplicates: usize,
) -> Option<Vec<[usize; 2]>> {
    use crate::families::standard::topology::{
        complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow, StandardIncidenceSearch,
    };

    let rows = (0u32..)
        .take(duplicates + 1)
        .map(|handle| {
            EdgeRow::new(
                1,
                vec![handle, handle],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let edge_faces = (0..duplicates)
        .map(|face| [face, face])
        .chain([[duplicates, duplicates + 1]])
        .collect::<Vec<_>>();
    let edge_points = (0..duplicates)
        .map(|_| [0, 1])
        .chain([[2, 3]])
        .collect::<Vec<_>>();
    crate::test_support::with_service_context(|ctx| {
        complete_duplicate_face_slots(
            ctx,
            &rows,
            &edge_faces,
            &edge_points,
            duplicates + 2,
            StandardIncidenceSearch {
                budget: topology_budget,
                edge_classes: None,
                mesh_bytes: None,
            },
        )
    })
    .expect("a bounded search is not a decode error")
}

#[test]
fn duplicate_face_bound_exhausts_the_topology_search() {
    use crate::families::standard::decode::StandardTopologyFailure;

    // Seven duplicates try 13,699 face assignments and eight try 109,600,
    // either side of `MAX_DUPLICATE_FACE_OPERATIONS`.
    let complete = crate::test_support::topology_budget();
    assert_eq!(unclosable_duplicate_face_completion(&complete, 7), None);
    assert!(!complete.exhausted());
    assert_eq!(
        StandardTopologyFailure::unsolved(complete.exhausted(), None),
        StandardTopologyFailure::NoTopologySolution
    );

    let bounded = crate::test_support::topology_budget();
    assert_eq!(unclosable_duplicate_face_completion(&bounded, 8), None);
    assert!(bounded.exhausted());
    let failure = StandardTopologyFailure::unsolved(bounded.exhausted(), None);
    assert_eq!(failure, StandardTopologyFailure::TopologySearchExhausted);
    assert!(failure.message().contains("exhausted its operation"));
}

#[test]
fn native_port_binding_bound_leaves_the_ports_unbound_with_a_loss() {
    use crate::families::standard::decode::native_port_endpoint_pairs;

    let ports = [[0, 1], [1, 2]];
    let bind = |choices: usize, refusal: &mut crate::nurbs::LaneRefusals| {
        let options = (0..ports.len())
            .map(|_| {
                (0..choices)
                    .map(|point| [point, point + 1])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        crate::test_support::with_service_context(|ctx| {
            native_port_endpoint_pairs(ctx, &options, &ports, &[], refusal)
        })
        .expect("a bounded binding is not a decode error")
    };

    let mut within = crate::nurbs::LaneRefusals::new();
    bind(2, &mut within);
    assert_eq!(within.note_count(), 0);

    // Two edges of 32,769 choices state 65,538, past `MAX_NATIVE_PORT_CHOICES`.
    let mut bounded = crate::nurbs::LaneRefusals::new();
    assert_eq!(bind(32_769, &mut bounded), None);
    let notes = bounded.take_notes();
    let [note] = notes.as_slice() else {
        panic!("one bounded native-port binding");
    };
    assert_eq!(
        note.code,
        crate::loss::CatiaLossCode::TopologyStandardSearchBounded.kind()
    );
    assert!(note.message.contains("bound of 65536 endpoint choices"));
    assert!(note.message.contains("2 edges state 65538 choices"));
}

#[test]
fn local_topology_search_exhaustion_leaves_the_candidate_unsolved() {
    use crate::families::standard::decode::StandardTopologyFailure;
    use crate::solve::mesh_quotient::{
        MeshCandidateAmbiguity, MeshCandidateExhaustion, MeshCandidateFailure,
        MeshCandidateRejection,
    };

    let exhausted = Some(MeshCandidateFailure::Exhausted(
        MeshCandidateExhaustion::EndpointResolution,
    ));
    let ambiguous = Some(MeshCandidateFailure::Ambiguous(
        MeshCandidateAmbiguity::EndpointResolution,
    ));
    let rejected = Some(MeshCandidateFailure::Rejected(
        MeshCandidateRejection::InputStructure,
    ));
    for (search_exhausted, mesh_failure, expected) in [
        (true, None, StandardTopologyFailure::TopologySearchExhausted),
        (
            true,
            ambiguous,
            StandardTopologyFailure::TopologySearchExhausted,
        ),
        (
            false,
            exhausted,
            StandardTopologyFailure::TopologySearchExhausted,
        ),
        (
            false,
            ambiguous,
            StandardTopologyFailure::AmbiguousTopologySolution,
        ),
        (false, rejected, StandardTopologyFailure::NoTopologySolution),
        (false, None, StandardTopologyFailure::NoTopologySolution),
    ] {
        assert_eq!(
            StandardTopologyFailure::unsolved(search_exhausted, mesh_failure),
            expected
        );
    }
}
