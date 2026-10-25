// SPDX-License-Identifier: Apache-2.0
//! GUI synthesis from neutral presentation and typed native value trees.

pub(super) mod colors;
mod document;
mod materials;

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use crate::native::{GuiPropertyRecord, GuiViewProviderRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) struct Provider {
    name: String,
    expanded: Option<bool>,
    properties: BTreeMap<String, String>,
    extensions: Vec<String>,
    native: BTreeMap<String, GuiPropertyRecord>,
}

impl Provider {
    fn new(name: String) -> Self {
        Self {
            name,
            expanded: None,
            properties: BTreeMap::new(),
            extensions: Vec::new(),
            native: BTreeMap::new(),
        }
    }
    fn set(&mut self, name: &str, kind: &str, tag: &str, value: impl ToString + Copy) {
        self.properties
            .insert(name.to_owned(), xml::scalar(name, kind, tag, value));
    }
    fn visible(&mut self, value: bool) {
        self.set("Visibility", "App::PropertyBool", "Bool", u8::from(value));
    }
    fn scalar_override(&mut self, name: &str, value: &str) -> Result<bool, CodecError> {
        if let Some(original) = self.native.get(name) {
            let mut property = original.clone();
            if let Some(root) = property.values.iter_mut().find(|value| {
                value.attributes.contains_key("value") || value.attributes.contains_key("Value")
            }) {
                let key = if root.attributes.contains_key("value") {
                    "value"
                } else {
                    "Value"
                };
                root.attributes.insert(key.into(), value.into());
                if let Some(output) = native_property(&property)? {
                    self.properties.insert(name.into(), output);
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        self.set(name, "App::PropertyString", "String", value);
        Ok(true)
    }
}

fn native_property(record: &GuiPropertyRecord) -> Result<Option<String>, CodecError> {
    xml::native_gui(record)
}

fn native(
    ir: &CadIr,
    names: &HashMap<String, String>,
    live: &HashSet<&str>,
    losses: &mut Vec<LossNote>,
    entries: &mut BTreeMap<String, Vec<u8>>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
    emitted_meshes: &mut HashSet<String>,
    has_geometry: &mut bool,
    native_only: bool,
) -> Result<BTreeMap<String, Provider>, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(BTreeMap::new());
    };
    let mut records = namespace.arena_as::<GuiViewProviderRecord>("gui_view_providers")?;
    let properties = namespace.arena_as::<GuiPropertyRecord>("gui_properties")?;
    let mut extensions = namespace.arena_as::<crate::native::ExtensionRecord>("extensions")?;
    extensions.sort_by_key(|extension| extension.order);
    records.sort_by_key(|provider| provider.order);
    let mut providers = BTreeMap::new();
    for record in records {
        if ir.scope.model
            && !ir
                .model
                .view_presentations
                .iter()
                .any(|view| view.native_ref.as_deref() == Some(record.id.as_str()))
        {
            continue;
        }
        let name =
            match &record.object {
                Some(object) => match names
                    .get(object.as_str())
                    .filter(|name| live.contains(name.as_str()))
                {
                    Some(name) => name.clone(),
                    None => {
                        losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(
                            format!("GUI provider {} refers to an omitted object", record.id),
                        ));
                        continue;
                    }
                },
                None => record.name.clone(),
            };
        let mut provider = Provider::new(name.clone());
        provider.expanded = record.expanded;
        for mut property in properties
            .iter()
            .filter(|property| property.owner == record.id)
            .cloned()
        {
            if super::application_payloads::gui_kernel_deleted(ir, &property)? {
                super::dispositions::record(
                    outcomes,
                    "gui_properties",
                    &property.id,
                    cadmpeg_ir::codec::write::NativeDisposition::Dropped,
                );
                continue;
            }
            crate::gui::validate_property_links(&property)?;
            if property.values.has_missing()
                || super::xml::expression_metadata_omitted(
                    &property.type_name,
                    &property.values,
                    &property.links,
                )?
            {
                losses.push(
                    cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                        "GUI property {} omits undecoded value or unassociated reference metadata parts",
                        property.id
                    )),
                );
            }

