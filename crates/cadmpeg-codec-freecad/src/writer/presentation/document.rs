// SPDX-License-Identifier: Apache-2.0
//! Neutral GUI document state and typed native-only descendants.

use crate::loss::FreecadLossCode;
use crate::native::{GuiDocumentRecord, PropertyBody, PropertyFamily, PropertyRecord, RetainedXml};
use crate::writer::xml;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::presentation::PresentationStateKind;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeMap;

fn state(
    tag: &str,
    attributes: &BTreeMap<String, String>,
    content: &str,
) -> Result<String, CodecError> {
    if tag.is_empty()
        || !tag.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':')
        })
    {
        return Err(CodecError::malformed(
            "GUI state has invalid XML element name",
        ));
    }
    let attributes: Vec<_> = attributes
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let root = xml::element(tag, &attributes);
    Ok(if content.is_empty() {
        root
    } else {
        format!(
            "{}{content}</{tag}>",
            root.trim_end_matches("/>").to_owned() + ">"
        )
    })
}

fn descendants(values: &[crate::native::ValueRecord]) -> Result<String, CodecError> {
    let property = PropertyRecord {
        id: "synthesized-gui-state".into(),
        owner: "gui-document".into(),
        name: "state".into(),
        type_name: "App::PropertyString".into(),
        family: PropertyFamily::String,
        status: None,
        body: PropertyBody::Persisted {
            values: values.to_owned(),
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: RetainedXml::from_source("<state/>", 0)?,
    };
    Ok(xml::native(&property)?
        .and_then(|value| {
            value
                .split_once('>')
                .map(|(_, content)| content.trim_end_matches("</Property>").to_owned())
        })
        .unwrap_or_default())
}

fn camera_settings(camera: &cadmpeg_ir::presentation::CameraState) -> String {
    let settings = camera
        .properties
        .iter()
        .find(|(key, _)| key.as_str() == "settings")
        .map_or("OrthographicCamera { }", |(_, value)| value.as_str());
    let tokens: Vec<_> = settings.split_whitespace().collect();
    let family = if tokens.first() == Some(&"PerspectiveCamera") {
        "PerspectiveCamera"
    } else {
        "OrthographicCamera"
    };
    let mut extras = Vec::new();
    let mut index = 2;
    while index + 1 < tokens.len() {
        match tokens[index] {
            "position" => index += 4,
            "orientation" => index += 5,
            token => {
                extras.push(token);
                index += 1;
            }
        }
    }
    let position = camera.position.map_or(String::new(), |value| {
        format!(
            " position {}",
            value.get().map(|value| value.to_string()).join(" ")
        )
    });
    let orientation = camera.orientation.map_or(String::new(), |value| {
        format!(
            " orientation {}",
            value.get().map(|value| value.to_string()).join(" ")
        )
    });
    format!("{family} {{{position}{orientation} {} }}", extras.join(" "))
}

pub(super) fn write(
    ir: &CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(u32, String, String), CodecError> {
    let native = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<GuiDocumentRecord>("gui_documents"))
        .transpose()?
        .unwrap_or_default();
    let mut states = String::new();
    let mut schema = 1;
    let mut root = BTreeMap::new();
    if let Some(document) = ir.model.presentation_documents.first() {
        schema = document.schema_version.unwrap_or(1);
        if let Some(active) = &document.active_view {
            root.insert("activeView".to_owned(), active.clone());
        }
        let mut ordered: Vec<_> = document.states().iter().collect();
        ordered.sort_by_key(|state| state.order);
        for value in ordered {
            let mut attributes: BTreeMap<_, _> = value
                .attributes
                .iter()
                .map(|(key, value)| (key.as_str().to_owned(), value.clone()))
                .collect();
            let tag = match &value.kind {
                PresentationStateKind::Camera(camera) => {
                    for (key, value) in &camera.properties {
                        attributes.insert(key.as_str().to_owned(), value.clone());
                    }
                    attributes.insert("settings".into(), camera_settings(camera));
                    "Camera"
                }
                PresentationStateKind::Native(tag) => tag,
            };
            let original = document
                .native_ref
                .as_ref()
                .and_then(|id| native.iter().find(|record| &record.id == id))
                .and_then(|record| record.states.get(usize::try_from(value.order).ok()?));
            if !value.assets.is_empty()
                || original.is_some_and(|original| !original.side_entries.is_empty())
            {
                losses.push(
                    FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                        "GUI state {tag} requires an opaque asset and is omitted"
                    )),
                );
                continue;
            }
            let content = original
                .filter(|original| original.kind == tag)
                .map(|original| descendants(&original.values))
                .transpose()?
                .unwrap_or_default();
            states.push_str(&state(tag, &attributes, &content)?);
        }
        if ir.model.presentation_documents.len() > 1 {
            losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(
                "FreeCAD holds one GUI document; additional presentation documents are omitted",
            ));
        }
    } else if !ir.scope.model {
        for document in &native {
            for value in &document.states {
                if !value.side_entries.is_empty() {
                    losses.push(
                        FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                            "GUI state {} requires an opaque asset and is omitted",
                            value.id
                        )),
                    );
                    continue;
                }
                states.push_str(&state(
                    &value.kind,
                    &value.attributes,
                    &descendants(&value.values)?,
                )?);
            }
        }
    }
    let mut attributes = String::new();
    for (key, value) in &root {
        attributes.push(' ');
        attributes.push_str(key);
        attributes.push_str("=\"");
        attributes.push_str(&xml::escaped(value));
        attributes.push('"');
    }
    Ok((schema, attributes, states))
}
