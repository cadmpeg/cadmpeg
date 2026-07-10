// SPDX-License-Identifier: Apache-2.0
//! # cadmpeg-codec-creo
//!
//! Decoder for PTC Creo Parametric / Pro-ENGINEER `.prt` (PSB) files.
//!
//! ## What this format is
//!
//! `.prt` is an overloaded extension: Creo/Pro-E and Siemens NX both use it, so
//! detection is **magic-based** on the `#UGC:2` ASCII framing, never on the file
//! extension. The container is PSB ("Pro/E Session Binary"): an ASCII header and
//! table of contents followed by a run of named binary sections.
//!
//! ## What is implemented
//!
//! Creo PSB is characterized well enough to walk the container and read the
//! surface/curve namespace headers, but per-instance model-space geometry is
//! gated behind several undecoded PSB layers, and `VisibGeom` stores surface
//! *prototypes* (first-instance templates), not per-instance geometry. This codec
//! is therefore, by design, the lowest-fidelity of the family: a solid `inspect`
//! is the deliverable.
//!
//! - [`CreoCodec::detect`] recognizes the `#UGC:2` container magic;
//! - [`CreoCodec::inspect`] parses the header/TOC, enumerates and classifies the
//!   named sections (spec §2.2), identifies the ND vs DEPDB layout family (spec
//!   §1), reads the byte-backed `srf_array`/`crv_array` namespace counts (spec §4,
//!   §5), and flags the JPEG thumbnail;
//! - [`CreoCodec::decode`] performs an honest structural decode: it emits the
//!   container facts and namespace census, decodes datum planes as derived plane
//!   carriers (`geometry_transferred` is true only when at least one transfers),
//!   preserves the PSB geometry sections as
//!   [`cadmpeg_ir::unknown::UnknownRecord`]s, and reports each gate as a counted
//!   loss note. It never presents prototype geometry as per-instance geometry.
//!
//! The PSB primitive layer — compact integers, structural tokens, and the 3-byte
//! compact-float short form (spec §3) — is decoded and unit-tested in [`psb`], and
//! used by [`container`] to read the namespace count headers.

pub mod container;
pub mod curve;
pub mod datum;
pub mod decode;
pub mod psb;
pub mod scalar;
pub mod surface;
pub mod topology;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};

/// The Creo Parametric `.prt` (PSB) codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct CreoCodec;

impl Codec for CreoCodec {
    fn id(&self) -> &'static str {
        "creo"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        // The `#UGC:2` ASCII magic is unique to the Creo/Pro-E PSB container and
        // distinguishes it from a Siemens NX `.prt` sharing the extension.
        if container::looks_like_creo(prefix) {
            Confidence::High
        } else {
            Confidence::No
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

#[cfg(test)]
mod tests;
