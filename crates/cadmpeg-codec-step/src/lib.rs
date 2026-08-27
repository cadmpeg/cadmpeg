// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Reads and writes [`cadmpeg_ir::CadIr`] documents as ISO 10303-21 STEP Part
//! 21 exchange structures for AP203, AP214, and AP242.
//!
//! <!-- generated: capability -->
//! Support: depth L9, breadth 4 of >=4 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#step-part-21)).
//! <!-- /generated: capability -->
//!
//! [`write_step`] emits the application protocol named in the call as a
//! [`StepSchema`]. It writes product and representation context,
//! connected exact shape, product occurrences, tessellation, presentation,
//! and PMI when the target schema carries those domains.
//!
//! # Export workflow
//!
//! Construct or decode a [`cadmpeg_ir::CadIr`], name the target [`StepSchema`],
//! choose the header metadata in [`StepWriteOptions`], then write to any
//! [`std::io::Write`] sink:
//!
//! ```
//! use cadmpeg_ir::examples::unit_cube;
//! use cadmpeg_codec_step::{write_step, StepSchema, StepWriteOptions};
//!
//! let ir = unit_cube();
//! let mut bytes = Vec::new();
//! let report = write_step(
//!     &ir,
//!     &mut bytes,
//!     StepSchema::Ap242Edition3,
//!     &StepWriteOptions::default(),
//! )?;
//!
//! assert!(bytes.starts_with(b"ISO-10303-21;"));
//! assert!(report.census.total() > 0);
//! # Ok::<(), cadmpeg_codec_step::StepError>(())
//! ```
//!
//! Review [`cadmpeg_ir::ExportReport::losses`] before retaining report-mode
//! output. [`StepUnsupportedPolicy::Reject`] rejects all such losses before any
//! output byte is written. Opaque records, source attributes, unsupported
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
//! [`StepError`] reports [`StepError::Unsupported`] under
//! [`StepUnsupportedPolicy::Reject`] before any output is written, and
//! [`StepError::Io`] for output-sink failures. Because the writer streams the
//! header and DATA section after acceptance, an I/O failure can leave partial
//! output.

mod archive;
mod codec;
mod dialect;
mod error;
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
pub use error::StepError;
pub use export::write_step;
pub use options::{StepSchema, StepUnsupportedPolicy, StepWriteOptions};

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
