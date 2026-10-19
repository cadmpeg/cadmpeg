// SPDX-License-Identifier: Apache-2.0
use super::BodyPredecessors;
use crate::native::{
    LinkTarget, LinkTargetWire, ObjectRecord, PropertyBody, PropertyFamily, PropertyRecord,
    RetainedXml,
};
use cadmpeg_ir::features::FeatureId;
use std::collections::{BTreeMap, HashMap};

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
            LinkTarget::optional_from_wire(LinkTargetWire {
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
fn body_predecessors_keep_first_usable_body_and_first_member_winners() {
    let first_body = object("first-body", 0);
    let second_body = object("second-body", 1);
    let third_body = object("third-body", 2);
    let first_members = membership_property(first_body.id(), &["member", "base-a", "member"]);
    let second_members = membership_property(second_body.id(), &["base-b", "member", "member"]);
    let third_members = membership_property(third_body.id(), &["base-c", "member"]);
    let objects = [first_body, second_body, third_body];
    let properties_by_owner = HashMap::from([
        (objects[0].id().as_str(), vec![&first_members]),
        (objects[1].id().as_str(), vec![&second_members]),
        (objects[2].id().as_str(), vec![&third_members]),
    ]);
    let mut features = HashMap::new();
    for name in ["base-a", "base-b", "base-c"] {
        features.insert(
            name,
            FeatureId::mint(format!("test:test:feature#{name}")).expect("valid feature identity"),
        );
    }

    let index = BodyPredecessors::new(&objects, &features, &properties_by_owner);
    assert!(index.entries.get().is_none());
    assert_eq!(index.get("member"), features.get("base-b"));
    assert!(index.entries.get().is_some());
    assert_eq!(index.get("absent"), None);
    assert_eq!(index.get("member"), features.get("base-b"));
}
