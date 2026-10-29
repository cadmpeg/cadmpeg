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
        let mut properties = graph
            .properties
            .iter()
            .map(|property| {
                graph
                    .property_sources
                    .get(&property.id)
                    .map(|source| (source.record_range().start, property))
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "property {} has no original source range",
                            property.id
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        properties.sort_by_key(|(start, _)| *start);
        let mut requests = Vec::new();
        for (_, property) in properties {
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
        let properties = [(
            "Label".to_owned(),
            crate::writer::xml::string("Label", &self.label),
        )]
        .into();
        let objects = self
            .objects
            .iter()
            .map(|object| {
                let mut output = crate::writer::xml::Object::new(
                    object.name.clone(),
                    &object.type_name,
                    &object.name,
                );
                output.properties.clear();
                output.dependencies.clone_from(&object.dependencies);
                output
                    .declaration_attributes
                    .insert("Touched".to_owned(), "1".to_owned());
                for property in &object.properties {
                    let mut content = String::new();
                    for value in &property.values {
                        write_value(value, &mut content);
                    }
                    output.set(
                        &property.name,
                        crate::writer::xml::property(&property.name, &property.type_name, &content),
                    );
                }
                output
            })
            .collect::<Vec<_>>();
        document_xml(&objects, &properties, "4")
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

fn properties_xml(properties: &BTreeMap<String, String>) -> String {
    let transient = properties
        .values()
        .filter(|value| value.starts_with("<_Property "))
        .count();
    let persisted = properties
        .values()
        .filter(|value| !value.starts_with("<_Property "))
        .count();
    let declaration = format!("<Properties Count=\"{persisted}\" TransientCount=\"{transient}\">");
    format!(
        "{declaration}{}</Properties>",
        properties
            .values()
            .filter(|property| property.starts_with("<_Property"))
            .chain(
                properties
                    .values()
                    .filter(|property| !property.starts_with("<_Property"))
            )
            .cloned()
            .collect::<String>()
    )
}

pub(crate) fn document_xml(
    objects: &[crate::writer::xml::Object],
    properties: &BTreeMap<String, String>,
    schema: &str,
) -> String {
    let (objects_tag, data_tag, object_tag) = if schema == "2" {
        ("Features", "FeatureData", "Feature")
    } else {
        ("Objects", "ObjectData", "Object")
    };
    let mut result = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Document SchemaVersion=\"{schema}\" FileVersion=\"1\" ProgramVersion=\"cadmpeg\" StringHasher=\"1\"><StringHasher saveall=\"0\" threshold=\"0\" count=\"0\"></StringHasher>{}<{objects_tag} Count=\"{}\"{}>", properties_xml(properties), objects.len(), if schema == "2" { "" } else { " Dependencies=\"1\"" });
    if schema != "2" {
        for object in objects {
            result.push_str("<ObjectDeps Name=\"");
            result.push_str(&crate::writer::xml::escaped(&object.name));
            result.push_str("\" Count=\"");
            result.push_str(&object.dependencies.len().to_string());
            result.push_str("\">");
            for dependency in &object.dependencies {
                result.push_str(&crate::writer::xml::element(
                    "Dep",
                    &[("Name", dependency.clone())],
                ));
            }
            result.push_str("</ObjectDeps>");
        }
    }
    for (index, object) in objects.iter().enumerate() {
        result.push('<');
        result.push_str(object_tag);
        result.push_str(" name=\"");
        result.push_str(&crate::writer::xml::escaped(&object.name));
        result.push_str("\" type=\"");
        result.push_str(&crate::writer::xml::escaped(&object.kind));
        result.push_str("\" id=\"");
        result.push_str(&(index + 1).to_string());
        result.push('"');
        for (name, value) in &object.declaration_attributes {
            result.push(' ');
            result.push_str(name);
            result.push_str("=\"");
            result.push_str(&crate::writer::xml::escaped(value));
            result.push('"');
        }
        result.push_str("/>");
    }
    result.push_str("</");
    result.push_str(objects_tag);
    result.push_str("><");
    result.push_str(data_tag);
    result.push_str(" Count=\"");
    result.push_str(&objects.len().to_string());
    result.push_str("\">");
    for object in objects {
        result.push('<');
        result.push_str(object_tag);
        result.push_str(" name=\"");
        result.push_str(&crate::writer::xml::escaped(&object.name));
        result.push('"');
        if !object.extensions.is_empty() {
            result.push_str(" Extensions=\"True\"");
        }
        result.push('>');
        if !object.extensions.is_empty() {
            result.push_str("<Extensions Count=\"");
            result.push_str(&object.extensions.len().to_string());
            result.push_str("\">");
            result.push_str(&object.extensions.join(""));
            result.push_str("</Extensions>");
        }
        result.push_str(&properties_xml(&object.properties));
        result.push_str("</");
        result.push_str(object_tag);
        result.push('>');
    }
    result.push_str("</");
    result.push_str(data_tag);
    result.push_str("></Document>");
    result
}
