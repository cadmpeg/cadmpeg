// SPDX-License-Identifier: Apache-2.0
//! `FCStd` output authored from CADIR, with an admitted unchanged layout path.
//!
//! What to write is decided in [`target`], which is the one resolution gate this
//! codec has. This module carries out what that gate settled, and gates nothing
//! of its own.

mod attributes;
pub(crate) mod body_styles;
mod brep;
mod coverage;
mod design;
mod dispositions;
mod geometry;
mod names;
mod native;
mod native_geometry;
pub(crate) mod ordinals;
mod parameters;
mod pcurves;
mod presentation;
pub(crate) mod products;
mod synthesis;
mod synthesis_graph;
pub(crate) mod target;
pub(crate) mod xml;

use std::collections::{HashMap, HashSet};
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::CodecError;
use zip::write::SimpleFileOptions;

use crate::native::{EntryRecord, ExtensionRecord, ObjectRecord, PropertyBody, PropertyRecord};
use target::Resolution;

trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

fn write(output: &mut dyn Write, resolution: &Resolution<'_>) -> Result<WriteOutcome, CodecError> {
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
fn write_seekable(
    output: &mut dyn WriteSeek,
    resolution: &Resolution<'_>,
) -> Result<WriteOutcome, CodecError> {
    let target = resolution.target();
    let ir = resolution.ir();
    let namespace = resolution.namespace();
    let document = resolution.document();
    let schema_version = resolution.schema_version();
    let fidelity = resolution.fidelity();
    let mut entries = namespace.arena_as::<EntryRecord>("entries")?;
    let objects = namespace.arena_as::<ObjectRecord>("objects")?;
    let extensions = namespace.arena_as::<ExtensionRecord>("extensions")?;
    let properties = namespace.arena_as::<PropertyRecord>("properties")?;
    let archive_image = crate::source_archive::image(fidelity)?;
    let source_arena = cadmpeg_core::decode::DecodeArena::new();
    let source_policy = cadmpeg_core::decode::DecodePolicy::default();
    let (source_context, source_root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        archive_image,
        &source_arena,
        &source_policy,
    )?;
    let source_archive =
        crate::source_archive::OriginalArchive::new(&source_context, source_root, fidelity)?;
    let mut payloads = HashMap::new();
    for entry in &entries {
        if entry.id() != crate::native::native_id("entry", entry.name()) {
            return Err(CodecError::malformed(
                "current entry identity differs from its original archive member",
            ));
        }
        payloads.insert(
            entry.name().to_owned(),
            source_archive.open(&source_context, entry.name())?,
        );
    }
    validate_entry_names(&entries)?;
    let source_document = entries
        .iter()
        .find(|entry| entry.name() == "Document.xml")
        .ok_or_else(|| {
            CodecError::Malformed("FCStd native graph has no Document.xml entry".into())
        })?;
    let document_xml = payloads
        .get(source_document.name())
        .ok_or_else(|| {
            CodecError::UnsupportedContent("the source document image is unavailable".into())
        })?
        .window()
        .to_vec();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&document_xml, &arena, &policy)?;
    let written_xml = crate::container::admit_document(&ctx, &document_xml)?;
    let mut written_graph = crate::persistence::parse_document(
        &written_xml,
        crate::persistence::Vocabulary::from_declaration(schema_version)?,
    )?;
    validate_declarations(
        &objects,
        &extensions,
        &written_graph.objects,
        &written_graph.extensions,
    )?;
    let mut entry_views = std::collections::BTreeMap::new();
    for entry in &entries {
        entry_views.insert(entry.name().to_owned(), payloads[entry.name()]);
    }
    crate::persistence::resolve_side_entries(
        &mut written_graph.properties,
        &entry_views,
        &mut written_graph.losses,
    )?;
    validate_properties(&properties, &written_graph.properties)?;
    for property in &written_graph.properties {
        for entry in property.side_entries() {
            if !entries.iter().any(|candidate| candidate.name() == *entry) {
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
            (left.name() != "Document.xml", left.name())
                .cmp(&(right.name() != "Document.xml", right.name()))
        });
        for entry in &entries {
            archive
                .start_file(entry.name(), file_options)
                .map_err(|error| {
                    std::io::Error::other(format!("cannot write {}: {error}", entry.name()))
                })?;
            archive.write_all(if entry.name() == "Document.xml" {
                &document_xml
            } else {
                payloads[entry.name()].window()
            })?;
        }
        archive.finish().map_err(|error| {
            std::io::Error::other(format!("cannot finish FCStd archive: {error}"))
        })?;
    }
    let notes = vec![
        format!(
            "semantic FCStd archive written for {target} (SchemaVersion={} FileVersion={})",
            schema_version,
            document.file_version.as_str()
        ),
        "retained layout reused after current typed and neutral projections agreed".into(),
    ];
    Ok(WriteOutcome {
        geometry_empty: written_graph.objects.is_empty()
            && written_graph.properties.iter().all(|property| {
                property.type_name == "App::PropertyString"
                    && property.links().is_empty()
                    && property.side_entries().is_empty()
                    && matches!(property.values(), [value] if value.tag == "String" && value.parent.is_none())
            }),
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: ir.census(),
        },
        notes,
    })
}

