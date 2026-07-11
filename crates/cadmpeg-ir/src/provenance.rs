// SPDX-License-Identifier: Apache-2.0
//! Provenance and exactness value types.
//!
//! [`Exactness`] classifies how an IR value relates to source bytes. Current
//! documents store exactness and source locations in
//! [`crate::annotations::Annotations`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::topology::Color;

/// Native object identity and effective display metadata for a free carrier.
///
/// `format` identifies the source format. `object_id` is the source format's
/// native object identifier, not an IR arena identifier. `name`, `color`, and
/// `visible` are the effective object display values. `layer` is the native
/// layer identifier. `instance_path` contains native instance identifiers in
/// outermost-to-innermost order; an empty path means that the object is not
/// nested in an instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceObjectAssociation {
    /// Source format identifier.
    pub format: String,
    /// Native source object identifier.
    pub object_id: String,
    /// Effective source object name, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Effective source object color, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Effective source object visibility, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Native source layer identifier, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// Native instance identifiers from outermost to innermost.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_path: Vec<String>,
}

/// Source location for an entity's bytes.
///
/// `offset` is relative to `stream`; `tag` identifies the source record class
/// when known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    /// Source container format.
    pub format: String,
    /// Named stream within the container (a decompressed entry name, or empty).
    pub stream: String,
    /// Byte offset of the record within `stream`.
    pub offset: u64,
    /// Source record/class name/tag, when the decoder can attribute one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// How an entity or field value was established from its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Exactness {
    /// Read verbatim from the source stream with no transformation beyond
    /// documented unit conversion.
    ByteExact,
    /// Computed deterministically from byte-exact inputs.
    Derived,
    /// Filled in from context or convention rather than an explicit source field.
    Inferred,
    /// Origin or trustworthiness could not be established.
    Unknown,
}
