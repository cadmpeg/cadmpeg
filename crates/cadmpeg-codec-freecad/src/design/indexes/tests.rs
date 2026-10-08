// SPDX-License-Identifier: Apache-2.0
//! Admission and winner-order tests for design transfer indexes.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FeatureId;

use crate::native::{
    LinkTarget, LinkTargetWire, ObjectRecord, PropertyBody, PropertyFamily, PropertyRecord,
    RetainedXml,
};

use super::{BodyPredecessors, ObjectIndex};

fn object(id: &str, order: usize) -> ObjectRecord {
    ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            format!("fcstd:native:object#{id}"),
            id.to_owned(),
        )
        .expect("valid object identity"),
        type_name: "PartDesign::Body".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order,
        data: None,
    }
}

fn membership_property(owner: &str, targets: &[&str]) -> PropertyRecord {
    let links = targets
        .iter()
        .map(|target| {
            LinkTarget::optional_from_wire(LinkTargetWire::<String> {
                document: None,
                document_attribute: None,
                object: Some((*target).to_owned()),
                subelements: Vec::new(),
            })
            .expect("valid object link")
        })
        .collect();
    PropertyRecord {
        id: format!("fcstd:native:property#{owner}:Group"),
        owner: owner.to_owned(),
        name: "Group".into(),
        type_name: "App::PropertyLinkList".into(),
        family: PropertyFamily::Unknown,
        status: None,
        body: PropertyBody::Persisted {
            values: Vec::new(),
            links,
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: RetainedXml::from_text("<Property/>".into(), 0).expect("valid property XML"),
    }
}

fn work_used_after_successful_prefix(ctx: &DecodeContext<'_>) -> u64 {
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "test FreeCAD index work oracle")
        .expect_err("the work marker exceeds the successful prefix")
    else {
        panic!("work marker must refuse after the successful prefix");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn object_index_builds_only_after_a_lookup_is_requested() {
    let object = object("shape", 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::CollectionItems,
        "fcstd design object index",
        None,
    );

    let index = ObjectIndex::new(&ctx, std::slice::from_ref(&object)).expect("lazy index");
    assert!(ctx.resource_refusal().is_none());

    let error = index
        .get(object.id(), "test object lookup")
        .expect_err("the requested index build meets the refusal probe");
    assert!(matches!(
        &error,
        CodecError::ResourceLimit(limit) if limit.operation == "fcstd design object index"
    ));
    assert_eq!(
        ctx.resource_refusal().map(|limit| limit.operation),
        Some("fcstd design object index")
    );
}

#[test]
fn object_index_keeps_the_first_source_object_for_a_duplicate_id() {
    let mut first = object("duplicate", 0);
    first.type_name = "First::Type".into();
    let mut second = object("duplicate", 1);
    second.type_name = "Second::Type".into();
    let objects = [first, second];

    crate::test_support::with_service_context(&[], |ctx| {
        let index = ObjectIndex::new(ctx, &objects).expect("object index");
        let found = index
            .get(objects[0].id(), "test object lookup")
            .expect("object lookup")
            .expect("indexed object");
        assert_eq!(found.type_name, "First::Type");
    });
}

#[test]
fn body_predecessors_build_only_after_a_lookup_is_requested() {
    let body = object("body", 0);
    let features = HashMap::new();
    let properties_by_owner = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "fcstd body predecessor objects",
        None,
    );

    let index = BodyPredecessors::new(
        &ctx,
        std::slice::from_ref(&body),
        &features,
        &properties_by_owner,
    )
    .expect("lazy predecessor index");
    assert!(ctx.resource_refusal().is_none());

    let error = index
        .get("member", "test predecessor lookup")
        .expect_err("the requested index build meets the refusal probe");
    assert!(matches!(
        &error,
        CodecError::ResourceLimit(limit) if limit.operation == "fcstd body predecessor objects"
    ));
    assert_eq!(
        ctx.resource_refusal().map(|limit| limit.operation),
        Some("fcstd body predecessor objects")
    );
}

#[test]
fn body_predecessors_keep_first_usable_body_and_first_member_winners() {
    let first_body = object("first-body", 0);
    let second_body = object("second-body", 1);
    let third_body = object("third-body", 2);
    let first_members = membership_property(first_body.id(), &["member", "base-a", "member"]);
    let second_members =
        membership_property(second_body.id(), &["base-b", "member", "member"]);
    let third_members = membership_property(third_body.id(), &["base-c", "member"]);
    let objects = [first_body, second_body, third_body];
    let properties_by_owner = BTreeMap::from([
        (objects[0].id().as_str(), vec![&first_members]),
        (objects[1].id().as_str(), vec![&second_members]),
        (objects[2].id().as_str(), vec![&third_members]),
    ]);
    let mut features = HashMap::new();
    for name in ["base-a", "base-b", "base-c"] {
        features.insert(
            name,
            FeatureId::mint(format!("test:test:feature#{name}"))
                .expect("valid feature identity"),
        );
    }

    crate::test_support::with_service_context(&[], |ctx| {
        let index = BodyPredecessors::new(ctx, &objects, &features, &properties_by_owner)
            .expect("predecessor index");
        let predecessor = index
            .get("member", "test predecessor lookup")
            .expect("predecessor lookup")
            .expect("first usable predecessor");
        assert_eq!(predecessor, features.get("base-b").expect("base-b feature"));
    });
}

#[test]
fn object_index_first_insertion_refuses_before_visiting_long_suffix() {
    let oracle_objects = [object("first", 0)];
    let oracle_arena = DecodeArena::new();
    let mut oracle_policy = DecodePolicy::service();
    oracle_policy.limits.max_work_units = u64::MAX;
    oracle_policy.limits.max_collection_items = u64::MAX;
    let (oracle_ctx, _) =
        DecodeContext::from_root_bytes(&[], &oracle_arena, &oracle_policy).expect("oracle context");
    let oracle_index = ObjectIndex::new(&oracle_ctx, &oracle_objects).expect("lazy object index");
    assert_eq!(
        oracle_index
            .get(oracle_objects[0].id(), "test object lookup")
            .expect("object lookup")
            .map(|found| found.type_name.as_str()),
        Some("PartDesign::Body")
    );
    let work_cap = work_used_after_successful_prefix(&oracle_ctx);

    let suffix_len = usize::try_from(work_cap)
        .expect("object-index Work cap fits usize")
        .checked_add(1)
        .expect("object-index suffix length fits usize");
    let object_count = suffix_len
        .checked_add(1)
        .expect("object-index source length fits usize");
    let mut objects = Vec::with_capacity(object_count);
    objects.push(object("first", 0));
    for order in 1..object_count {
        let id = format!("suffix-{order}");
        objects.push(object(&id, order));
    }
    assert!(work_cap < u64::try_from(objects.len()).expect("object count fits"));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work_cap;
    policy.limits.max_collection_items = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let index = ObjectIndex::new(&ctx, &objects).expect("lazy object index");
    assert!(ctx.resource_refusal().is_none());
    let _probe = RefusalProbe::arm(
        ResourceDimension::CollectionItems,
        "fcstd design object index",
        None,
    );

    let CodecError::ResourceLimit(limit) = index
        .get(objects[0].id(), "test object lookup")
        .expect_err("the first index insertion meets the refusal probe")
    else {
        panic!("the first object index insertion must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "fcstd design object index");
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn body_predecessor_visits_source_objects_before_admitting_suffix() {
    let oracle_objects = [object("body", 0)];
    let oracle_features = HashMap::new();
    let oracle_properties = BTreeMap::new();
    let oracle_arena = DecodeArena::new();
    let mut oracle_policy = DecodePolicy::service();
    oracle_policy.limits.max_work_units = u64::MAX;
    let (oracle_ctx, _) =
        DecodeContext::from_root_bytes(&[], &oracle_arena, &oracle_policy).expect("oracle context");
    let oracle_index = BodyPredecessors::new(
        &oracle_ctx,
        &oracle_objects,
        &oracle_features,
        &oracle_properties,
    )
    .expect("lazy predecessor index");
    let _oracle = oracle_index
        .build()
        .expect("short predecessor index build");
    let work_cap = work_used_after_successful_prefix(&oracle_ctx);

    let mut objects = vec![object("body", 0)];
    for order in 1..=1024 {
        let id = format!("suffix-{order}");
        objects.push(object(&id, order));
    }
    assert!(work_cap < u64::try_from(objects.len()).expect("object count fits"));

    let features = HashMap::new();
    let properties = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work_cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let index = BodyPredecessors::new(&ctx, &objects, &features, &properties)
        .expect("lazy predecessor index");

    let CodecError::ResourceLimit(limit) = index
        .get("member", "test predecessor lookup")
        .expect_err("the next object visit exceeds the short-prefix work cap")
    else {
        panic!("the object suffix must exceed the short-prefix work cap");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "fcstd body predecessor objects");
    assert_eq!(limit.used, work_cap);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn body_predecessor_member_first_insertion_refuses_before_long_suffix() {
    let oracle_body = object("body", 0);
    let oracle_members = membership_property(oracle_body.id(), &["base", "member"]);
    let oracle_properties = BTreeMap::from([(
        oracle_body.id().as_str(),
        vec![&oracle_members],
    )]);
    let oracle_features = HashMap::from([(
        "base",
        FeatureId::mint("test:test:feature#base").expect("base feature identity"),
    )]);
    let oracle_arena = DecodeArena::new();
    let mut oracle_policy = DecodePolicy::service();
    oracle_policy.limits.max_work_units = u64::MAX;
    oracle_policy.limits.max_collection_items = u64::MAX;
    let (oracle_ctx, _) =
        DecodeContext::from_root_bytes(&[], &oracle_arena, &oracle_policy).expect("oracle context");
    let oracle_index = BodyPredecessors::new(
        &oracle_ctx,
        std::slice::from_ref(&oracle_body),
        &oracle_features,
        &oracle_properties,
    )
    .expect("lazy predecessor index");
    assert_eq!(
        oracle_index
            .get("member", "test predecessor lookup")
            .expect("predecessor lookup"),
        oracle_features.get("base")
    );
    let work_cap = work_used_after_successful_prefix(&oracle_ctx);

    let body = object("body", 0);
    let suffix_len = usize::try_from(work_cap)
        .expect("predecessor Work cap fits usize")
        .checked_add(1)
        .expect("predecessor suffix length fits usize");
    let member_count = suffix_len
        .checked_add(2)
        .expect("predecessor source length fits usize");
    let mut targets = Vec::with_capacity(member_count);
    targets.extend(["base".to_owned(), "member".to_owned()]);
    targets.extend((0..suffix_len).map(|index| format!("suffix-{index}")));
    assert!(work_cap < u64::try_from(targets.len()).expect("member count fits"));
    let target_refs: Vec<_> = targets.iter().map(String::as_str).collect();
    let members = membership_property(body.id(), &target_refs);
    let properties = BTreeMap::from([(body.id().as_str(), vec![&members])]);
    let features = HashMap::from([(
        "base",
        FeatureId::mint("test:test:feature#base").expect("base feature identity"),
    )]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work_cap;
    policy.limits.max_collection_items = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let index = BodyPredecessors::new(
        &ctx,
        std::slice::from_ref(&body),
        &features,
        &properties,
    )
    .expect("lazy predecessor index");
    assert!(ctx.resource_refusal().is_none());
    let _probe = RefusalProbe::arm(
        ResourceDimension::CollectionItems,
        "fcstd body predecessor index",
        None,
    );

    let CodecError::ResourceLimit(limit) = index
        .get("member", "test predecessor lookup")
        .expect_err("the first predecessor insertion meets the refusal probe")
    else {
        panic!("the first predecessor insertion must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "fcstd body predecessor index");
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn empty_index_routes_preserve_a_fused_resource_refusal() {
    let objects = [];
    let features = HashMap::new();
    let properties = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let object_index = ObjectIndex::new(&ctx, &objects).expect("lazy object index");
    let body_index =
        BodyPredecessors::new(&ctx, &objects, &features, &properties).expect("lazy body index");
    let CodecError::ResourceLimit(prior) = ctx
        .charge_work(2, "test prior refusal")
        .expect_err("the work request exceeds the test limit")
    else {
        panic!("the initial work request must refuse");
    };

    let CodecError::ResourceLimit(object_refusal) = object_index
        .get("missing", "test object lookup")
        .expect_err("the empty object index must retain the prior refusal")
    else {
        panic!("the empty object index must refuse");
    };
    assert_eq!(object_refusal, prior);

    let CodecError::ResourceLimit(body_refusal) = body_index
        .get("missing", "test predecessor lookup")
        .expect_err("the empty predecessor index must retain the prior refusal")
    else {
        panic!("the empty predecessor index must refuse");
    };
    assert_eq!(body_refusal, prior);
    assert_eq!(ctx.resource_refusal(), Some(prior));
}

#[test]
fn body_predecessor_empty_members_keep_a_later_fused_refusal() {
    let body = object("body", 0);
    let members = membership_property(body.id(), &[]);
    let properties = BTreeMap::from([(body.id().as_str(), vec![&members])]);
    let features = HashMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let index = BodyPredecessors::new(
        &ctx,
        std::slice::from_ref(&body),
        &features,
        &properties,
    )
    .expect("lazy predecessor index");
    assert!(index
        .get("missing", "test predecessor lookup")
        .expect("empty member route")
        .is_none());

    let CodecError::ResourceLimit(prior) = ctx
        .charge_work(u64::MAX, "test prior refusal")
        .expect_err("the work marker exceeds the empty member route")
    else {
        panic!("the work marker must refuse after the empty member route");
    };
    let CodecError::ResourceLimit(actual) = index
        .get("missing", "test predecessor lookup")
        .expect_err("the cached empty index must retain the prior refusal")
    else {
        panic!("the cached empty index must refuse");
    };
    assert_eq!(actual, prior);
    assert_eq!(ctx.resource_refusal(), Some(prior));
}
