use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn limited_context(arena: &DecodeArena, cap: u64) -> DecodeContext<'_> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = cap;
    DecodeContext::from_root_bytes(&[], arena, &policy)
        .unwrap()
        .0
}

#[test]
fn validation_native_arena_reload_refuses_collection_limit() {
    let result: Result<(), cadmpeg_core::CodecError> =
        Err(cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "load typed native record",
            |cap| {
                let board = crate::history_records::AsmBulletinBoard {
                    id: "f3d:native:bulletin#1".into(),
                    parent: "f3d:native:state#1".into(),
                    byte_offset: 0,
                    owner_ref: 0,
                    number: 0,
                    changes: Vec::new(),
                };
                let mut ir = cadmpeg_ir::CadIr::empty();
                ir.native
                    .namespace_mut("f3d")
                    .set_arena(
                        &cadmpeg_test_support::service_decode_context(),
                        "asm_bulletin_boards",
                        &[board],
                    )
                    .unwrap();
                let arena = DecodeArena::new();
                let ctx = limited_context(&arena, cap);
                let result = super::super::reload_native_arena::<
                    crate::history_records::AsmBulletinBoard,
                >(&ctx, &ir, "asm_bulletin_boards");

                result.map(|_| ())
            },
        ));
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "load typed native record")
    );
}

#[test]
fn validation_map_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D test map",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let error = ctx
                .collect_hash_map([("key", 1)], "index F3D test map")
                .unwrap_err();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D test map"
    ));
}

#[test]
fn validation_set_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D test set",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let error = ctx
                .collect_hash_set(["key"], "index F3D test set")
                .unwrap_err();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D test set"
    ));
}

#[test]
fn validation_design_header_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D design headers",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let ir = cadmpeg_ir::examples::unit_cube().unwrap();
            let native = crate::native::F3dNative {
                design_record_headers: vec![crate::records::decal::DesignRecordHeader {
                    id: "f3d:Design/BulkStream.dat:design-record-header#1".into(),
                    record_index: 1,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "123".to_owned(),
                    )
                    .unwrap(),
                    byte_offset: 0,
                }],
                ..crate::native::F3dNative::default()
            };
            let error = super::super::Ctx::new(&ir, &native, &ctx).err().unwrap();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D design headers"
    ));
}

#[test]
fn validation_typed_sketch_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D typed sketch records",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let error = ctx
                .collect_hash_set(
                    [("Design/BulkStream.dat", 1)],
                    "index F3D typed sketch records",
                )
                .unwrap_err();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D typed sketch records")
    );
}

#[test]
fn validation_sketch_operand_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D sketch operands",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let error = ctx
                .collect_hash_map(
                    [(("Design/BulkStream.dat", 1), 1)],
                    "index F3D sketch operands",
                )
                .unwrap_err();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch operands")
    );
}

#[test]
fn validation_sketch_relation_owner_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D sketch relation owners",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let mut owners = std::collections::HashMap::new();
            let error = ctx
                .insert_hash_map(
                    &mut owners,
                    ("Design/BulkStream.dat", 1),
                    2,
                    "index F3D sketch relation owners",
                )
                .unwrap_err();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch relation owners")
    );
}

#[test]
fn validation_sketch_owner_finding_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D sketch owner findings",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let mut findings = Vec::new();
            let error = super::super::emit_sketch_relation_finding(
                &ctx,
                &mut findings,
                "f3d:native:sketch#1",
                "conflicting owner",
            )
            .unwrap_err();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D sketch owner findings")
    );
}

#[test]
fn validation_sketch_owner_finding_id_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D sketch owner finding ID",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            super::super::emit_sketch_relation_finding(
                &ctx,
                &mut findings,
                "f3d:native:sketch#1",
                "conflicting owner",
            )
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    let error = super::super::emit_sketch_relation_finding(
        &ctx,
        &mut findings,
        "f3d:native:sketch#1",
        "conflicting owner",
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D sketch owner finding ID")
    );
}

#[test]
fn validation_profile_face_group_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D decoded profile face groups",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let error = ctx
                .collect_hash_set(
                    [("Design/BulkStream.dat", 1)],
                    "index F3D decoded profile face groups",
                )
                .unwrap_err();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D decoded profile face groups")
    );
}

