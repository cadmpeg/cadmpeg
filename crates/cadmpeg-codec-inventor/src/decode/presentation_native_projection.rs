// SPDX-License-Identifier: Apache-2.0
//! Native records projected from presentation inventory.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::native::{
    PmAppDefaultStyleRecord, PmAppRenderingStyleRecord, PmAppRenderingStyleRecordWire,
    PmGraphicsFaceRecord, PmGraphicsPrimaryColorStyleRecord, PmGraphicsStyleCollectionRecord,
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
    for style in &inventory.default_styles {
        let token = style.identity.segment_token.as_str();
        ctx.charge_entities(1, "admit Inventor native default style")?;
        projection.default_styles.push(PmAppDefaultStyleRecord {
            id: format!(
                "inventor:presentation:default-style#{token}-{}",
                style.identity.record_ordinal
            ),
            segment_token: token.to_owned(),
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
            suffix_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(style.suffix.window()),
        });
    }
    for style in &inventory.rendering_styles {
        let token = style.identity.segment_token.as_str();
        let extension = style.extension.as_ref();
        let wire = PmAppRenderingStyleRecordWire {
            id: format!(
                "inventor:presentation:rendering-style#{token}-{}",
                style.identity.record_ordinal
            ),
            segment_token: token.to_owned(),
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
            name: style.name.clone(),
            comment: style.comment.clone(),
            long_name: style.long_name.clone(),
            style_state: extension.map(|value| value.style_state),
            style_label: extension.map(|value| value.style_label.clone()),
            asset_guid: extension.map(|value| value.asset_guid.clone()),
            material_id: extension.map(|value| value.material_id.clone()),
            asset_library_id: extension.map(|value| value.asset_library_id.clone()),
            style_values: extension.map(|value| value.style_values),
            guid: extension.map(|value| value.guid.clone()),
            suffix_len: cadmpeg_core::decode::u64_from_index(style.suffix.window().len()),
            suffix_sha256: String::from(cadmpeg_ir::hash::digest::Sha256Digest::digest(
                style.suffix.window(),
            )),
        };
        match PmAppRenderingStyleRecord::try_from(wire) {
            Ok(record) => {
                ctx.charge_entities(1, "admit Inventor native rendering style")?;
                projection.rendering_styles.push(record);
            }
            Err(detail) => {
                ctx.charge_entities(1, "admit Inventor rendering conversion issue")?;
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Presentation,
                    segment_token: cadmpeg_ir::ids::IdentityKey::try_new(token.to_owned())
                        .map_err(CodecError::malformed)?,
                    record_ordinal: style.identity.record_ordinal,
                    detail,
                });
            }
        }
    }
    for face in &inventory.graphics_faces {
        let token = face.identity.segment_token.as_str();
        let id = format!(
            "inventor:presentation:graphics-face#{token}-{}",
            face.identity.record_ordinal
        );
        let segment_token = token.to_owned();
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
        let id = format!(
            "inventor:presentation:graphics-style-collection#{token}-{}",
            collection.identity.record_ordinal
        );
        let segment_token = token.to_owned();
        ctx.charge_entities(1, "admit Inventor native graphics style collection")?;
        projection.graphics_style_collections.push(
            PmGraphicsStyleCollectionRecord::new(
                id,
                cadmpeg_ir::ids::IdentityKey::try_new(segment_token)
                    .map_err(CodecError::malformed)?,
                collection.identity.record_ordinal,
                collection.segment_version_major,
                collection.style_references.clone(),
            )
            .map_err(CodecError::malformed)?,
        );
    }
    for style in &inventory.graphics_primary_color_styles {
        let token = style.identity.segment_token.as_str();
        ctx.charge_entities(1, "admit Inventor native primary color style")?;
        projection
            .graphics_primary_color_styles
            .push(PmGraphicsPrimaryColorStyleRecord {
                id: format!(
                    "inventor:presentation:graphics-primary-color#{token}-{}",
                    style.identity.record_ordinal
                ),
                segment_token: token.to_owned(),
                record_ordinal: style.identity.record_ordinal,
                segment_version_major: style.segment_version_major,
                header_value: style.header_value,
                controls: style.controls,
                color_header: style.color_header.into(),
                colors: style.colors,
                color_tail: style.color_tail,
                state: style.state,
                values: style.values,
                terminal_state: style.terminal_state,
            });
    }
    Ok(projection)
}
