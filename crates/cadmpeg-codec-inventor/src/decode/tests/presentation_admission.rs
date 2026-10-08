// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;

use crate::decode::presentation_native_projection;
use crate::pmdc::{PmDcPairedReferenceList, PmDcReference};
use crate::presentation::{
    PmAppDefaultStyle, PmAppRenderingStyle, PmGraphicsFace, PmGraphicsPrimaryColorStyle,
    PmGraphicsStyleCollection, PresentationInventory, RenderingStyleExtension,
};
use crate::record_identity::Located;

#[test]
fn presentation_default_native_record_refuses_before_id_creation() {
    let bytes = [];
    let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
    let style = Located::new(
        PmAppDefaultStyle {
            segment_version_major: 0,
            header_value: 0,
            header_id: 0,
            material_reference: 0,
            rendering_style_reference: 0,
            related_references: [0; 7],
            state: 0,
            terminal_reference: 0,
            suffix: View::over_retained(&bytes),
        },
        crate::record_identity::RecordTypeId::from_bytes(
            &cadmpeg_test_support::service_decode_context(),
            [0; 16],
            "retain Inventor fixture type id",
        )
        .expect("fixture type id"),
        token
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        1,
    );
    let mut inventory = PresentationInventory {
        default_styles: vec![style],
        rendering_styles: Vec::new(),
        graphics_faces: Vec::new(),
        graphics_style_collections: Vec::new(),
        graphics_primary_color_styles: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("inventor:presentation:default-style#segment-1".len() - 1)
            .expect("id length fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        presentation_native_projection::project(&ctx, &mut inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor default style id"
    ));
    policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        presentation_native_projection::project(&ctx, &mut inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor native default style"
    ));
    policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        presentation_native_projection::project(&ctx, &mut inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor native default style"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    presentation_native_projection::project(&ctx, &mut inventory).expect("admitted default style");
}

#[test]
fn presentation_projection_stops_before_unvisited_default_style_on_entity_refusal() {
    let suffix = [1_u8];
    let fixture_context = cadmpeg_test_support::service_decode_context();
    let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
    let style = |record_ordinal| {
        Located::new(
            PmAppDefaultStyle {
                segment_version_major: 0,
                header_value: 0,
                header_id: 0,
                material_reference: 0,
                rendering_style_reference: 0,
                related_references: [0; 7],
                state: 0,
                terminal_reference: 0,
                suffix: View::over_retained(&suffix),
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &fixture_context,
                [0; 16],
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            token
                .try_clone_for_decode(&fixture_context, "Inventor located fixture token")
                .expect("service fixture token"),
            record_ordinal,
        )
    };
    let mut inventory = PresentationInventory {
        default_styles: vec![style(1), style(2)],
        rendering_styles: Vec::new(),
        graphics_faces: Vec::new(),
        graphics_style_collections: Vec::new(),
        graphics_primary_color_styles: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let error = match presentation_native_projection::project(&ctx, &mut inventory) {
        Ok(_) => panic!("first default style exceeds the entity cap"),
        Err(error) => error,
    };
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("default-style entity admission must refuse");
    };
    assert_eq!(refusal.dimension, ResourceDimension::Entities);
    assert_eq!(refusal.operation, "admit Inventor native default style");
    assert_eq!(refusal.limit, 0);
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(limit)) if limit == refusal
    ));
}

