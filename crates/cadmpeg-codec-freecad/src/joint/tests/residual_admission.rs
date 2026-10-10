// SPDX-License-Identifier: Apache-2.0
//! Joint consumer selection, prefix admission, and discarded-output controls.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::scalar::FiniteReal;

use crate::native::joint::{JointBody, JointConnectorRecord, JointRecord, PairedJointFamily};
use crate::native::{LinkTarget, ObjectRecord, PropertyRecord};

fn object() -> ObjectRecord {
    ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Joint".into(),
            "Joint".into(),
        )
        .expect("object identity"),
        type_name: "App::FeaturePython".into(),
        persistent_id: None,
        view_type: None,
        attributes: BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    }
}

fn carrier(type_name: &str) -> PropertyRecord {
    let mut property = super::diagnostic_property("ObjectToGround", type_name, "<Property/>");
    property.owner = object().id().clone();
    property
}

fn link(document: Option<&str>, object: Option<&str>) -> LinkTarget {
    serde_json::from_value(serde_json::json!({
        "document": document, "document_attribute": document.map(|_| "file"),
        "object": object, "subelements": ["Face1"]
    }))
    .expect("link selection")
}

fn record(body: JointBody, parameters: BTreeMap<String, String>) -> JointRecord {
    crate::test_support::with_service_context(&[], |ctx| {
        JointRecord::try_new(
            ctx,
            "fcstd:native:joint#Joint".into(),
            "fcstd:native:object#Joint".into(),
            body,
            crate::native::joint::JointParameters::from_raw(parameters, "fcstd:native:joint#Joint")
                .expect("checked values"),
        )
        .expect("native record")
    })
}

fn grounded(reference: Option<LinkTarget>) -> JointRecord {
    record(
        JointBody::Grounded {
            reference,
            placement: Default::default(),
        },
        BTreeMap::new(),
    )
}

fn pair(
    first: Option<LinkTarget>,
    second: Option<LinkTarget>,
    parameters: BTreeMap<String, String>,
) -> JointRecord {
    record(
        JointBody::Pair {
            kind: PairedJointFamily::new("CustomCoupling".into()).expect("custom family"),
            connectors: Box::new([
                JointConnectorRecord {
                    reference: first,
                    placement: Default::default(),
                    offset: Default::default(),
                },
                JointConnectorRecord {
                    reference: second,
                    placement: Default::default(),
                    offset: Default::default(),
                },
            ]),
        },
        parameters,
    )
}

fn occurrence(suffix: &str, native: Option<&str>) -> Occurrence {
    Occurrence {
        id: OccurrenceId::mint(format!("fcstd:model:occurrence#{suffix}")).expect("identity"),
        prototype: PrototypeReference::Unresolved {},
        parent: OccurrenceParent::Root {},
        ordinal: 0,
        transform: cadmpeg_ir::transform::Transform::identity(),
        linked_prototype: None,
        scale: [FiniteReal::new(1.0).unwrap(); 3],
        name: None,
        visible: None,
        link: None,
        native_ref: native.map(str::to_owned),
    }
}

