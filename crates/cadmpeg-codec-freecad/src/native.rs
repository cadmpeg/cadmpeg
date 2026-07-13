// SPDX-License-Identifier: Apache-2.0
//! Versioned FCStd-native records.

use serde::{Deserialize, Serialize};

/// Native namespace schema emitted by this crate.
pub const VERSION: u32 = 1;

/// One physical archive span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveSpan {
    /// Stable span identity.
    pub id: String,
    /// Inclusive byte offset.
    pub start: u64,
    /// Exclusive byte offset.
    pub end: u64,
    /// Structural role.
    pub role: String,
    /// Owning entry, when applicable.
    pub entry: Option<String>,
}

/// Metadata read from the persistence document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentFacts {
    /// Stable document-record identity.
    pub id: String,
    /// Persistence schema version.
    pub schema_version: String,
    /// Persistence file version.
    pub file_version: String,
    /// Producing application version, when carried.
    pub program_version: Option<String>,
    /// XML document element name.
    pub root_name: String,
    /// Number of declared application objects.
    pub object_count: usize,
}
