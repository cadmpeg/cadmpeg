// SPDX-License-Identifier: Apache-2.0
//! Transactional presentation bodies and native value construction.
use super::{
    PmAppDefaultStyle, PmAppRenderingStyle, PmGraphicsFace, PmGraphicsPrimaryColorStyle,
    PmGraphicsStyleCollection,
};
use crate::record_identity::{Located, RecordTypeId};
use crate::records::RseRecordFrame;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

pub(super) enum Parsed<'a> {
    Default(Located<PmAppDefaultStyle<'a>>),
    Rendering(Located<PmAppRenderingStyle<'a>>),
    Face(Located<PmGraphicsFace>),
    Collection(Located<PmGraphicsStyleCollection>),
    Primary(Located<PmGraphicsPrimaryColorStyle>),
}

pub(super) fn decode<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    expanded: View<'a>,
    token: &IdentityKey,
    frame: &RseRecordFrame<'a>,
    version: u8,
) -> Result<Option<Parsed<'a>>, CodecError> {
    let kind = match frame.type_id {
        super::DEFAULT_STYLE_TYPE => "default-style",
        super::RENDERING_STYLE_TYPE => "rendering-style",
        super::GRAPHICS_FACE_TYPE => "graphics-face",
        super::GRAPHICS_STYLE_COLLECTION_TYPE => "graphics-style-collection",
        super::GRAPHICS_PRIMARY_COLOR_STYLE_TYPE => "graphics-primary-color",
        _ => {
            return Err(CodecError::malformed(
                "unsupported presentation record dispatch",
            ))
        }
    };
    let id = format!("inventor:presentation:{kind}#{token}-{}", frame.ordinal);
    crate::records::source_admission::decode_body(
        ctx,
        output,
        expanded,
        token.as_str(),
        frame,
        |stage| {
            ctx.record(&id, frame.payload, || {
                let type_id = RecordTypeId::from_bytes(frame.type_id);
                Ok(match frame.type_id {
                    super::DEFAULT_STYLE_TYPE => {
                        let value = Located::new(
                            super::parse_default_style(frame.payload, version)?,
                            type_id,
                            token.clone(),
                            frame.ordinal,
                        );
                        stage.insert_native(
                            ctx,
                            "pm_app_default_styles",
                            super::native::default_style(&value),
                        )?;
                        Parsed::Default(value)
                    }
                    super::RENDERING_STYLE_TYPE => {
                        let value = Located::new(
                            super::parse_rendering_style(ctx, frame.payload, version)?,
                            type_id,
                            token.clone(),
                            frame.ordinal,
                        );
                        stage.insert_native(
                            ctx,
                            "pm_app_rendering_styles",
                            super::native::rendering_style(&value)?,
                        )?;
                        Parsed::Rendering(value)
                    }
                    super::GRAPHICS_FACE_TYPE => {
                        let value = Located::new(
                            super::parse_graphics_face(frame.payload, version)?,
                            type_id,
                            token.clone(),
                            frame.ordinal,
                        );
                        stage.insert_native(
                            ctx,
                            "pm_graphics_faces",
                            super::native::graphics_face(&value),
                        )?;
                        Parsed::Face(value)
                    }
                    super::GRAPHICS_STYLE_COLLECTION_TYPE => {
                        let value = Located::new(
                            super::parse_graphics_style_collection(frame.payload, version)?,
                            type_id,
                            token.clone(),
                            frame.ordinal,
                        );
                        stage.insert_native(
                            ctx,
                            "pm_graphics_style_collections",
                            super::native::graphics_style_collection(&value)?,
                        )?;
                        Parsed::Collection(value)
                    }
                    super::GRAPHICS_PRIMARY_COLOR_STYLE_TYPE => {
                        let value = Located::new(
                            super::parse_graphics_primary_color_style(frame.payload, version)?,
                            type_id,
                            token.clone(),
                            frame.ordinal,
                        );
                        stage.insert_native(
                            ctx,
                            "pm_graphics_primary_color_styles",
                            super::native::primary_color(&value),
                        )?;
                        Parsed::Primary(value)
                    }
                    _ => {
                        return Err(CodecError::malformed(
                            "unsupported presentation record dispatch",
                        ))
                    }
                })
            })
        },
    )
}

#[cfg(test)]
mod tests;
