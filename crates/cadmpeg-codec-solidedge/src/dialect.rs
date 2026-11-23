// SPDX-License-Identifier: Apache-2.0
//! Solid Edge source identity.

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

include!("dialect/registry_ids.rs");

pub(crate) fn layers() -> DialectLayers {
    DialectLayers::of(DialectMatch::admitted(SOLIDEDGE_CFB))
}
