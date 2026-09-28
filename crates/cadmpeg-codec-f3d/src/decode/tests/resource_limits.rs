use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn context<'a>(arena: &'a DecodeArena, max_collection_items: u64) -> DecodeContext<'a> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    DecodeContext::from_root_bytes(&[], arena, &policy).unwrap().0
}

macro_rules! append_refuses_collection_limit {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let ctx = context(&arena, 1);
            let mut target = vec![1u32];
            let error = super::super::append_decode_items(
                &ctx,
                &mut target,
                vec![2u32, 3u32],
                $operation,
            )
            .unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
            assert_eq!(target, [1]);
        }
    };
}

append_refuses_collection_limit!(subd_loss_append_refuses_collection_limit, "append F3D T-spline losses");
append_refuses_collection_limit!(dimension_constraint_append_refuses_collection_limit, "append F3D dimension constraints");
append_refuses_collection_limit!(spatial_dimension_constraint_append_refuses_collection_limit, "append F3D spatial dimension constraints");
append_refuses_collection_limit!(material_note_append_refuses_collection_limit, "append F3D material notes");
append_refuses_collection_limit!(local_component_append_refuses_collection_limit, "append F3D local components");
append_refuses_collection_limit!(local_occurrence_append_refuses_collection_limit, "append F3D local occurrences");
append_refuses_collection_limit!(unresolved_occurrence_append_refuses_collection_limit, "append F3D unresolved occurrences");

#[test]
fn model_brep_candidate_index_refuses_collection_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let blob_names: Vec<String> = crate::container::design_breps(&scan)
        .map(|brep| brep.name.rsplit('/').next().unwrap().to_owned())
        .collect();
    assert!(!blob_names.is_empty());
    let limited = context(&arena, 0);
    let error = super::super::model_brep_candidates(&limited, &scan, &blob_names).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D model BREP candidates"));
}

#[test]
fn undecoded_brep_loss_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = super::super::push_decode_loss(
        &ctx,
        &mut report,
        crate::loss::F3dLossCode::BrepBlobUndecoded,
        format_args!("1 Design-referenced BREP blob(s) could not be decoded."),
        "collect F3D undecoded BREP loss",
        "retain F3D undecoded BREP loss",
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D undecoded BREP loss"));
}

#[test]
fn primary_brep_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::copy_decode_string(
        &ctx,
        "Breps.BlobParts/BREP0.smb",
        "retain F3D primary BREP name",
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D primary BREP name"));
}

#[test]
fn asm_history_collection_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let mut histories = Vec::new();
    let error = super::super::push_decode_item(
        &ctx,
        &mut histories,
        1_u32,
        "collect F3D ASM histories",
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D ASM histories"));
    assert!(histories.is_empty());
}

#[test]
fn source_image_copy_refuses_retained_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(bytes.len()).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::preserve_source_image(&limited, &scan).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D source image"));
}

#[test]
fn unique_asset_append_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let asset = cadmpeg_ir::assets::Asset::try_new(
        cadmpeg_ir::assets::AssetId::mint("f3d:model:asset#one").unwrap(),
        Some("one.png".into()),
        Some("image/png".into()),
        cadmpeg_ir::assets::AssetContent::Embedded {
            data: cadmpeg_ir::assets::AssetData::new(vec![1]).unwrap(),
        },
        None,
    )
    .unwrap();
    let mut assets = Vec::new();
    let error = super::super::extend_unique_assets(&ctx, &mut assets, vec![asset]).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "append F3D unique assets"));
    assert!(assets.is_empty());
}

#[test]
fn archive_member_dialect_clone_refuses_collection_limit() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = DecodeArena::new();
    let (scan_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_ctx, root).unwrap();
    let layers = cadmpeg_core::dialect::DialectLayers::of(scan.kind.dialect().clone());
    let limited = context(&arena, 0);
    let error = super::super::decode_archive_member(&limited, &scan, &layers)
        .err().expect("dialect copy must refuse");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "clone F3Z member dialect layers"));
}

#[test]
fn text_brep_fact_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::copy_decode_string(
        &ctx,
        "Breps.BlobParts/BREP0.sat",
        "retain F3D text B-rep fact name",
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D text B-rep fact name"));
}

