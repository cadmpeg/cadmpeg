// SPDX-License-Identifier: Apache-2.0
//! Embedded and externally referenced document resources.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable identity of one document asset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct AssetId(pub String);

impl AssetId {
    /// Borrow the underlying id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Bytes or location supplying an asset's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetContent {
    /// Content embedded in the decoded document.
    Embedded {
        /// Exact resource bytes.
        #[serde(with = "crate::bytes")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        data: Vec<u8>,
    },
    /// Content resolved outside the decoded document.
    External {
        /// Stable external resource identifier or URI.
        uri: String,
    },
}

/// A document resource referenced by model, drawing, or presentation entities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Asset {
    /// Stable asset identity.
    pub id: AssetId,
    /// Source display name or basename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// IANA media type when identified from the source container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Embedded bytes or an external resource location.
    pub content: AssetContent,
    /// Full-fidelity source record or container-entry identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}
