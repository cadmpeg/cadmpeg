use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn limited_context<'a>(arena: &'a DecodeArena) -> DecodeContext<'a> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    DecodeContext::from_root_bytes(&[], arena, &policy).unwrap().0
}

#[test]
fn validation_map_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let error = super::super::collect_index(Some(&ctx), [("key", 1)], "index F3D test map")
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D test map"
    ));
}

#[test]
fn validation_set_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let error = super::super::collect_index_set(Some(&ctx), ["key"], "index F3D test set")
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D test set"
    ));
}

#[test]
fn validation_design_header_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = limited_context(&arena);
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let native = crate::native::F3dNative {
        design_record_headers: vec![crate::records::decal::DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#1".into(),
            record_index: 1,
            class_tag: crate::records::references::DesignClassTag::try_from("123".to_owned())
                .unwrap(),
            byte_offset: 0,
        }],
        ..crate::native::F3dNative::default()
    };
    let error = super::super::Ctx::new(&ir, &native, Some(&ctx))
        .err()
        .unwrap();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D design headers"
    ));
}
