// SPDX-License-Identifier: Apache-2.0
//! `FCStd` output authored from CADIR, with an admitted unchanged layout path.
//!
//! What to write is decided in [`target`], which is the one resolution gate this
//! codec has. This module carries out what that gate settled, and gates nothing
//! of its own.

mod attributes;
mod brep;
mod coverage;
mod design;
mod dispositions;
mod drawings;
mod geometry;
mod indexed_polygon;
mod mesh_kernel;
mod names;
mod native;
mod native_geometry;
mod parameters;
mod pcurves;
mod point_kernel;
mod polygon_edges;
mod polygon_nodes;
mod polygon_pair;
mod polygon_sources;
mod presentation;
pub(crate) mod products;
mod synthesis;
mod synthesis_graph;
pub(crate) mod target;
pub(crate) mod xml;

use std::collections::{HashMap, HashSet};
use std::io::Write;

use cadmpeg_core::CodecError;
use zip::write::SimpleFileOptions;

use crate::native::{EntryRecord, ExtensionRecord, ObjectRecord, PropertyBody, PropertyRecord};
use target::Resolution;

/// `FreeCAD` has no fixed-width field; every count is an XML attribute or a
/// ZIP member, so there is no capacity row (#238).
pub(crate) const CAPACITY_FIELDS: &[cadmpeg_ir::codec::write::CapacityField] = &[];

/// Normalization rows: neutral paths `FreeCAD` stores in reduced form, each
/// with the loss the writer charges (#238 §5.5, #232 §5.2). A same-format
/// round trip never reaches one, because a `FreeCAD`-sourced document already
/// satisfies `FreeCAD`'s constraints; they occur only cross-format and under
/// §5.1(c) edits.
pub(crate) const REDUCED_FIELDS: &[cadmpeg_ir::codec::write::ReducedField] = &[
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.bodies[].name",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/body.name-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.bodies[].color",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/appearance.body-style-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.bodies[].visible",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/appearance.body-style-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.bodies[].color.r",
        storage: cadmpeg_ir::codec::write::ReducedStorage::U8Channel,
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.bodies[].color.g",
        storage: cadmpeg_ir::codec::write::ReducedStorage::U8Channel,
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.bodies[].color.b",
        storage: cadmpeg_ir::codec::write::ReducedStorage::U8Channel,
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.bodies[].color.a",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Decimal(2),
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.product_definitions[].label",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/product.label-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.product_definitions[].source_name",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/product.source-name-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.occurrences[].linked_prototype",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/product.linked-placement-folded",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.features[].source_tag",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/feature.source-tag-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.parameters[].name",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/parameter.name-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.parameters[].driving",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/parameter.driving-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.parameters[].expression",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/parameter.expression-normalized",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.occurrences[]",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/product.placement-not-rigid",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.assembly_joints[].operands",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/joint.frame-not-rigid",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.sketches[].placement",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/sketch.placement-normalized",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.sketches[].profiles",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/sketch.profile-orientation-derived",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.presentation_documents[].schema_version",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/presentation.schema-normalized",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.presentation_documents[].states",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/presentation.state-order-normalized",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.view_presentations[]",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/presentation.view-mode-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.appearances[]",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/appearance.name-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.appearance_bindings[]",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/appearance.channel-derived",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.semantic_annotations[].parameters",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/drawing.parameter-not-transferred",
        },
    },
    cadmpeg_ir::codec::write::ReducedField {
        path: "model.semantic_annotations[]",
        storage: cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "fcstd/annotation.position-planar",
        },
    },
];

#[derive(Debug)]
enum PatchError {
    Declined(String),
    Codec(CodecError),
}

impl From<CodecError> for PatchError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<cadmpeg_ir::native::NativeConvertError> for PatchError {
    fn from(error: cadmpeg_ir::native::NativeConvertError) -> Self {
        Self::Codec(error.into())
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for PatchError {
    fn from(error: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Codec(error.into())
    }
}

impl From<std::io::Error> for PatchError {
    fn from(error: std::io::Error) -> Self {
        Self::Codec(error.into())
    }
}

/// Repack the retained entry set with a patched `Document.xml`.
///
/// The replay law is already settled when this runs. A [`Resolution`] comes only
/// from [`resolve`], which takes its options from [`retained_baseline`], so the
/// dialect written here is the one the retained document already declares.
/// This function carries out that decision; it does not gate it.
fn write_seekable(
    output: &mut std::io::Cursor<Vec<u8>>,
    resolution: &Resolution<'_>,
    encode_context: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<WriteOutcome, PatchError> {
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
    let session = cadmpeg_core::encode::EncodeSession::from_context(encode_context)?;
    let (source_context, source_root) =
        cadmpeg_core::decode::DecodeContext::for_encode_session_root(
            &source_arena,
            &session,
            archive_image,
        )?;
    let source_archive = cadmpeg_container::ArchiveSnapshot::new(&source_context, source_root)?;
    let mut payloads = HashMap::new();
    for entry in &entries {
        if entry.id() != crate::native::native_id("entry", entry.name()) {
            return Err(PatchError::Declined(
                "current entry identity differs from its original archive member".into(),
            ));
        }
        payloads.insert(
            entry.name().to_owned(),
            source_archive.open(&source_context, entry.name())?,
        );
    }
    validate_entry_names(&entries)?;
    let source_offsets = source_archive
        .entries()
        .iter()
        .map(|entry| (entry.name.as_str(), entry.header_start))
        .collect::<HashMap<_, _>>();
    let source_document = entries
        .iter()
        .find(|entry| entry.name() == "Document.xml")
        .ok_or_else(|| PatchError::Declined("the native graph has no Document.xml entry".into()))?;
    let document_xml = payloads
        .get(source_document.name())
        .ok_or_else(|| PatchError::Declined("the source document image is unavailable".into()))?
        .window()
        .to_vec();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::for_encode_session_root(
        &arena,
        &session,
        &document_xml,
    )?;
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
                return Err(PatchError::Declined(format!(
                    "edited property {} references missing side entry {entry}",
                    property.id
                )));
            }
        }
    }

