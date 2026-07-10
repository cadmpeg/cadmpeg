// SPDX-License-Identifier: Apache-2.0
//! # cadmpeg-ir
//!
//! The provenance-rich intermediate representation (IR) and codec contract at
//! the heart of the cadmpeg CAD transcoder.
//!
//! ## What the IR is
//!
//! A decoded CAD document is a [`CadIr`]: units, tolerances, and an exact B-rep
//! stored as flat, id-referenced arenas following the ACIS/ASM topology
//! hierarchy `body → lump → shell → face → loop → coedge → edge → vertex`, with
//! geometry attached by reference (surfaces, curves, pcurves, points). Every
//! entity carries [`EntityMeta`] — [`Provenance`] (where the bytes came from)
//! and [`Exactness`] (how faithfully they were transferred). Records the decoder
//! recognized but could not interpret are preserved as [`UnknownRecord`]s rather
//! than dropped.
//!
//! ## What is deliberately reserved
//!
//! Feature history is represented as ordered, source-provenanced operations.
//! Assembly structure remains reserved. See `docs/cad-ir.md`.
//!
//! ## The codec contract
//!
//! Format plugins implement [`Codec`]: [`Codec::detect`], [`Codec::inspect`]
//! (container enumeration), and [`Codec::decode`] (into a [`DecodeResult`] with
//! an honest [`DecodeReport`]). [`validate`] checks a document with in-IR
//! arithmetic only.

pub mod annotations;
pub mod appearance;
pub mod attributes;
pub mod bytes;
pub mod codec;
pub mod design;
pub mod diff;
pub mod document;
pub mod examples;
pub mod features;
pub mod geometry;
pub mod history;
pub mod ids;
pub mod math;
pub mod native;
pub mod provenance;
pub mod report;
pub mod reserved;
pub mod tessellation;
pub mod topology;
pub mod transform;
pub mod units;
pub mod validate;

pub use annotations::{AnnotationBuilder, Annotations, ExactnessNote};
pub use codec::{
    Codec, CodecError, Confidence, ContainerEntry, ContainerSummary, DecodeOptions, DecodeResult,
    Encoder, ReadSeek,
};
pub use diff::{diff, ArenaDiff, IrDiff, ModifiedEntity};
pub use document::{CadIr, SourceMeta, IR_VERSION};
pub use features::{Feature, FeatureDefinition, FeatureId};
pub use native::{F3dNative, LossCount, Native, SldprtNative};
pub use provenance::{EntityMeta, Exactness, Provenance};
pub use report::{
    Check, DecodeReport, Finding, LossCategory, LossNote, Severity, ValidationReport,
};
pub use unknown::UnknownRecord;
pub use validate::validate;

pub mod unknown;

/// Generate the JSON Schema for [`CadIr`] via `schemars`. Used by tooling and
/// documentation to publish the IR contract.
pub fn cadir_json_schema() -> schemars::schema::RootSchema {
    schemars::schema_for!(CadIr)
}

#[cfg(test)]
mod tests;
