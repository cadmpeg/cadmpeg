// SPDX-License-Identifier: Apache-2.0
//! Product traversal and scratch-storage budget regressions.

use super::super::{
    bool_list, integer_property, metadata_string, parse_finite, parse_placement_list,
    product_cycle_nodes, require_root, transfer,
};
use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, RetainedXml, ValueRecord};
use crate::test_support::{refusal_at, with_service_context};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn external_prototype_name_does_not_create_a_local_product_cycle() {
    use crate::native::{
        ExternalDocument, LinkArray, LinkOccurrence, ProductNode, ProductNodeRecord,
    };

    let assembly = "fcstd:native:object#Assembly";
    let occurrence = "fcstd:native:object#Occurrence";
    let mut link = ProductNodeRecord {
        id: "fcstd:native:product#Occurrence".into(),
        object: occurrence.into(),
        node: ProductNode::Occurrence(Box::new(LinkOccurrence {
            members: Vec::new(),
            prototype: Some(assembly.into()),
            external_document: Some(ExternalDocument::File(
                "other.FCStd".try_into().expect("external path"),
            )),
            local_transform: None,
            placement_property: None,
            array: LinkArray::try_new(None, Vec::new(), Vec::new(), Vec::new(), Vec::new())
                .expect("scalar link array"),
            link_transform: None,
            linked_subelements: Vec::new(),
            claim_child: None,
            copy_on_change: None,
            scale: None,
        })),
    };
    with_service_context(&[], |ctx| {
        let records = [super::node(assembly, &[occurrence]), link.clone()];
        assert!(product_cycle_nodes(ctx, &records)
            .expect("external prototype")
            .is_empty());
    });
    let ProductNode::Occurrence(value) = &mut link.node else {
        unreachable!("occurrence fixture")
    };
    value.external_document = None;
    with_service_context(&[], |ctx| {
        let records = [super::node(assembly, &[occurrence]), link];
        assert_eq!(
            product_cycle_nodes(ctx, &records).expect("local prototype"),
            std::collections::BTreeSet::from([assembly, occurrence]),
        );
    });
}

fn property(name: &str, type_name: &str, xml: &str) -> PropertyRecord {
    let document = roxmltree::Document::parse(xml).expect("test XML");
    let values = document
        .root_element()
        .children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
        .map(|(order, node)| ValueRecord {
            tag: node.tag_name().name().into(),
            order,
            attributes: node
                .attributes()
                .map(|attribute| (attribute.name().into(), attribute.value().into()))
                .collect(),
            text: None,
            raw_xml: xml[node.range()].into(),
        })
        .collect();
    PropertyRecord {
        id: "fcstd:native:property#Owner:Value".into(),
        owner: "fcstd:native:object#Owner".into(),
        name: name.into(),
        type_name: type_name.into(),
        family: PropertyFamily::Unknown,
        status: None,
        body: PropertyBody::Persisted {
            values,
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: RetainedXml::from_text(xml.into(), 0).expect("test span"),
    }
}

#[test]
fn product_owner_index_visits_and_storage_refuse_at_core_boundaries() {
    let item = property(
        "Unused",
        "App::PropertyString",
        "<Property><String value=\"test\"/></Property>",
    );
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Owner".into(),
            "Owner".into(),
        )
        .expect("object identity"),
        type_name: "App::Part".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
    ] {
        refusal_at(dimension, &[], "fcstd product owner index", |ctx| {
            transfer(
                ctx,
                std::slice::from_ref(&object),
                std::slice::from_ref(&item),
                &BTreeMap::new(),
            )
        });
    }
}

#[test]
fn product_owner_index_and_acyclic_graph_keep_scratch_out_of_retained_budget() {
    let item = property(
        "Unused",
        "App::PropertyString",
        "<Property><String value=\"test\"/></Property>",
    );
    let records = [super::node("A", &["B"]), super::node("B", &[])];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let index = ctx
        .collect_scoped_btree_groups(
            std::iter::once((item.owner.as_str(), &item)),
            "fcstd product owner index",
        )
        .expect("scoped owners");
    let storage = index.1;
    let owners = index.0;
    assert!(std::ptr::eq(owners[item.owner.as_str()][0], &item));
    drop((owners, storage));
    assert!(product_cycle_nodes(&ctx, &records)
        .expect("scoped graph")
        .is_empty());
}

#[test]
fn product_real_list_range_refuses_before_first_position() {
    let mut bytes = vec![0xff; 9];
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7 * 8]);
    let view = View::over_retained(&bytes)
        .child(9, bytes.len())
        .expect("bounded list");
    let mut item = property(
        "PlacementList",
        "App::PropertyPlacementList",
        "<Property><PlacementList/></Property>",
    );
    let PropertyBody::Persisted { side_entries, .. } = &mut item.body else {
        panic!("persisted list")
    };
    side_entries.push("positions".into());
    let entries = BTreeMap::from([("positions".into(), view)]);
    refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "fcstd product placement positions",
        |ctx| parse_placement_list(ctx, &[&item], &entries),
    );
}

#[test]
fn product_metadata_xml_traversal_refuses_at_core_boundaries() {
    let item = property(
        "Label",
        "App::PropertyString",
        "<Property><String value=\"label\"/></Property>",
    );
    for operation in [
        "fcstd product metadata property search",
        "fcstd product metadata root",
        "fcstd product metadata children",
        "fcstd product metadata tag",
        "fcstd product metadata value children",
        "fcstd product metadata attribute",
    ] {
        refusal_at(ResourceDimension::WorkUnits, &[], operation, |ctx| {
            metadata_string(ctx, &[&item], "Label")
        });
    }
}

