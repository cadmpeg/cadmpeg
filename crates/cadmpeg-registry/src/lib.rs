// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! The cadmpeg codec registry and dialect registries, as a library.
//!
//! An application embedding cadmpeg as its file layer asks four questions.
//! This crate carries the two that are answered statically or at inspection
//! depth, and nothing above them: no conversion pipeline, no artifact store,
//! no command layer.
//!
//! 1. **What is this file?** — [`resolve_and_inspect_with()`] applies the
//!    loader's source selection, including forced input, and reconstructs the
//!    selected container. Its summary carries the classified dialect.
//! 2. **What can I save as?** — [`Format`] and [`build_encoder`] give the
//!    synthesis catalogs (`Encoder::targets`), and [`dialect_table`] / [`support`]
//!    serve the registries from tables compiled into the binary.
//!
//! The other two are answered elsewhere and stay there. "What will I lose?" is
//! `Encoder::plan`, which reports against a live document; "what did I open or
//! write?" is `SourceMeta::dialect` and the serialized export report's
//! `identity.target` field on the artifacts a run produced. Preservation is per-input and never advertised
//! statically, so no capability matrix appears here.
//!
//! The crate root is the facade: every public name is re-exported here, and
//! the implementation modules stay private, so each item has one path.

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
pub use registry::{support, DialectEntry, RegistryLoadError};
pub use views::{
    dialect_provenance, dialect_table, format_rows, DialectProvenance, DialectTableError,
    FormatDialects, FormatRow, UnknownFormat,
};
