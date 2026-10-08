// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{LoopId, ShellId};

use super::super::BrepFaceReferences;



fn references_result(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<BrepFaceReferences, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    BrepFaceReferences::from_loops(
        &ctx,
        5,
        &ShellId::compose(&crate::identity::VISIBGEOM_SHELL, 1),
        2,
    )
}

fn assert_refusal(error: &CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn brep_face_identity_refuses_retained_limit() {
    assert_refusal(
        &references_result(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo B-rep face identity"), |cap| references_result(u64::MAX, cap))).err().expect("face ID refused"),
        ResourceDimension::RetainedBytes,
        "creo B-rep face identity",
    );
}

#[test]
fn brep_face_shell_identity_copy_refuses_retained_limit() {
    assert_refusal(
        &references_result(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo B-rep face shell identity copy"), |cap| references_result(u64::MAX, cap))).err().expect("shell ID refused"),
        ResourceDimension::RetainedBytes,
        "creo B-rep face shell identity copy",
    );
}

#[test]
fn brep_face_loop_ids_refuse_collection_limit() {
    assert_refusal(
        &references_result(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo B-rep face loop IDs"), |cap| references_result(cap, u64::MAX)), u64::MAX)
            .err()
            .expect("loop Vec refused"),
        ResourceDimension::CollectionItems,
        "creo B-rep face loop IDs",
    );
}

#[test]
fn brep_loop_identities_refuse_retained_limit() {
    assert_refusal(
        &references_result(
            16,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some("creo B-rep loop identities"),
                |cap| references_result(u64::MAX, cap),
            ),
        )
        .err()
        .expect("loop ID refused"),
        ResourceDimension::RetainedBytes,
        "creo B-rep loop identities",
    );
}

#[test]
fn brep_outer_loop_id_copy_refuses_retained_limit() {
    assert_refusal(
        &references_result(
            16,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some("creo B-rep outer loop ID copy"),
                |cap| references_result(u64::MAX, cap),
            ),
        )
        .err()
        .expect("outer copy refused"),
        ResourceDimension::RetainedBytes,
        "creo B-rep outer loop ID copy",
    );
}

#[test]
fn brep_inner_loop_ids_refuse_collection_limit() {
    assert_refusal(
        &references_result(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo B-rep inner loop IDs"), |cap| references_result(cap, u64::MAX)), u64::MAX)
            .err()
            .expect("inner Vec refused"),
        ResourceDimension::CollectionItems,
        "creo B-rep inner loop IDs",
    );
}

#[test]
fn brep_inner_loop_id_copies_refuse_retained_limit() {
    assert_refusal(
        &references_result(
            16,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some("creo B-rep inner loop ID copies"),
                |cap| references_result(u64::MAX, cap),
            ),
        )
        .err()
        .expect("inner copy refused"),
        ResourceDimension::RetainedBytes,
        "creo B-rep inner loop ID copies",
    );
}

#[test]
fn brep_face_references_preserve_service_loop_order() {
    let references = references_result(u64::MAX, u64::MAX).expect("service face references admitted");
    assert_eq!(references.face.as_str(), "creo:visibgeom:face#5");
    assert_eq!(references.shell_id.as_str(), "creo:visibgeom:shell#1");
    assert_eq!(
        references
            .loop_ids
            .iter()
            .map(LoopId::as_str)
            .collect::<Vec<_>>(),
        ["creo:visibgeom:loop#5", "creo:visibgeom:loop#5:1"]
    );
    assert_eq!(
        references
            .face_loops
            .iter()
            .map(LoopId::as_str)
            .collect::<Vec<_>>(),
        ["creo:visibgeom:loop#5", "creo:visibgeom:loop#5:1"]
    );
}

#[test]
fn brep_face_loop_index_traversal_refuses_work_and_preserves_service_order() {
    let shell_id = ShellId::compose(&crate::identity::VISIBGEOM_SHELL, 1);
    let references = crate::test_support::assert_work_boundaries(
        &["creo B-rep face loop index traversal"],
        |ctx| BrepFaceReferences::from_loops(ctx, 5, &shell_id, 2),
    );
    assert_eq!(
        references
            .loop_ids
            .iter()
            .map(LoopId::as_str)
            .collect::<Vec<_>>(),
        ["creo:visibgeom:loop#5", "creo:visibgeom:loop#5:1"]
    );
    assert_eq!(
        references
            .face_loops
            .iter()
            .map(LoopId::as_str)
            .collect::<Vec<_>>(),
        ["creo:visibgeom:loop#5", "creo:visibgeom:loop#5:1"]
    );
}
