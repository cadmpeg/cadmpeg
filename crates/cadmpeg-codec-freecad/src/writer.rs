// SPDX-License-Identifier: Apache-2.0
//! Lossless retained-document serialization: the ZIP repack and the
//! `Document.xml` patch.
//!
//! What to write is decided in [`target`], which is the one resolution gate this
//! codec has. This module carries out what that gate settled, and gates nothing
//! of its own.

pub(crate) mod target;

use std::collections::HashSet;
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::CodecError;
use zip::write::SimpleFileOptions;

use crate::native::{EntryRecord, ExtensionRecord, ObjectRecord, PropertyRecord, ValueRecord};
use target::Resolution;

pub(crate) trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

pub(crate) fn write(
    output: &mut dyn Write,
    resolution: &Resolution<'_>,
) -> Result<WriteOutcome, CodecError> {
    let mut staged = tempfile::tempfile()?;
    let report = write_seekable(&mut staged, resolution)?;
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, output)?;
    Ok(report)
}

/// Repack the retained entry set with a patched `Document.xml`.
///
/// The replay law is already settled when this runs. A [`Resolution`] comes only
/// from [`resolve`], which takes its options from [`retained_baseline`], so the
/// dialect written here is the one the retained document already declares.
/// This function carries out that decision; it does not gate it.
pub(crate) fn write_seekable(
    output: &mut dyn WriteSeek,
    resolution: &Resolution<'_>,
) -> Result<WriteOutcome, CodecError> {
    let target = resolution.target();
    let ir = resolution.ir();
    let namespace = resolution.namespace();
    let document = resolution.document();
    let entry_records = namespace
        .arenas
        .get("entries")
        .map_or(&[][..], Vec::as_slice);
    let mut entries = entry_records
        .iter()
        .enumerate()
        .map(|(record_index, record)| {
            record
                .field("name")
                .and_then(|name| name.as_str().map(str::to_owned))
                .map(|name| EntrySlot { record_index, name })
                .ok_or_else(|| {
                    CodecError::Malformed("FCStd entry record has no string name".into())
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let objects = namespace.arena_as::<ObjectRecord>("objects")?;
    let extensions = namespace.arena_as::<ExtensionRecord>("extensions")?;
    let properties = namespace.arena_as::<PropertyRecord>("properties")?;
    validate_entry_names(&entries)?;
    let source_document_slot = entries
        .iter()
        .find(|entry| entry.name == "Document.xml")
        .ok_or_else(|| {
            CodecError::Malformed("FCStd native graph has no Document.xml entry".into())
        })?;
    let source_document = entry_at(namespace, source_document_slot.record_index)?;
    let document_xml = patch_document(&source_document.data, &properties)?;
    drop(source_document);
    let written_graph = crate::persistence::parse_with_context(&document_xml, document, None)?;
    validate_declarations(
        &objects,
        &extensions,
        &written_graph.objects,
        &written_graph.extensions,
    )?;
    for property in &written_graph.properties {
        for entry in &property.side_entries {
            if !entries.iter().any(|candidate| candidate.name == *entry) {
                return Err(CodecError::malformed(format_args!(
                    "edited property {} references missing side entry {entry}",
                    property.id
                )));
            }
        }
    }

    {
        let mut archive = zip::ZipWriter::new(&mut *output);
        let file_options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        entries.sort_by(|left, right| {
            (left.name != "Document.xml", left.name.as_str())
                .cmp(&(right.name != "Document.xml", right.name.as_str()))
        });
        for slot in &entries {
            let entry = entry_at(namespace, slot.record_index)?;
            archive
                .start_file(&entry.name, file_options)
                .map_err(|error| {
                    CodecError::malformed(format_args!("cannot write {}: {error}", entry.name))
                })?;
            archive.write_all(if entry.name == "Document.xml" {
                &document_xml
            } else {
                &entry.data
            })?;
        }
        archive.finish().map_err(|error| {
            CodecError::malformed(format_args!("cannot finish FCStd archive: {error}"))
        })?;
    }
    let notes = vec![
        format!(
            "semantic FCStd archive written for {target} (SchemaVersion={} FileVersion={})",
            document.schema_version, document.file_version
        ),
        "unsupported retained entries and unedited XML records were preserved".into(),
    ];
    Ok(WriteOutcome {
        census: cadmpeg_ir::EntityCensus {
            basis: cadmpeg_ir::CensusBasis::IrArenas,
            counts: ir.census(),
        },
        notes,
    })
}

pub(crate) struct WriteOutcome {
    pub(crate) census: cadmpeg_ir::EntityCensus,
    pub(crate) notes: Vec<String>,
}

struct EntrySlot {
    record_index: usize,
    name: String,
}

fn entry_at(
    namespace: &cadmpeg_ir::native::NativeNamespace,
    index: usize,
) -> Result<EntryRecord, CodecError> {
    namespace
        .arena_iter_as::<EntryRecord>("entries")
        .nth(index)
        .ok_or_else(|| CodecError::Malformed("FCStd entry record disappeared".into()))?
        .map_err(CodecError::from)
}

fn validate_entry_names(entries: &[EntrySlot]) -> Result<(), CodecError> {
    let mut names = HashSet::new();
    for entry in entries {
        if entry.name.is_empty()
            || entry.name.starts_with('/')
            || entry
                .name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(CodecError::malformed(format_args!(
                "unsafe FCStd output entry name {:?}",
                entry.name
            )));
        }
        if !names.insert(entry.name.as_str()) {
            return Err(CodecError::malformed(format_args!(
                "duplicate FCStd output entry {}",
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
                        && expected.dependency_allow_partial == written.dependency_allow_partial
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
            return Err(CodecError::malformed(format_args!(
                "invalid retained span for property {}",
                property.id
            )));
        }
        let retained = source_text.get(start..end).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "retained span for property {} is not on UTF-8 boundaries",
                property.id
            ))
        })?;
        if retained != property.raw_xml {
            return Err(CodecError::malformed(format_args!(
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
    let wrapped = format!("<Root>{}</Root>", property.raw_xml);
    let parsed = roxmltree::Document::parse(&wrapped).map_err(|error| {
        CodecError::malformed(format_args!("invalid retained property XML: {error}"))
    })?;
    let source_ranges = parsed
        .root_element()
        .first_element_child()
        .into_iter()
        .flat_map(|property| {
            property
                .descendants()
                .filter(move |node| node.is_element() && *node != property)
        })
        .map(|node| (node.range().start - 6, node.range().end - 6))
        .collect::<Vec<_>>();
    if source_ranges.len() != property.values.len() {
        return Err(CodecError::malformed(format_args!(
            "property {} value provenance count changed",
            property.id
        )));
    }
    let mut edits = Vec::new();
    for (value, (start, end)) in property.values.iter().zip(source_ranges) {
        let serialized = serialize_value(value)?;
        if serialized == value.raw_xml {
            continue;
        }
        if property.raw_xml[start..end] != value.raw_xml {
            return Err(CodecError::malformed(format_args!(
                "property {} retained value {} disagrees with provenance",
                property.id, value.order
            )));
        }
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
        CodecError::malformed(format_args!("invalid retained property XML: {error}"))
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
        CodecError::malformed(format_args!("invalid retained property value XML: {error}"))
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
            '\t' => output.push_str("&#9;"),
            '\n' => output.push_str("&#10;"),
            '\r' => output.push_str("&#13;"),
            other => output.push(other),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
