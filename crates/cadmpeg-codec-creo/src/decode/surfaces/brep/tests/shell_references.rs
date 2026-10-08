// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{FaceId, ShellId};

use super::super::{BrepShellReferences, NeutralShellSpec};

fn references_result(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<BrepShellReferences, CodecError> {
    let shell = NeutralShellSpec {
        faces: vec![5],
        wire_curves: BTreeSet::from([10]),
    };
    let shell_id = ShellId::compose(&crate::identity::VISIBGEOM_SHELL, 1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let mut storage = ctx.reserve_scoped(0, "shell references workspace")?;
    BrepShellReferences::from_shell(&ctx, &shell, &shell_id, &mut BTreeMap::new(), &mut storage)
}

fn assert_refusal(error: &CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn brep_face_shell_nodes_refuse_collection_limit() {
    assert_refusal(
        &references_result(
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo B-rep face-shell nodes"),
                |cap| references_result(cap, u64::MAX),
            ),
            u64::MAX,
        )
        .err()
        .expect("node refused"),
        ResourceDimension::CollectionItems,
        "creo B-rep face-shell nodes",
    );
}

#[test]
fn brep_shell_face_references_refuse_collection_limit() {
    assert_refusal(
        &references_result(
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo B-rep shell face references"),
                |cap| references_result(cap, u64::MAX),
            ),
            u64::MAX,
        )
        .err()
        .expect("face refused"),
        ResourceDimension::CollectionItems,
        "creo B-rep shell face references",
    );
}

#[test]
fn brep_shell_face_identities_refuse_retained_limit() {
    assert_refusal(
        &references_result(
            16,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some("creo B-rep shell face identities"),
                |cap| references_result(u64::MAX, cap),
            ),
        )
        .err()
        .expect("face ID refused"),
        ResourceDimension::RetainedBytes,
        "creo B-rep shell face identities",
    );
}

#[test]
fn brep_shell_edge_references_refuse_collection_limit() {
    assert_refusal(
        &references_result(
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo B-rep shell edge references"),
                |cap| references_result(cap, u64::MAX),
            ),
            u64::MAX,
        )
        .err()
        .expect("edge refused"),
        ResourceDimension::CollectionItems,
        "creo B-rep shell edge references",
    );
}

#[test]
fn brep_shell_edge_identities_refuse_retained_limit() {
    assert_refusal(
        &references_result(
            16,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some("creo B-rep shell edge identities"),
                |cap| references_result(u64::MAX, cap),
            ),
        )
        .err()
        .expect("edge ID refused"),
        ResourceDimension::RetainedBytes,
        "creo B-rep shell edge identities",
    );
}

#[test]
fn brep_shell_references_preserve_service_order() {
    let references =
        references_result(u64::MAX, u64::MAX).expect("service shell references admitted");
    assert_eq!(
        references.face_ids,
        vec![FaceId::compose(&crate::identity::VISIBGEOM_FACE, 5)]
    );
    assert_eq!(references.edge_ids.len(), 1);
    assert_eq!(references.edge_ids[0].as_str(), "creo:visibgeom:edge#10");
}
