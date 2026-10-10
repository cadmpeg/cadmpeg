// SPDX-License-Identifier: Apache-2.0
//! Demand-driven attachment indexes and prefix-only visits.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::native::{ObjectRecord, PropertyBody};

#[test]
fn attachment_out_of_range_index_admits_complete_diagnostic() {
    let property = super::diagnostic_property(
        "App::PropertyEnumeration",
        vec![super::enum_value("Integer", Some("999"))],
    );
    crate::test_support::assert_retained_refusal_at(
        &[],
        "FreeCAD attachment map-mode error",
        |ctx| super::super::map_mode_value(ctx, &property),
    );
    crate::test_support::with_service_context(&[], |ctx| {
        let error = super::super::map_mode_value(ctx, &property).expect_err("out of range");
        assert_eq!(error.to_string(),
            "malformed container: attachment property fcstd:native:property#Attachment:MapMode: map_mode index 999 is out of range");
        assert_eq!(ctx.resource_refusal(), None);
    });
}

pub(super) fn object() -> ObjectRecord {
    ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Attachment".into(),
            "Attachment".into(),
        )
        .expect("object identity"),
        type_name: "Part::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    }
}

#[test]
fn attachment_empty_objects_need_no_property_work_or_storage() {
    let property = super::diagnostic_property("App::PropertyEnumeration", Vec::new());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(
        super::super::transfer(&ctx, &[], std::slice::from_ref(&property))
            .unwrap()
            .is_empty()
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn attachment_no_carrier_needs_only_candidate_scan() {
    let mut property = super::diagnostic_property("App::PropertyEnumeration", Vec::new());
    property.name = "Other".into();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 256;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(
        super::super::transfer(&ctx, &vec![object(); 4097], &[property])
            .unwrap()
            .is_empty()
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn attachment_non_object_carriers_need_no_owner_property_or_record_slots() {
    for (owner, slots) in [
        ("fcstd:native:document#0", 0),
        ("fcstd:native:extension#Other", 1),
    ] {
        let mut property = super::diagnostic_property("App::PropertyEnumeration", Vec::new());
        property.owner = owner.into();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = slots;
        policy.limits.max_retained_bytes = 0;
        if slots == 0 {
            policy.limits.max_materialized_bytes = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(super::super::transfer(&ctx, &[object()], &[property])
            .unwrap()
            .is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn attachment_selected_owner_keeps_consumed_properties_and_skips_other_groups() {
    let object = object();
    let mut mode = super::diagnostic_property(
        "App::PropertyEnumeration",
        vec![super::enum_value("Integer", Some("5"))],
    );
    mode.owner = object.id().clone();
    let mut other = mode.clone();
    other.name = "Other".into();
    let mut properties = vec![mode, other];
    for index in 0..128 {
        let mut unused = properties[1].clone();
        unused.owner = format!("unused{index}");
        properties.push(unused);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Only the selected owner, its property, and the output record need slots.
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let records = super::super::transfer(&ctx, std::slice::from_ref(&object), &properties).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].object, *object.id());
    assert_eq!(records[0].map_mode.unwrap().to_string(), "5");
}

#[test]
fn attachment_duplicate_mode_precedes_unread_object_suffix() {
    let objects = vec![object(); 8193];
    let mut property = super::diagnostic_property("App::PropertyEnumeration", Vec::new());
    property.owner = objects[0].id().clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(
        matches!(super::super::transfer(&ctx, &objects, &[property.clone(), property]),
        Err(CodecError::Malformed(message)) if message == "attachment property MapMode occurs more than once")
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn attachment_first_candidate_storage_refusal_does_not_precharge_suffix() {
    let object = object();
    let mut property = super::diagnostic_property("App::PropertyEnumeration", Vec::new());
    property.owner = object.id().clone();
    let mut properties = vec![property];
    for _ in 0..8192 {
        let mut unused = properties[0].clone();
        unused.name = "Other".into();
        properties.push(unused);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1024;
    policy.limits.max_collection_items = 0;
    assert!(policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(properties.len()));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) =
        super::super::transfer(&ctx, &[object], &properties).expect_err("owner slot")
    else {
        panic!("collection refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn attachment_first_malformed_object_does_not_precharge_object_suffix() {
    let object = object();
    let mut property = super::diagnostic_property("App::PropertyString", Vec::new());
    property.owner = object.id().clone();
    let objects = vec![object; 8193];
    let message = format!(
        "attachment property {} has runtime type {}, expected App::PropertyEnumeration",
        property.id, property.type_name,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4096;
    assert!(policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(objects.len()));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(
        matches!(super::super::transfer(&ctx, &objects, &[property]),
        Err(CodecError::Malformed(value)) if value == message)
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn attachment_first_support_copy_refusal_does_not_precharge_link_suffix() {
    let mut property = super::diagnostic_property(
        "App::PropertyLinkSubList",
        vec![super::enum_value("LinkSubList", None)],
    );
    let link: crate::native::LinkTarget = serde_json::from_value(serde_json::json!({
        "document": null, "document_attribute": null,
        "object": "fcstd:native:object#Target", "subelements": []
    }))
    .expect("local link");
    let mut links = vec![Some(link)];
    links.resize(1025, None);
    let storage = links.len() * std::mem::size_of::<Option<crate::native::LinkTarget>>();
    property.body = PropertyBody::Persisted {
        values: property.values().to_vec(),
        links,
        side_entries: Vec::new(),
        dynamic: None,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(storage);
    policy.limits.max_work_units = 512;
    assert!(
        policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(property.links().len())
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) =
        super::super::support_links(&ctx, &property).expect_err("first object copy refuses")
    else {
        panic!("retained refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
