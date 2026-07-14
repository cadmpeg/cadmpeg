// SPDX-License-Identifier: Apache-2.0
//! Lossless retained-document serialization.

use std::collections::HashSet;
use std::io::{Cursor, Write};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::ExportReport;
use zip::write::SimpleFileOptions;

use crate::native::{
    DocumentFacts, EntryRecord, ExtensionRecord, ObjectRecord, PropertyRecord, ValueRecord,
};
use crate::FcstdWriteOptions;

pub(crate) fn write(
    ir: &CadIr,
    output: &mut dyn Write,
    options: FcstdWriteOptions,
) -> Result<ExportReport, CodecError> {
    if (options.schema_version, options.file_version) != (4, 1) {
        return Err(CodecError::NotImplemented(format!(
            "FCStd write target SchemaVersion={} FileVersion={}",
            options.schema_version, options.file_version
        )));
    }
    let namespace = ir.native.namespace("fcstd").ok_or_else(|| {
        CodecError::NotImplemented(
            "source-less FCStd generation requires a constructed native document graph".into(),
        )
    })?;
    let documents = namespace.arena_as::<DocumentFacts>("document")?;
    let document = exactly_one(&documents, "document record")?;
    if document.schema_version != options.schema_version.to_string()
        || document.file_version != options.file_version.to_string()
    {
        return Err(CodecError::NotImplemented(format!(
            "cannot transcode retained SchemaVersion={} FileVersion={} to SchemaVersion={} FileVersion={}",
            document.schema_version,
            document.file_version,
            options.schema_version,
            options.file_version
        )));
    }
    let entries = namespace.arena_as::<EntryRecord>("entries")?;
    let objects = namespace.arena_as::<ObjectRecord>("objects")?;
    let extensions = namespace.arena_as::<ExtensionRecord>("extensions")?;
    let properties = namespace.arena_as::<PropertyRecord>("properties")?;
    validate_entries(&entries)?;
    let source_document = entries
        .iter()
        .find(|entry| entry.name == "Document.xml")
        .ok_or_else(|| {
            CodecError::Malformed("FCStd native graph has no Document.xml entry".into())
        })?;
    let document_xml = patch_document(&source_document.data, &properties)?;
    let written_graph = crate::persistence::parse(&document_xml)?;
    validate_declarations(
        &objects,
        &extensions,
        &written_graph.objects,
        &written_graph.extensions,
    )?;
    for property in &written_graph.properties {
        for entry in &property.side_entries {
            if !entries.iter().any(|candidate| candidate.name == *entry) {
                return Err(CodecError::Malformed(format!(
                    "edited property {} references missing side entry {entry}",
                    property.id
                )));
            }
        }
    }

    let mut archive_bytes = Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut archive_bytes);
        let file_options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        let mut ordered_entries = entries.iter().collect::<Vec<_>>();
        ordered_entries.sort_by_key(|entry| (entry.name != "Document.xml", entry.name.as_str()));
        for entry in ordered_entries {
            archive
                .start_file(&entry.name, file_options)
                .map_err(|error| {
                    CodecError::Malformed(format!("cannot write {}: {error}", entry.name))
                })?;
            archive.write_all(if entry.name == "Document.xml" {
                &document_xml
            } else {
                &entry.data
            })?;
        }
        archive.finish().map_err(|error| {
            CodecError::Malformed(format!("cannot finish FCStd archive: {error}"))
        })?;
    }
    output.write_all(archive_bytes.get_ref())?;

    let validation = cadmpeg_ir::validate(ir, Vec::new());
    let total_entities = validation.entity_counts.values().sum();
    Ok(ExportReport {
        format: "fcstd".into(),
        entity_counts: validation.entity_counts,
        total_entities,
        losses: Vec::new(),
        notes: vec![
            format!(
                "semantic FCStd archive written for SchemaVersion={} FileVersion={}",
                options.schema_version, options.file_version
            ),
            "unsupported retained entries and unedited XML records were preserved".into(),
        ],
    })
}

