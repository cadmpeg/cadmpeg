// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;

use crate::pmdc::{PmDcPairedReferenceList, PmDcReference};
use crate::presentation::native;
use crate::presentation::{
    PmAppDefaultStyle, PmAppRenderingStyle, PmGraphicsFace, PmGraphicsPrimaryColorStyle,
    PmGraphicsStyleCollection, PresentationInventory, RenderingStyleExtension,
};
use crate::record_identity::Located;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

#[test]
fn presentation_default_native_record_refuses_entity_limit_before_creation() {
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
        crate::record_identity::RecordTypeId::from_bytes([0; 16]),
        token.clone(),
        1,
    );
    let inventory = PresentationInventory {
        default_styles: vec![style],
        rendering_styles: Vec::new(),
        graphics_faces: Vec::new(),
        graphics_style_collections: Vec::new(),
        graphics_primary_color_styles: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        project(&ctx, &inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor native default style"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("build the decode context");
    project(&ctx, &inventory).expect("admitted default style");
}

#[test]
fn presentation_other_native_records_refuse_entity_limits_before_creation() {
    let suffix = [1_u8];
    let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
    let reference = PmDcReference::new(1, false).expect("test reference index fits 31 bits");
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
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
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
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            1,
        )],
        graphics_style_collections: vec![Located::new(
            PmGraphicsStyleCollection {
                segment_version_major: 0,
                style_references: PmDcPairedReferenceList::new(Some([0; 2]), vec![reference])
                    .expect("paired references"),
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
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
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            1,
        )],
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("build the decode context");
    project(&ctx, &inventory).expect("admitted presentation records");
    let mut entity_cap = 0;
    let mut entity_operations = Vec::new();
    for _ in 0..4 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = entity_cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let Err(error) = project(&ctx, &inventory) else {
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
}

#[test]
fn rendering_conversion_issue_refuses_entity_limit_before_creation() {
    let bytes = [];
    let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
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
                name: String::new(),
                comment: String::new(),
                long_name: String::new(),
                extension: None,
                suffix: View::over_retained(&bytes),
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            1,
        )],
        graphics_faces: Vec::new(),
        graphics_style_collections: Vec::new(),
        graphics_primary_color_styles: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        project(&ctx, &inventory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor rendering conversion issue"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("build the decode context");
    project(&ctx, &inventory).expect("admitted rendering issue");
}

#[test]
fn held_invalid_rendering_extension_refuses_without_a_failure_carrier_or_source() {
    let bytes = [];
    let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
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
                name: String::new(),
                comment: String::new(),
                long_name: String::new(),
                extension: None,
                suffix: View::over_retained(&bytes),
            },
            crate::record_identity::RecordTypeId::from_bytes([0; 16]),
            token.clone(),
            1,
        )],
        graphics_faces: Vec::new(),
        graphics_style_collections: Vec::new(),
        graphics_primary_color_styles: Vec::new(),
    };
    let error = native::rendering_style(&inventory.rendering_styles[0])
        .expect_err("invalid held rendering syntax");
    assert!(error
        .to_string()
        .contains("rendering style extension disagrees with segment_version_major"));
}

fn project(
    ctx: &DecodeContext<'_>,
    inventory: &PresentationInventory<'_>,
) -> Result<PartOutput, CodecError> {
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Inventor);
    for value in &inventory.default_styles {
        output.insert_native(ctx, "pm_app_default_styles", native::default_style(value))?;
    }
    for value in &inventory.rendering_styles {
        output.insert_native(
            ctx,
            "pm_app_rendering_styles",
            native::rendering_style(value)?,
        )?;
    }
    for value in &inventory.graphics_faces {
        output.insert_native(ctx, "pm_graphics_faces", native::graphics_face(value))?;
    }
    for value in &inventory.graphics_style_collections {
        output.insert_native(
            ctx,
            "pm_graphics_style_collections",
            native::graphics_style_collection(value)?,
        )?;
    }
    for value in &inventory.graphics_primary_color_styles {
        output.insert_native(
            ctx,
            "pm_graphics_primary_color_styles",
            native::primary_color(value),
        )?;
    }
    Ok(output)
}
