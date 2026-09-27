use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn context<'a>(arena: &'a DecodeArena, max_collection_items: u64) -> DecodeContext<'a> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    DecodeContext::from_root_bytes(&[], arena, &policy).unwrap().0
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
