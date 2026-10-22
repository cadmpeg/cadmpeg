// SPDX-License-Identifier: Apache-2.0
//! Native presentation values formed inside their source-body stage.
use super::{
    PmAppDefaultStyle, PmAppRenderingStyle, PmGraphicsFace, PmGraphicsPrimaryColorStyle,
    PmGraphicsStyleCollection,
};
use crate::native::{
    PmAppDefaultStyleRecord, PmAppRenderingStyleRecord, PmAppRenderingStyleRecordWire,
    PmGraphicsFaceRecord, PmGraphicsPrimaryColorStyleRecord, PmGraphicsStyleCollectionRecord,
};
use crate::record_identity::Located;
use cadmpeg_core::CodecError;

pub(crate) fn default_style(style: &Located<PmAppDefaultStyle<'_>>) -> PmAppDefaultStyleRecord {
    let token = style.identity.segment_token.as_str();
    PmAppDefaultStyleRecord {
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
    }
}

pub(crate) fn rendering_style(
    style: &Located<PmAppRenderingStyle<'_>>,
) -> Result<PmAppRenderingStyleRecord, CodecError> {
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
    PmAppRenderingStyleRecord::try_from(wire).map_err(CodecError::malformed)
}

pub(crate) fn graphics_face(face: &Located<PmGraphicsFace>) -> PmGraphicsFaceRecord {
    let token = face.identity.segment_token.as_str();
    let id = format!(
        "inventor:presentation:graphics-face#{token}-{}",
        face.identity.record_ordinal
    );
    let segment_token = token.to_owned();
    PmGraphicsFaceRecord {
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
    }
}

pub(crate) fn graphics_style_collection(
    collection: &Located<PmGraphicsStyleCollection>,
) -> Result<PmGraphicsStyleCollectionRecord, CodecError> {
    let token = collection.identity.segment_token.as_str();
    let id = format!(
        "inventor:presentation:graphics-style-collection#{token}-{}",
        collection.identity.record_ordinal
    );
    let segment_token = token.to_owned();
    PmGraphicsStyleCollectionRecord::new(
        id,
        cadmpeg_ir::ids::IdentityKey::try_new(segment_token).map_err(CodecError::malformed)?,
        collection.identity.record_ordinal,
        collection.segment_version_major,
        collection.style_references.clone(),
    )
    .map_err(CodecError::malformed)
}

pub(crate) fn primary_color(
    style: &Located<PmGraphicsPrimaryColorStyle>,
) -> PmGraphicsPrimaryColorStyleRecord {
    let token = style.identity.segment_token.as_str();
    PmGraphicsPrimaryColorStyleRecord {
        id: format!(
            "inventor:presentation:graphics-primary-color#{token}-{}",
            style.identity.record_ordinal
        ),
        segment_token: token.to_owned(),
        record_ordinal: style.identity.record_ordinal,
        segment_version_major: style.segment_version_major,
        header_value: style.header_value,
        controls: style.controls,
        color_header_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&style.color_header),
        colors: style.colors,
        color_tail: style.color_tail,
        state: style.state,
        values: style.values,
        terminal_state: style.terminal_state,
    }
}
