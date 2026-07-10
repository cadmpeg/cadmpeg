// SPDX-License-Identifier: Apache-2.0
//! # cadmpeg-codec-f3d
//!
//! Container-level codec for Autodesk Fusion 360 `.f3d` files.
//!
//! ## What is implemented
//!
//! A `.f3d` is a ZIP archive whose entries follow documented naming families
//! (BREP streams, `.protein` material ZIPs, design/ACT/browser bulk & meta
//! streams, manifests, previews). This codec:
//!
//! - [`F3dCodec::detect`] recognizes the ZIP magic plus f3d marker strings;
//! - [`F3dCodec::inspect`] enumerates and classifies every entry and reads the
//!   ASM `BinaryFile` header of each BREP stream (magic/width, version words,
//!   `product_family`/`product_version`/`save_date`, `scale`/`resabs`/`resnor`)
//!   and locates the `delta_state` history boundary;
//! - [`F3dCodec::decode`] frames the active BREP's SAB record stream and builds
//!   the IR B-rep graph (`body → region → shell → face → loop → coedge → edge →
//!   vertex → point`) plus the analytic surface/curve carriers it references.
//!
//! ## What is decoded, and what is reported as loss
//!
//! The SAB record stream ([`sab`]) is framed token-by-token so record
//! boundaries stay exact even across records this codec does not interpret. From
//! that `RecordTable` ([`brep`]) it decodes the topology graph and the analytic
//! carriers — `plane`, `cone`/cylinder, `sphere`, `torus`, `straight` line,
//! `ellipse`/circle — with lengths converted centimetre→millimetre.
//!
//! Cached spline/procedural surfaces and curves, UV pcurves, linked ASM
//! attributes, body transforms, nested Protein appearance assets, and Design
//! body assignments are transferred. Unsupported source records remain
//! explicit in the [`cadmpeg_ir::report::DecodeReport`]. When the active stream
//! is not a decodable `BinaryFile8` SAB, decode falls back to container metadata.

mod act;
pub mod asm_header;
pub mod brep;
pub mod container;
pub mod decode;
pub mod design;
pub mod history;
pub mod materials;
pub mod nurbs;
pub mod sab;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};

/// The ZIP local-file-header magic.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The Fusion 360 `.f3d` container codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct F3dCodec;

impl Codec for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(ZIP_MAGIC) {
            return Confidence::No;
        }
        // A ZIP alone is a weak signal (many formats are ZIPs). An f3d marker
        // string in the prefix — entry names are stored in cleartext in ZIP
        // local headers — makes it conclusive.
        if container::DETECT_MARKERS
            .iter()
            .any(|m| contains_subslice(prefix, m))
        {
            Confidence::High
        } else {
            Confidence::Low
        }
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(reader)?;
        Ok(container::summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(reader, options)
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests;
