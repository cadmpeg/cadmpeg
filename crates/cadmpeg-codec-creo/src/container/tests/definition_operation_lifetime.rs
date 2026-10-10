// SPDX-License-Identifier: Apache-2.0

use super::super::{feature_definitions, Section};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SECTION_HEADER: &[u8] = b"#DEPDB_DATA\n";
const ROOT: &[u8] = b"\xe0\x00p_dep_db\0\xe3";

fn definition_section(payload: &[u8]) -> Vec<u8> {
    let mut bytes = SECTION_HEADER.to_vec();
    bytes.extend_from_slice(ROOT);
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn temporary_definition_operations_release_owned_names() {
    let bytes = definition_section(b"Round id 7\0");
    let section = Section::scan_for_test("DEPDB_DATA".into(), 0, bytes.len(), None, &bytes)
        .expect("bounded definition section");
    let cap = crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, |allowed| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = allowed;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        feature_definitions(&ctx, std::slice::from_ref(&section))
    });
    for nested in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut parent = ctx.reserve_scoped(0, "test definition parent").expect("parent");
        let definitions = if nested {
            parent.with_storage(|| feature_definitions(&ctx, std::slice::from_ref(&section)))
        } else {
            feature_definitions(&ctx, std::slice::from_ref(&section))
        }.expect("operation names are temporary and no definition is stored");
        assert!(definitions.is_empty());
        ctx.reserve_scoped(cap, "after temporary definition operations")
            .expect("all operation storage ended at its last use");
    }
    let refusal = crate::test_support::last_refusal_at(&[], ResourceDimension::MaterializedBytes,
        "creo operation display counts", |ctx| feature_definitions(ctx, std::slice::from_ref(&section)));
    assert!(matches!(refusal, CodecError::ResourceLimit(ref resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo operation display counts"));
}

#[test]
fn temporary_definition_owner_selection_keeps_native_identity() {
    const OPERATION: &[u8] = b"protextrude\0Extrude id 7\0";
    const DEFINITION: &[u8] = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0";
    let mut payload = OPERATION.to_vec();
    payload.extend_from_slice(DEFINITION);
    let bytes = definition_section(&payload);
    let section = Section::scan_for_test("DEPDB_DATA".into(), 0, bytes.len(), None, &bytes)
        .expect("bounded definition section");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let definitions = feature_definitions(&ctx, std::slice::from_ref(&section))
        .expect("complete section definition and operation owner");
    let [definition] = definitions.as_slice() else {
        panic!("one native section definition");
    };
    assert_eq!(definition.identity.id(), 2);
    assert_eq!(definition.identity.owner_feature_id(), Some(7));
    assert_eq!(definition.offset, SECTION_HEADER.len() + ROOT.len() + OPERATION.len());
}
