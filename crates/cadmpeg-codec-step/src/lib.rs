// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Reads and writes [`cadmpeg_ir::CadIr`] documents as ISO 10303-21 STEP Part
//! 21 exchange structures for AP203, AP214, and AP242.
//!
//! <!-- generated: capability -->
//! Support: L9 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#step-part-21)).
//! <!-- /generated: capability -->
//!
//! [`StepCodec`] emits the application protocol selected through
//! [`cadmpeg_ir::codec::Encoder::plan`]. It writes product and representation context,
//! connected exact shape, product occurrences, tessellation, presentation,
//! and PMI when the target schema carries those domains.
//!
//! # Export workflow
//!
//! Construct or decode a [`cadmpeg_ir::CadIr`], configure [`StepCodec`], and
//! select a target from the codec's encoder catalog. Planning validates the
//! request and returns the bytes with the export report.
//!
//! Review [`cadmpeg_ir::ExportReport::losses`] before retaining output. Opaque
//! records, source attributes, unsupported
//! procedural definitions, and target-schema incompatibilities are reported or
//! rejected rather than silently discarded. Body and face colors become
//! per-face `STYLED_ITEM` presentation; direct geometry and tessellation
//! bindings retain their native presentation targets.
//!
//! Coordinates are emitted unchanged under a millimetre length-unit context.
//! Callers must convert non-millimetre geometry before export. Analytic curves
//! and surfaces map to their corresponding STEP carriers. Rational and
//! non-rational NURBS use the `*_WITH_KNOTS` entities.
//!
//! Output-sink failures return [`std::io::Error`]. Because the writer streams
//! the header and DATA section after acceptance, an I/O failure can leave
//! partial output.

mod archive;
mod codec;
mod dialect;
mod export;
mod geometry;
mod ids;
mod lex;
#[allow(dead_code)] // Loss catalog is consumed by tests and the writer.
mod loss;
mod options;
mod parse;
mod reader;
mod signature;
#[allow(dead_code)] // String helpers are part of the internal parser layer.
mod strings;
mod writer;

#[doc(hidden)]
pub mod fuzz;

pub use codec::StepCodec;
#[cfg(test)]
pub(crate) use export::write_step;
pub use options::{StepSchema, StepWriteOptions};

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