#[test]
fn presentation_other_native_records_refuse_before_ids_text_and_reference_copies() {
    let suffix = [1_u8];
    let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
    let reference = PmDcReference::new(1, false).expect("test reference index fits 31 bits");
    let mut inventory = PresentationInventory {
        default_styles: Vec::new(),
        rendering_styles: vec![Located::new(
            PmAppRenderingStyle {
                segment_version_major: 17,
                header_value: 0,
                header_id: 0,
                state: 0,
                flags: 0,
                values: [0; 2],
                default_state: 0,
                value: 0,
                name_reference: 0,
                name: "name".into(),
                comment: String::new(),
                long_name: "long".into(),
                extension: Some(RenderingStyleExtension {
                    style_state: 0,
                    style_label: "label".into(),
                    asset_guid: "asset".into(),
                    material_id: "material".into(),
                    asset_library_id: "library".into(),
                    style_values: [0; 2],
                    guid: "guid".into(),
                }),
                suffix: View::over_retained(&suffix),
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &cadmpeg_test_support::service_decode_context(),
                [0; 16],
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            token
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        )],
        graphics_faces: vec![Located::new(
            PmGraphicsFace {
                segment_version_major: 0,
                header_value: 0,
                header_id: 0,
                flags: 0,
                styles: reference,
                surface: reference,
                parent: reference,
                state: 0,
                edge_references: PmDcPairedReferenceList::new(Some([0; 2]), vec![reference])
                    .expect("paired references"),
                visibility_state: 0,
                bounds: [cadmpeg_ir::scalar::FiniteReal::ZERO; 6],
                key: 0,
                values: [0; 2],
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &cadmpeg_test_support::service_decode_context(),
                [0; 16],
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            token
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        )],
        graphics_style_collections: vec![Located::new(
            PmGraphicsStyleCollection {
                segment_version_major: 0,
                style_references: PmDcPairedReferenceList::new(Some([0; 2]), vec![reference])
                    .expect("paired references"),
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &cadmpeg_test_support::service_decode_context(),
                [0; 16],
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            token
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        )],
        graphics_primary_color_styles: vec![Located::new(
            PmGraphicsPrimaryColorStyle {
                segment_version_major: 0,
                header_value: 0,
                controls: [0; 7],
                color_header: [0; 2],
                colors: [[cadmpeg_ir::scalar::FiniteBinary32::ZERO; 4]; 4],
                color_tail: [0; 2],
                state: 0,
                values: [0; 2],
                terminal_state: 0,
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &cadmpeg_test_support::service_decode_context(),
                [0; 16],
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            token
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        )],
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut cap = 0;
    let mut operations = Vec::new();
    let mut admitted = false;
    for _ in 0..128 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        match presentation_native_projection::project(&ctx, &mut inventory) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert!(limit.used + limit.additional > cap);
                operations.push(limit.operation);
                cap = limit.used + limit.additional;
            }
            Ok(_) => {
                admitted = true;
                break;
            }
            Err(error) => panic!("unexpected projection error: {error}"),
        }
    }
    assert!(admitted, "presentation projection did not reach success");
    for operation in [
        "retain Inventor rendering style id",
        "retain Inventor rendering style text",
        "retain Inventor rendering extension text",
        "retain Inventor rendering style suffix digest",
        "retain Inventor graphics face id",
        "retain Inventor graphics style collection id",
        "retain Inventor primary color style id",
    ] {
        assert!(
            operations.contains(&operation),
            "no refusal for {operation}"
        );
    }
    let mut entity_cap = 0;
    let mut entity_operations = Vec::new();
    for _ in 0..4 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = entity_cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let Err(error) = presentation_native_projection::project(&ctx, &mut inventory) else {
            panic!("native presentation record needs entity admission");
        };
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected entity refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::Entities);
        entity_operations.push(limit.operation);
        entity_cap = limit.used + limit.additional;
    }
    for operation in [
        "admit Inventor native rendering style",
        "admit Inventor native graphics face",
        "admit Inventor native graphics style collection",
        "admit Inventor native primary color style",
    ] {
        assert!(
            entity_operations.contains(&operation),
            "no refusal for {operation}"
        );
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        presentation_native_projection::project(&ctx, &mut inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "copy Inventor graphics face edge references"
    ));
    inventory.graphics_faces.clear();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        presentation_native_projection::project(&ctx, &mut inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "copy Inventor graphics style references"
    ));
}