#[test]
fn text_brep_loss_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::format_decode_string(
        &ctx,
        "report F3D text geometry loss",
        format_args!("text carrier {}", "BREP0.sat"),
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "report F3D text geometry loss"));
}

#[test]
fn joined_text_brep_names_refuse_retained_limit() {
    let bytes = crate::test_support::assembly_test::f3d_with_text_brep(&[
        "FusionAssetName[Active]/Breps.BlobParts/BREP0.sat",
    ]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut limited_policy = DecodePolicy::service();
    limited_policy.limits.max_retained_bytes = 0;
    let (limited, _) =
        DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
    let error = super::super::join_text_brep_names(&limited, &scan).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "join F3D text B-rep names"));
}

fn dimension_native() -> crate::native::F3dNative {
    use crate::records::parameters::{
        DesignCompanionPayload, DesignParameter, DesignParameterDraft, DesignParameterOwner,
        DesignParameterOwnerWire, DesignParameterSource,
    };
    use crate::records::references::DesignClassTag;

    let stream = "f3d:test/BulkStream.dat";
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: format!("{stream}:design-parameter#28"),
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 28,
        source_ordinal: 0,
        source: DesignParameterSource::new(
            "Linear Dimension-2".into(),
            Some(29),
            Some(crate::records::identity::Located {
                value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                offset: 22,
            }),
        )
        .unwrap(),
        expression: "5 mm".into(),
        expression_offset: 40,
        source_kind_offset: 60,
        unit: Some(crate::records::identity::RecordedValue {
            value: "mm".into(),
            offset: 90,
        }),
        name: "d1".into(),
        name_offset: 100,
        evaluated_value: 0.5,
        evaluated_value_offset: 110,
    })
    .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("{stream}:design-parameter-owner#29"),
        byte_offset: 120,
        frame_length: 104,
        class_tag: DesignClassTag::try_from("292".to_owned()).unwrap(),
        record_index: 29,
        scope_record_index: 1,
        local_ordinal: 0,
        evaluated_value: 0.5,
        evaluated_value_offset: 160,
        parameter_record_index: 28,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 30,
    })
    .unwrap();
    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        format!("{stream}:design-parameter-companion#30"),
        220,
        DesignClassTag::try_from("408".to_owned()).unwrap(),
        30,
        29,
        std::num::NonZeroU64::new(1).unwrap(),
        262,
    )
    .bound(DesignCompanionPayload::new(278, 100, Vec::new()));
    crate::native::F3dNative {
        design_parameters: vec![parameter],
        design_parameter_owners: vec![owner],
        design_parameter_companions: vec![companion],
        ..crate::native::F3dNative::default()
    }
}

#[test]
fn dimension_owner_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 1);
    let native = dimension_native();
    let ir = cadmpeg_ir::document::CadIr::empty();
    let error = super::super::unresolved_dimension_companion_count(&ctx, &native, &ir).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D dimension owners"
    ));
}

#[test]
fn unresolved_dimension_loss_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 2);
    let native = dimension_native();
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::full(true),
    );
    let error = super::super::report_unresolved_dimension_companions(
        &ctx,
        &mut report,
        &native,
        &ir,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "report unresolved F3D dimensions"
    ));
}

#[test]
fn typed_dimension_index_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let ctx = context(&arena, 0);
    let native = crate::native::F3dNative {
        design_dimension_recipe_records: vec![
            crate::records::dimensions::DesignDimensionRecipeRecord {
                id: "f3d:test:design-dimension-recipe-record#1".into(),
                companion_record_index: 1,
                recipe_ordinal: 0,
                recipe_id: "f3d:test:construction-recipe#1".into(),
                recipe_kind: crate::records::recipes::ConstructionRecipeKind::Edge,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned())
                    .unwrap(),
                record_index: 1,
                frame_length: 100,
                prefix_offset: 20,
                prefix_bytes: Vec::new(),
                references: Vec::new(),
                program_offset: 40,
                program: vec![-1],
                matching_edge_operand_ids: Vec::new(),
            },
        ],
        ..crate::native::F3dNative::default()
    };
    let ir = cadmpeg_ir::document::CadIr::empty();
    let error = super::super::unresolved_dimension_companion_count(&ctx, &native, &ir).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3D typed dimension companions"
    ));
}