fn exactly_one<'a, T>(values: &'a [T], description: &str) -> Result<&'a T, CodecError> {
    if values.len() != 1 {
        return Err(CodecError::Malformed(format!(
            "FCStd native graph must contain exactly one {description}"
        )));
    }
    Ok(&values[0])
}

fn validate_entries(entries: &[EntryRecord]) -> Result<(), CodecError> {
    let mut names = HashSet::new();
    for entry in entries {
        if entry.name.is_empty()
            || entry.name.starts_with('/')
            || entry
                .name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(CodecError::Malformed(format!(
                "unsafe FCStd output entry name {:?}",
                entry.name
            )));
        }
        if !names.insert(entry.name.as_str()) {
            return Err(CodecError::Malformed(format!(
                "duplicate FCStd output entry {}",
                entry.name
            )));
        }
        if entry.byte_len != entry.data.len() as u64 || entry.sha256 != sha256_hex(&entry.data) {
            return Err(CodecError::Malformed(format!(
                "FCStd output entry {} has stale length or digest metadata",
                entry.name
            )));
        }
    }
    Ok(())
}

fn validate_declarations(
    expected_objects: &[ObjectRecord],
    expected_extensions: &[ExtensionRecord],
    written_objects: &[ObjectRecord],
    written_extensions: &[ExtensionRecord],
) -> Result<(), CodecError> {
    if expected_objects.len() != written_objects.len()
        || !expected_objects.iter().all(|expected| {
            written_objects
                .iter()
                .find(|written| written.id == expected.id)
                .is_some_and(|written| {
                    expected.id == written.id
                        && expected.name == written.name
                        && expected.type_name == written.type_name
                        && expected.persistent_id == written.persistent_id
                        && expected.view_type == written.view_type
                        && expected.attributes == written.attributes
                        && expected.dependencies == written.dependencies
                        && expected.order == written.order
                })
        })
    {
        return Err(CodecError::NotImplemented(
            "object declaration edits require source-less graph regeneration".into(),
        ));
    }
    if expected_extensions.len() != written_extensions.len()
        || !expected_extensions.iter().all(|expected| {
            written_extensions
                .iter()
                .find(|written| written.id == expected.id)
                == Some(expected)
        })
    {
        return Err(CodecError::NotImplemented(
            "extension declaration edits require a typed serializer".into(),
        ));
    }
    Ok(())
}

fn patch_document(source: &[u8], properties: &[PropertyRecord]) -> Result<Vec<u8>, CodecError> {
    let source_text = std::str::from_utf8(source)
        .map_err(|_| CodecError::Malformed("retained Document.xml is not UTF-8".into()))?;
    let mut ordered = properties.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|property| property.byte_start);
    if ordered
        .windows(2)
        .any(|pair| pair[0].byte_end > pair[1].byte_start)
    {
        return Err(CodecError::Malformed(
            "overlapping retained FCStd property spans".into(),
        ));
    }
    let mut result = Vec::with_capacity(source.len());
    let mut cursor = 0usize;
    for property in ordered {
        let start = usize::try_from(property.byte_start)
            .map_err(|_| CodecError::Malformed("property start exceeds address space".into()))?;
        let end = usize::try_from(property.byte_end)
            .map_err(|_| CodecError::Malformed("property end exceeds address space".into()))?;
        if start < cursor || end > source.len() || start >= end {
            return Err(CodecError::Malformed(format!(
                "invalid retained span for property {}",
                property.id
            )));
        }
        if source_text[start..end] != property.raw_xml {
            return Err(CodecError::Malformed(format!(
                "retained bytes disagree with property {} provenance",
                property.id
            )));
        }
        result.extend_from_slice(&source[cursor..start]);
        result.extend_from_slice(&serialize_property(property)?);
        cursor = end;
    }
    result.extend_from_slice(&source[cursor..]);
    Ok(result)
}