            let mut payload_members = super::property_payloads::write_gui(
                ir,
                Some(namespace),
                &property,
                entries,
                losses,
                outcomes,
            )?;
            let mut property_has_geometry = false;
            let kernel = super::application_payloads::write_gui_property(
                ir,
                &mut property,
                entries,
                outcomes,
                losses,
                &mut property_has_geometry,
            )?;
            payload_members.extend(kernel.members);
            emitted_meshes.extend(kernel.meshes);
            for additional in kernel.additional {
                if let Some(value) = xml::native_gui_payloads(&additional, names, &payload_members)?
                {
                    provider.properties.insert(additional.name.clone(), value);
                    provider.native.insert(additional.name.clone(), additional);
                }
            }
            let (shape_members, shape_meshes) = super::brep::native_tables::gui::write_property(
                ir,
                &mut property,
                entries,
                outcomes,
                losses,
                &mut property_has_geometry,
                native_only,
            )?;
            payload_members.extend(shape_members);
            provider
                .native
                .insert(property.name.clone(), property.clone());
            match xml::native_gui_payloads(&property, names, &payload_members)? {
                Some(value) => {
                    *has_geometry |= property_has_geometry;
                    emitted_meshes.extend(shape_meshes);
                    provider.properties.insert(property.name.clone(), value);
                    provider
                        .native
                        .insert(property.name.clone(), property.clone());
                }
                None if matches!(
                    property.type_name.as_str(),
                    "App::PropertyColorList" | "App::PropertyMaterialList"
                ) => {}
                None => losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(
                    format!(
                        "GUI property {} has no typed writable side payload and is omitted",
                        property.id
                    ),
                )),
            }
        }
        for extension in extensions
            .iter()
            .filter(|extension| extension.owner == record.id)
        {
            let mut content = Vec::new();
            for mut property in properties
                .iter()
                .filter(|property| property.owner == extension.id)
                .cloned()
            {
                if super::application_payloads::gui_kernel_deleted(ir, &property)? {
                    super::dispositions::record(
                        outcomes,
                        "gui_properties",
                        &property.id,
                        cadmpeg_ir::codec::write::NativeDisposition::Dropped,
                    );
                    continue;
                }
                crate::gui::validate_property_links(&property)?;
                if property.values.has_missing()
                    || super::xml::expression_metadata_omitted(
                        &property.type_name,
                        &property.values,
                        &property.links,
                    )?
                {
                    losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!("GUI property {} omits undecoded value or unassociated reference metadata parts",property.id)));
                }

                let mut payload_members = super::property_payloads::write_gui(
                    ir,
                    Some(namespace),
                    &property,
                    entries,
                    losses,
                    outcomes,
                )?;
                let mut property_has_geometry = false;
                let kernel = super::application_payloads::write_gui_property(
                    ir,
                    &mut property,
                    entries,
                    outcomes,
                    losses,
                    &mut property_has_geometry,
                )?;
                payload_members.extend(kernel.members);
                emitted_meshes.extend(kernel.meshes);
                for additional in kernel.additional {
                    if let Some(value) =
                        xml::native_gui_payloads(&additional, names, &payload_members)?
                    {
                        content.push(value);
                    }
                }
                let (shape_members, shape_meshes) =
                    super::brep::native_tables::gui::write_property(
                        ir,
                        &mut property,
                        entries,
                        outcomes,
                        losses,
                        &mut property_has_geometry,
                        native_only,
                    )?;
                payload_members.extend(shape_members);
                if let Some(value) = xml::native_gui_payloads(&property, names, &payload_members)? {
                    *has_geometry |= property_has_geometry;
                    emitted_meshes.extend(shape_meshes);
                    content.push(value);
                } else {
                    losses.push(
                        cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(
                            format!(
                                "GUI extension property {} has no writable typed value",
                                property.id
                            ),
                        ),
                    );
                }
            }
            provider.extensions.push(format!("<Extension name=\"{}\" type=\"{}\"><Properties Count=\"{}\">{}</Properties></Extension>",xml::escaped(&extension.name),xml::escaped(&extension.type_name),content.len(),content.join("")));
        }
        providers.insert(name, provider);
    }
    Ok(providers)
}

