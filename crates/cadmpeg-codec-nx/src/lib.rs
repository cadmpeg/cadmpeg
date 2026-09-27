// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read Siemens NX `.prt` files into [`cadmpeg_ir::document::CadIr`].
//!
//! [`NxCodec`] is the normal public decode API. A hidden `fuzz` module
//! exposes `()`-returning parser wrappers. The codec recognizes the `SPLMSSTR`
//! container signature, extracts compressed Parasolid neutral-binary streams
//! from the canonical part payload, and decodes supported geometry and
//! topology. Detection uses file content because NX and Creo share the `.prt`
//! extension.
//!
//! <!-- generated: capability nx -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#siemens-nx-prt)).
//! <!-- /generated: capability nx -->
//!
//! Connected B-rep on selected or terminal-lineage-resolved body images
//! shows as extras. `RMFastLoad` body selection retains every body
//! whose complete nonempty topology node-ID set is covered by the active
//! object-ID set; when no body has that complete membership, it declines and
//! falls back to terminal lineage when complete.
//!
//! # Decode a part
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_nx::NxCodec;
//! use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.prt")?;
//! let result = NxCodec.decode(&mut input, &DecodeOptions::default())?;
//!
//! println!("{} bodies", result.ir().model.bodies.len());
//! for loss in &result.report().losses {
//!     println!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Codec::inspect`](cadmpeg_ir::codec::Codec::inspect) returns the SPLMSSTR directory and embedded-stream
//! classifications without decoding entities. `DecodeOptions::container_only`
//! produces metadata IR and skips entity decode.
//!
//! # Model and loss boundaries
//!
//! NX stores part geometry in zlib-compressed Parasolid partition, deltas, or
//! plain streams. The decoder converts Parasolid metre values to millimetres and
//! emits points; analytic curves and surfaces; NURBS curves and surfaces;
//! selected trimmed curves; and resolvable body, region, shell, face, loop,
//! coedge, edge, and vertex topology. Each inflated Parasolid stream is also
//! retained as an unknown record.
//!
//! Read [`cadmpeg_ir::report::decode::DecodeReport`] before using the model as a complete
//! representation. Deltas streams pair with the preceding equal-schema partition
//! in validated `UG_PART` segment order and apply
//! supported non-topology full records and exact-key tombstones using the last
//! event for each key. Valid partition topology remains authoritative. Unmatched
//! tombstone relations remain unresolved. Segment body aliases, primary-body
//! writers, and Boolean tool operands select terminal partition images when the
//! complete body lineage is unambiguous. Assembly files may contain only
//! references to external child parts.
//!
//! Ordered feature-operation records, body-write GROUP ownership, body
//! dependencies, Boolean operations, sketch record lanes, and numeric
//! expressions transfer from the NX object model. Current-body writers and
//! their complete earlier dependency closure transfer as active; other
//! operation suppression remains unresolved. Embedded
//! JT coordinates and triangle connectivity transfer as canonical tessellations.
//! Complete design history, assembly occurrence placement, material and appearance
//! assignment and `.prt` writing are not supported.
//! Part attributes transfer as document attributes. The object-model extraction
//! and attachment tier (record families, feature semantics, and IR writing) is
//! crate-internal and reached only through the decode entry point.

mod canonical_uuid;
mod container;
mod decode;
mod deltas;
mod dialect;
mod evaluation;
mod framing;
mod geometry;
mod intersection;
mod iter_wire;
mod jt;
mod jt_topology;
/// Byte-offset constants generated from `docs/layouts/nx.toml`.
mod layout;
#[allow(dead_code)] // Loss catalog is consumed by tests and the writer.
mod loss;
mod native;
mod nurbs;
mod om;
mod om_tokens;
mod parasolid;
mod payload_text;
mod printable_string;
mod topology;
mod vec3_at;

#[doc(hidden)]
pub mod fuzz;

#[doc(hidden)]
pub use native::hex::Sha256Hex;

#[doc(hidden)]
pub use evaluation::{
    saved_body_census_evidence, BodyCensusEvaluation, FeatureBoundary, UnsupportedBodyCensusReason,
};