#[test]
fn joint_empty_objects_need_no_owner_index() {
    let property = carrier("App::PropertyLink");
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
fn joint_no_carrier_needs_only_candidate_scan() {
    let mut property = carrier("App::PropertyString");
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
fn joint_non_object_carriers_need_no_owner_property_or_record_slots() {
    for (owner, slots) in [
        ("fcstd:native:document#0", 0),
        ("fcstd:native:extension#Other", 1),
    ] {
        let mut property = carrier("App::PropertyLink");
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
fn joint_first_candidate_storage_refusal_does_not_precharge_suffix() {
    let mut properties = vec![carrier("App::PropertyLink")];
    for _ in 0..4096 {
        let mut unused = properties[0].clone();
        unused.name = "Other".into();
        properties.push(unused);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4096;
    assert!(policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(properties.len()));
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) =
        super::super::transfer(&ctx, &[object()], &properties).expect_err("owner slot")
    else {
        panic!("resource");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn joint_first_malformed_object_does_not_precharge_object_suffix() {
    let object = object();
    let property = carrier("App::PropertyString");
    let objects = vec![object; 8193];
    let message = "joint property property has the wrong runtime type for ObjectToGround";
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
fn joint_first_malformed_parameter_does_not_precharge_parameter_suffix() {
    let mut bad = super::scalar_property(
        "App::PropertyBool",
        "<Property><Float value=\"1\"/></Property>",
    );
    bad.owner = object().id().clone();
    let mut properties = vec![carrier("App::PropertyLink"), bad];
    for _ in 0..8192 {
        let mut unused = properties[0].clone();
        unused.name = "Distance".into();
        properties.push(unused);
    }
    // The core oracle includes all real index-building work before parameter consumption.
    let CodecError::ResourceLimit(boundary) = crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "fcstd joint parameter properties",
        |ctx| super::super::transfer(ctx, &[object()], &properties),
    ) else {
        panic!("work boundary");
    };
    let message = "joint parameter property property has runtime type App::PropertyBool, expected App::PropertyAngle";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Allow a fixed first-error margin after the required grouping prelude.
    policy.limits.max_work_units = boundary.used + 4096;
    assert!(4096 < properties.len() - 2);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(
        matches!(super::super::transfer(&ctx, &[object()], &properties),
        Err(CodecError::Malformed(value)) if value == message)
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn joint_empty_records_need_no_occurrence_work_or_storage() {
    let occurrences = vec![occurrence("Unused", Some("fcstd:native:object#Unused")); 1025];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(super::super::transfer_neutral(&ctx, &[], &occurrences)
        .unwrap()
        .is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn joint_unusable_references_retain_no_discarded_identity_kind_or_operand() {
    let cases = [
        grounded(None),
        grounded(Some(link(Some("external.FCStd"), None))),
        pair(
            None,
            Some(link(None, Some("fcstd:native:object#Target"))),
            BTreeMap::new(),
        ),
        pair(
            Some(link(None, Some("fcstd:native:object#Target"))),
            None,
            BTreeMap::new(),
        ),
        pair(
            Some(link(None, Some("fcstd:native:object#Target"))),
            Some(link(None, None)),
            BTreeMap::new(),
        ),
    ];
    let occurrences = vec![occurrence("Unused", Some("fcstd:native:object#Target")); 4097];
    for record in cases {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 256;
        assert!(
            policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(occurrences.len())
        );
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(
            super::super::transfer_neutral(&ctx, &[record], &occurrences)
                .unwrap()
                .is_empty()
        );
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn joint_incomplete_pair_still_validates_enabled_limits() {
    let record = pair(
        Some(link(None, Some("fcstd:native:object#Target"))),
        None,
        BTreeMap::from([
            ("LengthMin".into(), "2".into()),
            ("LengthMax".into(), "1".into()),
            ("EnableLengthMin".into(), "true".into()),
            ("EnableLengthMax".into(), "true".into()),
        ]),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(
        matches!(super::super::transfer_neutral(&ctx, &[record], &[]),
        Err(CodecError::Malformed(value)) if value == "joint limits minimum/maximum must be finite and ordered")
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn joint_external_only_references_need_no_occurrence_index() {
    let record = grounded(Some(link(Some("external.FCStd"), Some("ExternalPart"))));
    let CodecError::ResourceLimit(boundary) = crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "after external joint",
        |ctx| {
            super::super::transfer_neutral(ctx, std::slice::from_ref(&record), &[])?;
            ctx.charge_work(1, "after external joint")
        },
    ) else {
        panic!("output work boundary");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One subelement and one emitted joint; no occurrence index slots.
    policy.limits.max_collection_items = 2;
    // Measure complete output work and allow a generous margin.
    policy.limits.max_work_units = boundary.used * 2 + 256;
    let occurrence_count =
        usize::try_from(policy.limits.max_work_units + 4097).expect("occurrence suffix count");
    let occurrences =
        vec![occurrence("Unused", Some("fcstd:native:object#Unused")); occurrence_count];
    assert!(policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(occurrences.len()));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let output = super::super::transfer_neutral(&ctx, &[record], &occurrences).unwrap();
    assert_eq!(output.len(), 1);
    let connector = output[0].connectors().next().unwrap();
    assert!(matches!(
        connector.operand.container,
        cadmpeg_ir::products::OperandContainer::External { .. }
    ));
    assert_eq!(connector.operand.object, "ExternalPart");
    assert_eq!(connector.operand.subelements, ["Face1"]);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn joint_local_references_resolve_last_occurrence_and_preserve_record_order() {
    let first = grounded(Some(link(None, Some("fcstd:native:object#Target"))));
    let second = grounded(Some(link(None, Some("fcstd:native:object#Missing"))));
    // Record order is source order, including repeated native record identities.
    let occurrences = [
        occurrence("First", Some("fcstd:native:object#Target")),
        occurrence("Last", Some("fcstd:native:object#Target")),
    ];
    crate::test_support::with_service_context(&[], |ctx| {
        let output = super::super::transfer_neutral(ctx, &[first, second], &occurrences).unwrap();
        assert_eq!(output.len(), 2);
        let first = output[0].connectors().next().unwrap();
        assert!(
            matches!(&first.operand.container, cadmpeg_ir::products::OperandContainer::Occurrence { occurrence } if occurrence == &occurrences[1].id)
        );
        assert_eq!(first.operand.object, "fcstd:native:object#Target");
        assert_eq!(first.operand.subelements, ["Face1"]);
        assert!(matches!(
            output[1].connectors().next().unwrap().operand.container,
            cadmpeg_ir::products::OperandContainer::Root {}
        ));
    });
}

#[test]
fn joint_first_occurrence_storage_refusal_does_not_precharge_suffix() {
    let record = grounded(Some(link(None, Some("fcstd:native:object#Target"))));
    let first = occurrence("First", Some("fcstd:native:object#Target"));
    let occurrences = vec![first; 4097];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 4096;
    assert!(policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(occurrences.len()));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) =
        super::super::transfer_neutral(&ctx, &[record], &occurrences)
            .expect_err("occurrence index slot")
    else {
        panic!("resource");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn joint_first_emitted_identity_refusal_does_not_precharge_record_suffix() {
    let record = grounded(Some(link(None, Some("fcstd:native:object#Target"))));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = 512;
    let records = vec![record; 1025];
    assert!(policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(records.len()));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) =
        super::super::transfer_neutral(&ctx, &records, &[]).expect_err("emitted identity storage")
    else {
        panic!("resource");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn joint_selected_owner_keeps_parameters_and_skips_other_groups() {
    let mut angle = super::scalar_property(
        "App::PropertyAngle",
        "<Property><Float value=\"15.5\"/></Property>",
    );
    angle.owner = object().id().clone();
    let mut properties = vec![angle, carrier("App::PropertyLink")];
    for index in 0..128 {
        let mut unused = properties[0].clone();
        unused.owner = format!("unused{index}");
        properties.push(unused);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One owner, two properties, one checked parameter, and the output record.
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let output = super::super::transfer(&ctx, &[object()], &properties).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].object(), "fcstd:native:object#Joint");
    assert_eq!(output[0].parameters().raw("Angle"), Some("15.5"));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn joint_identity_refusal_precedes_invalid_enabled_limits() {
    let record = pair(
        Some(link(None, Some("fcstd:native:object#First"))),
        Some(link(None, Some("fcstd:native:object#Second"))),
        BTreeMap::from([
            ("LengthMin".into(), "2".into()),
            ("LengthMax".into(), "1".into()),
            ("EnableLengthMin".into(), "true".into()),
            ("EnableLengthMax".into(), "true".into()),
        ]),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) =
        super::super::transfer_neutral(&ctx, &[record], &[]).expect_err("identity precedes limits")
    else {
        panic!("retained refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn joint_occurrence_index_refusal_precedes_identity_allocation() {
    let record = grounded(Some(link(None, Some("fcstd:native:object#Target"))));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) = super::super::transfer_neutral(
        &ctx,
        &[record],
        &[occurrence("Target", Some("fcstd:native:object#Target"))],
    )
    .expect_err("index precedes identity") else {
        panic!("collection refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
