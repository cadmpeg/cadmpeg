// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::native_loop_ring;

fn native_loop() -> crate::topology::Loop {
    crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: vec![
            crate::topology::HalfEdgeId {
                curve_id: 10,
                side: crate::topology::Side::Zero,
            },
            crate::topology::HalfEdgeId {
                curve_id: 11,
                side: crate::topology::Side::One,
            },
        ],
    }
}

fn ring_result(collection_limit: u64, retained_limit: u64) -> Result<cadmpeg_ir::topology::LoopRing, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    native_loop_ring(&ctx, &native_loop(), 5)
}

fn assert_refusal(error: CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation), "{error:?}");
}

#[test]
fn brep_ring_coedge_ids_refuse_collection_limit() {
    assert_refusal(
        ring_result(0, u64::MAX).err().expect("ring Vec refused"),
        ResourceDimension::CollectionItems,
        "creo B-rep ring coedge IDs",
    );
}

#[test]
fn brep_ring_coedge_identities_refuse_retained_limit() {
    assert_refusal(
        ring_result(16, 0).err().expect("ring ID refused"),
        ResourceDimension::RetainedBytes,
        "creo B-rep ring coedge identities",
    );
}

#[test]
fn brep_ring_validation_nodes_refuse_collection_limit() {
    assert_refusal(
        ring_result(2, u64::MAX).err().expect("validation node refused"),
        ResourceDimension::CollectionItems,
        "creo native loop ring validation nodes",
    );
}

#[test]
fn brep_native_loop_ring_preserves_service_order() {
    let ring = ring_result(16, u64::MAX).expect("service native ring admitted");
    assert_eq!(ring.coedges().iter().map(cadmpeg_ir::ids::CoedgeId::as_str).collect::<Vec<_>>(),
        ["creo:visibgeom:coedge#10:0", "creo:visibgeom:coedge#11:1"]);
}

#[test]
fn brep_loop_ring_error_refuses_retained_text_limit() {
    let native = crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = native_loop_ring(&ctx, &native, 5)
        .expect_err("loop error text exceeds retained limit");
    assert_refusal(error, ResourceDimension::RetainedBytes, "creo B-rep loop ring error text");
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = native_loop_ring(ctx, &native, 5)
            .expect_err("empty loop is malformed");
        assert!(error.to_string().contains("VisibGeom face 5 loop ring"));
        Ok::<(), CodecError>(())
    }).expect("service error text admitted");
}
