// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;

use crate::decode::admit_presentation_native_projection;
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
        "type".into(),
        &token,
        1,
    );
    let inventory = PresentationInventory {
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
        admit_presentation_native_projection(&ctx, &inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor default style id"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    admit_presentation_native_projection(&ctx, &inventory).expect("admitted default style");
}

#[test]
fn presentation_other_native_records_refuse_before_ids_text_and_reference_copies() {
    let suffix = [1_u8];
    let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
    let reference = PmDcReference {
        index: 1,
        qualified: false,
    };
    let inventory = PresentationInventory {
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
            "type".into(),
            &token,
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
            "type".into(),
            &token,
            1,
        )],
        graphics_style_collections: vec![Located::new(
            PmGraphicsStyleCollection {
                segment_version_major: 0,
                style_references: PmDcPairedReferenceList::new(Some([0; 2]), vec![reference])
                    .expect("paired references"),
            },
            "type".into(),
            &token,
            1,
        )],
        graphics_primary_color_styles: vec![Located::new(
            PmGraphicsPrimaryColorStyle {
                segment_version_major: 0,
                header_value: 0,
                controls: [0; 7],
                color_header: [0; 2],
                colors: [[0.0; 4]; 4],
                color_tail: [0; 2],
                state: 0,
                values: [0; 2],
                terminal_state: 0,
            },
            "type".into(),
            &token,
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
        match admit_presentation_native_projection(&ctx, &inventory) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert!(limit.used + limit.additional > cap);
                operations.push(limit.operation);
                cap = limit.used + limit.additional;
            }
            Ok(()) => {
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        admit_presentation_native_projection(&ctx, &inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "copy Inventor graphics face edge references"
    ));
    let mut inventory = inventory;
    inventory.graphics_faces.clear();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        admit_presentation_native_projection(&ctx, &inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "copy Inventor graphics style references"
    ));
}
