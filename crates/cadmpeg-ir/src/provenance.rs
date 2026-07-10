// SPDX-License-Identifier: Apache-2.0
//! Provenance and exactness value types.
//!
//! The whole point of the IR is to preserve *where each fact came from* and
//! *how much we trust it*, so that a downstream export can report loss honestly
//! rather than silently normalizing. IR v1 stores these facts in document-wide
//! annotation tables.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where an entity's bytes originated in the source file.
///
/// `offset` is a byte offset into the named `stream` (for a `.f3d` this is the
/// decompressed ZIP entry, e.g. `Breps.BlobParts/…​.smbh`), and `tag` is the
/// source record/class name when known (e.g. the ASM record name `face`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    /// Source container format, e.g. `"f3d"` or `"synthetic"` for hand-built IR.
    pub format: String,
    /// Named stream within the container (a decompressed entry name, or empty).
    pub stream: String,
    /// Byte offset of the record within `stream`.
    pub offset: u64,
    /// Source record/class name/tag, when the decoder can attribute one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// How faithfully an entity reflects the source bytes.
///
/// This is the honesty knob. A plane whose coefficients were read verbatim is
/// [`Exactness::ByteExact`]; a bounding box computed from vertices is
/// [`Exactness::Derived`]; a unit guessed from context is
/// [`Exactness::Inferred`]; anything the decoder could not attribute is
/// [`Exactness::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Exactness {
    /// Read verbatim from the source stream with no transformation beyond
    /// documented unit conversion.
    ByteExact,
    /// Computed deterministically from byte-exact inputs (e.g. a derived bbox).
    Derived,
    /// Filled in from context or convention rather than an explicit source field.
    Inferred,
    /// Origin or trustworthiness could not be established.
    Unknown,
}
