// SPDX-License-Identifier: Apache-2.0
//! Product index storage is admitted only when a consumer needs its values.

use super::super::transfer;
use crate::native::{self, ProductNodeRecord, PropertyBody, PropertyFamily, PropertyRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
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

fn empty_projection_error(properties: &[PropertyRecord], expected: &str) {
    let joint = crate::test_support::with_service_context(&[], |ctx| {
        crate::native::joint::JointRecord::try_new(
            ctx,
            "fcstd:native:joint#Ground".into(),
            "fcstd:native:object#Ground".into(),
            crate::native::joint::JointBody::Grounded {
                reference: None,
                placement: crate::native::frame::FiniteFrame::default(),
            },
            crate::native::joint::JointParameters::default(),
        )
        .expect("grounded joint")
    });
    for joints in [Vec::new(), vec![joint]] {
        crate::test_support::with_service_context(&[], |ctx| {
            let error =
                super::super::transfer_neutral(ctx, &[], &joints, &[], properties, &[], &[])
                    .expect_err("placement admission before empty output");
            assert!(matches!(error, CodecError::Malformed(message) if message == expected));
        });
    }
}

fn valid_placement(name: &str) -> PropertyRecord {
    property(
        "Shape",
        name,
        "App::PropertyPlacement",
        value(
            "PropertyPlacement",
            &[
                ("Px", "0"),
                ("Py", "0"),
                ("Pz", "0"),
                ("Q0", "0"),
                ("Q1", "0"),
                ("Q2", "0"),
                ("Q3", "1"),
            ],
        ),
    )
}

#[test]
fn empty_product_projection_rejects_invalid_placement() {
    let malformed = property(
        "Shape",
        "LinkPlacement",
        "App::PropertyPlacement",
        value(
            "PropertyPlacement",
            &[("Px", "0"), ("Py", "0"), ("Pz", "0")],
        ),
    );
    empty_projection_error(&[malformed], "placement property fcstd:native:property#Shape:LinkPlacement has an invalid Q0 quaternion component");
}

#[test]
fn empty_product_projection_rejects_duplicate_placement() {
    let placement = valid_placement("LinkPlacement");
    empty_projection_error(
        &[placement.clone(), placement],
        "product property LinkPlacement occurs more than once",
    );
}

#[test]
fn empty_product_projection_rejects_ambiguous_placement_policy() {
    empty_projection_error(
        &[
            valid_placement("LinkPlacement"),
            valid_placement("Placement"),
        ],
        "LinkPlacement and Placement require a valid LinkTransform policy",
    );
}

#[test]
fn non_product_link_placement_decode_preserves_malformed_message() {
    use cadmpeg_ir::Codec;
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0"/></Property></Properties></Object></ObjectData></Document>"#;
    let error = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(crate::test_support::test_archive::archive(document)),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .expect_err("invalid placement on non-product object");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(message)) if message == "placement property fcstd:native:property#Shape:LinkPlacement has an invalid Q0 quaternion component")
    );
}

#[test]
fn product_parent_identity_retains_only_output_copies() {
    use cadmpeg_core::decode::ResourceDimension;
    let measure = |width| {
        let parent = format!("fcstd:native:object#Parent{}", "a".repeat(width));
        let link = external_occurrence();
        let records = [super::node(&parent, &[link.object.as_str()]), link];
        crate::test_support::with_service_context(&[], |ctx| {
            let (_, occurrences) =
                super::super::transfer_neutral(ctx, &records, &[], &[], &[], &[], &[])
                    .expect("parent projection");
            let cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } =
                &occurrences[0].parent
            else {
                panic!("child parent")
            };
            assert_eq!(occurrence, &occurrences[1].id);
            assert_eq!(occurrences[0].ordinal, 0);
            assert_eq!(occurrences[1].ordinal, 0);
            assert_eq!(ctx.resource_refusal(), None);
            let CodecError::ResourceLimit(limit) = ctx
                .charge_retained(u64::MAX, "retained projection measure")
                .expect_err("retained overflow")
            else {
                panic!("resource refusal")
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            limit.used
        })
    };
    // Definition id/native ref; container id/prototype/name/native ref; child parent id.
    assert_eq!(measure(200) - measure(100), 7 * 100);
}