use crate::framing::node_kind::NodeKind;
use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// Decoder and inspector for Siemens NX `.prt` files.
#[derive(Debug, Default, Clone, Copy)]
pub struct NxCodec;

impl CodecBackend for NxCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn validate_native(
        ctx: &DecodeContext<'_>,
        ir: &cadmpeg_ir::CadIr,
    ) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
        let Some(namespace) = ir.native.namespace("nx") else {
            return Ok(Vec::new());
        };
        let admitted = native::display_jt::admission::DisplayJtGraph::from_namespace_with_context(
            ctx, namespace,
        )
        .and_then(|_| namespace.admit::<native::structure::occurrences::FastLoadOccurrences>());
        Ok(match admitted {
            Ok(_) => Vec::new(),
            Err(error) => {
                let message = error.to_string();
                let codec_error = CodecError::from(error);
                if matches!(codec_error, CodecError::ResourceLimit(_)) {
                    return Err(codec_error);
                }
                vec![cadmpeg_ir::report::check::Finding {
                    check: cadmpeg_ir::report::check::Check::NativeLinks,
                    severity: cadmpeg_ir::report::Severity::Error,
                    message,
                    entity: None,
                }]
            }
        })
    }

    fn detect_impl(&self, prefix: &[u8]) -> Confidence {
        if container::looks_like_nx(prefix) || container::looks_like_legacy_nx(prefix) {
            Confidence::High
        } else {
            Confidence::No
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = decode::scan(ctx, root)?;
        summarize(ctx, &scan)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root)
    }
}

