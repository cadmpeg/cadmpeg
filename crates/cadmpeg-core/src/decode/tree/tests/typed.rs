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

fn pass_work(ctx: &DecodeContext<'_>, text: &str, bound: &JsonBound) -> u64 {
    ctx.json_pass_work(text, bound, "typed JSON tree")
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
    let parser_work = pass_work(&probe, text, &bound);
    let typed_work = pass_work(&probe, text, &bound);
    let conversion_work = probe
        .json_conversion_work(text, &bound, "typed JSON tree")
        .expect("small fixture work fits");
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
    assert_eq!(limit.additional, conversion_work);
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

#[test]
fn json_object_work_grows_with_key_search_depth_not_entry_count() {
    let entries = (0..1000)
        .map(|index| format!("\"k{index}\":{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let text = format!("{{{entries}}}");
    let length = u64_from_index(text.len());
    // One bounding scan, one parse pass (a scan, a string copy and at most
    // eleven comparisons per level of a 1000-key B-tree search path) and one
    // nesting level.
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = length * (1 + 2 + 11 * 10) + 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
    let (value, _storage) = ctx
        .parse_json_value(&text, "wide JSON object")
        .expect("a wide object parses within its key-search bound");
    assert_eq!(value.as_object().map(serde_json::Map::len), Some(1000));
}