struct WriteOutcome {
    geometry_empty: bool,
    census: cadmpeg_ir::report::export::EntityCensus,
    notes: Vec<String>,
}

fn validate_entry_names(entries: &[EntryRecord]) -> Result<(), CodecError> {
    let mut names = HashSet::new();
    for entry in entries {
        if !names.insert(entry.name()) {
            return Err(CodecError::malformed(format_args!(
                "duplicate FCStd output entry {}",
                entry.name()
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
                .find(|written| written.id() == expected.id())
                .is_some_and(|written| {
                    expected.id() == written.id()
                        && expected.name() == written.name()
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
        return Err(CodecError::UnsupportedContent(
            "object declaration edits require source-less graph regeneration".into(),
        ));
    }
    if expected_extensions.len() != written_extensions.len()
        || !expected_extensions.iter().all(|expected| {
            written_extensions
                .iter()
                .find(|written| written.id == expected.id)
                .is_some_and(|written| {
                    serde_json::to_value(written).ok() == serde_json::to_value(expected).ok()
                })
        })
    {
        return Err(CodecError::UnsupportedContent(
            "extension declaration edits require a typed serializer".into(),
        ));
    }
    Ok(())
}

fn validate_properties(
    expected: &[PropertyRecord],
    written: &[PropertyRecord],
) -> Result<(), CodecError> {
    let mut expected_by_id = HashMap::with_capacity(expected.len());
    for property in expected {
        if expected_by_id
            .insert(property.id.as_str(), property)
            .is_some()
        {
            return Err(CodecError::malformed(format_args!(
                "edited FCStd property graph has duplicate property {}",
                property.id
            )));
        }
    }
    let mut written_by_id = HashMap::with_capacity(written.len());
    for property in written {
        if written_by_id
            .insert(property.id.as_str(), property)
            .is_some()
        {
            return Err(CodecError::malformed(format_args!(
                "written FCStd property graph has duplicate property {}",
                property.id
            )));
        }
    }
    if expected_by_id.len() != written_by_id.len() {
        return Err(CodecError::UnsupportedContent(
            "edited FCStd property graph changes its record count".into(),
        ));
    }
    for (id, property) in expected_by_id {
        let Some(candidate) = written_by_id.get(id).copied() else {
            return Err(CodecError::UnsupportedContent(format!(
                "edited FCStd property {} is missing from the written graph",
                property.id
            )));
        };
        if !same_property_semantics(property, candidate) {
            return Err(CodecError::UnsupportedContent(format!(
                "edited FCStd property {} changes its admitted value or link graph",
                property.id
            )));
        }
    }
    Ok(())
}

fn same_property_semantics(left: &PropertyRecord, right: &PropertyRecord) -> bool {
    if left.owner != right.owner
        || left.name != right.name
        || left.type_name != right.type_name
        || left.family != right.family
        || left.status != right.status
        || left.order != right.order
        || left.is_transient() != right.is_transient()
    {
        return false;
    }
    match (&left.body, &right.body) {
        (PropertyBody::Transient, PropertyBody::Transient) => true,
        (PropertyBody::Unreadable(left), PropertyBody::Unreadable(right)) => left == right,
        (
            PropertyBody::Persisted {
                values: left_values,
                links: left_links,
                side_entries: left_side_entries,
                dynamic: left_dynamic,
            },
            PropertyBody::Persisted {
                values: right_values,
                links: right_links,
                side_entries: right_side_entries,
                dynamic: right_dynamic,
            },
        ) => {
            left_values.len() == right_values.len()
                && left_values.iter().zip(right_values).all(|(left, right)| {
                    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
                })
                && left_links == right_links
                && left_side_entries == right_side_entries
                && left_dynamic == right_dynamic
        }
        _ => false,
    }
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
