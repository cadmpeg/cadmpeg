use super::{
    DesignEntityHeader, DesignEntityHeaderWire, DesignFeatureTimeline, DesignFeatureTimelineWire,
    SegmentType, SegmentTypeWire, ENTITY_HEADER_CLONE_COUNT, SEGMENT_TYPE_CLONE_COUNT,
    TIMELINE_CLONE_COUNT,
};

fn timeline() -> DesignFeatureTimeline {
    serde_json::from_str(
        r#"{"id":"f3d:Design/BulkStream.dat:design-feature-timeline#200","class_tag":"256","record_index":35,"source_ordinal":0,"context_record_index":17,"item_record_indices":[101,102]}"#,
    )
    .unwrap()
}

fn header() -> DesignEntityHeader {
    serde_json::from_str(
        r#"{"id":"f3d:Design/BulkStream.dat:design-entity-header#0","entity_id":"0_1","class_tag":"256","module":"MSketch","record_reference":33,"reference_indices":[34,35],"member_indices":[11]}"#,
    )
    .unwrap()
}

#[test]
fn feature_timeline_borrowed_wire_matches_owned_wire_bytes() {
    let record = timeline();
    let owned = DesignFeatureTimelineWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn feature_timeline_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = timeline();
    let arena_name = "design_feature_timelines";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    TIMELINE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    TIMELINE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

#[test]
fn entity_header_borrowed_wire_matches_owned_wire_bytes() {
    for wire in [
        r#"{"id":"header","entity_id":"0_1","class_tag":"256"}"#,
        r#"{"id":"header","entity_id":"0_1","class_tag":"256","module":"MSketch","record_reference":33,"reference_indices":[34,35],"member_indices":[11]}"#,
    ] {
        let record: DesignEntityHeader = serde_json::from_str(wire).unwrap();
        let owned = DesignEntityHeaderWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn entity_header_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = header();
    let arena_name = "design_entity_headers";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    ENTITY_HEADER_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    ENTITY_HEADER_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

fn segment(base_fields: &str) -> SegmentType {
    let wire = format!(
        r#"{{"id":"f3d:native/MetaStream.dat:design-type#0","type_guid":"11111111-2222-3333-4444-555555555555","class_tag":"256"{base_fields},"version":1,"module":"Fusion","entity_ids":[10,11]}}"#
    );
    serde_json::from_str(&wire).unwrap()
}

#[test]
fn segment_type_borrowed_wire_matches_owned_wire_bytes() {
    for base_fields in [
        "",
        r#", "base_type_guid":"""#,
        r#", "base_type_guid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee""#,
    ] {
        let entry = segment(base_fields);
        let owned = SegmentTypeWire::from(entry.clone());
        assert_eq!(
            serde_json::to_vec(&entry).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn segment_type_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let entry = segment(r#", "base_type_guid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee""#);
    let arena_name = "design_types";
    let needed = serde_json::to_vec(&entry).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    SEGMENT_TYPE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&entry))
        .unwrap_err();
    SEGMENT_TYPE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&entry))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()[arena_name][0]).unwrap(),
        serde_json::to_value(&entry).unwrap()
    );
}

#[test]
fn segment_type_rejects_unbound_native_identity() {
    let valid = segment("");
    let mut source = (*valid).clone();
    source.byte_offset = Some(0);
    assert!(SegmentType::try_new(
        "f3d:native/MetaStream.dat:design-type#0".into(),
        source.clone()
    )
    .is_ok());
    assert!(
        SegmentType::try_new("f3d:native/MetaStream.dat:design-type#1".into(), source).is_err()
    );
    let mut independent = SegmentTypeWire::from(valid.clone());
    independent.id = "f3d:native/MetaStream.dat:design-type#1".into();
    assert_eq!(
        SegmentType::try_from(independent).unwrap().id(),
        "f3d:native/MetaStream.dat:design-type#1"
    );
    for id in [
        "",
        "f3d:native/MetaStream.dat:other#0",
        "f3d:native/BulkStream.dat:design-type#0",
    ] {
        let mut wire = SegmentTypeWire::from(valid.clone());
        wire.id = id.into();
        assert!(SegmentType::try_from(wire).is_err());
        let mut wire = serde_json::to_value(&valid).unwrap();
        wire["id"] = serde_json::json!(id);
        assert!(serde_json::from_value::<SegmentType>(wire).is_err());
    }
}

#[test]
fn semantic_registry_keeps_dynamic_discriminator_without_source_positions() {
    let value = segment("");
    assert_eq!(value.class_tag.as_str(), "256");
    assert!(value.byte_offset.is_none());
    assert!(value.type_guid_offset.is_none());
    assert!(value.version_offset.is_none());
    assert!(value.entities.located_rows().is_none());
    let mut wire = serde_json::to_value(value).unwrap();
    wire["byte_offset"] = serde_json::json!(0);
    assert!(serde_json::from_value::<SegmentType>(wire).is_err());
}
#[test]
fn semantic_timeline_keeps_order_and_rejects_duplicate_items() {
    let value = timeline();
    assert!(value.frame().byte_offset().is_none());
    assert!(value
        .frame()
        .items()
        .iter()
        .all(|item| item.offset.is_none()));
    assert_eq!(
        value
            .frame()
            .items()
            .iter()
            .map(|item| item.value)
            .collect::<Vec<_>>(),
        [101, 102]
    );
    let mut wire = serde_json::to_value(value).unwrap();
    wire["item_record_indices"] = serde_json::json!([101, 101]);
    assert!(serde_json::from_value::<DesignFeatureTimeline>(wire).is_err());
}
