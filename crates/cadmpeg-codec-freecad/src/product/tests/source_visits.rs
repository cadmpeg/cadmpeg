// SPDX-License-Identifier: Apache-2.0
//! Prefix failures consume only source records reached before the failure.

use super::super::{linked_object_names, nonempty_subelements, product_record_index, transfer};
use crate::native::{self, ProductNodeRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap};

/// Measure Work only after the oracle has succeeded and has not fused.
fn successful_work(oracle: impl FnOnce(&DecodeContext<'_>) -> Result<(), CodecError>) -> u64 {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    oracle(&ctx).expect("successful short prefix");
    assert_eq!(ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "source visit Work oracle")
        .expect_err("Work overflow")
    else {
        panic!("Work refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

fn duplicate_prefix_work(records: &[ProductNodeRecord]) -> u64 {
    successful_work(|ctx| {
        let mut index = HashMap::new();
        let mut visits = records.iter();
        for _ in 0..2 {
            let record = ctx
                .next_charged(&mut visits, "fcstd product record index")?
                .expect("two prefix records");
            ctx.insert_hash_map(
                &mut index,
                record.object.as_str(),
                record,
                "fcstd product record index",
            )?;
        }
        ctx.format_retained(
            format_args!("product object A has duplicate product records"),
            "fcstd product duplicate record",
        )?;
        Ok(())
    })
}

#[test]
fn product_duplicate_record_does_not_prepay_long_suffix() {
    let prefix = [super::node("A", &[]), super::node("A", &[])];
    let cap = duplicate_prefix_work(&prefix);
    let mut records = prefix.to_vec();
    records.extend((0..=cap).map(|_| super::node("B", &[])));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    let error = product_record_index(&ctx, &records).expect_err("first duplicate");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "product object A has duplicate product records"));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn product_index_nested_refusal_does_not_prepay_long_suffix() {
    let prefix = [super::node("A", &[]), super::node("B", &[])];
    let cap = duplicate_prefix_work(&prefix);
    let mut records = prefix.to_vec();
    records.extend((0..=cap).map(|_| super::node("C", &[])));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    let error = product_record_index(&ctx, &records).expect_err("second inserted item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "fcstd product record index"));
}

fn target() -> native::LinkTarget {
    native::LinkTarget::optional_from_wire(native::LinkTargetWire::<String> {
        document: None,
        document_attribute: None,
        object: Some("A".into()),
        subelements: Vec::new(),
    })
    .expect("valid target")
    .expect("present target")
}

#[test]
fn product_link_copy_refusal_does_not_prepay_long_suffix() {
    let cap = successful_work(|ctx| {
        assert_eq!(linked_object_names(ctx, &[Some(target())])?, ["A"]);
        Ok(())
    });
    let mut links = vec![Some(target())];
    links.extend((0..=cap).map(|_| None));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    let error = linked_object_names(&ctx, &links).expect_err("first name copy");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "fcstd product linked object name"));
}

#[test]
fn product_subelement_copy_refusal_does_not_prepay_long_suffix() {
    let cap = successful_work(|ctx| {
        assert_eq!(nonempty_subelements(ctx, &["Face1".into()])?, ["Face1"]);
        Ok(())
    });
    let mut values = vec!["Face1".into()];
    values.extend((0..=cap).map(|_| String::new()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    let error = nonempty_subelements(&ctx, &values).expect_err("first subelement copy");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "fcstd product linked subelement"));
}

#[test]
fn product_empty_source_helpers_preserve_original_fused_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    assert!(product_record_index(&ctx, &[]).expect("empty index").is_empty());
    assert!(linked_object_names(&ctx, &[]).expect("empty links").is_empty());
    assert!(nonempty_subelements(&ctx, &[]).expect("empty subelements").is_empty());
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(1, "original source refusal")
        .expect_err("original refusal")
    else {
        panic!("resource refusal")
    };
    for error in [
        product_record_index(&ctx, &[]).expect_err("fused index"),
        linked_object_names(&ctx, &[]).expect_err("fused links"),
        nonempty_subelements(&ctx, &[]).expect_err("fused subelements"),
        transfer(&ctx, &[], &[], &BTreeMap::new()).expect_err("fused transfer"),
        super::super::transfer_neutral(&ctx, &[], &[], &[], &[], &[], &[])
            .expect_err("fused empty projection"),
        super::super::product_cycle_nodes(&ctx, &[]).expect_err("fused empty graph"),
    ] {
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit == original));
    }
}

