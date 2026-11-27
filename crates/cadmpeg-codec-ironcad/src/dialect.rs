// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` source identity.

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

include!("dialect/registry_ids.rs");

pub(crate) fn layers() -> DialectLayers {
    DialectLayers::of(DialectMatch::admitted(IRONCAD_CFB))
}