    session.finish()?;
    {
        let mut archive = zip::ZipWriter::new(&mut *output);
        let file_options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        restore_entry_order(&mut entries, &source_offsets)?;
        for entry in &entries {
            archive
                .start_file(entry.name(), file_options)
                .map_err(|error| {
                    PatchError::Declined(format!("cannot write {}: {error}", entry.name()))
                })?;
            archive.write_all(if entry.name() == "Document.xml" {
                &document_xml
            } else {
                payloads[entry.name()].window()
            })?;
        }
        archive.finish().map_err(|error| {
            PatchError::Declined(format!("cannot finish FCStd archive: {error}"))
        })?;
    }
    let notes = vec![
        format!(
            "semantic FCStd archive written for {target} (SchemaVersion={} FileVersion={})",
            schema_version,
            document.file_version.as_str()
        ),
        "retained layout reused for an unedited document".into(),
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

fn restore_entry_order(
    entries: &mut [EntryRecord],
    source_offsets: &HashMap<&str, u64>,
) -> Result<(), PatchError> {
    for entry in &*entries {
        if !source_offsets.contains_key(entry.name()) {
            return Err(PatchError::Declined(format!(
                "output entry {} has no retained restoration order",
                entry.name()
            )));
        }
    }
    entries.sort_by_key(|entry| (entry.name() != "Document.xml", source_offsets[entry.name()]));
    Ok(())
}

fn validate_entry_names(entries: &[EntryRecord]) -> Result<(), PatchError> {
    let mut names = HashSet::new();
    for entry in entries {
        if !names.insert(entry.name()) {
            return Err(PatchError::Declined(format!(
                "duplicate output entry {}",
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
) -> Result<(), PatchError> {
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
        return Err(PatchError::Declined(
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
        return Err(PatchError::Declined(
            "extension declaration edits require a typed serializer".into(),
        ));
    }
    Ok(())
}

fn validate_properties(
    expected: &[PropertyRecord],
    written: &[PropertyRecord],
) -> Result<(), PatchError> {
    let mut expected_by_id = HashMap::with_capacity(expected.len());
    for property in expected {
        if expected_by_id
            .insert(property.id.as_str(), property)
            .is_some()
        {
            return Err(PatchError::Declined(format!(
                "edited property graph has duplicate property {}",
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
            return Err(PatchError::Declined(format!(
                "written property graph has duplicate property {}",
                property.id
            )));
        }
    }
    if expected_by_id.len() != written_by_id.len() {
        return Err(PatchError::Declined(
            "edited FCStd property graph changes its record count".into(),
        ));
    }
    for (id, property) in expected_by_id {
        let Some(candidate) = written_by_id.get(id).copied() else {
            return Err(PatchError::Declined(format!(
                "edited FCStd property {} is missing from the written graph",
                property.id
            )));
        };
        if !same_property_semantics(property, candidate) {
            return Err(PatchError::Declined(format!(
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
        || left.dynamic != right.dynamic
        || left.order != right.order
        || left.is_transient() != right.is_transient()
    {
        return false;
    }
    match (&left.body, &right.body) {
        (None, None) => true,
        (Some(PropertyBody::Transient), Some(PropertyBody::Transient)) => true,
        (
            Some(PropertyBody::Persisted {
                values: left_values,
                links: left_links,
                side_entries: left_side_entries,
            }),
            Some(PropertyBody::Persisted {
                values: right_values,
                links: right_links,
                side_entries: right_side_entries,
            }),
        ) => {
            left_values.len() == right_values.len()
                && left_values.iter().zip(right_values).all(|(left, right)| {
                    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
                })
                && left_links == right_links
                && left_side_entries == right_side_entries
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
