// SPDX-License-Identifier: Apache-2.0
//! Passthrough for bytes the decoder recognized as a record but did not
//! interpret.
//!
//! Honest decoding means never dropping data on the floor. When a decoder
//! encounters a record it cannot (yet) map to a typed IR entity, it emits an
//! [`UnknownRecord`] carrying the raw byte span's location, a content hash, and
//! any ids it *could* attribute, so nothing silently disappears and a later
//! decoder version can resolve it.

use crate::ids::UnknownId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A recognized-but-uninterpreted record preserved verbatim by reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UnknownRecord {
    /// Arena id.
    pub id: UnknownId,
    /// Byte offset of the record within its source stream.
    pub offset: u64,
    /// Byte length of the record's span.
    pub byte_len: u64,
    /// Lowercase hex SHA-256 of the record bytes, for integrity and dedup.
    pub sha256: String,
    /// Preserved record bytes, when retained by the decoder.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::bytes::option"
    )]
    #[schemars(with = "Option<String>")]
    pub data: Option<Vec<u8>>,
    /// Ids of other IR entities this record is known to relate to (e.g. an
    /// attribute's owner), when the decoder can attribute them. Free-form
    /// string ids so a link can point into any arena.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}