#[test]
fn rendering_conversion_issue_refuses_before_failure_record_creation() {
    let bytes = [];
    let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
    let mut inventory = PresentationInventory {
        default_styles: Vec::new(),
        rendering_styles: vec![Located::new(
            PmAppRenderingStyle {
                segment_version_major: 17,
                header_value: 0,
                header_id: 0,
                state: 0,
                flags: 0,
                values: [0; 2],
                default_state: 0,
                value: 0,
                name_reference: 0,
                name: String::new(),
                comment: String::new(),
                long_name: String::new(),
                extension: None,
                suffix: View::over_retained(&bytes),
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &cadmpeg_test_support::service_decode_context(),
                [0; 16],
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            token
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        )],
        graphics_faces: Vec::new(),
        graphics_style_collections: Vec::new(),
        graphics_primary_color_styles: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let issue = "rendering style extension disagrees with segment_version_major";
    let token_len = "segment".len();
    let id_len = "inventor:presentation:rendering-style#segment-1".len();
    let suffix_digest_bytes = cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes)
        .as_str()
        .len();
    let issue_vector_bytes = 4 * std::mem::size_of::<crate::record_issue::RecordIssue>();
    let issue_prefix =
        u64::try_from(issue_vector_bytes + token_len).expect("issue prefix fits");
    let issue_detail_bytes = u64::try_from(issue.len()).expect("issue detail fits");
    let mut policy = DecodePolicy::service();
    // The rejected candidate retains only its issue: four initial slots, token, and detail.
    let retained_needed = issue_vector_bytes + token_len + issue.len();
    let retained_with_candidate = id_len + token_len + suffix_digest_bytes + retained_needed;
    policy.limits.max_retained_bytes =
        u64::try_from(retained_needed - 1).expect("issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        presentation_native_projection::project(&ctx, &mut inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor rendering issue detail"
                && limit.used == issue_prefix
                && limit.additional == issue_detail_bytes
    ));
    assert!(inventory.issues.is_empty());
    policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        presentation_native_projection::project(&ctx, &mut inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor rendering conversion issue"
    ));
    policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        presentation_native_projection::project(&ctx, &mut inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor rendering conversion issue"
    ));
    inventory.issues = Vec::new();
    policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(retained_needed).expect("full issue budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("full context");
    let projection = presentation_native_projection::project(&ctx, &mut inventory)
        .expect("admitted rendering issue");
    assert!(projection.rendering_styles.is_empty());
    assert_eq!(inventory.issues.len(), 1);
    assert_eq!(
        inventory.issues[0].family,
        crate::record_issue::RecordIssueFamily::Presentation
    );
    assert_eq!(inventory.issues[0].segment_token, token);
    assert_eq!(inventory.issues[0].record_ordinal, 1);
    assert_eq!(inventory.issues[0].detail, issue);
    assert!(ctx.finish_session().is_ok());

    inventory.issues = Vec::new();
    let mut candidate_cap_policy = DecodePolicy::service();
    candidate_cap_policy.limits.max_retained_bytes =
        u64::try_from(retained_with_candidate - 1).expect("candidate-sized budget fits");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &candidate_cap_policy)
        .expect("candidate-sized cap context");
    let projection = presentation_native_projection::project(&ctx, &mut inventory)
        .expect("issue fits when rejected candidates skip id and digest retention");
    assert!(projection.rendering_styles.is_empty());
    assert_eq!(inventory.issues.len(), 1);
    assert_eq!(
        inventory.issues[0].family,
        crate::record_issue::RecordIssueFamily::Presentation
    );
    assert_eq!(inventory.issues[0].segment_token, token);
    assert_eq!(inventory.issues[0].record_ordinal, 1);
    assert_eq!(inventory.issues[0].detail, issue);
    assert!(ctx.finish_session().is_ok());

    for operation in [
        "retain Inventor rendering style id",
        "retain Inventor rendering style suffix digest",
    ] {
        inventory.issues = Vec::new();
        let mut probe_policy = DecodePolicy::service();
        probe_policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &probe_policy).expect("probe context");
        let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let projection = presentation_native_projection::project(&ctx, &mut inventory)
            .expect("a rejected candidate stores only its issue");
        assert!(projection.rendering_styles.is_empty());
        assert_eq!(inventory.issues.len(), 1);
        let CodecError::ResourceLimit(refusal) = ctx
            .charge_retained(u64::MAX, operation)
            .expect_err("the armed probe has not seen candidate admission")
        else {
            panic!("candidate admission probe must refuse");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, operation);
        assert_eq!(refusal.limit, u64::try_from(retained_needed).expect("issue budget fits"));
        assert_eq!(refusal.used, refusal.limit);
        assert_eq!(refusal.additional, u64::MAX);
        drop(probe);
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == refusal
        ));
    }
}