fn neutral_views(
    ir: &CadIr,
    names: &HashMap<String, String>,
    live: &HashSet<&str>,
    providers: &mut BTreeMap<String, Provider>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let native_providers = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<GuiViewProviderRecord>("gui_view_providers"))
        .transpose()?
        .unwrap_or_default();
    for view in &ir.model.view_presentations {
        let name = view
            .object
            .as_ref()
            .and_then(|object| names.get(object))
            .filter(|name| live.contains(name.as_str()))
            .cloned()
            .or_else(|| {
                view.native_ref
                    .as_ref()
                    .and_then(|native| native_providers.iter().find(|record| &record.id == native))
                    .filter(|record| record.object.is_none())
                    .map(|record| record.name.clone())
            });
        let Some(name) = name else {
            losses.push(
                FreecadLossCode::ExportPresentationNotTransferred.note(format!(
                    "view {} has no output object and is omitted",
                    view.id
                )),
            );
            continue;
        };
        let provider = providers
            .entry(name.clone())
            .or_insert_with(|| Provider::new(name));
        provider.expanded = view.expanded;
        for name in [
            "Visibility",
            "DisplayMode",
            "SelectionStyle",
            "LineWidth",
            "PointSize",
        ] {
            provider.properties.remove(name);
        }
        if let Some(value) = view.visible {
            provider.visible(value);
        }
        if view.visible.is_some()
            && view.object.as_ref().is_some_and(|object| {
                ir.model.bodies.iter().any(|body| {
                    body.id.as_str() == object
                        || names
                            .get(object)
                            .is_some_and(|owner| names.get(body.id.as_str()) == Some(owner))
                })
            })
        {
            losses.push(FreecadLossCode::ExportPresentationNotTransferred.note(format!(
                "view {} independent visibility gate is combined with its body's gate in native Visibility", view.id
            )));
        }
        for (key, value) in [
            ("DisplayMode", view.display_mode.as_ref()),
            ("SelectionStyle", view.selection_style.as_ref()),
        ] {
            if let Some(value) = value {
                provider.set(key, "App::PropertyEnumeration", "Integer", value);
            }
        }
        if let Some(value) = view.line_width {
            provider.set(
                "LineWidth",
                "App::PropertyFloatConstraint",
                "Float",
                value.get(),
            );
        }
        if let Some(value) = view.point_size {
            provider.set(
                "PointSize",
                "App::PropertyFloatConstraint",
                "Float",
                value.get(),
            );
        }
        for (key, value) in &view.properties {
            if matches!(
                key.as_str(),
                "Visibility" | "DisplayMode" | "SelectionStyle" | "LineWidth" | "PointSize"
            ) {
                continue;
            }
            if !provider.scalar_override(key.as_str(), value)? {
                losses.push(FreecadLossCode::ExportPresentationNotTransferred.note(format!("view {} textual property {key} has no neutral scalar encoding; its typed native value tree is emitted independently",view.id)));
            }
        }
    }
    for occurrence in &ir.model.occurrences {
        if let Some(value) = occurrence.visible {
            if let Some(name) = names
                .get(occurrence.id.as_str())
                .filter(|name| live.contains(name.as_str()))
            {
                providers
                    .entry(name.clone())
                    .or_insert_with(|| Provider::new(name.clone()))
                    .visible(value);
            }
        }
    }
    Ok(())
}

