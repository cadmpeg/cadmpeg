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
