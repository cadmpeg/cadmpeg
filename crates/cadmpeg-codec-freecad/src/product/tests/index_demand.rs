// SPDX-License-Identifier: Apache-2.0
//! Product index storage is admitted only when a consumer needs its values.

use super::super::transfer;
use crate::native::{self, ProductNodeRecord, PropertyBody, PropertyFamily, PropertyRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn object(name: &str, type_name: &str) -> native::ObjectRecord {
    native::ObjectRecord {
        identity: native::object_identity::ObjectIdentity::try_new(
            format!("fcstd:native:object#{name}"),
            name.to_owned(),
        )
        .expect("object identity"),
        type_name: type_name.into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    }
}

fn property(
    owner: &str,
    name: &str,
    type_name: &str,
    value: native::ValueRecord,
) -> PropertyRecord {
    PropertyRecord {
        id: format!("fcstd:native:property#{owner}:{name}"),
        owner: format!("fcstd:native:object#{owner}"),
        name: name.into(),
        type_name: type_name.into(),
        family: if name == "Placement" {
            PropertyFamily::Placement
        } else {
            PropertyFamily::Unknown
        },
        status: None,
        body: PropertyBody::Persisted {
            values: vec![value],
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML span"),
    }
}

fn value(tag: &str, attributes: &[(&str, &str)]) -> native::ValueRecord {
    native::ValueRecord {
        tag: tag.into(),
        order: 0,
        attributes: attributes
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect(),
        text: None,
        raw_xml: String::new(),
    }
}

fn external_occurrence() -> ProductNodeRecord {
    ProductNodeRecord {
        id: "fcstd:native:product#Occurrence".into(),
        object: "fcstd:native:object#Occurrence".into(),
        node: native::ProductNode::Occurrence(Box::new(native::LinkOccurrence {
            members: Vec::new(),
            prototype: Some("RemotePart".into()),
            external_document: Some(native::ExternalDocument::File(
                "remote.FCStd".try_into().expect("external path"),
            )),
            local_transform: None,
            placement_property: None,
            array: native::LinkArray::try_new(None, Vec::new(), Vec::new(), Vec::new(), Vec::new())
                .expect("scalar link array"),
            link_transform: Some(true),
            linked_subelements: Vec::new(),
            claim_child: None,
            copy_on_change: None,
            scale: None,
        })),
    }
}

#[test]
fn product_transfer_skips_owner_index_without_a_supported_object_consumer() {
    let unused = property(
        "Object",
        "Unused",
        "App::PropertyString",
        value("String", &[("value", "text")]),
    );

    for objects in [Vec::new(), vec![object("Object", "PartDesign::Feature")]] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let records = transfer(
            &ctx,
            &objects,
            std::slice::from_ref(&unused),
            &BTreeMap::new(),
        )
        .expect("no object consumes the property owner index");
        assert!(records.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn product_transfer_admits_owner_index_for_a_real_product_consumer() {
    let object = object("Part", "App::Part");
    let unused = property(
        "Other",
        "Unused",
        "App::PropertyString",
        value("String", &[("value", "text")]),
    );

    let refusal = crate::test_support::refusal_at(
        ResourceDimension::CollectionItems,
        &[],
        "fcstd product owner index",
        |ctx| {
            transfer(
                ctx,
                std::slice::from_ref(&object),
                std::slice::from_ref(&unused),
                &BTreeMap::new(),
            )
        },
    );
    assert!(matches!(refusal, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "fcstd product owner index"));

    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    let records = transfer(
        &ctx,
        std::slice::from_ref(&object),
        std::slice::from_ref(&unused),
        &BTreeMap::new(),
    )
    .expect("product object consumes owner index");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].object.as_str(), object.id().as_str());
    assert!(matches!(records[0].node, native::ProductNode::Part(_)));
}

#[test]
fn external_only_product_projection_still_rejects_duplicate_records_first() {
    let occurrence = external_occurrence();
    let malformed_placement = property(
        "Occurrence",
        "Placement",
        "App::PropertyPlacement",
        value(
            "PropertyPlacement",
            &[("Px", "0"), ("Py", "0"), ("Pz", "0")],
        ),
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");

    let error = super::super::transfer_neutral(
        &ctx,
        &[occurrence.clone(), occurrence.clone()],
        &[],
        &[],
        &[malformed_placement],
        &[],
        &[],
    )
    .expect_err("duplicate source records remain invalid");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "product object fcstd:native:object#Occurrence has duplicate product records"));
}

#[test]
fn external_only_product_projection_still_validates_unconsumed_placement() {
    let occurrence = external_occurrence();
    let malformed_placement = property(
        "Occurrence",
        "Placement",
        "App::PropertyPlacement",
        value(
            "PropertyPlacement",
            &[("Px", "0"), ("Py", "0"), ("Pz", "0")],
        ),
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");

    let error = super::super::transfer_neutral(
        &ctx,
        &[occurrence],
        &[],
        &[],
        &[malformed_placement],
        &[],
        &[],
    )
    .expect_err("placement validation is retained without a transform consumer");
    assert!(matches!(error, CodecError::Malformed(message)
        if message == "placement property fcstd:native:property#Occurrence:Placement has an invalid Q0 quaternion component"));
}

#[test]
fn external_only_product_projection_keeps_occurrence_output_without_local_indexes() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");

    let (definitions, occurrences) =
        super::super::transfer_neutral(&ctx, &[external_occurrence()], &[], &[], &[], &[], &[])
            .expect("external-only occurrence projection");
    assert!(definitions.is_empty());
    assert_eq!(occurrences.len(), 1);
    assert_eq!(
        occurrences[0].native_ref.as_deref(),
        Some("fcstd:native:object#Occurrence")
    );
    assert!(matches!(
        occurrences[0].prototype,
        cadmpeg_ir::products::PrototypeReference::External { .. }
    ));
}
