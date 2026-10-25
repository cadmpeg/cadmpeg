// SPDX-License-Identifier: Apache-2.0
//! Property owners and local links declare their exact dependency positions.

use super::*;
use crate::native::{LinkTarget, LinkTargetWire, PropertyFamily, ValueRecord};
use std::collections::{BTreeMap, BTreeSet};

fn linked(list: bool) -> DocumentValue {
    let target = super::super::native_id("object", "Target");
    let link = LinkTarget::optional_from_wire(LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some(target),
        subelements: Vec::new(),
    })
    .unwrap();
    DocumentValue::new(PropertyRecord {
        id: super::super::native_id("property", "Link"),
        owner: super::super::native_id("object", "Owner"),
        name: "Link".into(),
        type_name: if list {
            "App::PropertyLinkList"
        } else {
            "App::PropertyLink"
        }
        .into(),
        family: PropertyFamily::Link,
        status: None,
        dynamic: None,
        order: 0,
        body: Some(PropertyBody::Persisted {
            values: (if list {
                vec![
                    ValueRecord {
                        item_index: None,
                        link_index: None,
                        tag: "LinkList".into(),
                        order: 0,
                        parent: None,
                        attributes: BTreeMap::from([
                            ("count".into(), "1".into()),
                            ("note".into(), "fcstd:native:object#literal".into()),
                        ]),
                        decoded_json: None,
                        text: None,
                    },
                    ValueRecord {
                        item_index: None,
                        link_index: None,
                        tag: "Link".into(),
                        order: 1,
                        parent: Some(0),
                        attributes: BTreeMap::from([("value".into(), "Target".into())]),
                        decoded_json: None,
                        text: None,
                    },
                ]
            } else {
                vec![ValueRecord {
                    item_index: None,
                    link_index: None,
                    tag: "Link".into(),
                    order: 0,
                    parent: None,
                    attributes: BTreeMap::from([("value".into(), "Target".into())]),
                    decoded_json: None,
                    text: None,
                }]
            })
            .into(),
            links: vec![link],
            side_entries: Vec::new(),
        }),
    })
    .unwrap()
}

#[test]
fn missing_local_link_drops_a_required_property_and_prunes_only_list_members() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let target = super::super::native_id("object", "Target");
    let deleted = BTreeSet::from([target]);
    let mut single = linked(false);
    assert_eq!(
        single.prune_reference_lists(&ctx, &deleted).unwrap(),
        ReferencePruneEffect::DropRequired
    );
    assert_eq!(single.0.links().len(), 1);
    let mut list = linked(true);
    assert_eq!(
        list.prune_reference_lists(&ctx, &deleted).unwrap(),
        ReferencePruneEffect::ListsChanged
    );
    assert_eq!(list.0.links(), &[None]);
    assert_eq!(list.0.values().len(), 1);
    assert_eq!(list.0.values()[0].attributes["count"], "1");
    let owner = BTreeSet::from([list.0.owner.clone()]);
    assert_eq!(
        list.prune_reference_lists(&ctx, &owner).unwrap(),
        ReferencePruneEffect::DropRequired
    );
}

#[test]
fn link_identity_rewrites_preserve_unrelated_literal_fields() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let original = linked(true);
    let mut map = IdentityMap::new(|id: &str| Ok(id.replace('#', "#changed-")));
    let rewritten = original.rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(
        rewritten.0.links()[0].as_ref().unwrap().object(),
        Some("fcstd:native:object#changed-Target")
    );
    assert_eq!(rewritten.0.values()[1].attributes["value"], "Target");
    assert_eq!(
        rewritten.0.values()[0].attributes["note"],
        "fcstd:native:object#literal"
    );
}

