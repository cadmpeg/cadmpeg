// SPDX-License-Identifier: Apache-2.0
//! Source-less schema-4 document construction.

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Write};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

/// One XML value element in a source-less `FCStd` property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcstdPropertyValue {
    /// Persistence value tag, such as `String`, `Float`, or `Bool`.
    tag: String,
    /// Ordered-independent XML attributes.
    attributes: BTreeMap<String, String>,
    /// Optional text content for a non-empty element.
    text: Option<String>,
    children: Vec<Self>,
}

impl FcstdPropertyValue {
    /// Construct an empty value element with one attribute.
    pub fn attribute(
        tag: impl Into<String>,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            tag: tag.into(),
            attributes: BTreeMap::from([(name.into(), value.into())]),
            text: None,
            children: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct Property {
    name: String,
    type_name: String,
    values: Vec<FcstdPropertyValue>,
}

#[derive(Debug, Clone)]
struct Object {
    name: String,
    type_name: String,
    properties: Vec<Property>,
    dependencies: Vec<String>,
}

/// Builds a deterministic, source-less `FCStd` application graph.
#[derive(Debug, Clone)]
pub struct FcstdDocumentBuilder {
    label: String,
    objects: Vec<Object>,
    side_entries: Vec<(String, Vec<u8>)>,
}

impl FcstdDocumentBuilder {
    /// Start a schema-4/file-1 document.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            objects: Vec::new(),
            side_entries: Vec::new(),
        }
    }

    /// Add an application object with its exact runtime type name.
    pub fn add_object(
        &mut self,
        name: impl Into<String>,
        type_name: impl Into<String>,
    ) -> Result<&mut Self, CodecError> {
        let name = name.into();
        valid_identifier(&name, "object")?;
        if self.objects.iter().any(|object| object.name == name) {
            return Err(CodecError::malformed(format_args!(
                "duplicate source-less FCStd object {name}"
            )));
        }
        self.objects.push(Object {
            name,
            type_name: type_name.into(),
            properties: Vec::new(),
            dependencies: Vec::new(),
        });
        Ok(self)
    }

