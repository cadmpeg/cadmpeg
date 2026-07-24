// SPDX-License-Identifier: Apache-2.0
//! Read Siemens NX `.prt` files into [`cadmpeg_ir::document::CadIr`].
//!
//! The codec recognizes the `SPLMSSTR` container signature, extracts compressed
//! Parasolid neutral-binary streams from the canonical part payload, and decodes
//! supported geometry and topology. Detection uses file content because NX and
//! Creo share the `.prt` extension.
//!
//! Support level: [L4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! for single-body, `RMFastLoad`-selected, and terminal-lineage-resolved body
//! images; L2 for unresolved multi-partition history.
//!
//! # Decode a part
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_nx::NxCodec;
//! use cadmpeg_ir::codec::{Codec, CodecEntry, DecodeOptions};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.prt")?;
//! let result = NxCodec.decode(&mut input, &DecodeOptions::default())?;
//!
//! println!("{} bodies", result.ir.model.bodies.len());
//! for loss in &result.report.losses {
//!     println!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`CodecEntry::inspect`](cadmpeg_ir::codec::CodecEntry::inspect) returns the SPLMSSTR directory and embedded-stream
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
//! Read [`cadmpeg_ir::report::DecodeReport`] before using the model as a complete
//! representation. Deltas streams pair with the preceding equal-schema partition
//! in validated `UG_PART` segment order and apply
//! supported non-topology full records and exact-key tombstones using the last
//! event for each key. Valid partition topology remains authoritative. Unmatched
//! tombstone relations remain unresolved. Segment body aliases, primary-body
//! writers, and Boolean tool operands select terminal partition images when the
//! complete body lineage is unambiguous. Assembly files may contain only
//! references to external child parts.
//!
//! Ordered feature-operation records, body dependencies, Boolean operations,
//! sketch record lanes, and numeric expressions transfer from the NX object
//! model. Operation suppression remains unresolved instead of being asserted
//! active. Embedded JT coordinates and triangle connectivity transfer as canonical
//! tessellations. Complete design history, assembly occurrence placement, material
//! and appearance assignment, class-specific entity attribute fields, and `.prt`
//! writing are not supported.
//! Part attributes transfer as document attributes. The public submodules
//! expose the lower-level container, stream, geometry, NURBS, intersection, and
//! topology decoders. The object-model extraction and attachment tier (record
//! families, feature semantics, and IR writing) is crate-internal and reached
//! only through the decode entry point. Applications that need a complete IR
//! entry point should use [`NxCodec`].

pub mod container;
pub mod decode;
pub mod deltas;
pub mod geometry;
pub mod intersection;
mod jt;
mod jt_topology;
pub(crate) mod loss;
pub(crate) mod native;
pub mod nurbs;
pub mod om;
pub mod om_tokens;
pub mod parasolid;
pub mod topology;

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerEntry, ContainerSummary, DecodeResult,
};
use cadmpeg_ir::decode::{DecodeContext, View};

/// Decoder and inspector for Siemens NX `.prt` files.
#[derive(Debug, Default, Clone, Copy)]
pub struct NxCodec;

impl Codec for NxCodec {
    fn id(&self) -> &'static str {
        "nx"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if container::looks_like_nx(prefix) {
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
        Ok(summarize(&scan))
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root)
    }
}

/// Build the container summary: one entry per catalogued directory stream, plus
/// one per embedded Parasolid stream, and the shared container notes.
fn summarize(scan: &decode::Scan) -> ContainerSummary {
    let mut entries = Vec::new();
    let semantic_streams = native::topology_streams(scan);

    for entry in &scan.container.entries {
        let mut attributes = BTreeMap::new();
        attributes.insert("region".to_string(), entry.region.label().to_string());
        let (compressed, uncompressed) = match entry.file_span {
            Some((off, size)) => {
                attributes.insert("file_offset".to_string(), off.to_string());
                (size, size)
            }
            None => {
                attributes.insert("kind".to_string(), "directory".to_string());
                (0, 0)
            }
        };
        entries.push(ContainerEntry {
            name: entry.name.clone(),
            role: "stream".to_string(),
            compression: "none".to_string(),
            compressed_size: compressed,
            uncompressed_size: uncompressed,
            attributes,
        });
    }

    for (si, stream) in scan.streams.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        attributes.insert("file_offset".to_string(), stream.file_offset.to_string());
        attributes.insert("kind".to_string(), stream.kind.label().to_string());
        if let Some(schema) = &stream.schema {
            attributes.insert("schema".to_string(), schema.clone());
        }
        if stream.kind.is_parasolid() {
            let graph = topology::Graph::parse(&stream.inflated);
            for (kind, name) in [
                (12, "body"),
                (13, "shell"),
                (14, "face"),
                (15, "loop"),
                (16, "edge"),
                (17, "fin"),
                (18, "vertex"),
                (19, "region"),
            ] {
                attributes.insert(
                    format!("records.{name}"),
                    graph.of_kind(kind).count().to_string(),
                );
            }
            if stream.kind == parasolid::StreamKind::Partition {
                let graph = topology::Graph::parse(&semantic_streams[si]);
                for (kind, name) in [
                    (12, "body"),
                    (13, "shell"),
                    (14, "face"),
                    (15, "loop"),
                    (16, "edge"),
                    (17, "fin"),
                    (18, "vertex"),
                    (19, "region"),
                ] {
                    attributes.insert(
                        format!("records.live.{name}"),
                        graph.of_kind(kind).count().to_string(),
                    );
                }
            } else if stream.kind == parasolid::StreamKind::Deltas {
                let census = deltas::walk(&stream.inflated);
                for (family, count) in census.full_counts {
                    attributes.insert(
                        format!("records.delta.full.{}", family.to_ascii_lowercase()),
                        count.to_string(),
                    );
                }
                for (family, count) in census.tombstone_counts {
                    attributes.insert(
                        format!("records.delta.tombstone.{}", family.to_ascii_lowercase()),
                        count.to_string(),
                    );
                }
            }
        }
        entries.push(ContainerEntry {
            name: format!("parasolid#{si}"),
            role: if stream.kind.is_parasolid() {
                "parasolid-stream".to_string()
            } else {
                "preview".to_string()
            },
            compression: "zlib".to_string(),
            compressed_size: 0,
            uncompressed_size: stream.inflated.len() as u64,
            attributes,
        });
    }

    ContainerSummary {
        format: "nx".to_string(),
        container_kind: "splmsstr".to_string(),
        entries,
        notes: decode::summary_notes(scan),
    }
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