#[test]
fn validation_face_group_member_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D face group members",
        |cap| {
            let arena = DecodeArena::new();
            let ctx = limited_context(&arena, cap);
            let error = ctx
                .collect_hash_set(
                    [("Design/BulkStream.dat", 1, 2)],
                    "index F3D face group members",
                )
                .unwrap_err();

            Err::<(), cadmpeg_core::CodecError>(error)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D face group members")
    );
}

#[test]
fn native_configuration_name_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let configuration = crate::test_support::with_decode_context(|ctx| {
        crate::records::configuration::DesignConfiguration::try_new_charged(
            ctx,
            "sample.dsgcfgrule".into(),
            crate::records::configuration::DesignConfigurationKind::Rule,
            Vec::new(),
            serde_json::Map::new(),
        )
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_configurations.push(configuration);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D configuration entry names",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
            let error = super::super::validate_configurations(&ctx, &mut Vec::new()).unwrap_err();
            Err::<(), cadmpeg_core::CodecError>(error)
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
    let error = super::super::validate_configurations(&ctx, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D configuration entry names")
    );
}

#[test]
fn native_duplicate_configuration_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let configuration = crate::test_support::with_decode_context(|ctx| {
        crate::records::configuration::DesignConfiguration::try_new_charged(
            ctx,
            "sample.dsgcfgrule".into(),
            crate::records::configuration::DesignConfigurationKind::Rule,
            Vec::new(),
            serde_json::Map::new(),
        )
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_configurations.push(configuration.clone());
    native.design_configurations.push(configuration);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D configuration entry ID",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
            super::super::validate_configurations(&ctx, &mut Vec::new())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
    let error = super::super::validate_configurations(&ctx, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D configuration entry ID")
    );
}

#[test]
fn native_duplicate_configuration_finding_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let configuration = crate::test_support::with_decode_context(|ctx| {
        crate::records::configuration::DesignConfiguration::try_new_charged(
            ctx,
            "sample.dsgcfgrule".into(),
            crate::records::configuration::DesignConfigurationKind::Rule,
            Vec::new(),
            serde_json::Map::new(),
        )
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_configurations.push(configuration.clone());
    native.design_configurations.push(configuration);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D native validation findings",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
            let error = super::super::validate_configurations(&ctx, &mut Vec::new()).unwrap_err();
            Err::<(), cadmpeg_core::CodecError>(error)
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let ctx = super::super::Ctx::new(&ir, &native, &decode).unwrap();
    let error = super::super::validate_configurations(&ctx, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

fn validation_parameter() -> crate::records::parameters::DesignParameter {
    use crate::records::identity::{Located, RecordedValue};
    use crate::records::parameters::{
        DesignParameter, DesignParameterDiscriminator, DesignParameterDraft, DesignParameterSource,
    };
    DesignParameter::try_from(DesignParameterDraft::<String> {
        id: "f3d:native:parameter#0".into(),
        byte_offset: 100,
        class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 1,
        source_ordinal: 0,
        source: DesignParameterSource::User {
            family_discriminator: Located {
                value: DesignParameterDiscriminator::Code0,
                offset: 122,
            },
        },
        expression: "-1 mm".into(),
        expression_offset: 140,
        source_kind_offset: 160,
        unit: Some(RecordedValue {
            value: "mm".into(),
            offset: 170,
        }),
        name: "Width".into(),
        name_offset: 180,
        evaluated_value: -1.0,
        evaluated_value_offset: 190,
    })
    .unwrap()
}

#[test]
fn native_parameter_validator_index_refuses_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_parameters.push(validation_parameter());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "index F3D validation parameters",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
                ctx.decode = &decode;
                let error = super::super::validate_parameters(&ctx, &mut Vec::new()).unwrap_err();
                Err::<(), cadmpeg_core::CodecError>(error)
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_parameters(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D validation parameters")
        );
    })
}

#[test]
fn native_parameter_validator_finding_refuses_collection_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let parameter = validation_parameter();
        let mut native = crate::native::F3dNative::default();
        native.design_parameters.push(parameter.clone());
        native.design_parameters.push(parameter);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "collect F3D native validation findings",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
                ctx.decode = &decode;
                let error = super::super::validate_parameters(&ctx, &mut Vec::new()).unwrap_err();
                Err::<(), cadmpeg_core::CodecError>(error)
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_parameters(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
        );
    })
}

