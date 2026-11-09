// SPDX-License-Identifier: Apache-2.0
//! Neutral GUI document state and typed native-only descendants.

use crate::loss::FreecadLossCode;
use crate::native::GuiDocumentRecord;
use crate::writer::xml;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::presentation::PresentationStateKind;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeMap;

/// The `GuiDocument.xml` schema the writer authors.
const GUI_SCHEMA: u32 = 1;

/// One GUI state element, or `None` when its tag is no XML element name.
fn state(tag: &str, attributes: &BTreeMap<String, String>, content: &str) -> Option<String> {
    if tag.is_empty()
        || !tag.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':')
        })
    {
        return None;
    }
    let attributes: Vec<_> = attributes
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let root = xml::element(tag, &attributes);
    Some(if content.is_empty() {
        root
    } else {
        format!(
            "{}{content}</{tag}>",
            root.trim_end_matches("/>").to_owned() + ">"
        )
    })
}

fn unwritable(tag: &str) -> LossNote {
    FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
        "GUI state {tag} has no well-formed XML encoding and is omitted"
    ))
}

/// The numbers that follow `field` in an Inventor camera string.
fn camera_field(settings: &str, field: &str, count: usize) -> Option<Vec<f64>> {
    let mut tokens = settings.split_whitespace();
    tokens.find(|token| *token == field)?;
    tokens
        .take(count)
        .map(|token| token.parse().ok())
        .collect::<Option<Vec<f64>>>()
        .filter(|values| values.len() == count)
}

fn camera_settings(camera: &cadmpeg_ir::presentation::CameraState) -> String {
    let stored = camera
        .properties
        .iter()
        .find(|(key, _)| key.as_str() == "settings")
        .map(|(_, value)| value.as_str());
    // The stored string is the camera's own spelling; it is rebuilt only when
    // the typed position or orientation moved off it.
    if let Some(stored) = stored.filter(|stored| {
        camera_field(stored, "position", 3) == camera.position.map(|value| value.get().to_vec())
            && camera_field(stored, "orientation", 4)
                == camera.orientation.map(|value| value.get().to_vec())
    }) {
        return stored.to_owned();
    }
    let settings = stored.unwrap_or("OrthographicCamera { }");
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
    let mut root = BTreeMap::new();
    if let Some(document) = ir.model.presentation_documents.first() {
        // The writer authors the schema-1 GUI vocabulary only.
        if document
            .schema_version
            .is_some_and(|version| version != GUI_SCHEMA)
        {
            losses.push(FreecadLossCode::PresentationSchemaNormalized.note(format!(
                "GUI document {} is stored in GUI schema {GUI_SCHEMA}",
                document.id
            )));
        }
        if let Some(active) = &document.active_view {
            root.insert("activeView".to_owned(), active.clone());
        }
        let mut ordered: Vec<_> = document.states().iter().collect();
        ordered.sort_by_key(|state| state.order);
        if ordered
            .iter()
            .enumerate()
            .any(|(position, state)| usize::try_from(state.order).ok() != Some(position))
        {
            losses.push(
                FreecadLossCode::PresentationStateOrderNormalized.note(format!(
                    "GUI document {} states are stored at consecutive positions",
                    document.id
                )),
            );
        }
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
            let content = match original.filter(|original| original.kind == tag) {
                Some(original) => xml::gui_state_content(original),
                None => Some(String::new()),
            };
            let Some(element) = content.and_then(|content| state(tag, &attributes, &content))
            else {
                losses.push(unwritable(tag));
                continue;
            };
            states.push_str(&element);
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
                let Some(element) = xml::gui_state_content(value)
                    .and_then(|content| state(&value.kind, &value.attributes, &content))
                else {
                    losses.push(unwritable(&value.kind));
                    continue;
                };
                states.push_str(&element);
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
    Ok((GUI_SCHEMA, attributes, states))
}