#[test]
fn rewritten_reference_lists_prune_by_typed_slots_across_repeated_passes() {
    let text = "<Document><Properties Count='1'><Property name='Links' type='App::PropertyXLinkSubList'><XLinkSubList count='2'><XLink name='A' count='1'><Sub value='Face1'/></XLink><XLink name='B'/></XLinkSubList></Property></Properties><Objects Count='2'><Object name='A' type='App::Feature'/><Object name='B' type='App::Feature'/></Objects><ObjectData Count='0'/></Document>";
    let xml = roxmltree::Document::parse(text).unwrap();
    let graph =
        crate::persistence::parse_document(&xml, crate::persistence::Vocabulary::Objects).unwrap();
    let original = &graph.properties[0];
    assert_eq!(original.values().len(), 4);
    let ctx = cadmpeg_test_support::service_decode_context();
    for explicit in [true, false] {
        let mut property = original.clone();
        if !explicit {
            let Some(PropertyBody::Persisted { values, .. }) = &mut property.body else {
                panic!("held authored reference tree")
            };
            for value in values {
                value.link_index = None;
            }
        }
        let mut map = IdentityMap::new(|id: &str| Ok(format!("arbitrary/{id}")));
        let mut rewritten = DocumentValue::new(property)
            .unwrap()
            .rewrite_identities(&ctx, &mut map)
            .unwrap();
        let targets: Vec<_> = rewritten
            .0
            .links()
            .iter()
            .map(|link| link.as_ref().unwrap().object().unwrap().to_owned())
            .collect();
        assert_eq!(rewritten.0.values()[1].attributes["name"], "A");
        assert_eq!(
            rewritten
                .prune_reference_lists(&ctx, &BTreeSet::from([targets[0].clone()]))
                .unwrap(),
            ReferencePruneEffect::ListsChanged
        );
        assert!(rewritten.0.links()[0].is_none());
        assert_eq!(rewritten.0.values().len(), 2);
        assert_eq!(rewritten.0.values()[1].order, 3);
        assert_eq!(rewritten.0.values()[1].attributes["name"], "B");
        assert_eq!(rewritten.0.values()[1].link_index, Some(1));
        assert_eq!(
            rewritten
                .prune_reference_lists(&ctx, &BTreeSet::from([targets[1].clone()]))
                .unwrap(),
            ReferencePruneEffect::ListsChanged
        );
        assert_eq!(rewritten.0.links(), &[None, None]);
        assert_eq!(rewritten.0.values().len(), 1);
        assert_eq!(rewritten.0.values()[0].attributes["count"], "2");
    }
    let mut last = original.values()[3].clone();
    last.link_index = None;
    let ambiguous =
        crate::native::ValueSlots::new(vec![original.values()[0].clone(), last.clone()], 4)
            .unwrap();
    assert!(link_carrier_slots(&original.type_name, &ambiguous, 2)
        .unwrap()
        .is_empty());
    last.link_index = Some(1);
    let explicit =
        crate::native::ValueSlots::new(vec![original.values()[0].clone(), last], 4).unwrap();
    assert_eq!(
        link_carrier_slots(&original.type_name, &explicit, 2).unwrap(),
        vec![(3, 1)]
    );
}

#[test]
fn literal_source_names_that_resemble_native_ids_keep_distinct_targets_and_prune_independently() {
    let text = "<Document><Properties Count='1'><Property name='Links' type='App::PropertyLinkList'><LinkList count='2'><Link value='A'/><Link value='fcstd:native:object#A'/></LinkList></Property></Properties><Objects Count='2'><Object name='A' type='App::Feature'/><Object name='fcstd:native:object#A' type='App::Feature'/></Objects><ObjectData Count='0'/></Document>";
    let xml = roxmltree::Document::parse(text).unwrap();
    let graph =
        crate::persistence::parse_document(&xml, crate::persistence::Vocabulary::Objects).unwrap();
    let original = graph.properties[0].clone();
    let first = crate::native::native_id("object", "A");
    let second = crate::native::native_id("object", "fcstd:native:object#A");
    assert_ne!(first, second);
    assert_eq!(
        original.links()[0].as_ref().unwrap().object(),
        Some(first.as_str())
    );
    assert_eq!(
        original.links()[1].as_ref().unwrap().object(),
        Some(second.as_str())
    );
    assert_eq!(original.values()[1].attributes["value"], "A");
    assert_eq!(
        original.values()[2].attributes["value"],
        "fcstd:native:object#A"
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut pruned = DocumentValue::new(original.clone()).unwrap();
    assert_eq!(
        pruned
            .prune_reference_lists(&ctx, &BTreeSet::from([first]))
            .unwrap(),
        ReferencePruneEffect::ListsChanged
    );
    assert!(pruned.0.links()[0].is_none());
    assert_eq!(
        pruned.0.links()[1].as_ref().unwrap().object(),
        Some(second.as_str())
    );
    assert_eq!(pruned.0.values().len(), 2);
    assert_eq!(pruned.0.values()[1].order, 2);
    assert_eq!(
        pruned.0.values()[1].attributes["value"],
        "fcstd:native:object#A"
    );
    let mut map = IdentityMap::new(|id: &str| Ok(id.replace('#', "#changed-")));
    let rewritten = DocumentValue::new(original.clone())
        .unwrap()
        .rewrite_identities(&ctx, &mut map)
        .unwrap();
    assert_eq!(rewritten.0.values(), original.values());
    let mut wire = serde_json::to_value(&original).unwrap();
    let before = wire["values"].clone();
    let mut map = IdentityMap::new(|id: &str| Ok(id.replace('#', "#changed-")));
    DocumentValue::rewrite_native_value(&ctx, &mut wire, &mut map).unwrap();
    assert_eq!(wire["values"], before);
    assert_ne!(
        wire["links"],
        serde_json::to_value(&original).unwrap()["links"]
    );
}