#[test]
fn native_parameter_validator_entity_refuses_retained_limit() {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let parameter = validation_parameter();
        let mut native = crate::native::F3dNative::default();
        native.design_parameters.push(parameter.clone());
        native.design_parameters.push(parameter);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "retain F3D parameter finding entity",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
                ctx.decode = &decode;
                super::super::validate_parameters(&ctx, &mut Vec::new())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_parameters(&ctx, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D parameter finding entity")
        );
    })
}

#[test]
fn validation_native_arena_reload_releases_scoped_storage() {
    let board = crate::history_records::AsmBulletinBoard {
        id: "f3d:native:bulletin#1".into(),
        parent: "f3d:native:state#1".into(),
        byte_offset: 0,
        owner_ref: 0,
        number: 0,
        changes: Vec::new(),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native
        .namespace_mut("f3d")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "asm_bulletin_boards",
            &[board],
        )
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // Reload admits the native object backing nodes within scoped storage.
    policy.limits.max_materialized_bytes = 16384;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (storage, records) = super::super::reload_native_arena::<
        crate::history_records::AsmBulletinBoard,
    >(&ctx, &ir, "asm_bulletin_boards")
    .unwrap();
    assert_eq!(records[0].id, "f3d:native:bulletin#1");
    drop(records);
    drop(storage);
    let full = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse validation storage",
        )
        .unwrap();
    drop(full);
}

#[test]
fn recipe_reference_comparison_preserves_work_refusal() {
    let actual = [super::recipe_reference()];
    let expected = actual.clone();
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "compare F3D recipe references",
        0,
        |ctx| super::super::recipe_reference_frames_match(ctx, &actual, &expected, false),
    );
}

#[test]
fn recipe_reference_token_comparison_preserves_work_refusal() {
    let actual = [super::recipe_reference()];
    let expected = actual.clone();
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "compare F3D recipe reference token",
        0,
        |ctx| super::super::recipe_reference_frames_match(ctx, &actual, &expected, true),
    );
}

#[test]
fn face_reference_comparison_preserves_work_refusal() {
    let actual = [cadmpeg_ir::ids::FaceId::mint("f3d:test:face#1").unwrap()];
    let expected = [&actual[0]];
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "compare F3D face identity",
        0,
        |ctx| super::super::face_ids_match_refs(ctx, &actual, &expected),
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(!super::super::face_ids_match_refs(ctx, &actual, &[]).unwrap());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn validation_stream_entry_matches_escape_and_xref_forms() {
    crate::test_support::with_decode_context(|decode| {
        for entry in [
            "Design/BulkStream.dat",
            "a:b#c%d e",
            "é\t雪",
            "a\u{2003}b",
            "",
            "a/b",
        ] {
            let escaped =
                crate::ids::native_scope(decode, entry, "build test native scope").unwrap();
            let xref = format!("f3d:xref/child/{entry}");
            for stream in [
                escaped.clone(),
                xref.clone(),
                format!("{escaped}extra"),
                format!("{xref}extra"),
                "foreign:scope".into(),
            ] {
                assert_eq!(
                    super::super::design_stream_contains_entry(decode, &stream, entry).unwrap(),
                    crate::ids::native_scope_matches(&stream, entry),
                    "{stream:?} / {entry:?}"
                );
            }
        }
    });
}

#[test]
fn validation_stream_entry_comparison_refuses_before_work() {
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "match F3D design stream entry",
        0,
        |decode| super::super::design_stream_contains_entry(decode, "f3d:a%3Ab", "a:b"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "match F3D design stream entry")
    );
}

#[test]
fn finalized_reference_validation_stops_at_first_mismatch() {
    let actual = super::recipe_reference();
    let mut expected = actual.clone();
    expected.selector += 1;
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D actual recipe reference frames",
        0,
        |decode| {
            super::super::recipe_reference_frames_match(
                decode,
                &[actual.clone()],
                &[expected.clone()],
                true,
            )
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit.used + limit.additional;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let actual = vec![actual; 4096];
    let expected = vec![expected; 4096];
    assert!(
        !super::super::recipe_reference_frames_match(&decode, &actual, &expected, true).unwrap()
    );
}

#[test]
fn scoped_validation_result_releases_storage_and_spares_retained_budget() {
    crate::test_support::with_decode_context(|service| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = super::construction_identity_limits::native(true);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 1 << 20;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service).unwrap();
        ctx.decode = &decode;
        let mut findings = Vec::new();
        let (groups, storage) =
            super::super::validate_construction_operand_identities(&ctx, &mut findings).unwrap();
        assert!(groups.contains(&("f3d:Design/BulkStream.dat", 100)));
        assert!(findings.is_empty());
        drop((groups, storage));
        let _reservation = decode
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released validator scratch",
            )
            .unwrap();
    });
}