    /// Add one typed property to an existing object.
    pub fn add_property(
        &mut self,
        object: &str,
        name: impl Into<String>,
        type_name: impl Into<String>,
        values: Vec<FcstdPropertyValue>,
    ) -> Result<&mut Self, CodecError> {
        let target = self
            .objects
            .iter_mut()
            .find(|candidate| candidate.name == object)
            .ok_or_else(|| CodecError::malformed(format_args!("missing FCStd object {object}")))?;
        let name = name.into();
        valid_identifier(&name, "property")?;
        if target
            .properties
            .iter()
            .any(|property| property.name == name)
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate FCStd property {object}.{name}"
            )));
        }
        for value in &values {
            validate_value(value)?;
        }
        target.properties.push(Property {
            name,
            type_name: type_name.into(),
            values,
        });
        Ok(self)
    }

    /// Add a named payload; typed references determine its restoration order.
    pub fn add_side_entry(
        &mut self,
        name: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<&mut Self, CodecError> {
        let name = name.into();
        if !crate::native::is_safe_entry_name(&name) || name == "Document.xml" {
            return Err(CodecError::malformed(format_args!(
                "invalid source-less FCStd side entry {name:?}"
            )));
        }
        if self
            .side_entries
            .iter()
            .any(|(candidate, _)| candidate == &name)
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate source-less FCStd side entry {name}"
            )));
        }
        self.side_entries.push((name, bytes));
        Ok(self)
    }

    /// Materialize the source-less graph as fully decoded CADIR.
    pub fn build(self) -> Result<CadIr, CodecError> {
        let bytes = self.archive_bytes()?;
        FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .map(|result| result.into_parts().0)
            .map_err(|failure| match failure {
                DecodeFailure::Codec(error) => error,
                failure => CodecError::malformed(format_args!(
                    "source-less FCStd graph was refused: {failure:?}"
                )),
            })
    }

    fn archive_bytes(self) -> Result<Vec<u8>, CodecError> {
        let document = self.document_xml();
        let side_entries = self.restoration_entries(&document)?;
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut cursor);
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .last_modified_time(zip::DateTime::default());
            archive
                .start_file("Document.xml", options)
                .map_err(|error| CodecError::Malformed(error.to_string()))?;
            archive.write_all(document.as_bytes())?;
            for (name, bytes) in side_entries {
                archive
                    .start_file(name, options)
                    .map_err(|error| CodecError::Malformed(error.to_string()))?;
                archive.write_all(bytes)?;
            }
            archive
                .finish()
                .map_err(|error| CodecError::Malformed(error.to_string()))?;
        }
        Ok(cursor.into_inner())
    }

    fn restoration_entries(&self, document: &str) -> Result<Vec<&(String, Vec<u8>)>, CodecError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            document.as_bytes(),
            &arena,
            &policy,
        )?;
        let xml = ctx.parse_xml(document, "source-less FreeCAD request schedule")?;
        let graph =
            crate::persistence::parse_document(&xml, crate::persistence::Vocabulary::Objects)?;
        let mut properties = graph.properties.iter().collect::<Vec<_>>();
        properties.sort_by_key(|property| property.xml.start());
        let mut requests = Vec::new();
        for property in properties {
            requests.extend(property.side_entries().iter().map(String::as_str));
        }
        let gui = self
            .side_entries
            .iter()
            .find(|(name, _)| name == "GuiDocument.xml");
        let gui_xml = gui
            .map(|(_, bytes)| {
                let text = std::str::from_utf8(bytes)
                    .map_err(|_| CodecError::Malformed("GuiDocument.xml is not UTF-8".into()))?;
                ctx.parse_xml(text, "source-less FreeCAD GUI request schedule")
            })
            .transpose()?;
        if let Some(gui_xml) = &gui_xml {
            requests.push("GuiDocument.xml");
            for node in gui_xml.descendants().filter(roxmltree::Node::is_element) {
                if node
                    .ancestors()
                    .find(|ancestor| ancestor.has_tag_name("Property"))
                    .and_then(|property| property.attribute("type"))
                    .is_some_and(crate::persistence::is_xlink_type)
                {
                    continue;
                }
                for attribute in node.attributes() {
                    if matches!(attribute.name(), "file" | "File") && !attribute.value().is_empty()
                    {
                        requests.push(attribute.value());
                    }
                }
            }
        }
        let by_name = self
            .side_entries
            .iter()
            .map(|entry| (entry.0.as_str(), entry))
            .collect::<BTreeMap<_, _>>();
        let mut requested = HashSet::new();
        let mut ordered = Vec::new();
        for name in requests {
            if !requested.insert(name) {
                return Err(CodecError::NotImplemented(format!(
                    "source-less FCStd side entry {name} has multiple restoration requests; supply distinct named payloads"
                )));
            }
            let entry = by_name.get(name).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "source-less FCStd restoration requests missing side entry {name}"
                ))
            })?;
            ordered.push(*entry);
        }
        ordered.extend(
            self.side_entries
                .iter()
                .filter(|(name, _)| !requested.contains(name.as_str())),
        );
        Ok(ordered)
    }

    fn document_xml(&self) -> String {
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        xml.push_str(
            "<Document SchemaVersion=\"4\" ProgramVersion=\"cadmpeg\" FileVersion=\"1\">\n",
        );
        xml.push_str("  <Properties Count=\"1\" TransientCount=\"0\">\n");
        xml.push_str(
            "    <Property name=\"Label\" type=\"App::PropertyString\" status=\"16777217\"><String value=\"",
        );
        crate::writer::escape_xml(&self.label, &mut xml, true);
        xml.push_str("\"/></Property>\n  </Properties>\n");
        let objects_open = format!(
            "  <Objects Count=\"{}\" Dependencies=\"1\">\n",
            self.objects.len()
        );
        xml.push_str(&objects_open);
        for object in &self.objects {
            xml.push_str("    <ObjectDeps Name=\"");
            crate::writer::escape_xml(&object.name, &mut xml, true);
            if object.dependencies.is_empty() {
                xml.push_str("\" Count=\"0\"/>\n");
            } else {
                let dependency_count = format!("\" Count=\"{}\">\n", object.dependencies.len());
                xml.push_str(&dependency_count);
                for dependency in &object.dependencies {
                    xml.push_str("      <Dep Name=\"");
                    crate::writer::escape_xml(dependency, &mut xml, true);
                    xml.push_str("\"/>\n");
                }
                xml.push_str("    </ObjectDeps>\n");
            }
        }
        for (index, object) in self.objects.iter().enumerate() {
            xml.push_str("    <Object type=\"");
            crate::writer::escape_xml(&object.type_name, &mut xml, true);
            xml.push_str("\" name=\"");
            crate::writer::escape_xml(&object.name, &mut xml, true);
            let object_id = format!("\" id=\"{}\" Touched=\"1\"/>\n", index + 1);
            xml.push_str(&object_id);
        }
        xml.push_str("  </Objects>\n");
        let object_data_open = format!("  <ObjectData Count=\"{}\">\n", self.objects.len());
        xml.push_str(&object_data_open);
        for object in &self.objects {
            xml.push_str("    <Object name=\"");
            crate::writer::escape_xml(&object.name, &mut xml, true);
            xml.push_str("\">\n");
            let properties_open = format!(
                "      <Properties Count=\"{}\" TransientCount=\"0\">\n",
                object.properties.len()
            );
            xml.push_str(&properties_open);
            for property in &object.properties {
                xml.push_str("        <Property name=\"");
                crate::writer::escape_xml(&property.name, &mut xml, true);
                xml.push_str("\" type=\"");
                crate::writer::escape_xml(&property.type_name, &mut xml, true);
                xml.push_str("\">");
                for value in &property.values {
                    write_value(value, &mut xml);
                }
                xml.push_str("</Property>\n");
            }
            xml.push_str("      </Properties>\n    </Object>\n");
        }
        xml.push_str("  </ObjectData>\n</Document>\n");
        xml
    }
}

