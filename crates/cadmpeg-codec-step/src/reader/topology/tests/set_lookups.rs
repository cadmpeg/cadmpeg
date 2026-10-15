// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::BodyId;

#[test]
fn existing_topology_body_group_preserves_lookup_refusal() {
    let body = BodyId::try_from("step:data:body#1").unwrap();
    let groups = BTreeMap::from([(1_u64, BTreeSet::from([body.clone()]))]);
    let before = groups.clone();
    // The numeric group lookup costs 8 units before the 16-unit body membership.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP topology body group membership",
        |cap| {
            let mut groups = before.clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::insert_topology_body_group(
                &mut groups,
                1,
                &body,
                &ctx,
                "test body groups",
                "test body members",
            );
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*limit));
            }
            assert_eq!(groups, before);
            result
        },
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("existing body lookup must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP topology body group membership");
    assert_eq!(groups, before);
}
