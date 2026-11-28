// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` source identity.

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

include!("dialect/registry_ids.rs");

/// Parasolid dialect grammars that the scene decoder applies and verifies
/// under their own identities.
pub(crate) const VERIFIED_PARASOLID_GRAMMARS: &[DialectId] =
    &[cadmpeg_core::dialect_id!("parasolid:format-13006")];

pub(crate) fn layers() -> DialectLayers {
    DialectLayers::of(DialectMatch::admitted(IRONCAD_CFB))
}