fn write_value(value: &FcstdPropertyValue, xml: &mut String) {
    xml.push('<');
    xml.push_str(&value.tag);
    for (name, content) in &value.attributes {
        xml.push(' ');
        xml.push_str(name);
        xml.push_str("=\"");
        crate::writer::escape_xml(content, xml, true);
        xml.push('"');
    }
    if let Some(text) = &value.text {
        xml.push('>');
        crate::writer::escape_xml(text, xml, false);
        xml.push_str("</");
        xml.push_str(&value.tag);
        xml.push('>');
    } else if !value.children.is_empty() {
        xml.push('>');
        for child in &value.children {
            write_value(child, xml);
        }
        xml.push_str("</");
        xml.push_str(&value.tag);
        xml.push('>');
    } else {
        xml.push_str("/>");
    }
}

fn validate_value(value: &FcstdPropertyValue) -> Result<(), CodecError> {
    valid_identifier(&value.tag, "value tag")?;
    for attribute in value.attributes.keys() {
        valid_identifier(attribute, "value attribute")?;
    }
    if value.text.is_some() && !value.children.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "FCStd value {} cannot contain both text and child elements",
            value.tag
        )));
    }
    for child in &value.children {
        validate_value(child)?;
    }
    Ok(())
}

fn valid_identifier(value: &str, role: &str) -> Result<(), CodecError> {
    let mut characters = value.chars();
    let valid = characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| {
            character == '_'
                || character == '-'
                || character == '.'
                || character.is_ascii_alphanumeric()
        });
    if !valid {
        return Err(CodecError::malformed(format_args!(
            "invalid FCStd {role} identifier {value:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;