fn serialize_property(property: &PropertyRecord) -> Result<Vec<u8>, CodecError> {
    validate_property_wrapper(property)?;
    let mut replacement = property.raw_xml.clone();
    let mut edits = Vec::new();
    for value in &property.values {
        let serialized = serialize_value(value)?;
        if serialized == value.raw_xml {
            continue;
        }
        let start = property.raw_xml.find(&value.raw_xml).ok_or_else(|| {
            CodecError::Malformed(format!(
                "property {} no longer contains retained value {}",
                property.id, value.order
            ))
        })?;
        let end = start + value.raw_xml.len();
        edits.push((start, end, serialized));
    }
    edits.sort_by_key(|(start, _, _)| *start);
    if edits.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(CodecError::NotImplemented(format!(
            "overlapping edits in nested FCStd property {}",
            property.id
        )));
    }
    for (start, end, serialized) in edits.into_iter().rev() {
        replacement.replace_range(start..end, &serialized);
    }
    Ok(replacement.into_bytes())
}

fn validate_property_wrapper(property: &PropertyRecord) -> Result<(), CodecError> {
    let wrapped = format!("<Root>{}</Root>", property.raw_xml);
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::Malformed(format!("invalid retained property XML: {error}"))
    })?;
    let element = parsed
        .root_element()
        .first_element_child()
        .ok_or_else(|| CodecError::Malformed("retained property has no element".into()))?;
    let expected_tag = if property.transient {
        "_Property"
    } else {
        "Property"
    };
    let status = element
        .attribute("status")
        .map(str::parse::<u64>)
        .transpose()
        .map_err(|_| CodecError::Malformed("retained property has invalid status".into()))?;
    if element.tag_name().name() != expected_tag
        || element.attribute("name") != Some(property.name.as_str())
        || element.attribute("type") != Some(property.type_name.as_str())
        || status != property.status
    {
        return Err(CodecError::NotImplemented(format!(
            "editing FCStd property declaration {} requires a typed serializer",
            property.id
        )));
    }
    Ok(())
}

fn serialize_value(value: &ValueRecord) -> Result<String, CodecError> {
    let wrapped = format!("<Root>{}</Root>", value.raw_xml);
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::Malformed(format!("invalid retained property value XML: {error}"))
    })?;
    let original = parsed
        .root_element()
        .first_element_child()
        .ok_or_else(|| CodecError::Malformed("retained property value has no element".into()))?;
    let original_attributes = original
        .attributes()
        .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let original_text = original
        .children()
        .find_map(|node| node.text())
        .map(str::to_owned);
    if original.tag_name().name() == value.tag
        && original_attributes == value.attributes
        && original_text == value.text
    {
        return Ok(value.raw_xml.clone());
    }
    if original.children().any(|node| node.is_element()) {
        return Err(CodecError::NotImplemented(format!(
            "editing nested FCStd value element {} requires a typed serializer",
            value.tag
        )));
    }
    let mut serialized = String::new();
    serialized.push('<');
    serialized.push_str(&value.tag);
    for (name, content) in &value.attributes {
        serialized.push(' ');
        serialized.push_str(name);
        serialized.push_str("=\"");
        escape_xml(content, &mut serialized, true);
        serialized.push('"');
    }
    match &value.text {
        Some(text) => {
            serialized.push('>');
            escape_xml(text, &mut serialized, false);
            serialized.push_str("</");
            serialized.push_str(&value.tag);
            serialized.push('>');
        }
        None => serialized.push_str("/>"),
    }
    Ok(serialized)
}

pub(crate) fn escape_xml(value: &str, output: &mut String, attribute: bool) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' if attribute => output.push_str("&quot;"),
            '\'' if attribute => output.push_str("&apos;"),
            other => output.push(other),
        }
    }
}