#[test]
fn loft_single_pass_preserves_role_grammar() {
    use crate::records::feature::extrude::DesignExtrudeOperation as Operation;
    use crate::records::topology::extrude_selection::DesignOperandRole as Role;
    crate::test_support::with_decode_context(|decode| {
        let cases = [
            (vec![(Role::PROFILE, 2), (Role::ROLE_0X43, 2)], true),
            (
                vec![(Role::PROFILE, 2), (Role::PROFILE, 2), (Role::ROLE_0X5, 2)],
                true,
            ),
            (
                vec![(Role::PROFILE, 2), (Role::PROFILE, 2), (Role::ROLE_0X7, 2)],
                true,
            ),
            (
                vec![
                    (Role::PROFILE, 2),
                    (Role::PROFILE, 2),
                    (Role::ROLE_0X5, 2),
                    (Role::ROLE_0X7, 2),
                ],
                false,
            ),
            (
                vec![
                    (Role::PROFILE, 2),
                    (Role::PROFILE, 2),
                    (Role::ROLE_0X7, 2),
                    (Role::ROLE_0X7, 2),
                ],
                false,
            ),
            (
                vec![(Role::PROFILE, 2), (Role::PROFILE, 2), (Role::ROLE_0X10, 2)],
                false,
            ),
            (
                vec![
                    (Role::ROLE_0X5, 1),
                    (Role::ROLE_0X43, 2),
                    (Role::ROLE_0X5, 2),
                ],
                true,
            ),
            (
                vec![
                    (Role::ROLE_0X5, 2),
                    (Role::ROLE_0X43, 2),
                    (Role::ROLE_0X5, 1),
                ],
                true,
            ),
            (
                vec![
                    (Role::ROLE_0X5, 2),
                    (Role::ROLE_0X5, 1),
                    (Role::ROLE_0X43, 2),
                ],
                false,
            ),
            (
                vec![
                    (Role::ROLE_0X5, 1),
                    (Role::ROLE_0X43, 2),
                    (Role::ROLE_0X5, 1),
                ],
                false,
            ),
            (vec![(Role::ROLE_0X5, 2), (Role::ROLE_0X5, 2)], true),
            (vec![(Role::PROFILE, 2)], false),
        ];
        for (roles, valid) in cases {
            assert_eq!(
                super::super::loft_operand_roles_are_valid(decode, Operation::NewBody, &roles)
                    .unwrap(),
                valid,
                "{roles:?}"
            );
            assert!(
                !super::super::loft_operand_roles_are_valid(decode, Operation::Join, &roles)
                    .unwrap()
            );
            let mut with_body = roles;
            with_body.insert(0, (Role::BODIES_A, 1));
            let has_two_sections = with_body
                .iter()
                .filter(|(role, _)| matches!(*role, Role::PROFILE | Role::ROLE_0X43))
                .count()
                >= 2;
            assert_eq!(
                super::super::loft_operand_roles_are_valid(decode, Operation::Join, &with_body)
                    .unwrap(),
                valid && has_two_sections,
                "{with_body:?}"
            );
            assert!(!super::super::loft_operand_roles_are_valid(
                decode,
                Operation::NewBody,
                &with_body
            )
            .unwrap());
        }
    });
}

#[test]
fn bounded_guid_keys_preserve_ascii_case_and_length() {
    let values = [
        "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee_",
        "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee__",
        "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeef",
    ];
    for left in values {
        assert!(crate::records::mesh::DesignRelaxedGuidText::try_from(left.to_owned()).is_ok());
        for right in values {
            assert_eq!(
                super::super::folded_guid(left) == super::super::folded_guid(right),
                left.eq_ignore_ascii_case(right),
            );
        }
    }
    assert!(super::super::folded_guid(&"a".repeat(39)).is_none());
}