pub(super) fn write(
    ir: &CadIr,
    names: &HashMap<String, String>,
    body_names: &HashMap<String, String>,
    objects: &[Object],
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
    emitted_meshes: &mut HashSet<String>,
    has_geometry: &mut bool,
    native_only: bool,
) -> Result<String, CodecError> {
    let live: HashSet<_> = objects.iter().map(|object| object.name.as_str()).collect();
    let mut providers = native(
        ir,
        names,
        &live,
        losses,
        entries,
        outcomes,
        emitted_meshes,
        has_geometry,
        native_only,
    )?;
    neutral_views(ir, names, &live, &mut providers, losses)?;
    colors::write(ir, names, body_names, &mut providers, entries, losses)?;
    materials::write(
        ir,
        names,
        body_names,
        &mut providers,
        entries,
        losses,
        outcomes,
    )?;
    for provider in providers.values() {
        for property in provider.native.values().filter(|property| {
            matches!(
                property.type_name.as_str(),
                "App::PropertyColorList" | "App::PropertyMaterialList"
            ) && !provider.properties.contains_key(&property.name)
        }) {
            losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!("GUI property {} has no uniquely associated current list or complete topology assignments and is omitted", property.id)));
        }
    }
    for view in &ir.model.view_presentations {
        let provider = view
            .object
            .as_ref()
            .and_then(|object| names.get(object))
            .and_then(|name| providers.get(name));
        let Some(provider) = provider else {
            continue;
        };
        for (key, expected) in &view.properties {
            if let Some(property) = provider.properties.get(key.as_str()) {
                let parsed = roxmltree::Document::parse(property).map_err(CodecError::malformed)?;
                let actual = parsed
                    .root_element()
                    .first_element_child()
                    .and_then(|value| {
                        value
                            .attribute("value")
                            .or_else(|| value.attribute("Value"))
                    });
                if actual.is_some_and(|actual| actual != expected) {
                    losses.push(FreecadLossCode::ExportPresentationNotTransferred.note(format!("view {} generic scalar {key} changes because its dedicated neutral field or appearance binding owns the effective GUI value", view.id)));
                }
            }
        }
    }
    let has_document = !ir.model.presentation_documents.is_empty()
        || (!ir.scope.model
            && ir.native.namespace("fcstd").is_some_and(|native| {
                native
                    .arenas()
                    .get("gui_documents")
                    .is_some_and(|records| !records.is_empty())
            }));
    if providers.is_empty() && !has_document {
        return Ok(String::new());
    }
    if providers.len() != ir.model.view_presentations.len()
        || (ir.model.presentation_documents.is_empty() && !providers.is_empty())
    {
        losses.push(FreecadLossCode::ExportPresentationNotTransferred.note("FreeCAD GUI carriers create presentation records for appearance or visibility state without an authored neutral presentation"));
    }
    let mut content = String::new();
    for provider in providers.values() {
        let expanded = provider.expanded.map_or(String::new(), |value| {
            format!(" expanded=\"{}\"", u8::from(value))
        });
        content.push_str("<ViewProvider name=\"");
        content.push_str(&xml::escaped(&provider.name));
        content.push('"');
        content.push_str(&expanded);
        content.push('>');
        if !provider.extensions.is_empty() {
            content.push_str(&format!(
                "<Extensions Count=\"{}\">{}</Extensions>",
                provider.extensions.len(),
                provider.extensions.join("")
            ));
        }
        content.push_str("<Properties Count=\"");
        content.push_str(&provider.properties.len().to_string());
        content.push_str("\">");
        content.push_str(&provider.properties.values().cloned().collect::<String>());
        content.push_str("</Properties></ViewProvider>");
    }
    let (schema, attributes, states) = document::write(ir, losses)?;
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><GuiDocument SchemaVersion=\"{schema}\"{attributes}>{states}<ViewProviderData Count=\"{}\">{content}</ViewProviderData></GuiDocument>",providers.len()))
}

#[cfg(test)]
mod tests;