#[test]
fn product_source_helpers_preserve_identity_and_source_order() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    let records = [super::node("A", &[]), super::node("B", &[])];
    let index = product_record_index(&ctx, &records).expect("unique products");
    assert!(std::ptr::eq(index["A"], &records[0]));
    assert!(std::ptr::eq(index["B"], &records[1]));
    assert_eq!(linked_object_names(&ctx, &[None, Some(target()), None]).expect("names"), ["A"]);
    assert_eq!(nonempty_subelements(&ctx, &["".into(), "Face2".into(), "Face1".into()])
        .expect("subelements"), ["Face2", "Face1"]);
}

#[test]
fn product_first_malformed_object_does_not_prepay_long_suffix() {
    let object = native::ObjectRecord {
        identity: native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#A".into(), "A".into(),
        ).expect("object identity"),
        type_name: "App::Part".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let mut property = native::PropertyRecord {
        id: "fcstd:native:property#A:Group".into(),
        owner: object.id().into(),
        name: "Group".into(),
        type_name: "App::PropertyLinkList".into(),
        family: native::PropertyFamily::Unknown,
        status: None,
        body: native::PropertyBody::Persisted {
            values: vec![native::ValueRecord {
                tag: "LinkList".into(),
                order: 0,
                attributes: BTreeMap::new(),
                text: None,
                raw_xml: "<LinkList/>".into(),
            }],
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: native::RetainedXml::from_text("<Property><LinkList/></Property>".into(), 0)
            .expect("XML span"),
    };
    let diagnostic = "product property fcstd:native:property#A:Group has runtime type Wrong, expected App::PropertyLinkList for Group";
    let cap = successful_work(|ctx| {
        let output = transfer(ctx, std::slice::from_ref(&object),
            std::slice::from_ref(&property), &BTreeMap::new())?;
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].id, "fcstd:native:product#A");
        assert_eq!(output[0].object.as_str(), object.id().as_str());
        ctx.format_retained(format_args!("{diagnostic}"), "fcstd product runtime type")?;
        Ok(())
    });
    property.type_name = "Wrong".into();
    let mut objects = vec![object.clone()];
    objects.extend((0..=cap).map(|_| object.clone()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    let error = transfer(&ctx, &objects, &[property], &BTreeMap::new())
        .expect_err("first runtime type");
    assert!(matches!(error, CodecError::Malformed(message) if message == diagnostic));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn product_occurrence_refusal_does_not_prepay_element_suffix() {
    let mut record = native::ProductNodeRecord {
        id: "fcstd:native:product#Occurrence".into(),
        object: "fcstd:native:object#Occurrence".into(),
        node: native::ProductNode::Occurrence(Box::new(native::LinkOccurrence {
            members: Vec::new(),
            prototype: None,
            external_document: None,
            local_transform: None,
            placement_property: None,
            array: native::LinkArray::try_new(Some(1), Vec::new(), Vec::new(),
                Vec::new(), Vec::new()).expect("one stated element"),
            link_transform: None,
            linked_subelements: Vec::new(),
            claim_child: None,
            copy_on_change: None,
            scale: None,
        })),
    };
    let cap = successful_work(|ctx| {
        let (definitions, occurrences) = super::super::transfer_neutral(
            ctx, std::slice::from_ref(&record), &[], &[], &[], &[], &[],
        )?;
        assert!(definitions.is_empty());
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].native_ref.as_deref(), Some(record.object.as_str()));
        Ok(())
    });
    let count = cap.checked_add(1).expect("finite element count");
    assert!(count <= 1_000_000, "short prefix fits the stated array limit");
    let native::ProductNode::Occurrence(occurrence) = &mut record.node else {
        panic!("occurrence fixture")
    };
    occurrence.array = native::LinkArray::try_new(Some(count), Vec::new(), Vec::new(),
        Vec::new(), Vec::new()).expect("stated element count");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    let error = super::super::transfer_neutral(
        &ctx, &[record], &[], &[], &[], &[], &[],
    ).expect_err("first occurrence identity");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "FreeCAD model identity"));
}

#[test]
fn product_cycle_target_refusal_does_not_prepay_member_suffix() {
    let prefix = [super::node("A", &["B"]), super::node("B", &[])];
    let cap = successful_work(|ctx| {
        assert!(super::super::product_cycle_nodes(ctx, &prefix)?.is_empty());
        Ok(())
    });
    let CodecError::ResourceLimit(collection) = crate::test_support::refusal_at(
        ResourceDimension::CollectionItems, &[], "fcstd product cycle targets",
        |ctx| super::super::product_cycle_nodes(ctx, &prefix),
    ) else {
        panic!("target collection refusal")
    };
    let mut records = prefix.to_vec();
    let native::ProductNode::Group(group) = &mut records[0].node else {
        panic!("group fixture")
    };
    group.members.extend((0..=cap).map(|_| "B".into()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    policy.limits.max_collection_items = collection.used;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty context");
    let error = super::super::product_cycle_nodes(&ctx, &records)
        .expect_err("first target allocation");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "fcstd product cycle targets"));
}
