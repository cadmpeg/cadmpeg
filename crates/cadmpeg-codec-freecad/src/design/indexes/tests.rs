// SPDX-License-Identifier: Apache-2.0
//! Admission and winner-order tests for design transfer indexes.

use std::collections::{BTreeMap, HashMap};

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