/// Build the container summary: one entry per catalogued directory stream, plus
/// one per embedded Parasolid stream, and the shared container notes.
fn summarize(ctx: &DecodeContext<'_>, scan: &decode::Scan) -> Result<ContainerSummary, CodecError> {
    let entry_count = scan
        .container
        .entries
        .len()
        .checked_add(scan.streams.len())
        .ok_or_else(|| ctx.refuse_codec_limit("nx summary entries", 0, u64::MAX))?;
    ctx.charge_collection_items(
        u64::try_from(entry_count).unwrap_or(u64::MAX),
        "nx summary entries",
    )?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(entry_count)
        .map_err(|_| ctx.refuse_codec_limit("nx summary entries", 0, 1))?;
    let semantic_streams = native::substrate::topology_streams(ctx, scan)?;

    for entry in &scan.container.entries {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "region",
            "",
            false,
            SummaryValue::Text(entry.region.label()),
        )?;
        let storage = match entry.file_span() {
            Some((off, size)) => {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "file_offset",
                    "",
                    false,
                    SummaryValue::Number(off),
                )?;
                EntryStorage::verbatim(VerbatimLabel::None, size)
            }
            None => {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "kind",
                    "",
                    false,
                    SummaryValue::Text("directory"),
                )?;
                EntryStorage::Directory
            }
        };
        entries.push(ContainerEntry {
            name: render_summary_text(
                ctx,
                "nx summary directory name",
                entry.name.len(),
                format_args!("{}", entry.name),
            )?,
            role: entry.content().role(),
            storage,
            attributes,
        });
    }

    let mut storage_notes: Vec<String> = Vec::new();
    for (si, stream) in scan.streams.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "file_offset",
            "",
            false,
            SummaryValue::Number(u64::try_from(stream.file_offset).unwrap_or(u64::MAX)),
        )?;
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "kind",
            "",
            false,
            SummaryValue::Text(stream.kind().label()),
        )?;
        if let Some(schema) = stream.schema_token() {
            insert_summary_attribute(
                ctx,
                &mut attributes,
                "schema",
                "",
                false,
                SummaryValue::Text(schema.value()),
            )?;
        }
        if stream.kind().is_parasolid() {
            let graph = topology::Graph::parse(&stream.inflated);
            for (kind, name) in [
                (NodeKind::Body, "body"),
                (NodeKind::Shell, "shell"),
                (NodeKind::Face, "face"),
                (NodeKind::Loop, "loop"),
                (NodeKind::Edge, "edge"),
                (NodeKind::Fin, "fin"),
                (NodeKind::Vertex, "vertex"),
                (NodeKind::Region, "region"),
            ] {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "records.",
                    name,
                    false,
                    SummaryValue::Number(
                        u64::try_from(graph.of_kind(kind).count()).unwrap_or(u64::MAX),
                    ),
                )?;
            }
            if stream.kind() == parasolid::StreamKind::Partition {
                let graph = topology::Graph::parse(&semantic_streams[si]);
                for (kind, name) in [
                    (NodeKind::Body, "body"),
                    (NodeKind::Shell, "shell"),
                    (NodeKind::Face, "face"),
                    (NodeKind::Loop, "loop"),
                    (NodeKind::Edge, "edge"),
                    (NodeKind::Fin, "fin"),
                    (NodeKind::Vertex, "vertex"),
                    (NodeKind::Region, "region"),
                ] {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.live.",
                        name,
                        false,
                        SummaryValue::Number(
                            u64::try_from(graph.of_kind(kind).count()).unwrap_or(u64::MAX),
                        ),
                    )?;
                }
            } else if stream.kind() == parasolid::StreamKind::Deltas {
                let census = deltas::census::walk(&stream.inflated);
                if census.transmit_header.is_some() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.transmit_headers", "", false, SummaryValue::Text("1"))?;
                }
                if !census.body_revisions.is_empty() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.body_revisions", "", false, SummaryValue::Number(u64::try_from(census.body_revisions.len()).unwrap_or(u64::MAX)))?;
                }
                if !census.term_use_numeric_tails.is_empty() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.term_use_numeric_tails", "", false, SummaryValue::Number(u64::try_from(census.term_use_numeric_tails.len()).unwrap_or(u64::MAX)))?;
                }
                if !census.tagged_reference_lanes.is_empty() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.tagged_reference_lanes", "", false, SummaryValue::Number(u64::try_from(census.tagged_reference_lanes.len()).unwrap_or(u64::MAX)))?;
                }
                if !census.reference_type_maps.is_empty() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.reference_type_maps", "", false, SummaryValue::Number(u64::try_from(census.reference_type_maps.len()).unwrap_or(u64::MAX)))?;
                }
                if !census.reference_state_packets.is_empty() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.reference_state_packets", "", false, SummaryValue::Number(u64::try_from(census.reference_state_packets.len()).unwrap_or(u64::MAX)))?;
                }
                if !census.reference_marker_packets.is_empty() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.reference_marker_packets", "", false, SummaryValue::Number(u64::try_from(census.reference_marker_packets.len()).unwrap_or(u64::MAX)))?;
                }
                if !census.inline_schema_declarations.is_empty() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.inline_schema_declarations", "", false, SummaryValue::Number(u64::try_from(census.inline_schema_declarations.len()).unwrap_or(u64::MAX)))?;
                }
                for (family, count) in census.full_counts() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.full.", family, true, SummaryValue::Number(u64::try_from(count).unwrap_or(u64::MAX)))?;
                }
                for (family, count) in census.tombstone_counts() {
                    insert_summary_attribute(ctx, &mut attributes, "records.delta.tombstone.", family, true, SummaryValue::Number(u64::try_from(count).unwrap_or(u64::MAX)))?;
                }
            }
        }
        let inflated_len = stream.inflated.len() as u64;
        let storage = match scan.container.layout {
            container::ContainerLayout::Modern { .. } => EntryStorage::Compressed {
                method: CompressionMethod::Zlib,
                stored: None,
                expanded: Some(inflated_len),
            },
            container::ContainerLayout::LegacyCfb { .. } => {
                match EntryStorage::framed(VerbatimLabel::Stored, inflated_len, stream.consumed) {
                    Ok(storage) => storage,
                    Err(message) => {
                        let note_len = [
                            "parasolid#".len(),
                            decimal_len(u64::try_from(si).unwrap_or(u64::MAX)),
                            ": ".len(),
                            message.len(),
                            ": ".len(),
                            decimal_len(stream.consumed),
                            "/".len(),
                            decimal_len(inflated_len),
                        ]
                        .into_iter()
                        .try_fold(0usize, usize::checked_add)
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit("nx summary storage note", 0, u64::MAX)
                        })?;
                        ctx.charge_collection_items(1, "nx summary storage notes")?;
                        storage_notes.try_reserve(1).map_err(|_| {
                            ctx.refuse_codec_limit("nx summary storage notes", 0, 1)
                        })?;
                        storage_notes.push(render_summary_text(
                            ctx,
                            "nx summary storage note",
                            note_len,
                            format_args!(
                                "parasolid#{si}: {message}: {}/{inflated_len}",
                                stream.consumed
                            ),
                        )?);
                        EntryStorage::payload_only(VerbatimLabel::Stored, inflated_len)
                    }
                }
            }
        };
        let name_len = "parasolid#"
            .len()
            .checked_add(decimal_len(u64::try_from(si).unwrap_or(u64::MAX)))
            .ok_or_else(|| ctx.refuse_codec_limit("nx summary stream name", 0, u64::MAX))?;
        entries.push(ContainerEntry {
            name: render_summary_text(
                ctx,
                "nx summary stream name",
                name_len,
                format_args!("parasolid#{si}"),
            )?,
            role: if stream.kind().is_parasolid() {
                ContainerRole::ParasolidStream
            } else {
                ContainerRole::Preview
            },
            storage,
            attributes,
        });
    }

    let (classification, mut notes) = decode::summarize(scan);
    notes.extend(storage_notes);
    let container_kind = classification.container_kind();
    let (dialects, dialect_losses) = classification.into_report_parts();
    Ok(ContainerSummary::classified(
        dialects,
        container_kind,
        entries,
        dialect_losses,
        notes,
    ))
}

