// SPDX-License-Identifier: Apache-2.0

use super::{entity_header, sketch_scope, ConstructionRecipe, ConstructionRecipeKind};
use super::super::{CompanionPayloadRecords, ParameterCompanionInputs, StreamPayloadRecords};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

#[test]
fn payload_session_preserves_native_recipe_order_and_preamble_then_releases_index_storage() {
    let native = "f3d:native";
    let recipe = |name: &str, offset| ConstructionRecipe {
        id: format!("{native}:construction-recipe#{name}"),
        byte_offset: offset,
        kind: ConstructionRecipeKind::Edge,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let recipes = [recipe("a", 80), recipe("first", 60), recipe("b", 80)];
    let scopes = [sketch_scope(native, 100, "Sketch_1")];
    let entities = [entity_header(native, 90, "Sketch_1")];
    let lengths = HashMap::from([(native.to_owned(), 200)]);
    let inputs = ParameterCompanionInputs {
        parameters: &[], owners: &[], scopes: &scopes, entities: &entities,
        headers: &[], recipes: &recipes, stream_lengths: &lengths,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 8192;
    policy.limits.max_retained_bytes = 16384;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let storage;
    let (records, held) = ctx.with_scoped_storage("test companion payload index", || {
        CompanionPayloadRecords::build(&ctx, &inputs)
    }).unwrap();
    storage = held;
    let stream = records.stream(native).unwrap().unwrap();
    assert_eq!(stream.preamble_start(58, 100).unwrap(), 90);
    assert_eq!(stream.preamble_start(91, 100).unwrap(), 100);
    let ids = stream.owned_recipe_ids(58, 90).unwrap();
    assert_eq!(ids, [recipes[1].id.clone(), recipes[0].id.clone(), recipes[2].id.clone()]);
    assert!(records.stream("f3d:missing").unwrap().is_none());
    drop(records);
    drop(storage);
    drop(ctx.reserve_scoped(8192, "after actual payload index destruction").unwrap());
    assert_eq!(ids, [recipes[1].id.clone(), recipes[0].id.clone(), recipes[2].id.clone()]);
    ctx.finish_session().unwrap();
}

#[test]
fn payload_session_original_fuse_dominates_existing_missing_and_empty_child_queries() {
    let native = "f3d:native";
    let recipe = ConstructionRecipe {
        id: "f3d:native:construction-recipe#60".into(), byte_offset: 60,
        kind: ConstructionRecipeKind::Edge, design: None, recipe_index: 0, record_index: None,
    };
    let lengths = HashMap::<String, usize>::new();
    let inputs = ParameterCompanionInputs {
        parameters: &[], owners: &[], scopes: &[], entities: &[], headers: &[],
        recipes: std::slice::from_ref(&recipe), stream_lengths: &lengths,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (other, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let storage;
    let (mut records, held) = ctx.with_scoped_storage("test refused payload index", || {
        CompanionPayloadRecords::build(&ctx, &inputs)
    }).unwrap();
    storage = held;
    let empty = StreamPayloadRecords::new(&ctx);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
        policy.limits.max_work_units + 1, "original companion payload session refusal",
    ) else { panic!("original creating session refusal"); };
    for query in [native, "f3d:missing"] {
        assert!(matches!(records.stream(query),
            Err(CodecError::ResourceLimit(later)) if later == original));
        match records.stream_mut(query) {
            Err(CodecError::ResourceLimit(later)) => assert_eq!(later, original),
            _ => panic!("existing and missing mutable stream queries must preserve original fuse"),
        }
    }
    for stream in [&records.streams[0], &empty] {
        assert!(matches!(stream.preamble_start(0, 100),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(stream.owned_recipe_ids(0, 100),
            Err(CodecError::ResourceLimit(later)) if later == original));
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(other.resource_refusal().is_none());
    other.charge_work(0, "unrelated payload session remains healthy").unwrap();
    drop(empty);
    drop(records);
    drop(storage);
    other.finish_session().unwrap();
}

#[test]
fn payload_session_empty_binding_and_child_queries_are_free_then_keep_original_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let lengths = HashMap::<String, usize>::new();
    let inputs = ParameterCompanionInputs {
        parameters: &[], owners: &[], scopes: &[], entities: &[], headers: &[],
        recipes: &[], stream_lengths: &lengths,
    };
    let empty = StreamPayloadRecords::new(&ctx);
    assert_eq!(empty.preamble_start(0, 100).unwrap(), 100);
    assert!(empty.owned_recipe_ids(0, 100).unwrap().is_empty());
    assert!(super::super::bind_parameter_companion_payloads(&ctx, Vec::new(), &inputs).unwrap().is_empty());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty payload refusal")
    else { panic!("zero-work original session must refuse"); };
    assert!(matches!(empty.preamble_start(0, 100),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(empty.owned_recipe_ids(0, 100),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(CompanionPayloadRecords::build(&ctx, &inputs),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(super::super::bind_parameter_companion_payloads(&ctx, Vec::new(), &inputs),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(empty);
}
