// SPDX-License-Identifier: Apache-2.0
//! Empty B-rep sources execute no admitted source step.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use super::super::{component_is_closed, merge_body_components, split_neutral_component_shells};

#[test]
fn empty_brep_traversals_need_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(merge_body_components(&ctx, Vec::new()).expect("empty components").is_empty());
    assert!(component_is_closed(
        &ctx, &BTreeSet::new(), &BTreeSet::new(), &BTreeMap::new(), &[],
    ).expect("empty closed component"));
    assert!(split_neutral_component_shells(
        &ctx, &[], &BTreeSet::new(), &BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new(),
    ).expect("empty shell partition").is_empty());
}

#[test]
fn empty_brep_traversals_preserve_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "prior B-rep work")
        .expect_err("seed refusal") else {
        panic!("resource refusal");
    };
    assert!(matches!(merge_body_components(&ctx, Vec::new()),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert!(matches!(component_is_closed(
        &ctx, &BTreeSet::new(), &BTreeSet::new(), &BTreeMap::new(), &[],
    ), Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert!(matches!(split_neutral_component_shells(
        &ctx, &[], &BTreeSet::new(), &BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new(),
    ), Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}
