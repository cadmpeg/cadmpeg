//! Triangle cache storage remains charged while its trim packet is live.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;

fn two_face_stream(closed_tetrahedron: bool) -> Vec<u8> {
    let mut bytes = vec![0x01, 0x43, if closed_tetrahedron { 5 } else { 1 }, 6, 0xff];
    bytes.extend_from_slice(&15_u32.to_le_bytes());
    bytes.extend_from_slice(&[if closed_tetrahedron { 0 } else { 2 }; 6]);
    for handle in [0_u16, 1, 2, 3, 4, 5, 3, 6, 4, 4, 6, 5, 5, 6, 3] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes.extend_from_slice(&[0x01, 0x41, 200, 0xff]);
    bytes.extend_from_slice(&600_u32.to_le_bytes());
    for handle in 20_u16..620 {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    for _ in 0..2 {
        bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    }
    // Reuse the complete edge/vertex tables, independently of the first
    // packet's closed tetrahedron or ignored two-handle strips.
    let quad = crate::test_support::test_topology::standard_quad_topology_stream();
    let edge_table = quad.windows(3).position(|part| part == [0x01, 0x01, 0x04])
        .expect("synthetic edge table");
    bytes.extend_from_slice(&quad[edge_table..]);
    bytes
}

fn second_face_refusal(bytes: &[u8]) -> ResourceLimit {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("synthetic input admitted");
    let probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes, "catia_boundary_edge_directions", None,
    );
    let CodecError::ResourceLimit(limit) = super::super::standard_mesh_analysis(&ctx, bytes)
        .expect_err("the second face's boundary map raises the live peak")
    else {
        panic!("materialized storage refusal");
    };
    drop(probe);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "catia_boundary_edge_directions");
    assert!(limit.additional > 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(super::super::standard_mesh_analysis(&ctx, bytes),
        Err(CodecError::ResourceLimit(actual)) if actual == limit));
    limit
}

#[test]
fn mesh_analysis_holds_prior_face_triangle_cache_until_trim_destruction() {
    let plain = two_face_stream(false);
    let closed = two_face_stream(true);
    for bytes in [&plain, &closed] {
        crate::test_support::with_service_context(|ctx| {
            let analysis = super::super::standard_mesh_analysis(ctx, bytes)
                .expect("service resource budget").expect("complete two-face analysis");
            assert_eq!(analysis.cycles.len(), 2);
            assert_eq!(analysis.cycles[0], [vec![0, 1, 2]]);
            assert_eq!(analysis.cycles[1].len(), 200);
        });
    }
    let plain_refusal = second_face_refusal(&plain);
    let closed_refusal = second_face_refusal(&closed);
    // The packets have equal handle/length capacities and identical first
    // boundaries. Amortized reservation gives the one-triangle cache four
    // slots and the five-triangle cache five slots.
    assert_eq!(closed_refusal.used - plain_refusal.used,
        std::mem::size_of::<[u32; 3]>() as u64);
}
