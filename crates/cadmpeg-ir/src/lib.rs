// SPDX-License-Identifier: Apache-2.0
//! Format-neutral CAD documents and the codec interfaces that produce them.
//!
//! [`CadIr`] stores units, tolerances, and flat entity arenas connected by
//! typed IDs. Its B-rep topology follows
//! `body → region → shell → face → loop → coedge → edge → vertex`; topology
//! references geometry carriers instead of nesting them. The document also
//! carries neutral construction features, tessellation, appearance, source
//! attributes, source-native namespaces, and uninterpreted [`UnknownRecord`]s.
//!
//! Start a hand-built document with [`CadIr::empty`], populate its arenas,
//! call [`CadIr::finalize`] to establish canonical identity order, then call
//! [`validate()`] to check structural and numeric invariants. Use
//! [`CadIr::to_canonical_json`] and [`CadIr::from_json`] for the versioned JSON
//! form, and [`diff()`] for identity-based structural comparison.
//!
//! Format crates implement [`Codec`]. Detection selects a codec from a byte
//! prefix, inspection enumerates a container, and decoding returns a
//! [`DecodeResult`]. Operation failures use [`CodecError`]. A successful decode
//! reports partial transfer through [`DecodeReport`] and [`LossNote`].
//!
//! [`Annotations`] records source locations and fidelity by globally unique
//! entity ID. An omitted exactness entry means byte-exact; explicit entries
//! distinguish derived, inferred, and unknown values. Native namespaces and
//! unknown records retain source-specific data outside the neutral model.
//!
//! Assembly instancing, component trees, and joint constraints are reserved.

pub mod annotations;
pub mod appearance;
pub mod attributes;
pub mod be;
pub mod bytes;
pub mod codec;
pub mod compression;
pub mod design;
pub mod diff;
pub mod document;
pub mod eval;
pub mod examples;
pub mod features;
pub mod geometry;
pub mod hash;
pub mod history;
pub mod ids;
pub mod le;
pub mod math;
pub mod native;
mod provenance;
pub mod read;
pub mod report;
pub mod subd;
pub mod tessellation;
pub mod topology;
pub mod transform;
pub mod units;
pub mod validate;

pub use annotations::{AnnotationBuilder, Annotations, ExactnessNote, Provenance};
pub use codec::{
    CadirEncoder, Codec, CodecError, Confidence, ContainerEntry, ContainerSummary, DecodeOptions,
    DecodeResult, Encoder, ReadSeek,
};
pub use diff::{diff, ArenaDiff, IrDiff, ModifiedEntity};
pub use document::{CadIr, SourceMeta, IR_VERSION};
pub use features::{Feature, FeatureDefinition, FeatureId};
pub use native::{F3dNative, LossCount, Native, SldprtNative};
/// Source location attached to a [`LossNote`].
pub use provenance::Provenance as LossProvenance;
pub use provenance::{Exactness, SourceObjectAssociation};
pub use report::{
    Check, DecodeReport, ExportReport, Finding, LossCategory, LossNote, Severity, ValidationReport,
};
pub use subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
pub use unknown::UnknownRecord;
pub use validate::validate;

pub mod unknown;

/// Generate the JSON Schema for the current [`CadIr`] representation.
pub fn cadir_json_schema() -> schemars::Schema {
    schemars::schema_for!(CadIr)
}

#[cfg(test)]
mod tests;
