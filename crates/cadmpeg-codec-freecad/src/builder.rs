// SPDX-License-Identifier: Apache-2.0
//! Source-less schema-4 document construction.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{CodecEntry, CodecError, DecodeOptions};
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

    /// Construct a value element with escaped text content.
    pub fn text(tag: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            attributes: BTreeMap::new(),
            text: Some(text.into()),
            children: Vec::new(),
        }
    }

    /// Construct an empty value element.
    pub fn empty(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            attributes: BTreeMap::new(),
            text: None,
            children: Vec::new(),
        }
    }

    /// Add or replace an attribute.
    #[must_use]
    pub fn with_attribute(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(name.into(), value.into());
        self
    }

    /// Append a nested value element for list, map, placement, and similar families.
    #[must_use]
    pub fn with_child(mut self, child: Self) -> Self {
        self.children.push(child);
        self
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
            return Err(CodecError::Malformed(format!(
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

    /// Record an ordered dependency between two declared objects.
    pub fn add_dependency(
        &mut self,
        object: &str,
        dependency: &str,
    ) -> Result<&mut Self, CodecError> {
        if object == dependency {
            return Err(CodecError::Malformed(format!(
                "FCStd object {object} cannot depend on itself"
            )));
        }
        if !self
            .objects
            .iter()
            .any(|candidate| candidate.name == dependency)
        {
            return Err(CodecError::Malformed(format!(
                "missing FCStd dependency object {dependency}"
            )));
        }
        let target = self
            .objects
            .iter_mut()
            .find(|candidate| candidate.name == object)
            .ok_or_else(|| CodecError::Malformed(format!("missing FCStd object {object}")))?;
        if target.dependencies.iter().any(|name| name == dependency) {
            return Err(CodecError::Malformed(format!(
                "duplicate FCStd dependency {object} -> {dependency}"
            )));
        }
        target.dependencies.push(dependency.to_owned());
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
            .ok_or_else(|| CodecError::Malformed(format!("missing FCStd object {object}")))?;
        let name = name.into();
        valid_identifier(&name, "property")?;
        if target
            .properties
            .iter()
            .any(|property| property.name == name)
        {
            return Err(CodecError::Malformed(format!(
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

    /// Add a named logical archive entry for a file-backed property.
    pub fn add_side_entry(
        &mut self,
        name: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<&mut Self, CodecError> {
        let name = name.into();
        if name.is_empty()
            || name.starts_with('/')
            || name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || name == "Document.xml"
            || self.side_entries.iter().any(|(entry, _)| entry == &name)
        {
            return Err(CodecError::Malformed(format!(
                "invalid or duplicate source-less FCStd entry {name:?}"
            )));
        }
        self.side_entries.push((name, bytes.into()));
        Ok(self)
    }

    /// Materialize the source-less graph as fully decoded CADIR.
    pub fn build(self) -> Result<CadIr, CodecError> {
        let bytes = self.archive_bytes()?;
        FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .map(|result| result.ir)
    }

    fn archive_bytes(self) -> Result<Vec<u8>, CodecError> {
        let document = self.document_xml()?;
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
            for (name, bytes) in self.side_entries {
                archive
                    .start_file(name, options)
                    .map_err(|error| CodecError::Malformed(error.to_string()))?;
                archive.write_all(&bytes)?;
            }
            archive
                .finish()
                .map_err(|error| CodecError::Malformed(error.to_string()))?;
        }
        Ok(cursor.into_inner())
    }

    fn document_xml(&self) -> Result<String, CodecError> {
        let mut names = HashSet::new();
        if !self.objects.iter().all(|object| names.insert(&object.name)) {
            return Err(CodecError::Malformed("duplicate FCStd object name".into()));
        }
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
        writeln!(
            xml,
            "  <Objects Count=\"{}\" Dependencies=\"1\">",
            self.objects.len()
        )
        .expect("writing to String cannot fail");
        for object in &self.objects {
            xml.push_str("    <ObjectDeps Name=\"");
            crate::writer::escape_xml(&object.name, &mut xml, true);
            if object.dependencies.is_empty() {
                xml.push_str("\" Count=\"0\"/>\n");
            } else {
                writeln!(xml, "\" Count=\"{}\">", object.dependencies.len())
                    .expect("writing to String cannot fail");
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
            writeln!(xml, "\" id=\"{}\"/>", index + 1).expect("writing to String cannot fail");
        }
        xml.push_str("  </Objects>\n");
        writeln!(xml, "  <ObjectData Count=\"{}\">", self.objects.len())
            .expect("writing to String cannot fail");
        for object in &self.objects {
            xml.push_str("    <Object name=\"");
            crate::writer::escape_xml(&object.name, &mut xml, true);
            xml.push_str("\">\n");
            writeln!(
                xml,
                "      <Properties Count=\"{}\" TransientCount=\"0\">",
                object.properties.len()
            )
            .expect("writing to String cannot fail");
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
        Ok(xml)
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
        return Err(CodecError::Malformed(format!(
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
        return Err(CodecError::Malformed(format!(
            "invalid FCStd {role} identifier {value:?}"
        )));
    }
    Ok(())
}
