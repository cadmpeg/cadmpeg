// SPDX-License-Identifier: Apache-2.0
//! Solid Edge source identity.

use cadmpeg_core::dialect::{DialectLayers, DialectMatch};

#[path = "dialect/registry_ids.rs"]
pub(crate) mod registry_ids;
use self::registry_ids::SOLIDEDGE_CFB;

pub(crate) fn layers() -> DialectLayers {
    DialectLayers::of(DialectMatch::admitted(SOLIDEDGE_CFB))
}
