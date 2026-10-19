// SPDX-License-Identifier: Apache-2.0
//! Source-less schema-4 document construction.

use std::collections::BTreeMap;
use std::fmt::Write as _;
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
            write!(
                result,
                "<ObjectDeps Name=\"{}\" Count=\"{}\">",
                crate::writer::xml::escaped(&object.name),
                object.dependencies.len()
            )
            .expect("writing to String cannot fail");
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
        write!(
            result,
            "<{object_tag} name=\"{}\" type=\"{}\" id=\"{}\"{}/>",
            crate::writer::xml::escaped(&object.name),
            crate::writer::xml::escaped(&object.kind),
            index + 1,
            object
                .declaration_attributes
                .iter()
                .map(|(name, value)| format!(
                    " {}=\"{}\"",
                    name,
                    crate::writer::xml::escaped(value)
                ))
                .collect::<String>()
        )
        .expect("writing to String cannot fail");
    }
    write!(
        result,
        "</{objects_tag}><{data_tag} Count=\"{}\">",
        objects.len()
    )
    .expect("writing to String cannot fail");
    for object in objects {
        write!(
            result,
            "<{object_tag} name=\"{}\"{}>{}{}</{object_tag}>",
            crate::writer::xml::escaped(&object.name),
            if object.extensions.is_empty() {
                String::new()
            } else {
                " Extensions=\"True\"".to_owned()
            },
            if object.extensions.is_empty() {
                String::new()
            } else {
                format!(
                    "<Extensions Count=\"{}\">{}</Extensions>",
                    object.extensions.len(),
                    object.extensions.join("")
                )
            },
            properties_xml(&object.properties)
        )
        .expect("writing to String cannot fail");
    }
    write!(result, "</{data_tag}></Document>").expect("writing to String cannot fail");
    result
}