#[test]
fn product_scalar_attributes_numbers_and_bit_scans_refuse_at_core_boundaries() {
    let integer = property(
        "ElementCount",
        "App::PropertyIntegerConstraint",
        "<Property><Integer value=\"12\"/></Property>",
    );
    for operation in [
        "fcstd product value attribute",
        "fcstd product integer parse",
    ] {
        refusal_at(ResourceDimension::WorkUnits, &[], operation, |ctx| {
            integer_property(ctx, &[&integer], "ElementCount")
        });
    }
    let scale = property(
        "Scale",
        "App::PropertyFloat",
        "<Property><Float value=\"2.5\"/></Property>",
    );
    refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "fcstd product finite parse",
        |ctx| parse_finite(ctx, "2.5", &scale, "Scale"),
    );
    let bits = property(
        "VisibilityList",
        "App::PropertyBoolList",
        "<Property><BoolList value=\"1010\"/></Property>",
    );
    for operation in [
        "fcstd product visibility validation",
        "fcstd product visibility bits",
    ] {
        refusal_at(ResourceDimension::WorkUnits, &[], operation, |ctx| {
            bool_list(ctx, &[&bits], "VisibilityList")
        });
    }
}

#[test]
fn product_duplicate_root_search_does_not_charge_unvisited_suffix() {
    let small = property(
        "Group",
        "App::PropertyLinkList",
        "<Property><LinkList/><LinkList/></Property>",
    );
    let large = property(
        "Group",
        "App::PropertyLinkList",
        "<Property><LinkList/><LinkList/><Link/><Link/><Link/><Link/></Property>",
    );
    let work = |item: &PropertyRecord| {
        let error = refusal_at(
            ResourceDimension::WorkUnits,
            &[],
            "after duplicate root",
            |ctx| {
                assert!(matches!(
                    require_root(ctx, item, "App::PropertyLinkList", "Group", "LinkList"),
                    Err(CodecError::Malformed(_))
                ));
                ctx.charge_work(1, "after duplicate root")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal")
        };
        limit.used
    };
    assert_eq!(work(&small), work(&large));
    with_service_context(&[], |ctx| {
        let error = require_root(ctx, &large, "App::PropertyLinkList", "Group", "LinkList")
            .expect_err("duplicate root");
        assert!(
            matches!(error, CodecError::Malformed(message) if message.contains("requires one LinkList"))
        );
    });
}

#[test]
fn product_cycle_scan_refuses_at_traversal_and_lookup_boundaries() {
    let records = [super::node("A", &["B"]), super::node("B", &["A"])];
    for operation in [
        "fcstd product cycle members",
        "fcstd product cycle target lookup",
        "fcstd product forward traversal",
        "fcstd product forward lookup",
        "fcstd product forward edges",
        "fcstd product reverse traversal",
        "fcstd product reverse lookup",
        "fcstd product reverse edges",
    ] {
        refusal_at(ResourceDimension::WorkUnits, &[], operation, |ctx| {
            product_cycle_nodes(ctx, &records)
        });
    }
}

fn invalid_binary_prefix(
    name: &str,
    type_name: &str,
    root: &str,
    components: usize,
    diagnostic: &str,
) {
    let mut item = property(name, type_name, &format!("<Property><{root}/></Property>"));
    let PropertyBody::Persisted { side_entries, .. } = &mut item.body else {
        panic!("persisted list")
    };
    side_entries.push("list".into());
    let bytes = |count: u32| {
        let mut bytes = count.to_le_bytes().to_vec();
        for _ in 0..count {
            for axis in 0..components {
                let value = if components == 3 && axis == 0 {
                    f64::NAN
                } else {
                    0.0
                };
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    };
    let run = |ctx: &DecodeContext<'_>, bytes: &[u8]| {
        let entries = BTreeMap::from([("list".into(), View::over_retained(bytes))]);
        let result = if components == 7 {
            super::super::parse_placement_list(ctx, &[&item], &entries).map(|_| ())
        } else {
            super::super::parse_vector_list(ctx, &[&item], &entries).map(|_| ())
        };
        let error = result.expect_err("invalid first tuple");
        assert!(matches!(error, CodecError::Malformed(message) if message == diagnostic));
        assert_eq!(ctx.resource_refusal(), None);
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    run(&ctx, &bytes(1));
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "list work measure")
        .expect_err("work overflow")
    else {
        panic!("work refusal")
    };
    let cap = limit.used;
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    run(&ctx, &bytes(u32::try_from(cap + 1).expect("finite count")));
}

#[test]
fn product_placement_list_rejects_first_invalid_tuple_without_prepaying_suffix() {
    invalid_binary_prefix(
        "PlacementList",
        "App::PropertyPlacementList",
        "PlacementList",
        7,
        "PlacementList contains an invalid placement value",
    );
}

#[test]
fn product_scale_list_rejects_first_invalid_tuple_without_prepaying_suffix() {
    invalid_binary_prefix(
        "ScaleList",
        "App::PropertyVectorList",
        "VectorList",
        3,
        "element_scales: scale vector components must be finite",
    );
}
