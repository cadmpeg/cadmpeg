// SPDX-License-Identifier: Apache-2.0
//! Independent fallback body styles alongside effective GUI appearance bindings.

use super::xml::{self, Object};
use crate::{brep::ShapePayloadRecord, native::PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    topology::{Body, Color},
    CadIr,
};

fn is_metadata(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix("CadmpegBody") else {
        return false;
    };
    let suffix = suffix.trim_start_matches(|character: char| character.is_ascii_digit());
    matches!(
        suffix,
        "ColorPresent" | "Red" | "Green" | "Blue" | "Alpha" | "VisiblePresent" | "Visible"
    )
}

fn hidden(mut property: String) -> String {
    if let Some(index) = property.find("<Property ") {
        property.insert_str(index + "<Property ".len(), "group=\"CADIR\" hide=\"1\" ");
    }
    property
}

pub(super) fn preserve(object: &mut Object, bodies: &[&Body]) {
    object.properties.retain(|name, _| !is_metadata(name));
    for (index, body) in bodies.iter().enumerate() {
        let prefix = format!("CadmpegBody{index}");
        let present = format!("{prefix}ColorPresent");
        object.set(
            &present,
            hidden(xml::scalar(
                &present,
                "App::PropertyBool",
                "Bool",
                u8::from(body.color.is_some()),
            )),
        );
        if let Some(color) = body.color {
            for (component, value) in [
                ("Red", color.r()),
                ("Green", color.g()),
                ("Blue", color.b()),
                ("Alpha", color.a()),
            ] {
                let key = format!("{prefix}{component}");
                object.set(
                    &key,
                    hidden(xml::scalar(&key, "App::PropertyFloat", "Float", value)),
                );
            }
        }
        let present = format!("{prefix}VisiblePresent");
        object.set(
            &present,
            hidden(xml::scalar(
                &present,
                "App::PropertyBool",
                "Bool",
                u8::from(body.visible.is_some()),
            )),
        );
        if let Some(visible) = body.visible {
            let key = format!("{prefix}Visible");
            object.set(
                &key,
                hidden(xml::scalar(
                    &key,
                    "App::PropertyBool",
                    "Bool",
                    u8::from(visible),
                )),
            );
        }
    }
}

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
