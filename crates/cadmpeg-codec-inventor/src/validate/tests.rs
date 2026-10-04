// SPDX-License-Identifier: Apache-2.0
use super::equal_hash_sets;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashSet;

#[test]
fn hash_set_equality_matches_contents_across_insertion_order() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("context");
    let left = HashSet::from(["north", "south"]);
    let equal = HashSet::from(["south", "north"]);
    let different = HashSet::from(["north", "east"]);

    assert!(equal_hash_sets(&ctx, &left, &equal, "compare test sets").expect("equal sets"));
    assert!(!equal_hash_sets(&ctx, &left, &different, "compare test sets").expect("different sets"));
}

#[test]
fn hash_set_equality_propagates_work_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let left = HashSet::from(["north"]);
    let right = HashSet::from(["north"]);

    assert!(matches!(
        equal_hash_sets(&ctx, &left, &right, "compare test sets"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "compare test sets"
    ));
}
