// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` source identity.

use cadmpeg_core::dialect::{DialectLayers, DialectMatch};

#[path = "dialect/registry_ids.rs"]
pub(crate) mod registry_ids;
use self::registry_ids::IRONCAD_CFB;

pub(crate) fn layers() -> DialectLayers {
    DialectLayers::of(DialectMatch::admitted(IRONCAD_CFB))
}
