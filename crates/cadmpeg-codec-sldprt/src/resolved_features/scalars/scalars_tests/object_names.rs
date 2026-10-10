//! Deferred source and text object-name indexes.

use super::super::{NameLookup, ObjectNames};
use crate::records::{Feature, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId};
use cadmpeg_core::decode::{
    refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn name(id: &str, source: Option<u32>, value: &str) -> FeatureInputName {
    FeatureInputName {
        id: id.into(), parent: "lane".into(), ordinal: 0, offset: 0,
        object_id: source.and_then(ObjectId::from_value), value: value.into(),
    }
}

fn lane(names: Vec<FeatureInputName>) -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(), configuration: None, native_payload: Vec::new(),
        classes: Vec::new(), names, scalars: Vec::new(), references: Vec::new(),
        relation_bindings: Vec::new(), relation_instances: Vec::new(),
        body_selections: Vec::new(), edge_selections: Vec::new(), surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(), sketch_entities: Vec::new(),
    }
}

fn feature(source: Option<u32>) -> Feature {
    Feature {
        id: "feature".into(), parent: "history".into(), xml_tag: "Feature".into(),
        tree_parent: None, source_id: source.and_then(FeatureSource::from_value), ordinal: 0,
        name: "Text".into(), kind: "Custom".into(), input_class: None, suppressed: false,
        parameters: BTreeMap::new(), dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(), text: None, content: Vec::new(),
    }
}

#[test]
fn object_names_source_binding_skips_text_and_preserves_duplicate_ambiguity() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let unique = lane(vec![name("source", Some(42), "Source"), name("text", None, "Text")]);
    let repeated = lane(vec![
        name("source-one", Some(42), "Source"), name("source-two", Some(42), "Other"),
        name("source-three", Some(42), "Third"), name("text", None, "Text"),
    ]);
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "index SLDPRT object names by text", None);
    {
        let names = ObjectNames::new(&ctx, &unique).expect("index owner");
        assert_eq!(names.of(&ctx, &feature(Some(42))).expect("source lookup").map(|name| name.id.as_str()), Some("source"));
        let names = ObjectNames::new(&ctx, &repeated).expect("index owner");
        assert!(matches!(names.lookup(&ctx, &feature(Some(42))).expect("duplicate source lookup"), NameLookup::Repeated));
        assert_eq!(names.of(&ctx, &feature(Some(42))).expect("repeated source remains ambiguous"), None);
    }
    ctx.finish_session().expect("no text-index work");
}

#[test]
fn object_names_text_fallback_skips_source_and_caches_its_index() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let lane = lane(vec![name("text", Some(42), "Text")]);
    {
        let source_probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "index SLDPRT object names by id", None);
        let names = ObjectNames::new(&ctx, &lane).expect("index owner");
        assert_eq!(names.of(&ctx, &feature(None)).expect("text lookup").map(|name| name.id.as_str()), Some("text"));
        drop(source_probe);
        let _text_probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "index SLDPRT object names by text", None);
        assert_eq!(names.of(&ctx, &feature(None)).expect("cached text lookup").map(|name| name.id.as_str()), Some("text"));
    }
    ctx.finish_session().expect("no unused index work");
}

#[test]
fn object_names_text_index_preserves_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let lane = lane(vec![name("text", None, "Text")]);
    let names = ObjectNames::new(&ctx, &lane).expect("constructor does not build the index");
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "index SLDPRT object names by text", Some(1));
    let error = names.lookup(&ctx, &feature(None)).err().expect("first text-index visit refuses");
    let CodecError::ResourceLimit(limit) = error else { panic!("expected index refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "index SLDPRT object names by text");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(names.of(&ctx, &feature(None)), Err(CodecError::ResourceLimit(repeated)) if repeated == limit));
}

fn source_name_table_storage(buckets: usize) -> u64 {
    // Core admits typed buckets, one control byte per bucket, trailing control
    // bytes and alignment padding. A new table holds three entries in four
    // buckets; fourth-entry growth admits sixteen buckets beside the old four.
    let lane = std::mem::size_of::<(u32, Option<&FeatureInputName>)>();
    let padding = std::mem::align_of::<(u32, Option<&FeatureInputName>)>().max(16) - 1;
    cadmpeg_core::decode::u64_from_index(buckets * lane + buckets + 16 + padding)
}

#[test]
fn object_names_source_index_keeps_live_storage_and_releases_it_with_owner() {
    let lane = lane(vec![
        name("first", Some(42), "First"),
        name("second", Some(43), "Second"),
        name("third", Some(44), "Third"),
    ]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let need = source_name_table_storage(4);
    policy.limits.max_materialized_bytes = need;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let names = ObjectNames::new(&ctx, &lane).unwrap();
        assert_eq!(names.of(&ctx, &feature(Some(42))).unwrap(), Some(&lane.names[0]));
        assert_eq!(names.of(&ctx, &feature(Some(43))).unwrap(), Some(&lane.names[1]));
        assert_eq!(names.of(&ctx, &feature(Some(44))).unwrap(), Some(&lane.names[2]));
        assert!(names.by_value.borrow().is_none());
        drop(names);
        let reuse = ctx.reserve_scoped(need, "reuse source name index storage").unwrap();
        drop(reuse);
    }
    let names = ObjectNames::new(&ctx, &lane).unwrap();
    assert_eq!(names.of(&ctx, &feature(Some(42))).unwrap(), Some(&lane.names[0]));
    let error = ctx.reserve_scoped(need, "reuse while source name index lives").unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected live index storage refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "reuse while source name index lives");
    assert_eq!(limit.used, need);
    assert_eq!(limit.additional, need);
    drop(names);
}

#[test]
fn object_names_source_growth_refusal_preserves_unpublished_state_and_original_fuse() {
    let lane = lane(vec![
        name("first", Some(42), "First"),
        name("second", Some(43), "Second"),
        name("third", Some(44), "Third"),
        name("fourth", Some(45), "Fourth"),
    ]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let before = source_name_table_storage(4);
    let growth = source_name_table_storage(16);
    policy.limits.max_materialized_bytes = before + growth - 1;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let names = ObjectNames::new(&ctx, &lane).unwrap();
    let error = names.lookup(&ctx, &feature(Some(42))).err().unwrap();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected source index growth refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "index SLDPRT object names by id");
    assert_eq!(first.used, before);
    assert_eq!(first.additional, growth);
    assert_eq!(ctx.resource_refusal(), Some(first.clone()));
    assert!(names.by_object.borrow().index.is_none());
    assert!(names.by_value.borrow().is_none());
    let error = names.of(&ctx, &feature(Some(42))).unwrap_err();
    let CodecError::ResourceLimit(next) = error else {
        panic!("expected original source index refusal");
    };
    assert_eq!(next, first);
}
