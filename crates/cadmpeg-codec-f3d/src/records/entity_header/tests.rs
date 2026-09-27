use super::{SegmentType, SegmentTypeWire, SEGMENT_TYPE_CLONE_COUNT};

fn segment(base_fields: &str) -> SegmentType {
    let wire = format!(
        r#"{{"id":"f3d:native:design-type#0","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4{base_fields},"version":1,"version_offset":80,"module":"Fusion","entity_ids":[10,11],"entity_id_offsets":[90,98]}}"#
    );
    serde_json::from_str(&wire).unwrap()
}

#[test]
fn segment_type_borrowed_wire_matches_owned_wire_bytes() {
    for base_fields in [
        "",
        r#", "base_type_guid":"","base_type_guid_offset":44"#,
        r#", "base_type_guid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","base_type_guid_offset":44"#,
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

    let entry = segment(
        r#", "base_type_guid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","base_type_guid_offset":44"#,
    );
    let arena_name = "design_types";
    let needed = serde_json::to_vec(&entry).unwrap().len() + arena_name.len();
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
