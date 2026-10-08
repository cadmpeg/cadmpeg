// SPDX-License-Identifier: Apache-2.0
//! Actual source visits stop at the first semantic failure.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn successful_work(run: impl FnOnce(&DecodeContext<'_>) -> Result<(), CodecError>) -> u64 {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    run(&ctx).expect("short core oracle succeeds");
    assert!(ctx.resource_refusal().is_none());
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "actual visit work oracle")
        .expect_err("overflow exposes successful work")
    else {
        panic!("expected Work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "actual visit work oracle");
    limit.used
}

#[test]
fn actual_visits_duplicate_state_stops_before_long_property_suffix() {
    let property = super::linked_property("State", "Visibility", "visible");
    let expected = "State states the property Visibility a second time";
    let cap = successful_work(|ctx| {
        let state = super::super::feature_state(ctx, "State", &[&property])?;
        let input = [&property];
        let mut source = input.iter();
        ctx.next_charged(&mut source, "fcstd feature state properties")?;
        ctx.copy_retained_text("visible", "fcstd feature state value")?;
        let name = ctx.copy_retained_text("Visibility", "fcstd feature state name")?;
        let name = cadmpeg_core::text::NonBlankString::for_decode(
            ctx, name, "validate nonblank text",
        )?.expect("literal state key");
        assert!(ctx.contains_key_btree_map(&state, &name, "fcstd feature state duplicate key")?);
        assert_eq!(ctx.format_retained(
            format_args!("State states the property {name} a second time"),
            "fcstd feature state duplicate key error",
        )?, expected);
        Ok(())
    });
    let suffix = usize::try_from(cap).expect("small oracle") + 1;
    let mut properties = vec![&property, &property];
    properties.extend(std::iter::repeat_n(&property, suffix));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(super::super::feature_state(&ctx, "State", &properties),
        Err(CodecError::Malformed(message)) if message == expected));
    assert!(ctx.resource_refusal().is_none());
}

fn numeric_input(values: &[f64]) -> (crate::native::PropertyRecord, crate::native::EntryRecord) {
    let mut property = super::linked_property("Numbers", "Values", "unused");
    property.type_name = "App::PropertyFloatList".into();
    property.body = crate::native::PropertyBody::Persisted {
        values: Vec::new(), links: Vec::new(), side_entries: vec!["numbers.bin".into()], dynamic: None,
    };
    property.xml = crate::native::RetainedXml::from_text(
        "<Property><FloatList file=\"numbers.bin\"/></Property>".into(), 0,
    ).expect("valid list XML");
    let mut data = u32::try_from(values.len()).expect("fixture count").to_le_bytes().to_vec();
    for value in values {
        data.extend(value.to_le_bytes());
    }
    let entry = crate::test_support::entry_record(
        crate::native::native_id("entry", "numbers.bin"), "numbers.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary, Vec::new(), data,
    );
    (property, entry)
}

#[test]
fn actual_visits_nonfinite_numeric_value_stops_before_long_list_suffix() {
    let (property, entry) = numeric_input(&[2.5]);
    let cap = successful_work(|ctx| {
        let values = super::super::numeric_list(ctx, &property, std::slice::from_ref(&entry))?
            .expect("finite numeric list");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].get(), 2.5);
        Ok(())
    });
    let suffix = usize::try_from(cap).expect("small oracle") + 1;
    let mut values = vec![f64::NAN];
    values.extend(std::iter::repeat_n(2.5, suffix));
    let (property, entry) = numeric_input(&values);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(super::super::numeric_list(&ctx, &property, &[entry])
        .expect("invalid value remains semantic").is_none());
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn actual_visits_empty_design_routes_preserve_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original empty-route fuse")
        .expect_err("fuse context") else { panic!("resource refusal") };
    let errors = [
        super::super::feature_state(&ctx, "State", &[]).map(drop).expect_err("state fuse"),
        super::super::native_parameters(&ctx, &[]).map(drop).expect_err("native parameter fuse"),
        super::super::external_link_indices(&ctx, None).map(drop).expect_err("link index fuse"),
        super::super::sketch_attributes(&ctx, None).map(drop).expect_err("attribute fuse"),
        super::super::builtin_reference_usage(&ctx, None).map(drop).expect_err("reference fuse"),
        super::super::multi_transform_stage_seeds(
            &ctx, "stage", &std::collections::HashMap::new(), &[], &std::collections::BTreeMap::new(),
        ).map(drop).expect_err("consumer fuse"),
    ];
    for error in errors {
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit == original));
    }
    let object = super::super::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#State".into(), "State".into(),
        ).expect("object identity"),
        type_name: "Part::Feature".into(), persistent_id: None, view_type: None,
        attributes: std::collections::BTreeMap::new(), dependencies: Vec::new(),
        dependency_allow_partial: None, order: 0, data: None,
    };
    assert!(matches!(super::super::append_operation_parameters(&ctx, &mut Vec::new(), &object, &[]),
        Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn actual_visits_nested_constraint_failure_stops_before_long_record_suffix() {
    let mut property = super::linked_property("Sketch", "Constraints", "unused");
    property.type_name = "Sketcher::PropertyConstraintList".into();
    let object = super::super::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Sketch".into(), "Sketch".into(),
        ).expect("object identity"),
        type_name: "Sketcher::SketchObject".into(), persistent_id: None, view_type: None,
        attributes: std::collections::BTreeMap::new(), dependencies: Vec::new(),
        dependency_allow_partial: None, order: 0, data: None,
    };
    let sketch = cadmpeg_ir::sketches::SketchId::mint("fcstd:design:sketch#Sketch")
        .expect("sketch identity");
    let mut text = "<Property><ConstraintList count=\"129\"><Constrain ElementIds=\"0\"/>".to_owned();
    for _ in 0..128 {
        text.push_str("<Constrain/>");
    }
    text.push_str("</ConstraintList></Property>");
    let xml = roxmltree::Document::parse(&text).expect("valid counted-list XML");
    let expected = format!("{} constraint 1: ElementIds and ElementPositions must both be present", property.id);
    // The counted-list owner must validate the whole container before the first
    // constraint. The core oracle then visits only the malformed first record.
    let cap = successful_work(|ctx| {
        let (storage, records);
        (records, storage) = super::super::direct_counted_records(
            ctx, &xml, "ConstraintList", "Constrain", &property.id,
        )?;
        let mut source = records.iter();
        let node = *ctx.next_charged(&mut source, "FreeCAD sketch constraints")?
            .expect("first constraint");
        assert!(ctx.xml_attribute(node, "Type", "FreeCAD design XML attribute")?.is_none());
        let (operand_storage, message);
        (message, operand_storage) = ctx.with_scoped_storage("fcstd constraint operand storage", || {
            assert_eq!(ctx.xml_attribute(node, "ElementIds", "FreeCAD design XML attribute")?, Some("0"));
            assert!(ctx.xml_attribute(node, "ElementPositions", "FreeCAD design XML attribute")?.is_none());
            ctx.format_retained(
                format_args!("ElementIds and ElementPositions must both be present"), "fcstd design diagnostic",
            )
        })?;
        assert_eq!(ctx.format_retained(
            format_args!("{} constraint 1: {message}", property.id), "fcstd design diagnostic",
        )?, expected);
        drop(message);
        drop(operand_storage);
        drop(records);
        drop(storage);
        Ok(())
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(super::super::parse_constraints(
        &ctx, &object, &[&property], &sketch, &[], Some((&property, &xml)),
    ), Err(CodecError::Malformed(message)) if message == expected));
    assert!(ctx.resource_refusal().is_none());
}
