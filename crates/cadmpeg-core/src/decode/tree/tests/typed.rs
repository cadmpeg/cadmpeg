// SPDX-License-Identifier: Apache-2.0
use super::super::JsonBound;
use crate::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

static DESERIALIZE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct Counted;

impl<'de> Deserialize<'de> for Counted {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let _source = String::deserialize(deserializer)?;
        DESERIALIZE_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(Self)
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct StandardCollections {
    hash_map: HashMap<String, u32>,
    hash_set: HashSet<u32>,
    btree_map: BTreeMap<String, u32>,
    btree_set: BTreeSet<String>,
}

fn checked_work(value: Option<u64>) -> u64 {
    value.expect("small fixture work fits")
}

fn typed_pass_work(ctx: &DecodeContext<'_>, text: &str, bound: &JsonBound) -> u64 {
    ctx.typed_json_work(text, bound, "typed JSON tree")
        .expect("small fixture work fits")
}

#[test]
fn typed_json_conversion_precharges_before_second_deserializer_call() {
    let text = r#""source""#;
    let probe_arena = DecodeArena::new();
    let (probe, _) = DecodeContext::from_root_bytes(b"", &probe_arena, &DecodePolicy::default())
        .expect("valid fixture");
    let bound = probe
        .json_bound(text, "typed JSON tree")
        .expect("JSON bound");
    let input_len = u64_from_index(text.len());
    let parser_work = input_len
        .checked_mul(checked_work(bound.entries.checked_add(1)))
        .and_then(|work| work.checked_mul(checked_work(bound.depth.checked_add(1))))
        .expect("small fixture work fits");
    let typed_work = typed_pass_work(&probe, text, &bound);
    let before_conversion = input_len
        .checked_add(parser_work)
        .and_then(|work| work.checked_add(typed_work))
        .expect("small fixture work fits");

    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = before_conversion;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
    DESERIALIZE_CALLS.store(0, Ordering::Relaxed);
    let error = ctx
        .parse_json::<Counted>(text, "typed JSON tree")
        .expect_err("second typed pass exceeds its work admission");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("typed conversion must preserve its resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.used, before_conversion);
    assert_eq!(limit.additional, typed_work);
    assert_eq!(limit.operation, "typed JSON tree");
    assert_eq!(DESERIALIZE_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn typed_json_standard_string_and_scalar_collections_succeed() {
    let text = r#"{"hash_map":{"a":1,"b":2},"hash_set":[3,4],"btree_map":{"c":5,"d":6},"btree_set":["e","f"]}"#;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
        .expect("valid fixture");
    let value: StandardCollections = ctx
        .parse_json(text, "typed JSON tree")
        .expect("standard key callbacks are covered by the typed source bound");
    assert_eq!(value.hash_map.len(), 2);
    assert!(value.hash_set.contains(&3));
    assert_eq!(value.btree_map.get("d"), Some(&6));
    assert!(value.btree_set.contains("f"));
}
