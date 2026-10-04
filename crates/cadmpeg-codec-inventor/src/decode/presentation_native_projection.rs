// SPDX-License-Identifier: Apache-2.0
//! Native records projected from presentation inventory.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::native::{
    PmAppDefaultStyleRecord, PmAppRenderingStyleRecord,
    PmAppRenderingStyleRecordWire, PmGraphicsFaceRecord, PmGraphicsPrimaryColorStyleRecord,
    PmGraphicsStyleCollectionRecord,
};
use crate::presentation::PresentationInventory;
use crate::record_issue::{RecordIssue, RecordIssueFamily};

pub(super) struct PresentationNativeProjection {
    pub(super) default_styles: Vec<PmAppDefaultStyleRecord>,
    pub(super) rendering_styles: Vec<PmAppRenderingStyleRecord>,
    pub(super) graphics_faces: Vec<PmGraphicsFaceRecord>,
    pub(super) graphics_style_collections: Vec<PmGraphicsStyleCollectionRecord>,
    pub(super) graphics_primary_color_styles: Vec<PmGraphicsPrimaryColorStyleRecord>,
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &mut PresentationInventory<'_>,
) -> Result<PresentationNativeProjection, CodecError> {
    let mut projection = PresentationNativeProjection {
        default_styles: Vec::new(),
        rendering_styles: Vec::new(),
        graphics_faces: Vec::new(),
        graphics_style_collections: Vec::new(),
        graphics_primary_color_styles: Vec::new(),
    };
    for style in ctx.admit_iter(&inventory.default_styles, "visit Inventor decode/presentation_native_projection items")? {
        let token = style.identity.segment_token.as_str();
        ctx.charge_entities(1, "admit Inventor native default style")?;
        ctx.push_vec(&mut projection.default_styles, PmAppDefaultStyleRecord {
            id: ctx.format_retained(
                format_args!(
                    "inventor:presentation:default-style#{token}-{}",
                    style.identity.record_ordinal
                ),
                "retain Inventor default style id",
            )?,
            segment_token: ctx.copy_retained_text(token, "retain Inventor default style token")?,
            record_ordinal: style.identity.record_ordinal,
            segment_version_major: style.segment_version_major,
            header_value: style.header_value,
            header_id: style.header_id,
            material_reference: style.material_reference,
            rendering_style_reference: style.rendering_style_reference,
            related_references: style.related_references,
            state: style.state,
            terminal_reference: style.terminal_reference,
            suffix_len: cadmpeg_core::decode::u64_from_index(style.suffix.window().len()),
            suffix_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                ctx,
                style.suffix.window(),
                "retain Inventor default style suffix digest",
            )?,
        }, "collect Inventor native default style")?;
    }
    for style in ctx.admit_iter(&inventory.rendering_styles, "visit Inventor decode/presentation_native_projection items")? {
        let token = style.identity.segment_token.as_str();
        let extension = style.extension.as_ref();
        let wire = PmAppRenderingStyleRecordWire {
            id: ctx.format_retained(
                format_args!(
                    "inventor:presentation:rendering-style#{token}-{}",
                    style.identity.record_ordinal
                ),
                "retain Inventor rendering style id",
            )?,
            segment_token: ctx
                .copy_retained_text(token, "retain Inventor rendering style token")?,
            record_ordinal: style.identity.record_ordinal,
            segment_version_major: style.segment_version_major,
            header_value: style.header_value,
            header_id: style.header_id,
            state: style.state,
            flags: style.flags,
            values: style.values,
            default_state: style.default_state,
            value: style.value,
            name_reference: style.name_reference,
            name: ctx.copy_retained_text(&style.name, "retain Inventor rendering style text")?,
            comment: ctx
                .copy_retained_text(&style.comment, "retain Inventor rendering style text")?,
            long_name: ctx
                .copy_retained_text(&style.long_name, "retain Inventor rendering style text")?,
            style_state: extension.map(|value| value.style_state),
            style_label: extension
                .map(|value| {
                    ctx.copy_retained_text(
                        &value.style_label,
                        "retain Inventor rendering extension text",
                    )
                })
                .transpose()?,
            asset_guid: extension
                .map(|value| {
                    ctx.copy_retained_text(
                        &value.asset_guid,
                        "retain Inventor rendering extension text",
                    )
                })
                .transpose()?,
            material_id: extension
                .map(|value| {
                    ctx.copy_retained_text(
                        &value.material_id,
                        "retain Inventor rendering extension text",
                    )
                })
                .transpose()?,
            asset_library_id: extension
                .map(|value| {
                    ctx.copy_retained_text(
                        &value.asset_library_id,
                        "retain Inventor rendering extension text",
                    )
                })
                .transpose()?,
            style_values: extension.map(|value| value.style_values),
            guid: extension
                .map(|value| {
                    ctx.copy_retained_text(&value.guid, "retain Inventor rendering extension text")
                })
                .transpose()?,
            suffix_len: cadmpeg_core::decode::u64_from_index(style.suffix.window().len()),
            suffix_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                ctx,
                style.suffix.window(),
                "retain Inventor rendering style suffix digest",
            )
            .map(String::from)?,
        };
        match wire.into_record(ctx) {
            Ok(record) => {
                ctx.charge_entities(1, "admit Inventor native rendering style")?;
                ctx.push_vec(&mut projection.rendering_styles, record, "collect Inventor native rendering style")?;
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(CodecError::Malformed(detail)) => {
                ctx.charge_entities(1, "admit Inventor rendering conversion issue")?;
                ctx.push_vec(&mut inventory.issues, RecordIssue {
                    family: RecordIssueFamily::Presentation,
                    segment_token: style.identity.segment_token.try_clone_for_decode(ctx, "retain Inventor rendering issue token")?,
                    record_ordinal: style.identity.record_ordinal,
                    detail,
                }, "collect Inventor rendering conversion issue")?;
            }
            Err(error) => return Err(error),
        }
    }
    for face in ctx.admit_iter(&inventory.graphics_faces, "visit Inventor decode/presentation_native_projection items")? {
        let token = face.identity.segment_token.as_str();
        let id = ctx.format_retained(
            format_args!(
                "inventor:presentation:graphics-face#{token}-{}",
                face.identity.record_ordinal
            ),
            "retain Inventor graphics face id",
        )?;
        let segment_token = ctx.copy_retained_text(token, "retain Inventor graphics face token")?;
        ctx.charge_entities(1, "admit Inventor native graphics face")?;
        ctx.push_vec(&mut projection.graphics_faces, PmGraphicsFaceRecord {
            id,
            segment_token,
            record_ordinal: face.identity.record_ordinal,
            segment_version_major: face.segment_version_major,
            header_value: face.header_value,
            header_id: face.header_id,
            flags: face.flags,
            styles: face.styles,
            surface: face.surface,
            parent: face.parent,
            state: face.state,
            edge_references: face.edge_references.try_clone_for_decode(ctx, "copy Inventor graphics face edge references")?,
            visibility_state: face.visibility_state,
            bounds: face.bounds,
            key: face.key,
            values: face.values,
        }, "collect Inventor native graphics face")?;
    }
    for collection in ctx.admit_iter(&inventory.graphics_style_collections, "visit Inventor decode/presentation_native_projection items")? {
        let token = collection.identity.segment_token.as_str();
        let id = ctx.format_retained(
            format_args!(
                "inventor:presentation:graphics-style-collection#{token}-{}",
                collection.identity.record_ordinal
            ),
            "retain Inventor graphics style collection id",
        )?;
        ctx.charge_entities(1, "admit Inventor native graphics style collection")?;
        ctx.push_vec(&mut projection.graphics_style_collections, PmGraphicsStyleCollectionRecord::new(
                ctx, id,
                collection.identity.segment_token.try_clone_for_decode(ctx, "retain Inventor graphics style collection token")?,
                collection.identity.record_ordinal,
                collection.segment_version_major,
                collection.style_references.try_clone_for_decode(ctx, "copy Inventor graphics style references")?,
            )
            ?, "collect Inventor native graphics style collection")?;
    }
    for style in ctx.admit_iter(&inventory.graphics_primary_color_styles, "visit Inventor decode/presentation_native_projection items")? {
        let token = style.identity.segment_token.as_str();
        ctx.charge_entities(1, "admit Inventor native primary color style")?;
        ctx.push_vec(&mut projection.graphics_primary_color_styles, PmGraphicsPrimaryColorStyleRecord {
                id: ctx.format_retained(
                    format_args!(
                        "inventor:presentation:graphics-primary-color#{token}-{}",
                        style.identity.record_ordinal
                    ),
                    "retain Inventor primary color style id",
                )?,
                segment_token: ctx
                    .copy_retained_text(token, "retain Inventor primary color style token")?,
                record_ordinal: style.identity.record_ordinal,
                segment_version_major: style.segment_version_major,
                header_value: style.header_value,
                controls: style.controls,
                color_header: style.color_header,
                colors: style.colors,
                color_tail: style.color_tail,
                state: style.state,
                values: style.values,
                terminal_state: style.terminal_state,
            }, "collect Inventor native primary color style")?;
    }
    Ok(projection)
}
