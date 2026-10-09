// SPDX-License-Identifier: Apache-2.0

use super::super::{collect_entity_adjacency, collect_owned_ids, collect_references,
    remap_owned_ids, retain_root_entities};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use serde_value::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

fn sequence() -> Value {
    Value::Seq(vec![Value::U8(1), Value::U8(2), Value::U8(3)])
}

fn fields() -> Value {
    Value::Map(BTreeMap::from([
        (Value::U8(1), Value::U8(1)),
        (Value::U8(2), Value::U8(2)),
        (Value::U8(3), Value::U8(3)),
    ]))
}

// Three primitive members execute no child lookup or allocation. One work
// unit visits the first member; the second member refuses one additional unit.
fn boundary(
    mut value: Value,
    mut empty: Value,
    operation: &'static str,
    mut run: impl FnMut(&DecodeContext<'_>, &mut Value) -> Result<(), CodecError>,
) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = value.clone();
    let first = match run(&ctx, &mut value) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected source-step refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
    assert_eq!(value, original);
    for replay in [&mut value, &mut empty] {
        assert!(matches!(run(&ctx, replay),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));

    let arena = DecodeArena::new();
    policy.limits.max_work_units = 3;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    run(&ctx, &mut value).unwrap();
    assert_eq!(value, original);
    run(&ctx, &mut empty).unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn owned_id_sequence_admits_only_actual_members_and_no_end_probe() {
    boundary(sequence(), Value::Seq(Vec::new()), "ASM serialized sequence items",
        |ctx, value| collect_owned_ids(ctx, value, &mut HashSet::new()));
}

#[test]
fn reference_sequence_admits_only_actual_members_and_no_end_probe() {
    boundary(sequence(), Value::Seq(Vec::new()), "ASM serialized sequence items",
        |ctx, value| collect_references(ctx, value, &HashSet::new(), &mut BTreeSet::new()));
}

#[test]
fn reference_map_admits_only_actual_fields_and_no_end_probe() {
    boundary(fields(), Value::Map(BTreeMap::new()), "ASM serialized map fields",
        |ctx, value| collect_references(ctx, value, &HashSet::new(), &mut BTreeSet::new()));
}

#[test]
fn adjacency_root_admits_only_actual_fields_and_no_end_probe() {
    boundary(fields(), Value::Map(BTreeMap::new()), "ASM adjacency root fields",
        |ctx, value| collect_entity_adjacency(ctx, value, &HashSet::new(), &mut HashMap::new()));
}

#[test]
fn retained_root_admits_only_actual_fields_and_no_end_probe() {
    boundary(fields(), Value::Map(BTreeMap::new()), "ASM retained root fields",
        |ctx, value| retain_root_entities(ctx, value, &HashSet::new()));
}

#[test]
fn remapped_sequence_admits_only_actual_members_and_no_end_probe() {
    boundary(sequence(), Value::Seq(Vec::new()), "ASM serialized sequence items",
        |ctx, value| remap_owned_ids(ctx, value, &HashMap::new()));
}

#[test]
fn owned_id_map_refuses_one_field_after_the_matching_id_probe() {
    let value = Value::Map(BTreeMap::from([
        (Value::String("id".into()), Value::U8(1)),
        (Value::String("z1".into()), Value::U8(2)),
        (Value::String("z2".into()), Value::U8(3)),
    ]));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The first sorted key matches "id". Its non-string value yields no id.
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut owned = HashSet::new();
    let first = match collect_owned_ids(&ctx, &value, &mut owned) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected map source refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM serialized map fields");
    assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
    assert!(owned.is_empty());
    assert!(matches!(collect_owned_ids(&ctx, &Value::Map(BTreeMap::new()), &mut owned),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn remapped_map_refuses_one_owned_field_before_insertion() {
    let mut value = fields();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match remap_owned_ids(&ctx, &mut value, &HashMap::new()) {
        Err(CodecError::ResourceLimit(first)) => first,
        other => panic!("expected owned map source refusal: {other:?}"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM remapped map fields");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    // The source map is taken before traversal, as in the original route.
    assert_eq!(value, Value::Map(BTreeMap::new()));
    assert!(matches!(remap_owned_ids(&ctx, &mut value, &HashMap::new()),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}
