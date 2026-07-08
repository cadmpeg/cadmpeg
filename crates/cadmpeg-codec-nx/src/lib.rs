// SPDX-License-Identifier: Apache-2.0
//! # cadmpeg-codec-nx
//!
//! Decoder for Siemens NX `.prt` files.
//!
//! ## What is implemented
//!
//! An NX `.prt` is an `SPLMSSTR` container (Siemens PLM master storage) wrapping
//! zlib-compressed **Parasolid neutral-binary** streams — NX authors geometry
//! directly with the Parasolid kernel. This codec:
//!
//! - [`NxCodec::detect`] recognizes the unique `SPLMSSTR` magic. NX and Creo share
//!   the `.prt` extension, so detection is magic-based, never extension-based: a
//!   Creo/Granite `.prt` never carries this magic.
//! - [`NxCodec::inspect`] parses the container header and HEADER/FOOTER directory,
//!   enumerates the named `/Root/...` streams, locates and classifies every
//!   embedded Parasolid stream (partition / deltas / plain cached body) by a
//!   zlib scan and its inflated prologue, and reads the `SCH_` schema token.
//! - [`NxCodec::decode`] reads the gate-passing analytic geometry carriers —
//!   POINT vertices, analytic surfaces (plane/cylinder/cone/sphere/torus), and
//!   analytic curves (line/circle/ellipse) — from every Parasolid stream into free
//!   carrier arenas, and preserves each stream verbatim as an unknown passthrough.
//!
//! ## What is decoded, and what is reported as loss
//!
//! Geometry decodes to a vertex point cloud plus analytic surface/curve carriers,
//! emitted as **unattached** geometry: the face→loop→edge topology graph is
//! byte-underdetermined without a full sequential record-framing walk (and its
//! active-body face set additionally hangs on the undecoded partition↔deltas
//! tombstone bridge), so it is reported, not fabricated. B-spline and procedural
//! blend surfaces, cross-body Boolean composition, assembly placements, and the NX
//! object-model metadata are counted in the [`cadmpeg_ir::report::DecodeReport`].

pub mod container;
pub mod decode;
pub mod geometry;
pub mod nurbs;
pub mod parasolid;
pub mod topology;

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerEntry, ContainerSummary, DecodeOptions, DecodeResult,
    ReadSeek,
};

/// The Siemens NX `.prt` codec.
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

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        let scan = decode::scan(reader)?;
        Ok(summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(reader, options)
    }
}

/// Build the container summary: one entry per catalogued directory stream, plus
/// one per embedded Parasolid stream, and the shared container notes.
fn summarize(scan: &decode::Scan) -> ContainerSummary {
    let mut entries = Vec::new();

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
mod tests;
