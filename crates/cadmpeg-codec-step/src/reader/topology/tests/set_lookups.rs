// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::BodyId;

#[test]
fn existing_topology_body_group_preserves_lookup_refusal() {
    let body = BodyId::try_from("step:data:body#1").unwrap();
    let mut groups = BTreeMap::from([(1_u64, BTreeSet::from([body.clone()]))]);
    let before = groups.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = super::super::insert_topology_body_group(
        &mut groups, 1, &body, &ctx, "test body groups", "test body members",
    ).unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("existing body lookup must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP topology body group membership");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(groups, before);
}
