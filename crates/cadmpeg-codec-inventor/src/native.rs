// SPDX-License-Identifier: Apache-2.0
//! Typed Inventor-native structural records.

use serde::{Deserialize, Serialize};

/// Current Inventor native namespace version.
pub(crate) const INVENTOR_NATIVE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StorageBandRecord {
    pub(crate) id: String,
    pub(crate) band: u32,
    pub(crate) database_directory_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentPairRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) metadata_directory_id: u32,
    pub(crate) bulk_directory_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UnpairedSegmentRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) missing_member: String,
}
