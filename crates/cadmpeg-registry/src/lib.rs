// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! The cadmpeg codec registry and dialect registries, as a library.
//!
//! An application embedding cadmpeg as its file layer asks four questions.
//! This crate carries the two that are answered statically or at inspection
//! depth, and nothing above them: no conversion pipeline, no artifact store,
//! no command layer.
//!
//! 1. **What is this file?** — [`identify()`], which uses the loader's prefix
//!    winner selection and then reconstructs that winner's container, so the
//!    answer carries a dialect and not just a format. Equal-confidence
//!    ambiguity remains explicit and is not inspected.
//! 2. **What can I save as?** — [`Format`] and [`build_encoder`] give the
//!    synthesis catalogs (`Encoder::targets`), and [`dialects`] / [`support`]
//!    serve the registries from tables compiled into the binary.
//!
//! The other two are answered elsewhere and stay there. "What will I lose?" is
//! `Encoder::plan`, which reports against a live document; "what did I open or
//! write?" is `SourceMeta::dialect` and `ExportReport::target` on the
//! artifacts a run produced. Preservation is per-input and never advertised
//! statically, so no capability matrix appears here.
//!
//! The crate root is the facade: every public name is re-exported here, and
//! the implementation modules stay private, so each item has one path.

mod catalog;
mod encoders;
mod format;
mod identify;
mod support;

pub use catalog::{
    DetectionOutcome, ForcedInput, InputCatalog, InputDescriptor, ResolveSourceError,
    ResolvedSource,
};
pub use encoders::build_encoder;
pub use format::Format;
pub use identify::{identify, identify_with, Identification, Inspection, DETECTION_PREFIX_LEN};
pub use support::{
    dialect_provenance, dialect_table, dialects, format_rows, support, DialectEntry,
    DialectProvenance, DialectRow, Disposition, FormatDialects, FormatRow, ReadDisposition,
    UnknownDialectKind, UnknownDisposition, UnknownFormat, WriteDisposition,
};
