// SPDX-License-Identifier: Apache-2.0
//! Typed identity and class registration of an ACT table.

use serde::{Deserialize, Serialize};

use super::references::DesignClassTag;

/// The table record owns the ordered GUID, reference and registry-channel
/// arenas of its ACT segment. Its class registration identifies the vendor
/// record grammar independently of its physical byte location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActTable {
    pub(crate) id: String,
    pub(crate) record_index: u32,
    pub(crate) class_tag: DesignClassTag,
}

rewrite_native_identity_record!(ActTable; record_index, class_tag);
