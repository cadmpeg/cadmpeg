// SPDX-License-Identifier: Apache-2.0
//! An absent property body is distinct from a known empty body.

use crate::native::{DynamicPropertyMeta, PropertyBody, PropertyFamily, PropertyRecord};

fn header() -> PropertyRecord {
    PropertyRecord {
        id: "property".into(),
        owner: "document".into(),
        name: "Count".into(),
        type_name: "App::PropertyIntegerConstraint".into(),
        family: PropertyFamily::Scalar,
        status: Some(7),
        dynamic: None,
        body: None,
        order: 0,
    }
}

#[test]
fn absent_body_has_required_null_payloads_and_independent_header_metadata() {
    let mut property = header();
    property.dynamic = Some(DynamicPropertyMeta {
        group: "Custom".into(),
        documentation: Some("kept".into()),
        attributes: Some(12),
        read_only: Some(true),
        hidden: Some(false),
    });
    let wire = serde_json::to_value(&property).unwrap();
    for key in ["values", "links", "side_entries"] {
        assert!(wire[key].is_null());
        let mut missing = wire.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<PropertyRecord>(missing).is_err());
        let mut mixed = wire.clone();
        mixed[key] = serde_json::json!([]);
        assert!(serde_json::from_value::<PropertyRecord>(mixed).is_err());
    }
    assert!(wire.get("payload_error").is_none());
    assert_eq!(wire["name"], "Count");
    assert_eq!(wire["type_name"], "App::PropertyIntegerConstraint");
    assert_eq!(wire["dynamic"]["group"], "Custom");
    assert_eq!(
        serde_json::from_value::<PropertyRecord>(wire.clone()).unwrap(),
        property
    );
    let mut contradictory = wire.clone();
    contradictory["payload_error"] = serde_json::json!("not admitted");
    assert!(serde_json::from_value::<PropertyRecord>(contradictory).is_err());
    let mut transient = wire;
    transient["transient"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PropertyRecord>(transient).is_err());
}

#[test]
fn known_empty_and_legacy_unreadable_bodies_keep_their_existing_wire_facts() {
    let mut property = header();
    property.body = Some(PropertyBody::Persisted {
        values: vec![],
        links: vec![],
        side_entries: vec![],
    });
    let wire = serde_json::to_value(&property).unwrap();
    for key in ["values", "links", "side_entries"] {
        assert_eq!(wire[key], serde_json::json!([]));
    }
    assert_eq!(
        serde_json::from_value::<PropertyRecord>(wire).unwrap(),
        property
    );
    property.body = Some(PropertyBody::Unreadable(
        "original parser diagnostic".into(),
    ));
    let wire = serde_json::to_value(&property).unwrap();
    assert_eq!(wire["payload_error"], "original parser diagnostic");
    for key in ["values", "links", "side_entries"] {
        assert_eq!(wire[key], serde_json::json!([]));
    }
    assert_eq!(
        serde_json::from_value::<PropertyRecord>(wire).unwrap(),
        property
    );
}
