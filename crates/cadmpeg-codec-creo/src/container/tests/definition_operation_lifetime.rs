// SPDX-License-Identifier: Apache-2.0

use super::super::{feature_definitions, Section};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SECTION_HEADER: &[u8] = b"#DEPDB_DATA\n";
const ROOT: &[u8] = b"\xe0\x00p_dep_db\0\xe3";
// The materialized allowance for an empty root is the policy base.
const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;

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
    for nested in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut parent = ctx.reserve_scoped(0, "test definition parent").expect("parent");
        let definitions = if nested {
            parent.with_storage(|| feature_definitions(&ctx, std::slice::from_ref(&section)))
        } else {
            feature_definitions(&ctx, std::slice::from_ref(&section))
        }.expect("operation names are temporary and no definition is stored");
        assert!(definitions.is_empty());
        // The full materialized allowance is free while the parent remains live.
        // This probe changes only the resource counter and allocates no bytes.
        let probe = ctx.reserve_scoped(EMPTY_ROOT_MATERIALIZED_BYTES, "after temporary definition operations")
            .expect("all operation storage ended at its last use");
        drop(probe);
        drop((definitions, parent));
        let original = ctx.charge_retained_limit(1, "after temporary definition names")
            .expect_err("zero retained cap");
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((original.used, original.additional), (0, 1));
        assert!(matches!(feature_definitions(&ctx, &[]),
            Err(CodecError::ResourceLimit(refusal)) if refusal == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let result = feature_definitions(&ctx, std::slice::from_ref(&section));
    let Err(CodecError::ResourceLimit(original)) = result else {
        panic!("the actual first operation table needs materialized storage");
    };
    // The first display count uses a four-bucket (u32, usize) table with
    // alignment padding, four control bytes and one sixteen-byte control group.
    let table_bytes = 4 * std::mem::size_of::<(u32, usize)>()
        + std::mem::align_of::<(u32, usize)>().max(16) - 1 + 4 + 16;
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(original.operation, "creo operation display counts");
    assert_eq!((original.used, original.additional),
        (0, u64::try_from(table_bytes).expect("fixture table bound")));
    assert!(matches!(feature_definitions(&ctx, &[]),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
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
