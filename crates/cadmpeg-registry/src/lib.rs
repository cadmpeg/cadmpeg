// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Format registration and composed document checks for library consumers.
//!
//! Detection and inspection use the input catalogue. Encoder targets and
//! dialect support use the compiled registries. The reproduction API lives in
//! [`repro`]. Composed validation also serves the CLI.

mod catalog;
mod descriptors;
mod disposition;
mod encoders;
mod format;
mod identify;
mod registry;
mod views;

#[cfg(test)]
mod integration_tests;

pub use catalog::{
    AmbiguousDetection, DetectionOutcome, ForcedInput, InputCatalog, InputDescriptor,
    ResolveSourceError, ResolvedSource, Selection,
};
pub use descriptors::{forced_input, input_names, FormatDescriptor, NativeDescriptor};
pub use disposition::{
    Disposition, LadderLevel, ReadDisposition, UnknownDisposition, WriteDisposition,
};
pub use encoders::build_encoder;
pub use format::Format;
pub use identify::{resolve_and_inspect_with, InspectError, Inspected, DETECTION_PREFIX_LEN};
pub use registry::{headline_level, support, DialectEntry, RegistryLoadError};
pub use views::{
    dialect_provenance, dialect_table, format_rows, DialectProvenance, DialectTableError,
    FormatDialects, FormatRow, UnknownFormat,
};

/// Library entry points for reproducible fragments.
pub mod repro;
/// Composed document validation.
mod validation;
pub use validation::{check_with_context, validate_native};
