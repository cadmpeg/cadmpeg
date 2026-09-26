// SPDX-License-Identifier: Apache-2.0
//! Native records projected from presentation inventory.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::native::{
    rendering_style_issue, PmAppDefaultStyleRecord, PmAppRenderingStyleRecord,
    PmAppRenderingStyleRecordWire, PmGraphicsFaceRecord, PmGraphicsPrimaryColorStyleRecord,
    PmGraphicsStyleCollectionRecord,
};
use crate::presentation::PresentationInventory;
use crate::record_issue::{RecordIssue, RecordIssueFamily};

use super::{
    charge_items, charge_retained_len, retained_clone, retained_format, retained_native_sha256,
    retained_sha256, wire_len,
};

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
    for style in &inventory.default_styles {
        let token = style.identity.segment_token.as_str();
        ctx.charge_collection_items(1, "collect Inventor native default style")?;
        ctx.charge_entities(1, "admit Inventor native default style")?;
        projection.default_styles.push(PmAppDefaultStyleRecord {
            id: retained_format(
                ctx,
                format_args!(
                    "inventor:presentation:default-style#{token}-{}",
                    style.identity.record_ordinal
                ),
                "retain Inventor default style id",
            )?,
            segment_token: retained_clone(ctx, token, "retain Inventor default style token")?,
            record_ordinal: style.identity.record_ordinal,
            segment_version_major: style.segment_version_major,
            header_value: style.header_value,
            header_id: style.header_id,
            material_reference: style.material_reference,
            rendering_style_reference: style.rendering_style_reference,
            related_references: style.related_references,
            state: style.state,
            terminal_reference: style.terminal_reference,
            suffix_len: wire_len(
                ctx,
                style.suffix.window().len(),
                "Inventor default style suffix length",
            )?,
            suffix_sha256: retained_native_sha256(
                ctx,
                style.suffix.window(),
                "retain Inventor default style suffix digest",
            )?,
        });
    }
    for style in &inventory.rendering_styles {
        let token = style.identity.segment_token.as_str();
        let extension = style.extension.as_ref();
        let wire = PmAppRenderingStyleRecordWire {
            id: retained_format(
                ctx,
                format_args!(
                    "inventor:presentation:rendering-style#{token}-{}",
                    style.identity.record_ordinal
                ),
                "retain Inventor rendering style id",
            )?,
            segment_token: retained_clone(ctx, token, "retain Inventor rendering style token")?,
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
            name: retained_clone(ctx, &style.name, "retain Inventor rendering style text")?,
            comment: retained_clone(ctx, &style.comment, "retain Inventor rendering style text")?,
            long_name: retained_clone(
                ctx,
                &style.long_name,
                "retain Inventor rendering style text",
            )?,
            style_state: extension.map(|value| value.style_state),
            style_label: extension
                .map(|value| {
                    retained_clone(
                        ctx,
                        &value.style_label,
                        "retain Inventor rendering extension text",
                    )
                })
                .transpose()?,
            asset_guid: extension
                .map(|value| {
                    retained_clone(
                        ctx,
                        &value.asset_guid,
                        "retain Inventor rendering extension text",
                    )
                })
                .transpose()?,
            material_id: extension
                .map(|value| {
                    retained_clone(
                        ctx,
                        &value.material_id,
                        "retain Inventor rendering extension text",
                    )
                })
                .transpose()?,
            asset_library_id: extension
                .map(|value| {
                    retained_clone(
                        ctx,
                        &value.asset_library_id,
                        "retain Inventor rendering extension text",
                    )
                })
                .transpose()?,
            style_values: extension.map(|value| value.style_values),
            guid: extension
                .map(|value| {
                    retained_clone(ctx, &value.guid, "retain Inventor rendering extension text")
                })
                .transpose()?,
            suffix_len: wire_len(
                ctx,
                style.suffix.window().len(),
                "Inventor rendering style suffix length",
            )?,
            suffix_sha256: retained_sha256(
                ctx,
                style.suffix.window(),
                "retain Inventor rendering style suffix digest",
            )?,
        };
        if let Some(detail) = rendering_style_issue(
            style.segment_version_major,
            &style.comment,
            style.extension.is_some(),
        ) {
            charge_retained_len(
                ctx,
                detail.len(),
                "retain Inventor rendering conversion issue",
            )?;
        }
        match PmAppRenderingStyleRecord::try_from(wire) {
            Ok(record) => {
                ctx.charge_collection_items(1, "collect Inventor native rendering style")?;
                ctx.charge_entities(1, "admit Inventor native rendering style")?;
                projection.rendering_styles.push(record);
            }
            Err(detail) => {
                ctx.charge_collection_items(1, "collect Inventor rendering conversion issue")?;
                ctx.charge_entities(1, "admit Inventor rendering conversion issue")?;
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Presentation,
                    segment_token: retained_clone(
                        ctx,
                        token,
                        "retain Inventor rendering issue token",
                    )?,
                    record_ordinal: style.identity.record_ordinal,
                    detail,
                });
            }
        }
    }
    for face in &inventory.graphics_faces {
        let token = face.identity.segment_token.as_str();
        let id = retained_format(
            ctx,
            format_args!(
                "inventor:presentation:graphics-face#{token}-{}",
                face.identity.record_ordinal
            ),
            "retain Inventor graphics face id",
        )?;
        let segment_token = retained_clone(ctx, token, "retain Inventor graphics face token")?;
        charge_items(
            ctx,
            face.edge_references.references().len(),
            "copy Inventor graphics face edge references",
        )?;
        ctx.charge_collection_items(1, "collect Inventor native graphics face")?;
        ctx.charge_entities(1, "admit Inventor native graphics face")?;
        projection.graphics_faces.push(PmGraphicsFaceRecord {
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
            edge_references: face.edge_references.clone(),
            visibility_state: face.visibility_state,
            bounds: face.bounds,
            key: face.key,
            values: face.values,
        });
    }
    for collection in &inventory.graphics_style_collections {
        let token = collection.identity.segment_token.as_str();
        let id = retained_format(
            ctx,
            format_args!(
                "inventor:presentation:graphics-style-collection#{token}-{}",
                collection.identity.record_ordinal
            ),
            "retain Inventor graphics style collection id",
        )?;
        let segment_token = retained_clone(
            ctx,
            token,
            "retain Inventor graphics style collection token",
        )?;
        charge_items(
            ctx,
            collection.style_references.references().len(),
            "copy Inventor graphics style references",
        )?;
        ctx.charge_collection_items(1, "collect Inventor native graphics style collection")?;
        ctx.charge_entities(1, "admit Inventor native graphics style collection")?;
        projection
            .graphics_style_collections
            .push(PmGraphicsStyleCollectionRecord {
                id,
                segment_token,
                record_ordinal: collection.identity.record_ordinal,
                segment_version_major: collection.segment_version_major,
                style_references: collection.style_references.clone(),
            });
    }
    for style in &inventory.graphics_primary_color_styles {
        let token = style.identity.segment_token.as_str();
        ctx.charge_collection_items(1, "collect Inventor native primary color style")?;
        ctx.charge_entities(1, "admit Inventor native primary color style")?;
        projection
            .graphics_primary_color_styles
            .push(PmGraphicsPrimaryColorStyleRecord {
                id: retained_format(
                    ctx,
                    format_args!(
                        "inventor:presentation:graphics-primary-color#{token}-{}",
                        style.identity.record_ordinal
                    ),
                    "retain Inventor primary color style id",
                )?,
                segment_token: retained_clone(
                    ctx,
                    token,
                    "retain Inventor primary color style token",
                )?,
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
            });
    }
    Ok(projection)
}
