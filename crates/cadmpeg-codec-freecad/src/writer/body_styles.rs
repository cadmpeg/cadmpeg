// SPDX-License-Identifier: Apache-2.0
//! Independent fallback body styles read back from retained documents.
//!
//! The writer emits body color and visibility through the view provider
//! (`ShapeColor`/`Visibility`) and no longer writes these properties; the
//! reader below restores styles from documents that still carry them.

use crate::{brep::ShapePayloadRecord, native::PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{topology::Color, CadIr};

pub(crate) fn apply(
    ir: &mut CadIr,
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
) -> Result<(), CodecError> {
    for payload in payloads {
        let Some(owner) = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map(|property| &property.owner)
        else {
            continue;
        };
        let value = |name: &str| {
            properties
                .iter()
                .find(|property| property.owner == *owner && property.name == name)
                .and_then(|property| property.values().first())
                .and_then(|value| value.attributes.get("value"))
        };
        let prefix = crate::native::model_id("body", &payload.id, "");
        for (index, body) in ir
            .model
            .bodies
            .iter_mut()
            .filter(|body| body.id.as_str().starts_with(&prefix))
            .enumerate()
        {
            let prefix = format!("CadmpegBody{index}");
            match value(&format!("{prefix}ColorPresent")).map(String::as_str) {
                Some("0" | "false") => body.color = None,
                Some("1" | "true") => {
                    let component = |name| {
                        value(&format!("{prefix}{name}"))
                            .and_then(|value| value.parse::<f32>().ok())
                    };
                    let (((r, g), b), a) = component("Red")
                        .zip(component("Green"))
                        .zip(component("Blue"))
                        .zip(component("Alpha"))
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "CADIR body fallback color has absent or invalid components",
                            )
                        })?;
                    body.color = Some(Color::new(r, g, b, a).ok_or_else(|| {
                        CodecError::malformed(
                            "CADIR body fallback color is outside the unit interval",
                        )
                    })?);
                }
                _ => {}
            }
            match value(&format!("{prefix}VisiblePresent")).map(String::as_str) {
                Some("0" | "false") => body.visible = None,
                Some("1" | "true") => {
                    body.visible =
                        value(&format!("{prefix}Visible")).and_then(|value| match value.as_str() {
                            "0" | "false" => Some(false),
                            "1" | "true" => Some(true),
                            _ => None,
                        });
                }
                _ => {}
            }
        }
    }
    Ok(())
}