#[derive(Clone, Copy)]
enum SummaryValue<'a> {
    Text(&'a str),
    Number(u64),
}

fn decimal_len(number: u64) -> usize {
    if number == 0 {
        1
    } else {
        usize::try_from(u64::from(number.ilog10()) + 1).unwrap_or(usize::MAX)
    }
}

fn render_summary_text(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    len: usize,
    args: std::fmt::Arguments<'_>,
) -> Result<String, CodecError> {
    ctx.charge_retained(u64::try_from(len).unwrap_or(u64::MAX), operation)?;
    let mut text = String::new();
    text.try_reserve_exact(len)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    text.write_fmt(args)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(text)
}

fn insert_summary_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    prefix: &str,
    suffix: &str,
    lowercase_suffix: bool,
    value: SummaryValue<'_>,
) -> Result<(), CodecError> {
    let key_len = prefix
        .len()
        .checked_add(suffix.len())
        .ok_or_else(|| ctx.refuse_codec_limit("nx summary attribute text", 0, u64::MAX))?;
    let value_len = match value {
        SummaryValue::Text(text) => text.len(),
        SummaryValue::Number(number) => decimal_len(number),
    };
    let text_len = key_len
        .checked_add(value_len)
        .ok_or_else(|| ctx.refuse_codec_limit("nx summary attribute text", 0, u64::MAX))?;
    ctx.charge_collection_items(1, "nx summary attributes")?;
    ctx.charge_retained(
        u64::try_from(text_len).unwrap_or(u64::MAX),
        "nx summary attribute text",
    )?;
    let mut key = String::new();
    key.try_reserve_exact(key_len)
        .map_err(|_| ctx.refuse_codec_limit("nx summary attribute text", 0, 1))?;
    key.push_str(prefix);
    if lowercase_suffix {
        for character in suffix.chars() {
            key.push(character.to_ascii_lowercase());
        }
    } else {
        key.push_str(suffix);
    }
    let mut rendered = String::new();
    rendered
        .try_reserve_exact(value_len)
        .map_err(|_| ctx.refuse_codec_limit("nx summary attribute text", 0, 1))?;
    match value {
        SummaryValue::Text(text) => rendered.push_str(text),
        SummaryValue::Number(number) => write!(&mut rendered, "{number}")
            .map_err(|_| ctx.refuse_codec_limit("nx summary attribute text", 0, 1))?,
    }
    attributes.insert(key, rendered);
    Ok(())
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
